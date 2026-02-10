#![no_std]
#![no_main]

mod io_threads;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    Config, bind_interrupts, can::{self, CanConfigurator, RxFdBuf, TxFdBuf}, crc::{self, Crc}, gpio::{Level, Output, Speed}, mode::Async, peripherals::*, rcc::{self, mux::Fdcansel}, usart::{self, Uart, UartTx}, wdg::IndependentWatchdog
};
use embassy_time::{Timer, Duration};
use south_common::{
    Beacon, LSTBeacon, EPSBeacon, SensorboardBeacon, can_config::CanPeriphConfig, telemetry as tm
};

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

use openlst_driver::lst_receiver::LSTReceiver;
use openlst_driver::lst_sender::LSTSender;

// General setup stuff
const STARTUP_DELAY: u64 = 1000;
const OPENLST_HWID: u16 = 0x2DEC;
const NUM_RECV_BEC: usize = 2;

const LST_BEACON_INTERVAL: Duration = Duration::from_secs(10);
const EPS_BEACON_INTERVAL: Duration = Duration::from_secs(1);
const SENSORBOARD_BEACON_INTERVAL: Duration = Duration::from_millis(100);

const WATCHDOG_TIMEOUT_US: u32 = 300_000;
const WATCHDOG_PETTING_INTERVAL_US: u32 = WATCHDOG_TIMEOUT_US / 2;

// Static beacon allocation
static LRB: StaticCell<Mutex<ThreadModeRawMutex, LSTBeacon>> = StaticCell::new();
static MRB: StaticCell<Mutex<ThreadModeRawMutex, EPSBeacon>> = StaticCell::new();
static HRB: StaticCell<Mutex<ThreadModeRawMutex, SensorboardBeacon>> = StaticCell::new();

static BL: StaticCell<[&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>; NUM_RECV_BEC]> = StaticCell::new();

// Static peripheral allocation
static LST: StaticCell<Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>> =
    StaticCell::new();
static CRC: StaticCell<Mutex<ThreadModeRawMutex, Crc>> = StaticCell::new();

// Static can buffer
const C_RX_BUF_SIZE: usize = 512;
const C_TX_BUF_SIZE: usize = 32;

static C_RX_BUF: StaticCell<RxFdBuf<C_RX_BUF_SIZE>> = StaticCell::new();
static C_TX_BUF: StaticCell<TxFdBuf<C_TX_BUF_SIZE>> = StaticCell::new();

// Static uart buffer
const S_RX_BUF_SIZE: usize = 256;
static S_RX_BUF: StaticCell<[u8; S_RX_BUF_SIZE]> = StaticCell::new();

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
    USART3_4_5_6_LPUART1 => usart::InterruptHandler<USART5>;
});

/// Watchdog petting task
#[embassy_executor::task]
async fn petter(mut watchdog: IndependentWatchdog<'static, IWDG>) {
    loop {
        watchdog.pet();
        Timer::after_micros(WATCHDOG_PETTING_INTERVAL_US.into()).await;
    }
}

/// config rcc for higher sysclock and fdcan periph clock to make sure
/// all messages can be received without package drop
fn get_rcc_config() -> rcc::Config {
    let mut rcc_config = rcc::Config::default();
    rcc_config.hsi = Some(rcc::Hsi {
        sys_div: rcc::HsiSysDiv::DIV1,
    });
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

/// get CRC configuration for crc16_ccitt
fn get_crc_config() -> crc::Config {
    crc::Config::new(
        crc::InputReverseConfig::None,
        false,
        crc::PolySize::Width16,
        0xFFFF,
        0x1021,
    )
    .unwrap()
}

/// program entry
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc = get_rcc_config();
    let p = embassy_stm32::init(config);
    info!("Launching");

    // unleash independent watchdog
    let mut watchdog = IndependentWatchdog::new(p.IWDG, WATCHDOG_TIMEOUT_US);
    watchdog.unleash();

    // can configuration
    let mut can_configurator =
        CanPeriphConfig::new(CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs));

    can_configurator
        .add_receive_topic_range(tm::id_range())
        .unwrap();

    let can_instance = can_configurator.activate(
        C_TX_BUF.init(TxFdBuf::<C_TX_BUF_SIZE>::new()),
        C_RX_BUF.init(RxFdBuf::<C_RX_BUF_SIZE>::new()),
    );

    // set can standby pin to low
    let _can_standby = Output::new(p.PA10, Level::Low, Speed::Low);

    // -- Uart configuration
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;

    let (uart_tx, uart_rx) = Uart::new(
        p.USART5,
        p.PB4,
        p.PB3,
        Irqs,
        p.DMA1_CH1,
        p.DMA1_CH2,
        uart_config,
    )
    .unwrap()
    .split();

    let lst_tx = LST.init(Mutex::new(LSTSender::new(uart_tx, OPENLST_HWID)));
    let lst_rx = LSTReceiver::new(uart_rx.into_ring_buffered(S_RX_BUF.init([0; _])));

    // -- CRC setup
    let crc = CRC.init(Mutex::new(Crc::new(p.CRC, get_crc_config())));

    // -- Beacons
    let lst_beacon = LRB.init(Mutex::new(LSTBeacon::new()));
    let eps_beacon = MRB.init(Mutex::new(EPSBeacon::new()));
    let sensorboard_beacon = HRB.init(Mutex::new(SensorboardBeacon::new()));

    let receivable_beacons = BL.init([eps_beacon, sensorboard_beacon]);

    // Startup
    spawner.must_spawn(petter(watchdog));
    spawner.must_spawn(io_threads::can_receiver_thread(receivable_beacons, can_instance.reader()));

    // LST sender startup
    Timer::after_millis(STARTUP_DELAY).await;
    spawner.must_spawn(io_threads::telemetry_thread(lst_beacon, lst_tx, lst_rx));
    spawner.must_spawn(io_threads::lst_sender_thread(LST_BEACON_INTERVAL, lst_beacon, crc, lst_tx));
    spawner.must_spawn(io_threads::lst_sender_thread(EPS_BEACON_INTERVAL, eps_beacon, crc, lst_tx));
    spawner.must_spawn(io_threads::lst_sender_thread(SENSORBOARD_BEACON_INTERVAL, sensorboard_beacon, crc, lst_tx));

    core::future::pending::<()>().await;
}
