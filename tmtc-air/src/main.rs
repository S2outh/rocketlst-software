#![no_std]
#![no_main]

mod lst_sender;
mod lst_receiver;

use core::cmp::min;

use embassy_time::Timer;
use rodos_can_interface::{RodosCanInterface, receiver::RodosCanReceiver, sender::RodosCanSender};
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_stm32::{
    bind_interrupts, can::{self, CanConfigurator, RxBuf, TxBuf}, gpio::{Level, Output, Speed}, peripherals::*, rcc::{self, mux::Fdcansel}, usart::{self, Uart}, wdg::IndependentWatchdog, Config
};

use {defmt_rtt as _, panic_probe as _};

use static_cell::StaticCell;

use lst_sender::{LSTSender, LSTCmd};
use lst_receiver::{LSTReceiver, LSTMessage};

const RODOS_DEVICE_ID: u8 = 0x01;

#[repr(u16)]
enum TopicId {
    RawSend = 1000,
    RawRecv = 1001,
    Cmd = 1100,
    TelemReq = 1103,
    TelemUptime = 1420,
    TelemRssi = 1421,
    TelemLQI = 1422,
    TelemPacketsSend = 1423,
    TelemPacketsGood = 1424,
    TelemPacketsBadChecksum = 1425,
    TelemPacketsBadOther = 1426,
}

const LST_CMD_SUBSYS_ID: u8 = 0x00;

const RODOS_MAX_RAW_MSG_LEN: usize = 247;
const RODOS_TC_MSG_LEN: usize = 19; // 16 byte payload + subsys id, cmd id, pl len

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
async fn sender<const NOS: usize, const MPL: usize>(mut can: RodosCanReceiver<NOS, MPL>, mut lst: LSTSender<'static>) {

    const RODOS_TM_REQ_TOPIC_ID: u16 = TopicId::TelemReq as u16;
    const RODOS_TM_TOPIC_ID: u16 = TopicId::RawSend as u16;
    const RODOS_CMD_TOPIC_ID: u16 = TopicId::Cmd as u16;

    loop {

        // receive from can
        match can.receive().await {
            Ok(frame) => {
                match frame.topic() {
                    RODOS_TM_TOPIC_ID => {
                        let rodos_msg_len = frame.data()[0] as usize; // the first byte is the
                                                                      // number of actually used
                                                                      // bytes

                        info!("send: {}", rodos_msg_len);

                        if let Err(e) = lst.send(&frame.data()[1..rodos_msg_len+1]).await {
                            error!("could not send via lsp {}", e);
                        }
                    },
                    RODOS_CMD_TOPIC_ID => {
                        if frame.data().len() < 3 {
                            error!("bad cmd received");
                            continue;
                        }
                        // match subsystem id
                        if frame.data()[0] != LST_CMD_SUBSYS_ID {
                            continue;
                        }
                        // match cmd
                        match frame.data()[1] {
                            0x00 => lst.send_cmd(LSTCmd::Reboot).await.unwrap_or_else(|e| {
                                error!("could not execute cmd: {}", e);
                            }),
                            _ => error!("unknown cmd"),
                        }
                    }
                    RODOS_TM_REQ_TOPIC_ID => {
                        if let Err(e) = lst.send_cmd(LSTCmd::GetTelem).await {
                            error!("could not send cmd {}", e);
                        }
                    },
                    _ => {}
                }
            }
            Err(e) => error!("error in can frame! {}", e),
        };
    }
}

/// receive data from RocketLST and transmit via can
async fn receiver(mut can: RodosCanSender, mut lst: LSTReceiver<'static>) {
    let mut buffer = [0u8; 256];
    loop {
        match lst.receive(&mut buffer).await {
            Ok(msg) => {
                match msg {
                    LSTMessage::Relay(range) => {
                        let rodos_msg_len = min(RODOS_TC_MSG_LEN, range.len());
                        
                        info!("received: {}", rodos_msg_len);
                        // add msg len to rodos msg
                        let rodos_msg = &mut buffer[range.start - 1 .. range.start + rodos_msg_len];
                        rodos_msg[0] = rodos_msg_len as u8;

                        // otherwise send it via can to the rest of the system
                        if let Err(e) = can.send(TopicId::RawRecv as u16, rodos_msg).await {
                            error!("could not send frame via can: {}", e);
                        }


                    }
                    LSTMessage::Telem(telemetry) => {
                        info!("telem {}", telemetry);
                        let _ = can.send(TopicId::TelemUptime as u16, &telemetry.uptime.to_le_bytes()).await;
                        let _ = can.send(TopicId::TelemRssi as u16, &telemetry.rssi.to_le_bytes()).await;
                        let _ = can.send(TopicId::TelemLQI as u16, &telemetry.lqi.to_le_bytes()).await;
                        let _ = can.send(TopicId::TelemPacketsSend as u16, &telemetry.packets_sent.to_le_bytes()).await;
                        let _ = can.send(TopicId::TelemPacketsGood as u16, &telemetry.packets_good.to_le_bytes()).await;
                        let _ = can.send(TopicId::TelemPacketsBadChecksum as u16, &telemetry.packets_rejected_checksum.to_le_bytes()).await;
                        let _ = can.send(TopicId::TelemPacketsBadOther as u16, &telemetry.packets_rejected_other.to_le_bytes()).await;
                    }
                    LSTMessage::Ack => info!("ack :)"),
                    LSTMessage::Nack => info!("nack :("),
                    LSTMessage::Unknown(cmd) => error!("unknown lst msg received: {}", cmd),
                }
            }
            Err(e) => {
                error!("could receive from lst: {}", e);
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
        .add_receive_topic(TopicId::RawSend as u16, None).unwrap()
        .add_receive_topic(TopicId::Cmd as u16, None).unwrap()
        .add_receive_topic(TopicId::TelemReq as u16, None).unwrap();

    let (can_reader, can_sender, _active_instance) = rodos_can_configurator
        .activate::<4, RODOS_MAX_RAW_MSG_LEN, TX_BUF_SIZE, RX_BUF_SIZE>(
        TX_BUF.init(TxBuf::<TX_BUF_SIZE>::new()),
        RX_BUF.init(RxBuf::<RX_BUF_SIZE>::new()),
    ).split_buffered();

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

    let lst_tx = LSTSender::new(uart_tx);
    let lst_rx = LSTReceiver::new(uart_rx);

    join3(sender(can_reader, lst_tx), receiver(can_sender, lst_rx), petter(watchdog)).await;
}
