#![no_std]
#![no_main]

mod io_threads;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    Config, bind_interrupts,
    can::{self, CanConfigurator, RxFdBuf, TxFdBuf},
    crc::{self, Crc},
    exti::{self, ExtiInput},
    gpio::{Level, Output, Pull, Speed},
    interrupt::typelevel::EXTI15_10,
    mode::Async,
    peripherals::*,
    rcc,
    usart::{self, Uart, UartTx},
    wdg::IndependentWatchdog,
};
use embassy_time::{Duration, Timer};
use south_common::{
    beacons::{
        EPSBeacon, HighRateUpperSensorBeacon, LSTBeacon, LowRateUpperSensorBeacon,
        LowerSensorBeacon,
    },
    can_config::CanPeriphConfig,
    definitions::telemetry as tm,
    tmtc_system::Beacon,
};

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

use openlst_driver::lst_receiver::LSTReceiver;
use openlst_driver::lst_sender::LSTSender;

// General setup stuff
const STARTUP_DELAY: u64 = 1000;
const OPENLST_HWID: u16 = 0x2DEC;
const NUM_RECV_BEC: usize = 4;

const LST_BEACON_INTERVAL: Duration = Duration::from_secs(10);
const EPS_BEACON_INTERVAL: Duration = Duration::from_secs(5);
const HIGH_RATE_UPPER_BEACON_INTERVAL: Duration = Duration::from_millis(100);
const LOW_RATE_UPPER_BEACON_INTERVAL: Duration = Duration::from_secs(1);
const LOWER_SENSOR_INTERVAL: Duration = Duration::from_secs(1);

const WATCHDOG_TIMEOUT_US: u32 = 300_000;
const WATCHDOG_PETTING_INTERVAL_US: u32 = WATCHDOG_TIMEOUT_US / 2;

// Static beacon allocation
static LTB: StaticCell<Mutex<ThreadModeRawMutex, LSTBeacon>> = StaticCell::new();
static ESB: StaticCell<Mutex<ThreadModeRawMutex, EPSBeacon>> = StaticCell::new();
static HUB: StaticCell<Mutex<ThreadModeRawMutex, HighRateUpperSensorBeacon>> = StaticCell::new();
static LUB: StaticCell<Mutex<ThreadModeRawMutex, LowRateUpperSensorBeacon>> = StaticCell::new();
static LSB: StaticCell<Mutex<ThreadModeRawMutex, LowerSensorBeacon>> = StaticCell::new();

static BL: StaticCell<
    [&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>; NUM_RECV_BEC],
> = StaticCell::new();

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
    //FDCAN1_IT0 => can::IT0InterruptHandler<FDCAN1>;
    //FDCAN1_IT1 => can::IT1InterruptHandler<FDCAN1>;

    FDCAN2_IT0 => can::IT0InterruptHandler<FDCAN2>;
    FDCAN2_IT1 => can::IT1InterruptHandler<FDCAN2>;

    //USART2 => usart::InterruptHandler<USART2>;
    USART3 => usart::InterruptHandler<USART3>;

    EXTI15_10 => exti::InterruptHandler<EXTI15_10>;
});

/// Watchdog petting task
#[embassy_executor::task]
async fn petter(mut watchdog: IndependentWatchdog<'static, IWDG1>) {
    loop {
        watchdog.pet();
        trace!("petter");
        Timer::after_micros(WATCHDOG_PETTING_INTERVAL_US.into()).await;
    }
}

/// CC feedback
#[embassy_executor::task(pool_size = 2)]
async fn cc_mode(mut pin: ExtiInput<'static>, mut led: Output<'static>) {
    loop {
        pin.wait_for_any_edge().await;
        led.set_level(pin.get_level());
    }
}

/// config rcc
fn get_rcc_config() -> rcc::Config {
    let mut rcc_config = rcc::Config::default();
    rcc_config.hsi = Some(rcc::HSIPrescaler::DIV1); // 64 MHz
    rcc_config.pll1 = Some(rcc::Pll {
        source: rcc::PllSource::HSI,
        prediv: rcc::PllPreDiv::DIV8,   // 8 MHz
        mul: rcc::PllMul::MUL40,        // 320 MHz
        divp: None,                     // Deactivated
        divq: Some(rcc::PllDiv::DIV5),  // 64 MHz
        divr: Some(rcc::PllDiv::DIV5),  // 64 MHz
    });
    rcc_config.sys = rcc::Sysclk::HSI; // cpu runs with 64 MHz
    rcc_config.mux.fdcansel = rcc::mux::Fdcansel::PLL1_Q; // can runs with 64 MHz
    rcc_config.voltage_scale = rcc::VoltageScale::Scale1; // voltage scale for max 225 MHz
    //rcc_config.apb1_pre = rcc::APBPrescaler::DIV2;
    //rcc_config.apb2_pre = rcc::APBPrescaler::DIV2;
    //rcc_config.apb3_pre = rcc::APBPrescaler::DIV2;
    //rcc_config.apb4_pre = rcc::APBPrescaler::DIV2;
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

    const FW_VERSION: &str = env!("FW_VERSION");
    const FW_HASH: &str = env!("FW_HASH");

    info!(
        "Launching: FW version={} hash={}",
        FW_VERSION,
        FW_HASH
    );

    // unleash independent watchdog
    let mut watchdog = IndependentWatchdog::new(p.IWDG1, WATCHDOG_TIMEOUT_US);
    watchdog.unleash();

    // can configuration
    let mut can_configurator =
    //    CanPeriphConfig::new(CanConfigurator::new(p.FDCAN1, p.PD0, p.PD1, Irqs));
        CanPeriphConfig::new(CanConfigurator::new(p.FDCAN2, p.PB5, p.PB6, Irqs));

    can_configurator
        .add_receive_topic_range(tm::id_range())
        .unwrap();

    let can_instance = can_configurator.activate(
        C_TX_BUF.init(TxFdBuf::<C_TX_BUF_SIZE>::new()),
        C_RX_BUF.init(RxFdBuf::<C_RX_BUF_SIZE>::new()),
    );

    // set can standby pin to low
    // let _can_standby = Output::new(p.PD7, Level::Low, Speed::Low);
    let _can_standby = Output::new(p.PB7, Level::Low, Speed::Low);

    // -- Uart configuration
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;

    //let (uart_tx, uart_rx) = Uart::new(
    //    p.USART2,
    //    p.PA3,
    //    p.PD5,
    //    Irqs,
    //    p.DMA1_CH1,
    //    p.DMA1_CH2,
    //    uart_config,
    //)
    //.unwrap()
    //.split();

    let (uart_tx, uart_rx) = Uart::new(
        p.USART3,
        p.PD9,
        p.PB10,
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
    let lst_beacon = LTB.init(Mutex::new(LSTBeacon::new()));
    let eps_beacon = ESB.init(Mutex::new(EPSBeacon::new()));
    let high_rate_upper_beacon = HUB.init(Mutex::new(HighRateUpperSensorBeacon::new()));
    let low_rate_upper_beacon = LUB.init(Mutex::new(LowRateUpperSensorBeacon::new()));
    let lower_sensor_beacon = LSB.init(Mutex::new(LowerSensorBeacon::new()));

    let receivable_beacons = BL.init([
        eps_beacon,
        high_rate_upper_beacon,
        low_rate_upper_beacon,
        lower_sensor_beacon,
    ]);

    // lst feedback pins
    let cc_rx_pin = ExtiInput::new(p.PD14, p.EXTI14, Pull::None, Irqs);
    let cc_tx_pin = ExtiInput::new(p.PD13, p.EXTI13, Pull::None, Irqs);

    let cc_rx_led = Output::new(p.PE7, Level::Low, Speed::Low);
    let cc_tx_led = Output::new(p.PE8, Level::Low, Speed::Low);

    // Startup
    spawner.must_spawn(petter(watchdog));
    spawner.must_spawn(io_threads::can_receiver_thread(
        receivable_beacons,
        can_instance.reader(),
    ));
    spawner.must_spawn(io_threads::telemetry_thread(lst_beacon, lst_tx, lst_rx));
    spawner.must_spawn(cc_mode(cc_rx_pin, cc_rx_led));
    spawner.must_spawn(cc_mode(cc_tx_pin, cc_tx_led));

    // LST sender startup
    Timer::after_millis(STARTUP_DELAY).await;
    spawner.must_spawn(io_threads::lst_sender_thread(
        LST_BEACON_INTERVAL,
        lst_beacon,
        crc,
        lst_tx,
    ));
    spawner.must_spawn(io_threads::lst_sender_thread(
        EPS_BEACON_INTERVAL,
        eps_beacon,
        crc,
        lst_tx,
    ));
    spawner.must_spawn(io_threads::lst_sender_thread(
        HIGH_RATE_UPPER_BEACON_INTERVAL,
        high_rate_upper_beacon,
        crc,
        lst_tx,
    ));
    spawner.must_spawn(io_threads::lst_sender_thread(
        LOW_RATE_UPPER_BEACON_INTERVAL,
        low_rate_upper_beacon,
        crc,
        lst_tx,
    ));
    spawner.must_spawn(io_threads::lst_sender_thread(
        LOWER_SENSOR_INTERVAL,
        lower_sensor_beacon,
        crc,
        lst_tx,
    ));

    core::future::pending::<()>().await;
}
