use core::sync::atomic::Ordering;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};

use defmt::*;
use embassy_stm32::{
    can::{BufferedFdCanReceiver, BufferedFdCanSender, frame::FdFrame},
    crc::Crc,
    gpio::Output,
    mode::Async,
    usart::{RingBufferedUartRx, UartTx},
};
use embassy_time::{Duration, Instant, Ticker, with_timeout};
use openlst_driver::{
    lst_receiver::{LSTMessage, LSTReceiver, LSTTelemetry},
    lst_sender::{LSTCmd, LSTSender},
};
use portable_atomic::{AtomicU8, AtomicU64};
use south_common::{
    beacons::LSTBeacon,
    definitions::{internal_msgs, telemetry as tm},
    chell::{Beacon, BeaconOperationError, ChellValue, ChellDefinition}, types::Timesync,
};

/// Request a timesync frame every N seconds
const TIMESYNC_REQ_ID: u8 = if cfg!(feature = "primary") { 1 } else { 2 };
static REQ_TIME: AtomicU64 = AtomicU64::new(0);
static REQ_ANS_PRIO: AtomicU8 = AtomicU8::new(0);
static TIME_REF: AtomicU64 = AtomicU64::new(0);
#[embassy_executor::task]
pub async fn can_sender_thread(mut can_sender: BufferedFdCanSender) {
    const REQ_INTERVALL: Duration = Duration::from_secs(10);
    let mut ticker = Ticker::every(REQ_INTERVALL);
    loop {
        let frame = FdFrame::new_standard(internal_msgs::TimesyncRequest.id(), core::slice::from_ref(&TIMESYNC_REQ_ID)).unwrap();
        REQ_TIME.store(Instant::now().as_micros(), Ordering::Release);
        REQ_ANS_PRIO.store(u8::MAX, Ordering::Release);
        can_sender.write(frame).await;
        ticker.next().await;
    }
}

fn update_time_ref(frame: &FdFrame) {
    match Timesync::read(frame.data()) {
        Ok((_len, timesync_answer)) => {
            if timesync_answer.request_id != TIMESYNC_REQ_ID || timesync_answer.priority >= REQ_ANS_PRIO.load(Ordering::Acquire) {
                return;
            }
            REQ_ANS_PRIO.store(timesync_answer.priority, Ordering::Release);
            let transfer_time = Instant::now().as_micros() - REQ_TIME.load(Ordering::Acquire);
            let time_ref = timesync_answer.unix_time + transfer_time / 2 - Instant::now().as_micros();
            info!("Time ref is now {}", time_ref);
            TIME_REF.store(time_ref, Ordering::Relaxed);
        },
        Err(e) => error!("could not read timesync msg {}", Debug2Format(&e)),
    }
}
async fn insert_beacon(
    beacons: &'static [&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>],
    def: &dyn ChellDefinition,
    frame: &FdFrame
) {
    for beacon in beacons {
        if let Err(e) = beacon
            .lock()
            .await
            .insert_slice(def, frame.data())
        {
            match e {
                BeaconOperationError::DefNotInBeacon => (),
                BeaconOperationError::OutOfMemory => {
                    error!("received incomplete value: {}", def.address());
                }
            }
        }
    }
}

/// receive can messages and put them in the corresponding beacons
#[embassy_executor::task]
pub async fn can_receiver_thread(
    beacons: &'static [&'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>],
    can: BufferedFdCanReceiver,
    mut led: Output<'static>,
) {
    loop {
        // receive from can
        match can.receive().await {
            Ok(envelope) => {
                if let embedded_can::Id::Standard(id) = envelope.frame.id() {
                    led.toggle();
                    if let Ok(def) = internal_msgs::from_id(id.as_raw()) {
                        if def.as_any().is::<internal_msgs::TimesyncAnswer>() {
                            update_time_ref(&envelope.frame);
                        }
                    }
                    else if let Ok(def) = tm::from_id(id.as_raw()) {
                        insert_beacon(beacons, def, &envelope.frame).await;
                    }
                    else {
                        defmt::unreachable!("id not in any chell def block")
                    }
                } else {
                    defmt::unreachable!()
                };
            }
            Err(e) => error!("error in can frame! {}", e),
        };
    }
}

/// send a beacon to the rocketlst with a specific intervall
#[embassy_executor::task(pool_size = 5)]
pub async fn lst_sender_thread(
    send_intervall: Duration,
    beacon: &'static Mutex<ThreadModeRawMutex, dyn Beacon<Timestamp = u64>>,
    crc: &'static Mutex<ThreadModeRawMutex, Crc<'static>>,
    lst: &'static Mutex<ThreadModeRawMutex, LSTSender<UartTx<'static, Async>>>,
) {
    let mut ticker = Ticker::every(send_intervall);
    loop {
        {
            let mut beacon = beacon.lock().await;
            let timestamp = TIME_REF.load(Ordering::Relaxed) + Instant::now().as_micros();
            beacon.set_timestamp(timestamp);

            debug!("sending beacon: {}", beacon.name());

            let bytes = {
                let mut crc = crc.lock().await;
                crc.reset();
                let mut crc_func = |bytes: &[u8]| {
                    crc.feed_bytes(bytes); crc.read() as u16
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
                LSTMessage::Unknown(a) => debug!("unknown: {}", a),
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
        if let Ok(tm) = with_timeout(LST_TM_TIMEOUT, wait_for_telem(&mut lst_recv)).await {
            debug!("received lst telem msg: {}", tm);
            let mut lst_beacon = lst_beacon.lock().await;
            lst_beacon.uptime = Some(tm.uptime);
            lst_beacon.rssi = Some(tm.rssi);
            lst_beacon.lqi = Some(tm.lqi);
            lst_beacon.packets_sent = Some(tm.packets_sent);
            lst_beacon.packets_good = Some(tm.packets_good);
            lst_beacon.packets_rejected_checksum = Some(tm.packets_rejected_checksum);
            lst_beacon.packets_rejected_other = Some(tm.packets_rejected_other);
        } else {
            error!("lst did not answer");
        }
        ticker.next().await;
    }
}
