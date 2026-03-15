use crate::hal::BootloaderHal;
use crate::constants::ESP_MAX_PAYLOAD;
use crate::hal::Interface;
use crate::protocol::{common, encode_command, parse_command};
use crate::rf::{decode_rf_message, encode_rf_message};

pub const FLASH_WRITE_PAGE_SIZE: usize = 128;

pub mod bootloader_msg {
    pub const PING: u8 = 0x00;
    pub const ERASE: u8 = 0x0C;
    pub const WRITE_PAGE: u8 = 0x02;
    pub const ACK: u8 = 0x01;
    pub const NACK: u8 = 0x0F;
}

pub const BOOTLOADER_ACK_MSG_PONG: u8 = 0;
pub const BOOTLOADER_ACK_MSG_ERASED: u8 = 1;

#[derive(Clone, Copy, Debug)]
pub struct BootloaderConfig {
    pub command_watchdog_delay: u16,
    pub signature_grace_delay: u16,
}

impl Default for BootloaderConfig {
    fn default() -> Self {
        Self {
            command_watchdog_delay: 45_000,
            signature_grace_delay: 1_000,
        }
    }
}

fn process_packet<H: BootloaderHal>(
    hal: &mut H,
    interface: Interface,
    payload: &[u8],
    timeout: &mut u16,
    config: BootloaderConfig,
) {
    let mut command_buf = [0u8; ESP_MAX_PAYLOAD];
    let mut rf_uart_sel = 0u8;

    let command_payload = if interface == Interface::Rf {
        let Ok(decoded) = decode_rf_message(payload) else {
            return;
        };
        let command_len = decoded.command_len;
        command_buf[..command_len].copy_from_slice(decoded.command_slice());
        rf_uart_sel = decoded.uart_sel;
        &command_buf[..command_len]
    } else {
        payload
    };

    let Ok(frame) = parse_command(command_payload) else {
        return;
    };

    let mut reply_data = [0u8; ESP_MAX_PAYLOAD];
    let mut reply_data_len = 0usize;
    let mut reply_command = bootloader_msg::NACK;
    let mut reset_timeout = false;

    match frame.header.command {
        common::ACK => {
            reply_command = common::ACK;
        }
        common::NACK => {
            reply_command = common::NACK;
        }
        bootloader_msg::PING => {
            hal.watchdog_clear();
            reset_timeout = true;
            reply_command = bootloader_msg::ACK;
            reply_data[0] = BOOTLOADER_ACK_MSG_PONG;
            reply_data_len = 1;
        }
        bootloader_msg::ERASE => {
            hal.watchdog_clear();
            reset_timeout = true;
            hal.erase_app();
            reply_command = bootloader_msg::ACK;
            reply_data[0] = BOOTLOADER_ACK_MSG_ERASED;
            reply_data_len = 1;
        }
        bootloader_msg::WRITE_PAGE => {
            hal.watchdog_clear();
            reset_timeout = true;
            if frame.data.len() >= 1 + FLASH_WRITE_PAGE_SIZE {
                let page = frame.data[0];
                let mut page_data = [0u8; FLASH_WRITE_PAGE_SIZE];
                page_data.copy_from_slice(&frame.data[1..1 + FLASH_WRITE_PAGE_SIZE]);
                if hal.write_app_page(page, &page_data) {
                    if page == 255 && hal.signature_app_valid() {
                        *timeout = config.signature_grace_delay;
                    }
                    reply_command = bootloader_msg::ACK;
                    reply_data[0] = page;
                    reply_data_len = 1;
                }
            }
        }
        _ => {}
    }

    if reset_timeout {
        *timeout = config.command_watchdog_delay;
    }

    let mut out = [0u8; ESP_MAX_PAYLOAD];
    let reply_header = crate::protocol::CommandHeader {
        hwid: frame.header.hwid,
        seqnum: frame.header.seqnum,
        system: frame.header.system,
        command: reply_command,
    };
    if let Ok(length) = encode_command(reply_header, &reply_data[..reply_data_len], &mut out) {
        if interface == Interface::Rf {
            let mut out_rf = [0u8; ESP_MAX_PAYLOAD];
            if let Some(rf_len) = encode_rf_message(&out[..length], rf_uart_sel, &mut out_rf) {
                hal.send(Interface::Rf, &out_rf[..rf_len]);
            }
        } else {
            hal.send(interface, &out[..length]);
        }
    }
}

pub fn updater<H: BootloaderHal>(hal: &mut H, config: BootloaderConfig) {
    hal.updater_init();
    let mut timeout = config.command_watchdog_delay;

    while timeout > 0 {
        timeout = timeout.saturating_sub(1);
        hal.watchdog_clear();
        if let Some(packet) = hal.poll_rx() {
            process_packet(hal, packet.interface, packet.payload(), &mut timeout, config);
        }
    }
}

pub fn bootloader_main<H: BootloaderHal>(hal: &mut H, config: BootloaderConfig) {
    updater(hal, config);

    if hal.signature_app_valid() {
        hal.jump_to_application();
    } else {
        hal.stay_in_bootloader();
    }
}
