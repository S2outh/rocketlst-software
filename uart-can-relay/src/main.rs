#![no_std]
#![no_main]

use core::cmp::min;

use embassy_time::Timer;
use rodos_can_interface::{RodosCanInterface, receiver::RodosCanReceiver, sender::RodosCanSender};
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_stm32::{
    bind_interrupts, can::{self, CanConfigurator, RxBuf, TxBuf}, gpio::{Level, Output, Speed}, mode::Async, peripherals::*, rcc::{self, mux::Fdcansel}, usart::{self, Uart, UartRx, UartTx}, wdg::IndependentWatchdog, Config
};
use embedded_io_async::Write;
use heapless::Vec;

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

const RODOS_DEVICE_ID: u8 = 0x01;
const RODOS_REC_TOPIC_ID: u16 = 4000;
const RODOS_SND_TOPIC_ID: u16 = 4001;

const RODOS_MAX_RAW_MSG_LEN: usize = 247;

const RX_BUF_SIZE: usize = 500;
const TX_BUF_SIZE: usize = 30;

static RX_BUF: StaticCell<embassy_stm32::can::RxBuf<RX_BUF_SIZE>> = StaticCell::new();
static TX_BUF: StaticCell<embassy_stm32::can::TxBuf<TX_BUF_SIZE>> = StaticCell::new();

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
    USART3_4_5_6_LPUART1 => usart::InterruptHandler<USART5>;
});

/// take can telemetry frame, add necessary headers and relay to RocketLST via uart
async fn sender<const NOS: usize, const MPL: usize>(mut can: RodosCanReceiver<NOS, MPL>, mut uart: UartTx<'static, Async>) {
    let mut seq_num: u16 = 0;
    loop {
        match can.receive().await {
            Ok(frame) => {
                let rodos_msg_len = frame.data()[0]; // for now hardcoded, the first byte is the
                                                     // number of actually used bytes

                info!("send: {}", rodos_msg_len);

                let header = [
                    0x22, 0x69,                          // Uart start bytes
                    rodos_msg_len + 6,                   // packet length (+6 for remaining header)
                    0x00, 0x01,                          // Hardware ID
                    (seq_num >> 8) as u8, seq_num as u8, // SeqNum
                    0x11,                                // Destination
                    0x11                                 // Mode = ascii
                ];
                seq_num = seq_num.wrapping_add(1);

                let _ = frame.topic();
                let _ = frame.device();

                let mut packet: Vec<u8, 256> = Vec::new(); // max openlst data length
                packet.extend_from_slice(&header).unwrap();
                // skip first byte cause it is the msg length, send the rest up to length
                // number_of_bytes
                packet.extend_from_slice(&frame.data()[1..][..rodos_msg_len as usize]).unwrap();

                if let Err(e) = uart.write_all(&packet).await {
                    error!("dropped frames: {}", e)
                }
            }
            Err(e) => error!("error in frame! {}", e),
        };
    }
}

/// receive data from RocketLST and transmit via can
async fn receiver(mut can: RodosCanSender, mut uart: UartRx<'static, Async>) {
    let mut buffer: [u8; 257] = [0; 257];
    loop {
        match uart.read_until_idle(&mut buffer).await {
            Ok(len) => {
                const HEADER_LEN: usize = 9;

                if len <= HEADER_LEN {
                    // incomplete msg
                    continue;
                }

                let rodos_msg_len = min(RODOS_MAX_RAW_MSG_LEN, len-HEADER_LEN);
                
                info!("received: {}", rodos_msg_len);
                
                let rodos_buffer = &mut buffer[HEADER_LEN-1..];
                rodos_buffer[0] = rodos_msg_len as u8;
                
                if let Err(e) = can.send(RODOS_SND_TOPIC_ID, &rodos_buffer[..rodos_msg_len+1]).await {
                    error!("could not send frame via can: {}", e);
                }
            }
            Err(e) => {
                error!("could not receive uart frame: {}", e);
            }
        }
    }
}

/// Watchdog petting task
async fn petter(mut watchdog: IndependentWatchdog<'_, IWDG>) {
    loop {
        watchdog.pet();
        Timer::after_millis(200).await;
    }
}

/// config rcc for higher sysclock and fdcan periph clock to make sure
/// all messages can be received without package drop
fn get_rcc_config() -> rcc::Config {
    let mut rcc_config = rcc::Config::default();
    rcc_config.hsi = true;
    rcc_config.sys = rcc::Sysclk::PLL1_R;
    rcc_config.pll = Some(rcc::Pll {
        source: rcc::PllSource::HSI,
        prediv: rcc::PllPreDiv::DIV1,
        mul: rcc::PllMul::MUL8,
        divp: None,
        divq: Some(rcc::PllQDiv::DIV2),
        divr: Some(rcc::PllRDiv::DIV2),
    });
    rcc_config.mux.fdcansel = Fdcansel::PLL1_Q;
    rcc_config
}

/// program entry
#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = Config::default();
    config.rcc = get_rcc_config();
    let p = embassy_stm32::init(config);
    info!("Launching");
    
    // independent watchdog with timeout 300 MS
    let mut watchdog = IndependentWatchdog::new(p.IWDG, 300_000);
    watchdog.unleash();

    // -- CAN configuration
    let mut rodos_can_configurator = RodosCanInterface::new(
        CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs),
        RODOS_DEVICE_ID,
    );

    rodos_can_configurator
        .set_bitrate(1_000_000)
        .add_receive_topic(RODOS_REC_TOPIC_ID, None).unwrap();

    let (can_reader, can_sender, _active_instance) = rodos_can_configurator.split_buffered::<4, RODOS_MAX_RAW_MSG_LEN, TX_BUF_SIZE, RX_BUF_SIZE>(
        TX_BUF.init(TxBuf::<TX_BUF_SIZE>::new()),
        RX_BUF.init(RxBuf::<RX_BUF_SIZE>::new()),
    );

    // set can standby pin to low
    let _can_standby = Output::new(p.PA10, Level::Low, Speed::Low);

    // -- Uart configuration
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    //let (uart_tx, uart_rx) = Uart::new_with_rtscts(
    //    p.USART6,
    //    p.PA5,
    //    p.PA4,
    //    Irqs,
    //    p.PA7,
    //    p.PA6,
    //    p.DMA1_CH1,
    //    p.DMA1_CH2,
    //    uart_config,
    //)
    //.unwrap()
    //.split();
    
    let (uart_tx, uart_rx) = Uart::new(p.USART5,
        p.PB4, p.PB3,
        Irqs,
        p.DMA1_CH1, p.DMA1_CH2,
        uart_config).unwrap().split();

    join3(sender(can_reader, uart_tx), receiver(can_sender, uart_rx), petter(watchdog)).await;
}
