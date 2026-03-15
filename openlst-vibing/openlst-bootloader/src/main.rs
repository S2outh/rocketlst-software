use std::collections::VecDeque;

use openlst_core::bootloader::{bootloader_main, BootloaderConfig};
use openlst_core::hal::{BootloaderHal, Interface, RxPacket};

struct MockBootloader {
    app_valid: bool,
    jumped: bool,
    rx: VecDeque<RxPacket>,
    flash_pages: usize,
}

impl BootloaderHal for MockBootloader {
    fn updater_init(&mut self) {}

    fn watchdog_clear(&mut self) {}

    fn poll_rx(&mut self) -> Option<RxPacket> {
        self.rx.pop_front()
    }

    fn send(&mut self, _interface: Interface, _payload: &[u8]) {}

    fn erase_app(&mut self) {
        self.flash_pages = 0;
    }

    fn write_app_page(&mut self, _page: u8, _page_data: &[u8; 128]) -> bool {
        self.flash_pages = self.flash_pages.saturating_add(1);
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

fn main() {
    let mut bl = MockBootloader {
        app_valid: true,
        jumped: false,
        rx: VecDeque::new(),
        flash_pages: 0,
    };
    bootloader_main(&mut bl, BootloaderConfig::default());
}
