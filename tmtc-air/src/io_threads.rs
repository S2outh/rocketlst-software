use embassy_futures::select::{Either, select};

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_stm32::{
    can::BufferedFdCanReceiver, crc::Crc, usart::{BufferedUartRx, BufferedUartTx}
};
use embassy_time::{Instant, Timer};
use openlst_driver::{lst_receiver::{LSTMessage, LSTReceiver}, lst_sender::{LSTCmd, LSTSender}};
use south_common::{
    Beacon, BeaconOperationError, LSTBeacon, telemetry as tm
};

/// take a beacon, add necessary headers and relay to RocketLST via uart
#[embassy_executor::task(pool_size = 3)]
pub async fn lst_sender_thread(
    send_intervall: u64,
    beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon>,
    crc: &'static Mutex<ThreadModeRawMutex, Crc<'static>>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<BufferedUartTx<'static>>>,
) {
    loop {
        info!("sending beacon");
        {
            let mut beacon = beacon.lock().await;
            beacon.insert_slice(&tm::Timestamp, &(Instant::now().as_millis() as i64).to_le_bytes()).unwrap();

            let bytes = {
                let mut crc = crc.lock().await;
                crc.reset();
                let mut crc_func = |bytes: &[u8]| crc.feed_bytes(bytes) as u16;
                beacon.to_bytes(&mut crc_func)
            };

            if let Err(e) = lst.lock().await.send(bytes).await {
                error!("could not send via lsp: {}", e);
            }
            beacon.flush();
        }
        Timer::after_millis(send_intervall).await;
    }
}

/// receive can messages and put them in the corresponding beacons
#[embassy_executor::task]
pub async fn can_receiver_thread(
    beacons: &'static [&'static Mutex<ThreadModeRawMutex, dyn Beacon>],
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
                            .insert_slice(tm::from_id(id.as_raw()).unwrap(), envelope.frame.data()) {
                            match e {
                                BeaconOperationError::DefNotInBeacon => (),
                                BeaconOperationError::OutOfMemory => {
                                    error!("received incomplete value");
                                },
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
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<BufferedUartTx<'static>>>,
    mut lst_recv: LSTReceiver<BufferedUartRx<'static>>,
) {
    const LST_TM_INTERVAL_MS: u64 = 10_000;
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
                        lst_beacon.uptime = Some(tm.uptime);
                        lst_beacon.rssi = Some(tm.rssi);
                        lst_beacon.lqi = Some(tm.lqi);
                        lst_beacon.packets_send = Some(tm.packets_sent);
                        lst_beacon.packets_good = Some(tm.packets_good);
                        lst_beacon.packets_bad_checksum = Some(tm.packets_rejected_checksum);
                        lst_beacon.packets_bad_other = Some(tm.packets_rejected_other);
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
        
        Timer::after_millis(LST_TM_INTERVAL_MS).await;
    }
}
