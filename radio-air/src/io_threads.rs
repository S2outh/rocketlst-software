use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_stm32::{
    can::frame::FdEnvelope,
    crc::Crc,
    mode::Async,
    usart::{RingBufferedUartRx, UartTx},
};
use embassy_time::{Duration, Ticker, with_timeout};
use openlst_driver::{
    lst_receiver::{LSTMessage, LSTReceiver, LSTTelemetry},
    lst_sender::{LSTCmd, LSTSender},
};
use south_common::{
    beacons::LSTBeacon,
    chell::{Beacon, BeaconOperationError, ChellDefinition},
    definitions::telemetry::lst as tm,
    obdh::OnTMFunc,
    types::LSTCommand,
};

use crate::{
    LstCanReceiver, LstCanSender, LstComChannels, LstTCReceiver, LstTMContainer, LstTMSender,
};

pub struct BeaconIngress {
    beacons: &'static [&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>],
}
impl BeaconIngress {
    pub fn new(
        beacons: &'static [&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>],
    ) -> Self {
        Self { beacons }
    }
}
impl OnTMFunc for BeaconIngress {
    async fn call(&self, def: &dyn ChellDefinition, envelope: &FdEnvelope) {
        for beacon in self.beacons {
            if let Err(e) = beacon.lock().await.insert_slice(def, envelope.frame.data()) {
                match e {
                    BeaconOperationError::DefNotInBeacon => (),
                    BeaconOperationError::OutOfMemory => {
                        error!("received incomplete value: {}", def.address());
                    }
                }
            }
        }
    }
}

/// send a beacon to the rocketlst with a specific intervall
#[embassy_executor::task(pool_size = 6)]
pub async fn lst_sender_thread(
    send_intervall: Duration,
    com_channels: &'static LstComChannels,
    beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>,
    crc: &'static Mutex<ThreadModeRawMutex, Crc<'static>>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>,
) {
    let mut ticker = Ticker::every(send_intervall);
    loop {
        {
            let mut beacon = beacon.lock().await;
            let timestamp = com_channels.get_utc_us();
            beacon.set_timestamp(timestamp);

            debug!("sending beacon: {}", beacon.name());

            let bytes = {
                let mut crc = crc.lock().await;
                crc.reset();
                let mut crc_func = |bytes: &[u8]| {
                    crc.feed_bytes(bytes);
                    crc.read() as u16
                };
                beacon.to_bytes(&mut crc_func)
            };

            if let Err(e) = lst.lock().await.relay(bytes).await {
                error!("could not send via lsp: {}", e);
            }
            beacon.flush();
        }
        ticker.next().await;
    }
}

async fn wait_for_telem(lst_recv: &mut LSTReceiver<RingBufferedUartRx<'static>>) -> LSTTelemetry {
    loop {
        match lst_recv.receive().await {
            Ok(msg) => match msg {
                LSTMessage::Telem(tm) => return tm,
                LSTMessage::Ack => debug!("ack"),
                LSTMessage::Nack => debug!("nack"),
                LSTMessage::Unknown(a, b) => debug!("unknown, cmd: {}, data: {}", a, b),
                LSTMessage::Relay(_) => debug!("relay"),
            },
            Err(e) => {
                error!("could not receive from lst: {}", e);
            }
        }
    }
}

/// access lst telemetry
#[embassy_executor::task]
pub async fn lst_telemetry_thread(
    lst_beacon: &'static Mutex<ThreadModeRawMutex, LSTBeacon>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>,
    mut lst_recv: LSTReceiver<RingBufferedUartRx<'static>>,
    tm_sender: LstTMSender,
) {
    const LST_TM_INTERVAL: Duration = Duration::from_secs(10);
    const LST_TM_TIMEOUT: Duration = Duration::from_millis(1000);
    let mut ticker = Ticker::every(LST_TM_INTERVAL);
    loop {
        lst.lock()
            .await
            .cmd(LSTCmd::GetTelem)
            .await
            .unwrap_or_else(|e| error!("could not send cmd to lst: {}", e));
        if let Ok(lst_tm) = with_timeout(LST_TM_TIMEOUT, wait_for_telem(&mut lst_recv)).await {
            debug!("received lst telem msg: {}", lst_tm);
            let mut lst_beacon = lst_beacon.lock().await;

            macro_rules! process_tm {
                ($($val:ident),*) => { paste::paste!{ $(
                    let container = LstTMContainer::new(&tm::$val, &lst_tm.[<$val: snake>]).unwrap();
                    tm_sender.send(container).await;

                    lst_beacon.[<$val: snake>] = Some(lst_tm.[<$val: snake>]);
                )* } };
            }

            process_tm!(
                Uptime,
                Rssi,
                Lqi,
                PacketsSent,
                PacketsGood,
                PacketsRejectedChecksum,
                PacketsRejectedOther
            );
        } else {
            error!("lst did not answer");
        }
        ticker.next().await;
    }
}

#[embassy_executor::task]
pub async fn command_execution_task(
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>,
    tc_receiver: LstTCReceiver,
) {
    loop {
        match tc_receiver.receive().await {
            LSTCommand::Reboot => {
                if let Err(e) = lst.lock().await.cmd(LSTCmd::Reboot).await {
                    error!("could not reboot: {}", e);
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn can_receiver_task(mut can_receiver: LstCanReceiver) -> ! {
    can_receiver.run().await
}

#[embassy_executor::task]
pub async fn can_sender_task(mut can_sender: LstCanSender) -> ! {
    can_sender.run().await
}
