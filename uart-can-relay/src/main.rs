#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    can::{self, Can, CanConfigurator, frame::Frame},
    gpio::{Level, Output, Speed},
    peripherals::*,
};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
});

#[embassy_executor::task]
async fn test_can(mut can: Can<'static>) {

    loop {
        let frame = Frame::new_standard(0x321, &[0xBE, 0xEF, 0xDE, 0xAD]).unwrap(); // test data to be send
        info!("writing frame");
        can.write(&frame).await;

        match can.read().await {
            Ok(envelope) => info!(
                "received {:x} {:x} {:x} {:x}",
                envelope.frame.data()[0],
                envelope.frame.data()[1],
                envelope.frame.data()[2],
                envelope.frame.data()[3]
            ), // print received data
            Err(_) => error!("error in frame!"),
        };

        Timer::after_millis(250).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Launching");

    let mut can_config = CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs);

    can_config.set_bitrate(500_000); //to be ajusted
    
    // set standby pin to low
    let _can_standby = Output::new(p.PA10, Level::Low, Speed::Low);

    spawner
        .spawn(test_can(can_config.into_normal_mode()))
        .unwrap();

    let mut led = Output::new(p.PA2, Level::High, Speed::Low);

    loop {
        led.set_high();
        Timer::after_millis(1000).await;

        led.set_low();
        Timer::after_millis(1000).await;
    }
}
