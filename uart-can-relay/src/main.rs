#![no_std]
#![no_main]

mod rodos_can_relay;

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    can::{self, CanConfigurator},
    gpio::{Level, Output, Speed},
    mode::Async,
    peripherals::*,
    rcc::{self, mux::Fdcansel},
    usart::{self, Uart, UartTx}, Config
};
use heapless::Vec;
use embassy_time::Timer;
use embedded_io_async::Write;
use crate::rodos_can_relay::{receiver::RodosCanReceiver, RodosCanRelay};

use {defmt_rtt as _, panic_probe as _};

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
    USART3_4_5_6_LPUART1 => usart::InterruptHandler<USART6>;
});

#[embassy_executor::task]
async fn sender(mut can: RodosCanReceiver<16, 246>, mut uart: UartTx<'static, Async>) {

    let mut seq_num: u16 = 0;
    loop {
        match can.receive().await {
            Ok(frame) => {
                let header = [
                    0x22, 0x69, // Uart start bytes
                    frame.data().len() as u8 + 6, // packet length
                    0x00, 0x01, // Hardware ID
                    (seq_num >> 8) as u8, seq_num as u8, // SeqNum
                    0x11 // Destination
                ];
                seq_num = seq_num.wrapping_add(1);

                let mut packet: Vec<u8, 254> = Vec::new(); // max openlst data length
                packet.extend_from_slice(&header).unwrap();
                packet.extend_from_slice(frame.data()).unwrap();

                if let Err(e) = uart.write_all(&packet).await {
                    error!("dropped frames: {}", e)
                }
            }
            Err(_) => error!("error in frame!"),
        };
    }
}

// async fn receiver(mut can: CanTx<'static>, mut uart: UartRx<'static, Async>) {
//     // TODO
//     loop {
//         let frame = Frame::new_standard(0x321, &[0xBE, 0xEF, 0xDE, 0xAD]).unwrap(); // test data to be send
//         info!("writing frame");
//         can.write(&frame).await;
//     }
// }
//


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
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc = get_rcc_config();
    let p = embassy_stm32::init(config);
    info!("Launching");

    // -- CAN configuration
    let (can_reader, _can_sender, _active_instance) = RodosCanRelay::new(
        CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs),
        1_000_000,
        &[(0x00, None)],
        ).split::<16, 246>();

    // set can standby pin to low
    let _can_standby = Output::new(p.PA10, Level::Low, Speed::Low);

    // -- Uart configuration
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    // let (uart_tx, _uart_rx) = Uart::new_with_rtscts(p.USART6,
    //     p.PA5, p.PA4,
    //     Irqs,
    //     p.PA7, p.PA6,
    //     p.DMA1_CH1, p.DMA1_CH2,
    //     config).unwrap().split()
    let (uart_tx, _uart_rx) = Uart::new(p.USART6,
        p.PA5, p.PA4,
        Irqs,
        p.DMA1_CH1, p.DMA1_CH2,
        uart_config).unwrap().split();


    spawner.must_spawn(sender(can_reader, uart_tx));

    let mut led = Output::new(p.PA2, Level::High, Speed::Low);

    loop {
        led.set_high();
        Timer::after_millis(1000).await;

        led.set_low();
        Timer::after_millis(1000).await;
    }
}
