#![no_std]
#![no_main]

#![feature(const_trait_impl)]
#![feature(const_cmp)]
#![feature(never_type)]

mod macros;
mod ground_tm_defs;
mod nats;

use core::{convert::Infallible, net::SocketAddr};

use cortex_m::peripheral::SCB;

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{
    IpEndpoint, Stack, StackResources,
    dns::DnsQueryType,
    tcp::{self, TcpSocket},
    udp::{PacketMetadata, UdpSocket},
};
use embassy_stm32::{Config, bind_interrupts, eth::{self, Ethernet, GenericPhy, PacketQueue, Sma}, mode::Async, peripherals::{ETH, ETH_SMA, IWDG1, RNG, USART2}, rcc, rng::{self, Rng}, time::mhz, usart::{self, Uart, UartTx}, wdg::IndependentWatchdog};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::{Channel, DynamicReceiver, DynamicSender}};
use embassy_time::{Duration, Instant, Ticker, Timer, with_timeout};
use openlst_driver::{lst_receiver::{LSTMessage, LSTReceiver, LSTTelemetry}, lst_sender::{LSTCmd, LSTSender}};
use static_cell::StaticCell;

use crate::nats::{NatsCon, NatsRunner, NatsStack};

use {defmt_rtt as _, panic_probe as _};

use south_common::{
    beacons::{LSTBeacon, EPSBeacon, HighRateUpperSensorBeacon, LowRateUpperSensorBeacon, LowerSensorBeacon},
    tmtc_system::{Beacon, ParseError, ground_tm::{Serializer, SerializableTMValue}}
};

// General setup stuff
const WATCHDOG_TIMEOUT_US: u32 = 300_000;
const WATCHDOG_PETTING_INTERVAL_US: u32 = WATCHDOG_TIMEOUT_US / 2;

// Heap setup
const HEAP_KB: usize = 64;

#[global_allocator]
static ALLOCATOR: emballoc::Allocator<{HEAP_KB * 1024}> = emballoc::Allocator::new();
extern crate alloc;
use alloc::vec::Vec;

// lst setup
const OPENLST_HWID: u16 = 0x2DEC;

// Serialized value channel
const MSG_CHANNEL_BUF_SIZE: usize = 30;

type SerializedInfo = (&'static str, Vec<u8>);

static MSG: StaticCell<Channel<ThreadModeRawMutex, SerializedInfo, MSG_CHANNEL_BUF_SIZE>> =
    StaticCell::new();

// Static uart buffer
const S_RX_BUF_SIZE: usize = 256;
static S_RX_BUF: StaticCell<[u8; S_RX_BUF_SIZE]> = StaticCell::new();

// Ethernet
// queues for raw packets before and after processing
static PACKET_QUEUE: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
// resources to hold the sockets used by the net driver. One for DHCP, one for DNS and one for TCP
static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
// buffer sizes for tcp data before and after processing
const TCP_RX_BUF_SIZE: usize = 1024;
static TCP_RX_BUF: StaticCell<[u8; TCP_RX_BUF_SIZE]> = StaticCell::new();

const TCP_TX_BUF_SIZE: usize = 1024;
static TCP_TX_BUF: StaticCell<[u8; TCP_TX_BUF_SIZE]> = StaticCell::new();

// mac address. hardcoded for now
const MAC_ADDR: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

// NATS
// const NATS_ADDR: &str = "10.42.0.1";
const NATS_ADDR: &str = "nats.tichygames.de";
static NATS_STACK: StaticCell<NatsStack<'static>> = StaticCell::new();

// internet time sync (NTP)
const NTP_ADDR: &str = "pool.ntp.org";
const NTP_PORT: u16 = 123;
const NTP_UNIX_EPOCH_DIFF_SECS: u64 = 2_208_988_800;

type EthDevice = Ethernet<'static, ETH, GenericPhy<Sma<'static, ETH_SMA>>>;

// bin can interrupts
bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
    RNG => rng::InterruptHandler<RNG>;

    USART2 => usart::InterruptHandler<USART2>;
    //USART3 => usart::InterruptHandler<USART3>;
});

#[derive(Debug)]
pub enum GSTError {
    ConnectNATS(tcp::ConnectError),
    SubscribeNATS,
    SerialError(usart::Error),
}

fn get_rcc_config() -> rcc::Config {
    let mut rcc_config = rcc::Config::default();
    rcc_config.hsi = Some(rcc::HSIPrescaler::DIV1); // 64 MHz
    rcc_config.hsi48 = Some(Default::default()); // needed for RNG

    // pll to multiply clock cyclUART1_ENABLEDes
    rcc_config.pll1 = Some(rcc::Pll {
        source: rcc::PllSource::HSI,
        prediv: rcc::PllPreDiv::DIV8,   // 8 MHz
        mul: rcc::PllMul::MUL40,        // 320 MHz
        divp: Some(rcc::PllDiv::DIV2),  // 160 MHz
        divq: Some(rcc::PllDiv::DIV2),  // 160 MHz
        divr: Some(rcc::PllDiv::DIV5),  // 64 MHz
    });
    rcc_config.sys = rcc::Sysclk::PLL1_P; // cpu runs with 160 MHz
    rcc_config.mux.fdcansel = rcc::mux::Fdcansel::PLL1_Q; // can runs with 160 MHz
    rcc_config.voltage_scale = rcc::VoltageScale::Scale1; // voltage scale for max 225 MHz

    rcc_config.apb1_pre = rcc::APBPrescaler::DIV2; // APB 1-4 all run with 80 MHz due to hardware limits
    rcc_config.apb2_pre = rcc::APBPrescaler::DIV2;
    rcc_config.apb3_pre = rcc::APBPrescaler::DIV2;
    rcc_config.apb4_pre = rcc::APBPrescaler::DIV2;

    rcc_config
}

fn crc_ccitt(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

struct CborSerializer;
impl Serializer for CborSerializer {
    type Error = minicbor_serde::error::EncodeError<Infallible>;
    fn serialize_value<T: serde::Serialize>(&self, value: &T)
        -> Result<alloc::vec::Vec<u8>, Self::Error> {
        minicbor_serde::to_vec(value)
    }
}

/// Watchdog petting task
#[embassy_executor::task]
async fn petter(mut watchdog: IndependentWatchdog<'static, IWDG1>) {
    loop {
        watchdog.pet();
        Timer::after_micros(WATCHDOG_PETTING_INTERVAL_US.into()).await;
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, EthDevice>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn nats_task(mut runner: NatsRunner<'static>) -> ! {
    runner.run().await.unwrap_or_else(|_| SCB::sys_reset())
}

#[embassy_executor::task]
async fn sender_task(mut nats_client: NatsCon<'static>, receiver: DynamicReceiver<'static, SerializedInfo>) {
    loop {
        let (address, bytes) = receiver.receive().await;
        if let Err(e) = nats_client.publish(address, bytes).await {
            error!("lost connection to NATS server: {:?}", e);
            SCB::sys_reset();
        }
    }
}

#[embassy_executor::task]
async fn telemetry_request_thread(mut lst_sender: LSTSender<UartTx<'static, Async>>) {
    const LST_TM_INTERVALL: Duration = Duration::from_secs(1);
    let mut ticker = Ticker::every(LST_TM_INTERVALL);
    loop {
        ticker.next().await;
        if let Err(e) = lst_sender.cmd(LSTCmd::GetTelem).await {
            error!("could not send cmd over serial: {}", e);
        }
    }
}

async fn local_lst_telemetry(
    nats_sender: &DynamicSender<'static, SerializedInfo>,
    tm: LSTTelemetry,
    unix_time_offset_us: i64,
) {

    let timestamp = current_unix_time_micros(unix_time_offset_us);

    info!("Received local lst Telemetry at {}", timestamp);

    print_lst_values!(tm, (
        Rssi,
        Lqi,
        PacketsGood,
        PacketsRejectedChecksum,
        PacketsRejectedOther
    ));

    pub_lst_values!(nats_sender, tm, timestamp, (
        Uptime,
        Rssi,
        Lqi,
        PacketsSent,
        PacketsGood,
        PacketsRejectedChecksum,
        PacketsRejectedOther
    ));
}

fn current_unix_time_micros(offset: i64) -> u64 {
    let now = Instant::now().as_micros();

    if offset >= 0 {
        now.saturating_add(offset as u64)
    } else {
        now.saturating_sub((-offset) as u64)
    }
}

fn ntp_packet_to_unix_micros(packet: &[u8]) -> Option<u64> {
    if packet.len() < 48 {
        return None;
    }

    let secs = u32::from_be_bytes([packet[40], packet[41], packet[42], packet[43]]) as u64;
    let frac = u32::from_be_bytes([packet[44], packet[45], packet[46], packet[47]]) as u64;
    if secs < NTP_UNIX_EPOCH_DIFF_SECS {
        return None;
    }

    let unix_secs = secs - NTP_UNIX_EPOCH_DIFF_SECS;
    let micros_from_frac = (frac * 1_000_000) >> 32;
    Some(unix_secs.saturating_mul(1_000_000).saturating_add(micros_from_frac))
}

async fn try_sync_internet_time(stack: &Stack<'_>) -> Result<i64, &'static str> {
    let ips = stack
        .dns_query(NTP_ADDR, DnsQueryType::A)
        .await
        .map_err(|_| "dns")?;
    let Some(ip) = ips.first() else {
        return Err("dns-empty");
    };

    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buf = [0u8; 96];
    let mut tx_buf = [0u8; 96];
    let mut socket = UdpSocket::new(*stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(0).map_err(|_| "bind")?;

    let endpoint = IpEndpoint::new((*ip).into(), NTP_PORT);
    let mut request = [0u8; 48];
    request[0] = 0x23; // LI=0, VN=4, Mode=3 (client)

    let t0 = Instant::now().as_micros();
    socket.send_to(&request, endpoint).await.map_err(|_| "send")?;

    let mut response = [0u8; 48];
    let (len, _src) = with_timeout(Duration::from_secs(3), socket.recv_from(&mut response))
        .await
        .map_err(|_| "timeout")?
        .map_err(|_| "recv")?;
    let t1 = Instant::now().as_micros();

    let Some(mut unix_us) = ntp_packet_to_unix_micros(&response[..len]) else {
        return Err("parse");
    };

    unix_us = unix_us.saturating_add((t1 - t0) / 2);
    let offset = unix_us as i64 - t1 as i64;
    Ok(offset)
}

async fn sync_internet_time(stack: &Stack<'_>) -> i64 {
    for attempt in 1..=10 {
        match try_sync_internet_time(stack).await {
            Ok(offset) => {
                info!("internet time synced on attempt {}", attempt);
                return offset;
            }
            Err(e) => {
                warn!("internet time sync failed ({}): {}", attempt, e);
                Timer::after_secs(2).await;
            }
        }
    }

    warn!("internet time unavailable, using monotonic fallback");
    0
}

pub async fn parse_or_resolve(
       stack: &Stack<'_>,
       s: &str,
   ) -> Result<SocketAddr, embassy_net::dns::Error> {
   if let Ok(sa) = s.parse::<SocketAddr>() {
       return Ok(sa);
   }

   let ips = stack.dns_query(s, DnsQueryType::A).await?;
   let Some(ip) = ips.first() else {
       return Err(embassy_net::dns::Error::Failed);
   };
   Ok(SocketAddr::new((*ip).into(), 4222))
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc = get_rcc_config();
    let p = embassy_stm32::init(config);
    info!("Launching");

    // unleash independent watchdog
    let mut watchdog = IndependentWatchdog::new(p.IWDG1, WATCHDOG_TIMEOUT_US);
    watchdog.unleash();

    // Initialize UART and LST
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    
    let (uart_tx, uart_rx) = Uart::new(
        p.USART2,
        p.PA3,
        p.PD5,
        Irqs,
        p.DMA1_CH1,
        p.DMA1_CH2,
        uart_config,
    )
    .unwrap()
    .split();

    
    // let (uart_tx, uart_rx) = Uart::new(
    //     p.USART3,
    //     p.PD9,
    //     p.PB10,
    //     Irqs,
    //     p.DMA1_CH1,
    //     p.DMA1_CH2,
    //     uart_config,
    // )
    // .unwrap()
    // .split();

    let lst_tx = LSTSender::new(uart_tx, OPENLST_HWID);
    let mut lst_rx = LSTReceiver::new(uart_rx.into_ring_buffered(S_RX_BUF.init([0; _])));

    // Initialize ethernet
    let eth_int = p.ETH;
    let ref_clk = p.PA1;
    let mdio = p.PA2;
    let mdc = p.PC1;
    let crs = p.PA7;
    let rx_d0 = p.PC4;
    let rx_d1 = p.PC5;
    let tx_d0 = p.PB12;
    let tx_d1 = p.PB13;
    let tx_en = p.PB11;
    let sma = p.ETH_SMA;

    info!("Creating Ethernet device...");

    let device = Ethernet::new(
        PACKET_QUEUE.init(PacketQueue::<4, 4>::new()),
        eth_int,
        Irqs,
        ref_clk,
        crs,
        rx_d0,
        rx_d1,
        tx_d0,
        tx_d1,
        tx_en,
        MAC_ADDR,
        sma,
        mdio,
        mdc,
    );

    let net_cfg = embassy_net::Config::dhcpv4(Default::default());

    // Generate random seed.
    let mut rng = Rng::new(p.RNG, Irqs);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    let seed = u64::from_le_bytes(seed);
    
    // Initialize network stack
    info!("Initializing network task");
    let (stack, runner) = embassy_net::new(device, net_cfg, RESOURCES.init(StackResources::new()), seed);

    // Launch watchdog task
    spawner.must_spawn(petter(watchdog));

    // Launch network task
    spawner.must_spawn(net_task(runner));

    stack.wait_config_up().await;

    info!("Stack initialized");
    info!("Network initialized");

    let unix_time_offset_us = sync_internet_time(&stack).await;

    // Initizlize Nats socket
    let client = TcpSocket::new(stack, TCP_RX_BUF.init([0; _]), TCP_TX_BUF.init([0; _]));

    // resolve addr
    let socket_addr = loop {
        match parse_or_resolve(&stack, NATS_ADDR).await {
            Ok(addr) => break addr,
            Err(e) => {
                warn!("could not resolve nats addr: {:?}, retrying...", e);
                Timer::after_secs(2).await;
            }
        }
    };
    let nats = NATS_STACK.init(NatsStack::new(client, socket_addr));

    // nats connection
    let (nats_client, nats_runner) = match nats.connect_with_default()
        .await.map_err(GSTError::ConnectNATS) {
        Ok(nats_stack) => {
            info!("NATS succesfully connected to NATS server");
            nats_stack
        },
        Err(e) => defmt::panic!("Could not connect to NATS server: {}", Debug2Format(&e)),
    };

    // Initialize beacons
    let mut lst_beacon = LSTBeacon::new();
    let mut eps_beacon = EPSBeacon::new();
    let mut high_rate_upper_beacon = HighRateUpperSensorBeacon::new();
    let mut low_rate_upper_beacon = LowRateUpperSensorBeacon::new();
    let mut lower_sensor_beacon = LowerSensorBeacon::new();

    let channel = MSG.init(Channel::new());

    // launch local lst periodic telemetry request
    spawner.must_spawn(telemetry_request_thread(lst_tx));
    // launch nats sending thread
    spawner.must_spawn(sender_task(nats_client, channel.dyn_receiver()));
    spawner.must_spawn(nats_task(nats_runner));

    // receiving main loop
    loop {
        match lst_rx.receive().await {
            Ok(msg) => {
                match msg {
                    LSTMessage::Relay(data) => {
                        parse_beacon!(data, lst_beacon, channel, (packets_sent));
                        parse_beacon!(data, eps_beacon, channel, (bat1_voltage));
                        parse_beacon!(data, high_rate_upper_beacon, channel, (imu1_accel));
                        parse_beacon!(data, low_rate_upper_beacon, channel, (gps_pos));
                        parse_beacon!(data, lower_sensor_beacon, channel);
                    },
                    LSTMessage::Telem(tm) => {
                        local_lst_telemetry(&channel.dyn_sender(), tm, unix_time_offset_us).await;
                    },
                    LSTMessage::Ack => info!("LST Ack"),
                    LSTMessage::Nack => info!("LST Nack"),
                    LSTMessage::Unknown(a) => info!("LST Unknown: {}", a),
                }
            },
            Err(e) => {
                error!("error in receiving frame: {:?}", e);
            }
        }
    }
}
