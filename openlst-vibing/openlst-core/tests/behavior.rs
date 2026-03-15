use openlst_core::commands::{handle_command, CommandAction, CommandContext};
use openlst_core::constants::{ESP_MAX_PAYLOAD, MAX_RX_TICKS, MSG_TYPE_RADIO_IN};
use openlst_core::hal::{Interface, RadioHal, RxPacket};
use openlst_core::protocol::{common, encode_command, radio, CommandHeader};
use openlst_core::rf::{decode_rf_message, encode_rf_message};
use openlst_core::runtime::{RadioRuntime, RuntimeConfig};
use openlst_core::schedule::Scheduler;
use openlst_core::telemetry::Telemetry;
use openlst_core::time::TimeSpec;

#[test]
fn get_time_nacks_when_rtc_not_set() {
    let ctx = CommandContext {
        hwid_flash: 0x1201,
        telemetry: Telemetry::default(),
        rtc_set: false,
        now: TimeSpec::default(),
        callsign: [0; 8],
        ranging_responder_enabled: true,
    };
    let input = CommandHeader {
        hwid: 0x1201,
        seqnum: 44,
        system: 1,
        command: radio::GET_TIME,
    };

    let reply = handle_command(input, &[], &ctx);
    assert_eq!(reply.header.command, common::NACK);
}

#[test]
fn set_time_acks_with_action() {
    let ctx = CommandContext {
        hwid_flash: 0x1201,
        telemetry: Telemetry::default(),
        rtc_set: false,
        now: TimeSpec::default(),
        callsign: [0; 8],
        ranging_responder_enabled: true,
    };
    let input = CommandHeader {
        hwid: 0x1201,
        seqnum: 44,
        system: 1,
        command: radio::SET_TIME,
    };

    let mut payload = [0u8; 8];
    payload[0..4].copy_from_slice(&10u32.to_le_bytes());
    payload[4..8].copy_from_slice(&20u32.to_le_bytes());

    let reply = handle_command(input, &payload, &ctx);
    assert_eq!(reply.header.command, common::ACK);
    assert_eq!(reply.action, Some(CommandAction::SetTime(TimeSpec { seconds: 10, nanoseconds: 20 })));
}

#[test]
fn scheduler_relisten_after_rx_timeout() {
    let mut scheduler = Scheduler::new(Some(100), MAX_RX_TICKS);
    scheduler.init(0, Some(1000));

    let mut relisten = false;
    for uptime in 0..(MAX_RX_TICKS * 100 + 10) {
        let events = scheduler.tick_1ms(uptime);
        if events.radio_relisten {
            relisten = true;
            break;
        }
    }

    assert!(relisten);
}

struct TestHal {
    hwid: u16,
    uptime: u32,
    rtc_set: bool,
    now: TimeSpec,
    callsign: [u8; 8],
    telemetry: Telemetry,
    rx: Option<RxPacket>,
    last_tx: Option<[u8; ESP_MAX_PAYLOAD]>,
    last_tx_len: usize,
    last_tx_interface: Option<Interface>,
    last_rf_precise: bool,
}

impl RadioHal for TestHal {
    fn hwid_flash(&self) -> u16 {
        self.hwid
    }

    fn watchdog_clear(&mut self) {}

    fn reboot_now(&mut self) {}

    fn uptime_seconds(&self) -> u32 {
        self.uptime
    }

    fn rtc_set(&self) -> bool {
        self.rtc_set
    }

    fn get_time(&self) -> TimeSpec {
        self.now
    }

    fn set_time(&mut self, time: TimeSpec) {
        self.now = time;
        self.rtc_set = true;
    }

    fn get_callsign(&self) -> [u8; 8] {
        self.callsign
    }

    fn set_callsign(&mut self, callsign: [u8; 8]) {
        self.callsign = callsign;
    }

    fn telemetry_snapshot(&self) -> Telemetry {
        self.telemetry
    }

    fn adc_start_sample(&mut self) {}

    fn update_telemetry(&mut self) {}

    fn radio_listen(&mut self) {}

    fn poll_rx(&mut self) -> Option<RxPacket> {
        self.rx.take()
    }

    fn send(&mut self, interface: Interface, payload: &[u8]) {
        self.store_tx(interface, payload, false);
    }

    fn send_rf(&mut self, payload: &[u8], _uart_sel: u8) {
        self.store_tx(Interface::Rf, payload, false);
    }

    fn send_rf_precise(&mut self, payload: &[u8], _uart_sel: u8) {
        self.store_tx(Interface::Rf, payload, true);
    }
}

impl TestHal {
    fn store_tx(&mut self, interface: Interface, payload: &[u8], precise: bool) {
        let mut buffer = [0u8; ESP_MAX_PAYLOAD];
        let len = payload.len().min(ESP_MAX_PAYLOAD);
        buffer[..len].copy_from_slice(&payload[..len]);
        self.last_tx = Some(buffer);
        self.last_tx_len = len;
        self.last_tx_interface = Some(interface);
        self.last_rf_precise = precise;
    }
}

#[test]
fn runtime_replies_to_addressed_command() {
    let header = CommandHeader {
        hwid: 0x2222,
        seqnum: 7,
        system: MSG_TYPE_RADIO_IN,
        command: radio::GET_TIME,
    };
    let mut encoded = [0u8; ESP_MAX_PAYLOAD];
    let len = encode_command(header, &[], &mut encoded).expect("encode");

    let mut hal = TestHal {
        hwid: 0x2222,
        uptime: 0,
        rtc_set: false,
        now: TimeSpec::default(),
        callsign: [0; 8],
        telemetry: Telemetry::default(),
        rx: RxPacket::from_slice(Interface::Uart0, &encoded[..len]),
        last_tx: None,
        last_tx_len: 0,
        last_tx_interface: None,
        last_rf_precise: false,
    };

    let mut runtime = RadioRuntime::new(RuntimeConfig::default());
    runtime.init(&mut hal);
    runtime.tick_1ms(&mut hal);

    let tx = hal.last_tx.expect("reply present");
    let parsed = openlst_core::protocol::parse_command(&tx[..hal.last_tx_len]).expect("parse tx");
    assert_eq!(parsed.header.command, common::NACK);
}

#[test]
fn runtime_handles_rf_wrapped_command() {
    let header = CommandHeader {
        hwid: 0x2222,
        seqnum: 9,
        system: MSG_TYPE_RADIO_IN,
        command: radio::GET_TIME,
    };
    let mut encoded = [0u8; ESP_MAX_PAYLOAD];
    let cmd_len = encode_command(header, &[], &mut encoded).expect("encode");

    let mut rf_encoded = [0u8; ESP_MAX_PAYLOAD];
    let rf_len = encode_rf_message(&encoded[..cmd_len], 1, &mut rf_encoded).expect("rf encode");

    let mut hal = TestHal {
        hwid: 0x2222,
        uptime: 0,
        rtc_set: false,
        now: TimeSpec::default(),
        callsign: [0; 8],
        telemetry: Telemetry::default(),
        rx: RxPacket::from_slice(Interface::Rf, &rf_encoded[..rf_len]),
        last_tx: None,
        last_tx_len: 0,
        last_tx_interface: None,
        last_rf_precise: false,
    };

    let mut runtime = RadioRuntime::new(RuntimeConfig::default());
    runtime.init(&mut hal);
    runtime.tick_1ms(&mut hal);

    let tx = hal.last_tx.expect("rf reply present");
    let decoded = decode_rf_message(&tx[..hal.last_tx_len]).expect("rf decode");
    let parsed = openlst_core::protocol::parse_command(decoded.command_slice()).expect("parse command");
    assert_eq!(parsed.header.command, common::NACK);
}

#[test]
fn runtime_ranging_ack_uses_precise_rf_send() {
    let header = CommandHeader {
        hwid: 0x2222,
        seqnum: 11,
        system: MSG_TYPE_RADIO_IN,
        command: radio::RANGING,
    };
    let mut encoded = [0u8; ESP_MAX_PAYLOAD];
    let cmd_len = encode_command(header, &[], &mut encoded).expect("encode");
    let mut rf_encoded = [0u8; ESP_MAX_PAYLOAD];
    let rf_len = encode_rf_message(&encoded[..cmd_len], 1, &mut rf_encoded).expect("rf encode");

    let mut hal = TestHal {
        hwid: 0x2222,
        uptime: 0,
        rtc_set: false,
        now: TimeSpec::default(),
        callsign: [0; 8],
        telemetry: Telemetry::default(),
        rx: RxPacket::from_slice(Interface::Rf, &rf_encoded[..rf_len]),
        last_tx: None,
        last_tx_len: 0,
        last_tx_interface: None,
        last_rf_precise: false,
    };

    let mut runtime = RadioRuntime::new(RuntimeConfig::default());
    runtime.init(&mut hal);
    runtime.tick_1ms(&mut hal);

    assert_eq!(hal.last_tx_interface, Some(Interface::Rf));
    assert!(hal.last_rf_precise);
}
