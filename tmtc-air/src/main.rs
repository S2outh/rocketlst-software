#![no_std]
#![no_main]

use embassy_futures::select::{Either, select};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    Config, bind_interrupts,
    can::{self, BufferedFdCanReceiver, CanConfigurator, RxFdBuf, TxFdBuf},
    crc::{self, Crc},
    gpio::{Level, Output, Speed},
    peripherals::*,
    rcc::{self, mux::Fdcansel},
    usart::{self, BufferedUart, BufferedUartTx, BufferedUartRx},
    wdg::IndependentWatchdog,
};
use embassy_time::{Timer, Instant};
use south_common::{
    Beacon, BeaconOperationError, HighRateTelemetry, LowRateTelemetry, MidRateTelemetry, TMValue, can_config::CanPeriphConfig, telemetry as tm
};

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

use openlst_driver::lst_receiver::{LSTMessage, LSTReceiver};
use openlst_driver::lst_sender::{LSTCmd, LSTSender};

// General setup stuff
const STARTUP_DELAY: u64 = 1000;
const OPENLST_HWID: u16 = 1;

// Static object allocation
static LRB: StaticCell<Mutex<ThreadModeRawMutex, LowRateTelemetry>> = StaticCell::new();
static MRB: StaticCell<Mutex<ThreadModeRawMutex, MidRateTelemetry>> = StaticCell::new();
static HRB: StaticCell<Mutex<ThreadModeRawMutex, HighRateTelemetry>> = StaticCell::new();
static LST: StaticCell<Mutex<ThreadModeRawMutex, LSTSender<BufferedUartTx<'static>>>> =
    StaticCell::new();
static CRC: StaticCell<Mutex<ThreadModeRawMutex, Crc>> = StaticCell::new();

// Can static buffer
const C_RX_BUF_SIZE: usize = 500;
const C_TX_BUF_SIZE: usize = 30;

static C_RX_BUF: StaticCell<RxFdBuf<C_RX_BUF_SIZE>> = StaticCell::new();
static C_TX_BUF: StaticCell<TxFdBuf<C_TX_BUF_SIZE>> = StaticCell::new();

// Uart static buffer
const S_RX_BUF_SIZE: usize = 1024;
const S_TX_BUF_SIZE: usize = 256;

static S_RX_BUF: StaticCell<[u8; S_RX_BUF_SIZE]> = StaticCell::new();
static S_TX_BUF: StaticCell<[u8; S_TX_BUF_SIZE]> = StaticCell::new();

// bin can interrupts
bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
    USART3_4_5_6_LPUART1 => usart::BufferedInterruptHandler<USART5>;
});

/// take a beacon, add necessary headers and relay to RocketLST via uart
#[embassy_executor::task(pool_size = 3)]
async fn lst_sender_thread(
    send_intervall: u64,
    beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon>,
    crc: &'static Mutex<ThreadModeRawMutex, Crc<'static>>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<BufferedUartTx<'static>>>,
) {
    loop {
        info!("sending beacon");
        {
            let mut beacon = beacon.lock().await;
            beacon.insert_slice(&tm::Timestamp, &Instant::now().as_millis().to_bytes()).unwrap();

            let bytes = {
                let mut crc = crc.lock().await;
                crc.reset();
                let mut crc_func = |bytes: &[u8]| crc.feed_bytes(bytes) as u16;
                beacon.bytes(&mut crc_func)
            };

            if let Err(e) = lst.lock().await.send(bytes).await {
                error!("could not send via lsp: {}", e);
            }
        }
        Timer::after_millis(send_intervall).await;
    }
}

macro_rules! beacon_insert {
    ($beacon:ident, $id:ident, $envelope:ident) => {
        if let Err(e) = $beacon
            .lock()
            .await
            .insert_slice(tm::from_id($id.as_raw()).unwrap(), $envelope.frame.data()) {
                match e {
                    BeaconOperationError::DefNotInBeacon => (),
                    BeaconOperationError::OutOfMemory => {
                        error!("received incomplete value");
                    },
                }
            }
    };
}
// receive can messages and put them in the corresponding beacons
#[embassy_executor::task]
async fn can_receiver_thread(
    mid_rate_beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon>,
    high_rate_beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon>,
    can: BufferedFdCanReceiver,
) {
    loop {
        // receive from can
        match can.receive().await {
            Ok(envelope) => {
                if let embedded_can::Id::Standard(id) = envelope.frame.id() {
                    beacon_insert!(mid_rate_beacon, id, envelope);
                    beacon_insert!(high_rate_beacon, id, envelope);
                } else {
                    defmt::unreachable!()
                };
            }
            Err(e) => error!("error in can frame! {}", e),
        };
    }
}

// access lst telemetry
#[embassy_executor::task]
async fn telemetry_thread(
    lst_beacon: &'static Mutex<ThreadModeRawMutex, LowRateTelemetry>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<BufferedUartTx<'static>>>,
    mut lst_recv: LSTReceiver<BufferedUartRx<'static>>,
) {
    const LST_TM_INTERVALL_MS: u64 = 10_000;
    const LST_TM_TIMEOUT_MS: u64 = 3_000;
    loop {
        lst.lock()
            .await
            .send_cmd(LSTCmd::GetTelem)
            .await
            .unwrap_or_else(|e| error!("could not send cmd to lst: {}", e));
        let answer = select(
            lst_recv.receive(),
            Timer::after_millis(LST_TM_TIMEOUT_MS)
        ).await;
        if let Either::First(lst_answer) = answer {
            match lst_answer {
                Ok(msg) => match msg {
                    LSTMessage::Telem(tm) => {
                        info!("received lst telem msg: {}", tm);
                        let mut lst_beacon = lst_beacon.lock().await;
                        lst_beacon.uptime = tm.uptime;
                        lst_beacon.rssi = tm.rssi;
                        lst_beacon.lqi = tm.lqi;
                        lst_beacon.packets_send = tm.packets_sent;
                        lst_beacon.packets_good = tm.packets_good;
                        lst_beacon.packets_bad_checksum = tm.packets_rejected_checksum;
                        lst_beacon.packets_bad_other = tm.packets_rejected_other;
                    }
                    LSTMessage::Ack => info!("ack"),
                    LSTMessage::Nack => info!("nack"),
                    LSTMessage::Unknown(a) => info!("unknown: {}", a),
                    LSTMessage::Relay(_) => info!("relay"),
                },
                Err(e) => {
                    error!("could not receive from lst: {}", e);
                }
            }
        } else {
            lst_recv.reset();
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

    // independent watchdog with timeout 300 MS
    let mut watchdog = IndependentWatchdog::new(p.IWDG, 300_000);
    watchdog.unleash();

    // -- CAN configuration
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

    let (uart_tx, uart_rx) = BufferedUart::new(
        p.USART5,
        p.PB4,
        p.PB3,
        S_TX_BUF.init([0u8; S_TX_BUF_SIZE]),
        S_RX_BUF.init([0u8; S_RX_BUF_SIZE]),
        Irqs,
        uart_config,
    )
    .unwrap()
    .split();

    let lst_tx = LST.init(Mutex::new(LSTSender::new(uart_tx, OPENLST_HWID)));
    let lst_rx = LSTReceiver::new(uart_rx);

    // -- CRC setup
    let crc = CRC.init(Mutex::new(Crc::new(p.CRC, get_crc_config())));

    // -- Beacons
    let low_rate_beacon = LRB.init(Mutex::new(LowRateTelemetry::new()));
    let mid_rate_beacon = MRB.init(Mutex::new(MidRateTelemetry::new()));
    let high_rate_beacon = HRB.init(Mutex::new(HighRateTelemetry::new()));

    // Startup
    spawner.must_spawn(petter(watchdog));
    spawner.must_spawn(can_receiver_thread(mid_rate_beacon, high_rate_beacon, can_instance.reader()));

    // LST sender startup
    Timer::after_millis(STARTUP_DELAY).await;
    spawner.must_spawn(telemetry_thread(low_rate_beacon, lst_tx, lst_rx));
    spawner.must_spawn(lst_sender_thread(10_000, low_rate_beacon, crc, lst_tx));
    spawner.must_spawn(lst_sender_thread(1_000, mid_rate_beacon, crc, lst_tx));
    spawner.must_spawn(lst_sender_thread(100, high_rate_beacon, crc, lst_tx));

    core::future::pending::<()>().await;
}
