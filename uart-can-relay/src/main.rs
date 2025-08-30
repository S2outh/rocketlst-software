#![no_std]
#![no_main]

mod rodos_can_relay;

use crate::rodos_can_relay::{RodosCanRelay, receiver::RodosCanReceiver, sender::RodosCanSender};
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_stm32::{
    Config, bind_interrupts,
    can::{self, CanConfigurator},
    gpio::{Level, Output, Speed},
    mode::Async,
    peripherals::*,
    rcc::{self, mux::Fdcansel},
    usart::{self, Uart, UartRx, UartTx},
};
use embedded_io_async::Write;
use heapless::Vec;

use {defmt_rtt as _, panic_probe as _};

const RODOS_DEVICE_ID: u8 = 0x01;
const RODOS_TOPIC_ID: u16 = 0x01;

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
    USART3_4_5_6_LPUART1 => usart::InterruptHandler<USART6>;
});

/// take can telemetry frame, add necessary headers and relay to RocketLST via uart
async fn sender<const NOS: usize, const MPL: usize>(mut can: RodosCanReceiver<NOS, MPL>, mut uart: UartTx<'static, Async>) {
    let mut seq_num: u16 = 0;
    loop {
        match can.receive().await {
            Ok(frame) => {
                info!("received smt");
                let header = [
                    0x22, 0x69,                          // Uart start bytes
                    frame.data().len() as u8 + 6,        // packet length
                    0x00, 0x01,                          // Hardware ID
                    (seq_num >> 8) as u8, seq_num as u8, // SeqNum
                    0x11,                                // Destination
                ];
                seq_num = seq_num.wrapping_add(1);

                let _ = frame.topic();
                let _ = frame.device();

                let mut packet: Vec<u8, 254> = Vec::new(); // max openlst data length
                packet.extend_from_slice(&header).unwrap();
                packet.extend_from_slice(frame.data()).unwrap();

                if let Err(e) = uart.write_all(&packet).await {
                    error!("dropped frames: {}", e)
                }
            }
            Err(e) => error!("error in frame! {}", e),
        };
    }
}

/// receive data from RocketLST and transmit via can
/// TODO update to filter out eventual header data from the RocketLST
async fn receiver(mut can: RodosCanSender, mut uart: UartRx<'static, Async>) {
    let mut buffer: [u8; 254] = [0; 254];
    loop {
        match uart.read_until_idle(&mut buffer).await {
            Ok(len) => {
                if let Err(e) = can.send(RODOS_TOPIC_ID, &buffer[..len]).await {
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
    let (can_reader, can_sender, _active_instance) = RodosCanRelay::new(
        CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs),
        1_000_000,
        RODOS_DEVICE_ID,
        &[(0x0FA0, None)], // Some(0x46)
    )
    .split::<2, 246>();

    // set can standby pin to low
    let _can_standby = Output::new(p.PA10, Level::Low, Speed::Low);

    // -- Uart configuration
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let (uart_tx, uart_rx) = Uart::new_with_rtscts(
        p.USART6,
        p.PA5,
        p.PA4,
        Irqs,
        p.PA7,
        p.PA6,
        p.DMA1_CH1,
        p.DMA1_CH2,
        uart_config,
    )
    .unwrap()
    .split();
    // let (uart_tx, _uart_rx) = Uart::new(p.USART6,
    //     p.PA5, p.PA4,
    //     Irqs,
    //     p.DMA1_CH1, p.DMA1_CH2,
    //     uart_config).unwrap().split();

    join(sender(can_reader, uart_tx), receiver(can_sender, uart_rx)).await;
}
