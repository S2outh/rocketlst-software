use std::collections::VecDeque;

use openlst_core::bootloader::{bootloader_main, bootloader_msg, BootloaderConfig};
use openlst_core::constants::{ESP_MAX_PAYLOAD, MSG_TYPE_RADIO_IN};
use openlst_core::hal::{BootloaderHal, Interface, RxPacket};
use openlst_core::protocol::{encode_command, parse_command, CommandHeader};

struct TestBootloaderHal {
    rx: VecDeque<RxPacket>,
    tx: Vec<(Interface, Vec<u8>)>,
    app_valid: bool,
    jumped: bool,
    erase_count: usize,
    write_count: usize,
}

impl BootloaderHal for TestBootloaderHal {
    fn updater_init(&mut self) {}

    fn watchdog_clear(&mut self) {}

    fn poll_rx(&mut self) -> Option<RxPacket> {
        self.rx.pop_front()
    }

    fn send(&mut self, interface: Interface, payload: &[u8]) {
        self.tx.push((interface, payload.to_vec()));
    }

    fn erase_app(&mut self) {
        self.erase_count = self.erase_count.saturating_add(1);
    }

    fn write_app_page(&mut self, _page: u8, _page_data: &[u8; 128]) -> bool {
        self.write_count = self.write_count.saturating_add(1);
        true
    }

    fn signature_app_valid(&self) -> bool {
        self.app_valid
    }

    fn jump_to_application(&mut self) {
        self.jumped = true;
    }

    fn stay_in_bootloader(&mut self) {
        self.jumped = false;
    }
}

#[test]
fn bootloader_replies_to_ping_and_jumps_when_valid() {
    let header = CommandHeader {
        hwid: 0x1201,
        seqnum: 5,
        system: MSG_TYPE_RADIO_IN,
        command: bootloader_msg::PING,
    };
    let mut encoded = [0u8; ESP_MAX_PAYLOAD];
    let len = encode_command(header, &[], &mut encoded).expect("encode");

    let mut hal = TestBootloaderHal {
        rx: VecDeque::from([
            RxPacket::from_slice(Interface::Uart0, &encoded[..len]).expect("rx packet"),
        ]),
        tx: Vec::new(),
        app_valid: true,
        jumped: false,
        erase_count: 0,
        write_count: 0,
    };

    bootloader_main(
        &mut hal,
        BootloaderConfig {
            command_watchdog_delay: 2,
            signature_grace_delay: 1,
        },
    );

    assert!(hal.jumped);
    assert_eq!(hal.tx.len(), 1);
    let (_, reply) = &hal.tx[0];
    let parsed = parse_command(reply).expect("parse reply");
    assert_eq!(parsed.header.command, bootloader_msg::ACK);
    assert_eq!(parsed.data[0], 0);
}

#[test]
fn bootloader_stays_when_signature_invalid() {
    let mut hal = TestBootloaderHal {
        rx: VecDeque::new(),
        tx: Vec::new(),
        app_valid: false,
        jumped: true,
        erase_count: 0,
        write_count: 0,
    };

    bootloader_main(
        &mut hal,
        BootloaderConfig {
            command_watchdog_delay: 1,
            signature_grace_delay: 1,
        },
    );

    assert!(!hal.jumped);
}
