#![no_std]
#![no_main]

mod lst_sender;
mod lst_receiver;
mod can_config;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use tmtc_definitions::telemetry as tm;

use embassy_time::Timer;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    Config, bind_interrupts, can::{self, BufferedFdCanReceiver, CanConfigurator, RxFdBuf, TxFdBuf}, gpio::{Level, Output, Speed}, peripherals::*, rcc::{self, mux::Fdcansel}, usart::{self, Uart}, wdg::IndependentWatchdog
};
use tmtc_definitions::{DynBeacon, LowRateTelemetry, MidRateTelemetry};


use crate::can_config::CanPeriphConfig;

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

use lst_sender::{LSTSender, LSTCmd};
use lst_receiver::{LSTReceiver, LSTMessage};

// General setup stuff
const STARTUP_DELAY: u64 = 1000;

// Static beacon allocation
static LRB: StaticCell<Mutex<ThreadModeRawMutex, LowRateTelemetry>> = StaticCell::new();
static MRB: StaticCell<Mutex<ThreadModeRawMutex, MidRateTelemetry>> = StaticCell::new();
static LST: StaticCell<Mutex<ThreadModeRawMutex, LSTSender>> = StaticCell::new();

// Can setup stuff
const RX_BUF_SIZE: usize = 500;
const TX_BUF_SIZE: usize = 30;

static RX_BUF: StaticCell<RxFdBuf<RX_BUF_SIZE>> = StaticCell::new();
static TX_BUF: StaticCell<TxFdBuf<TX_BUF_SIZE>> = StaticCell::new();

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
    USART3_4_5_6_LPUART1 => usart::InterruptHandler<USART5>;
});

/// take a beacon, add necessary headers and relay to RocketLST via uart
#[embassy_executor::task(pool_size = 2)]
async fn lst_sender_thread(
    send_intervall: u64,
    beacon: &'static Mutex<ThreadModeRawMutex, dyn DynBeacon>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<'static>>) {

    loop {
        info!("sending beacon");
        
        if let Err(e) = lst.lock().await.send(beacon.lock().await.bytes()).await {
            error!("could not send via lsp {}", e);
        }
        Timer::after_millis(send_intervall).await;
    }
}

// receive can messages and put them in the corresponding beacons
#[embassy_executor::task]
async fn can_receiver_thread(
    mid_rate_beacon: &'static Mutex<ThreadModeRawMutex, dyn DynBeacon>,
    can: BufferedFdCanReceiver) {
    loop {
        // receive from can
        match can.receive().await {
            Ok(envelope) => {
                if let embedded_can::Id::Standard(id) = envelope.frame.id() {
                    mid_rate_beacon.lock().await.insert_slice(tm::from_id(id.as_raw()), envelope.frame.data()).unwrap();
                }
                else { defmt::unreachable!() };
            }
            Err(e) => error!("error in can frame! {}", e),
        };
    }
}

// access lst telemetry
#[embassy_executor::task]
async fn telemetry_thread(
    lst_beacon: &'static Mutex<ThreadModeRawMutex, dyn DynBeacon>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<'static>>,
    mut lst_recv: LSTReceiver<'static>) {
    const LST_TM_INTERVALL_MS: u64 = 10_000;
    let mut lst_buffer = [0u8; 64];
    loop {
        lst.lock().await.send_cmd(LSTCmd::GetTelem).await.unwrap_or_else(|e| error!("could not send cmd to lst: {}", e));
        loop {
            match lst_recv.receive(&mut lst_buffer).await {
                Ok(msg) => match msg {
                    LSTMessage::Telem(tm) => {
                        info!("received lst telem msg: {}", tm);
                        let mut lst_beacon = lst_beacon.lock().await;
                        lst_beacon.insert(&tm::lst::Uptime, &tm.uptime).unwrap();
                        lst_beacon.insert(&tm::lst::Rssi, &tm.rssi).unwrap();
                        lst_beacon.insert(&tm::lst::Lqi, &tm.lqi).unwrap();
                        lst_beacon.insert(&tm::lst::PacketsSend, &tm.packets_sent).unwrap();
                        lst_beacon.insert(&tm::lst::PacketsGood, &tm.packets_good).unwrap();
                        lst_beacon.insert(&tm::lst::PacketsBadChecksum, &tm.packets_rejected_checksum).unwrap();
                        lst_beacon.insert(&tm::lst::PacketsBadOther, &tm.packets_rejected_other).unwrap();
                        break;
                    }
                    _ => (), // ignore all other messages for now
                },
                Err(e) => {
                    error!("could not receive from lst: {}", e);
                    break;
                },
            }
        }
        Timer::after_millis(LST_TM_INTERVALL_MS).await;
    }
}

/// Watchdog petting task
#[embassy_executor::task]
async fn petter(mut watchdog: IndependentWatchdog<'static, IWDG>) {
    loop {
        watchdog.pet();
        Timer::after_millis(200).await;
    }
}

/// config rcc for higher sysclock and fdcan periph clock to make sure
/// all messages can be received without package drop
fn get_rcc_config() -> rcc::Config {
    let mut rcc_config = rcc::Config::default();
    rcc_config.hsi = Some(rcc::Hsi { sys_div: rcc::HsiSysDiv::DIV1 });
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
    
    // independent watchdog with timeout 300 MS
    let mut watchdog = IndependentWatchdog::new(p.IWDG, 300_000);
    watchdog.unleash();

    // -- CAN configuration
    let mut can_configurator = CanPeriphConfig::new(
        CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs),
    );

    can_configurator
        .add_receive_topic_range(tm::id_range()).unwrap();

    let can_instance = can_configurator.activate(
        TX_BUF.init(TxFdBuf::<TX_BUF_SIZE>::new()),
        RX_BUF.init(RxFdBuf::<RX_BUF_SIZE>::new()),
    );

    // set can standby pin to low
    let _can_standby = Output::new(p.PA10, Level::Low, Speed::Low);

    // -- Uart configuration
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    
    let (uart_tx, uart_rx) = Uart::new(p.USART5,
        p.PB4, p.PB3,
        Irqs,
        p.DMA1_CH1, p.DMA1_CH2,
        uart_config).unwrap().split();

    let lst_tx = LST.init(Mutex::new(LSTSender::new(uart_tx)));
    let lst_rx = LSTReceiver::new(uart_rx);

    // -- Beacons
    let low_rate_beacon = LRB.init(Mutex::new(LowRateTelemetry::new()));
    let mid_rate_beacon = MRB.init(Mutex::new(MidRateTelemetry::new()));

    // Startup
    spawner.must_spawn(petter(watchdog));
    spawner.must_spawn(can_receiver_thread(mid_rate_beacon, can_instance.reader()));

    // LST sender startup
    Timer::after_millis(STARTUP_DELAY).await;
    spawner.must_spawn(telemetry_thread(low_rate_beacon, lst_tx, lst_rx));
    spawner.must_spawn(lst_sender_thread(10_000, low_rate_beacon, lst_tx));
    spawner.must_spawn(lst_sender_thread(1_000, mid_rate_beacon, lst_tx));

    core::future::pending::<()>().await;
}
