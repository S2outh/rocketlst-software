#![no_std]
#![no_main]

use rodos_can_interface::{RodosCanInterface, receiver::RodosCanReceiver, sender::RodosCanSender};
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_stm32::{
    bind_interrupts, can::{self, CanConfigurator, RxBuf, TxBuf}, gpio::{Level, Output, Speed}, mode::Async, peripherals::*, rcc::{self, mux::Fdcansel}, usart::{self, Uart, UartRx, UartTx}, Config
};
use embedded_io_async::Write;
use heapless::Vec;

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

const RODOS_DEVICE_ID: u8 = 0x01;
const RODOS_REC_TOPIC_ID: u16 = 4000;
const RODOS_SND_TOPIC_ID: u16 = 4001;

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
                let number_of_bytes = frame.data()[256];

                let header = [
                    0x22, 0x69,                          // Uart start bytes
                    number_of_bytes + 6,                 // packet length (+6 for remaining header)
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
                packet.extend_from_slice(&frame.data()[..number_of_bytes as usize]).unwrap();

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
    let mut buffer: [u8; 256] = [0; 256];
    loop {
        match uart.read_until_idle(&mut buffer).await {
            Ok(len) => {
                const HEADER_LEN: usize = 9;
                const TELECMD_MAX_LEN: usize = 32;

                if len <= HEADER_LEN {
                    // incomplete msg
                    continue;
                }
                
                let mut rodos_buffer: [u8; TELECMD_MAX_LEN] = [0; TELECMD_MAX_LEN];
                rodos_buffer[..(TELECMD_MAX_LEN-HEADER_LEN)].copy_from_slice(&buffer[HEADER_LEN..]);
                rodos_buffer[TELECMD_MAX_LEN - 1] = (len - HEADER_LEN) as u8;

                if let Err(e) = can.send(RODOS_SND_TOPIC_ID, &rodos_buffer).await {
                    error!("could not send frame via can: {}", e);
                }
            }
            Err(e) => {
                error!("could not receive uart frame: {}", e);
            }
        }
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

    // -- CAN configuration
    let (can_reader, can_sender, _active_instance) = RodosCanInterface::new(
        CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs),
        TX_BUF.init(TxBuf::<TX_BUF_SIZE>::new()),
        RX_BUF.init(RxBuf::<RX_BUF_SIZE>::new()),
        1_000_000,
        RODOS_DEVICE_ID,
        &[(RODOS_REC_TOPIC_ID, None)], // Some(0x46)
    )
    .split::<4, 257>();

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
    
    // let (uart_tx, uart_rx) = Uart::new(p.USART6,
    //     p.PA5, p.PA4,
    //     Irqs,
    //     p.DMA1_CH1, p.DMA1_CH2,
    //     uart_config).unwrap().split();

    let (uart_tx, uart_rx) = Uart::new(p.USART5,
        p.PB4, p.PB3,
        Irqs,
        p.DMA1_CH1, p.DMA1_CH2,
        uart_config).unwrap().split();


    join(sender(can_reader, uart_tx), receiver(can_sender, uart_rx)).await;
}
