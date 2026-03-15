use crate::constants::ESP_MAX_PAYLOAD;
use crate::telemetry::Telemetry;
use crate::time::TimeSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interface {
    Uart0,
    Uart1,
    Rf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RxPacket {
    pub interface: Interface,
    pub len: usize,
    pub data: [u8; ESP_MAX_PAYLOAD],
}

impl RxPacket {
    pub fn from_slice(interface: Interface, payload: &[u8]) -> Option<Self> {
        if payload.is_empty() || payload.len() > ESP_MAX_PAYLOAD {
            return None;
        }
        let mut data = [0u8; ESP_MAX_PAYLOAD];
        data[..payload.len()].copy_from_slice(payload);
        Some(Self {
            interface,
            len: payload.len(),
            data,
        })
    }

    pub fn payload(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

pub trait RadioHal {
    fn hwid_flash(&self) -> u16;

    fn watchdog_clear(&mut self);
    fn reboot_now(&mut self);

    fn uptime_seconds(&self) -> u32;
    fn rtc_set(&self) -> bool;
    fn get_time(&self) -> TimeSpec;
    fn set_time(&mut self, time: TimeSpec);

    fn get_callsign(&self) -> [u8; 8];
    fn set_callsign(&mut self, callsign: [u8; 8]);

    fn telemetry_snapshot(&self) -> Telemetry;

    fn adc_start_sample(&mut self);
    fn update_telemetry(&mut self);
    fn radio_listen(&mut self);

    fn poll_rx(&mut self) -> Option<RxPacket>;
    fn send(&mut self, interface: Interface, payload: &[u8]);
    fn send_rf(&mut self, payload: &[u8], uart_sel: u8);
    fn send_rf_precise(&mut self, payload: &[u8], uart_sel: u8);
}

pub trait BootloaderHal {
    fn updater_init(&mut self);
    fn watchdog_clear(&mut self);
    fn poll_rx(&mut self) -> Option<RxPacket>;
    fn send(&mut self, interface: Interface, payload: &[u8]);

    fn erase_app(&mut self);
    fn write_app_page(&mut self, page: u8, page_data: &[u8; 128]) -> bool;

    fn signature_app_valid(&self) -> bool;
    fn jump_to_application(&mut self);
    fn stay_in_bootloader(&mut self);
}
