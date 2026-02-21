use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_stm32::{
    can::BufferedFdCanReceiver,
    crc::Crc,
    mode::Async,
    usart::{RingBufferedUartRx, UartTx},
};
use embassy_time::{Duration, Instant, Ticker, with_timeout};
use openlst_driver::{
    lst_receiver::{LSTMessage, LSTReceiver},
    lst_sender::{LSTCmd, LSTSender},
};
use south_common::{
    beacons::LSTBeacon,
    definitions::telemetry as tm,
    tmtc_system::{Beacon, BeaconOperationError},
};

/// send a beacon to the rocketlst with a specific intervall
#[embassy_executor::task(pool_size = 5)]
pub async fn lst_sender_thread(
    send_intervall: Duration,
    beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>,
    crc: &'static Mutex<ThreadModeRawMutex, Crc<'static>>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>,
) {
    let mut ticker = Ticker::every(send_intervall);
    loop {{
            let mut beacon = beacon.lock().await;
            beacon.set_timestamp(Instant::now().as_millis());

            info!("sending beacon: {}", beacon.name());

            let bytes = {
                let mut crc = crc.lock().await;
                crc.reset();
                let mut crc_func = |bytes: &[u8]| crc.feed_bytes(bytes) as u16;
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

/// receive can messages and put them in the corresponding beacons
#[embassy_executor::task]
pub async fn can_receiver_thread(
    beacons: &'static [&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>],
    can: BufferedFdCanReceiver,
) {
    loop {
        // receive from can
        match can.receive().await {
            Ok(envelope) => {
                if let embedded_can::Id::Standard(id) = envelope.frame.id() {
                    for beacon in beacons {
                        if let Err(e) = beacon
                            .lock()
                            .await
                            .insert_slice(tm::from_id(id.as_raw()).unwrap(), envelope.frame.data())
                        {
                            match e {
                                BeaconOperationError::DefNotInBeacon => (),
                                BeaconOperationError::OutOfMemory => {
                                    error!("received incomplete value: {}", id.as_raw());
                                }
                            }
                        }
                    }
                } else {
                    defmt::unreachable!()
                };
            }
            Err(e) => error!("error in can frame! {}", e),
        };
    }
}

/// access lst telemetry
#[embassy_executor::task]
pub async fn telemetry_thread(
    lst_beacon: &'static Mutex<ThreadModeRawMutex, LSTBeacon>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>,
    mut lst_recv: LSTReceiver<RingBufferedUartRx<'static>>,
) {
    const LST_TM_INTERVAL: Duration = Duration::from_secs(10);
    const LST_TM_TIMEOUT: Duration = Duration::from_millis(200);
    let mut ticker = Ticker::every(LST_TM_INTERVAL);
    loop {
        lst.lock()
            .await
            .cmd(LSTCmd::GetTelem)
            .await
            .unwrap_or_else(|e| error!("could not send cmd to lst: {}", e));
        if let Ok(lst_answer) = with_timeout(LST_TM_TIMEOUT, lst_recv.receive()).await {
            match lst_answer {
                Ok(msg) => match msg {
                    LSTMessage::Telem(tm) => {
                        info!("received lst telem msg: {}", tm);
                        let mut lst_beacon = lst_beacon.lock().await;
                        lst_beacon.uptime = Some(tm.uptime);
                        lst_beacon.rssi = Some(tm.rssi);
                        lst_beacon.lqi = Some(tm.lqi);
                        lst_beacon.packets_sent = Some(tm.packets_sent);
                        lst_beacon.packets_good = Some(tm.packets_good);
                        lst_beacon.packets_rejected_checksum = Some(tm.packets_rejected_checksum);
                        lst_beacon.packets_rejected_other = Some(tm.packets_rejected_other);
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
            error!("lst did not answer");
        }
        ticker.next().await;
    }
}
