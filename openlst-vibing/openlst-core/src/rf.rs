use crate::constants::ESP_MAX_PAYLOAD;
use crate::crc::crc16_ccitt_false;

pub const FLAGS_UART_SEL: u8 = 1 << 6;
pub const FLAGS_UART0_SEL: u8 = 0 << 6;
pub const FLAGS_UART1_SEL: u8 = 1 << 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RfDecodeError {
    TooShort,
    InvalidLength,
    CrcMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedRfMessage {
    pub command_len: usize,
    pub command: [u8; ESP_MAX_PAYLOAD],
    pub uart_sel: u8,
}

impl DecodedRfMessage {
    pub fn command_slice(&self) -> &[u8] {
        &self.command[..self.command_len]
    }
}

pub fn decode_rf_message(payload: &[u8]) -> Result<DecodedRfMessage, RfDecodeError> {
    if payload.len() < 6 {
        return Err(RfDecodeError::TooShort);
    }

    let rf_len = payload[0] as usize;
    if rf_len + 1 != payload.len() {
        return Err(RfDecodeError::InvalidLength);
    }

    let flags = payload[1];
    if rf_len < 1 + 2 + 2 {
        return Err(RfDecodeError::TooShort);
    }

    let footer_hwid_index = 1 + rf_len - 4;
    let footer_crc_index = 1 + rf_len - 2;

    let footer_hwid_low = payload[footer_hwid_index];
    let footer_hwid_high = payload[footer_hwid_index + 1];
    let expected_crc = u16::from_le_bytes([payload[footer_crc_index], payload[footer_crc_index + 1]]);

    let crc_region = &payload[0..footer_crc_index];
    let actual_crc = crc16_ccitt_false(crc_region);
    if actual_crc != expected_crc {
        return Err(RfDecodeError::CrcMismatch);
    }

    let command_payload_start = 2;
    let command_payload_end = footer_hwid_index;
    let command_payload_len = command_payload_end.saturating_sub(command_payload_start);
    let command_len = command_payload_len + 2;
    if command_len == 0 || command_len > ESP_MAX_PAYLOAD {
        return Err(RfDecodeError::InvalidLength);
    }

    let mut command = [0u8; ESP_MAX_PAYLOAD];
    command[0] = footer_hwid_low;
    command[1] = footer_hwid_high;
    command[2..2 + command_payload_len]
        .copy_from_slice(&payload[command_payload_start..command_payload_end]);

    Ok(DecodedRfMessage {
        command_len,
        command,
        uart_sel: if (flags & FLAGS_UART_SEL) != 0 { 1 } else { 0 },
    })
}

pub fn encode_rf_message(command: &[u8], uart_sel: u8, out: &mut [u8]) -> Option<usize> {
    if command.len() < 2 {
        return None;
    }

    let command_payload_len = command.len() - 2;
    let rf_len = 1 + command_payload_len + 2 + 2;
    let total_len = 1 + rf_len;
    if total_len > out.len() {
        return None;
    }

    out[0] = rf_len as u8;
    out[1] = if uart_sel == 0 {
        FLAGS_UART0_SEL
    } else {
        FLAGS_UART1_SEL
    };

    out[2..2 + command_payload_len].copy_from_slice(&command[2..]);

    let footer_hwid_index = 2 + command_payload_len;
    out[footer_hwid_index] = command[0];
    out[footer_hwid_index + 1] = command[1];

    let crc = crc16_ccitt_false(&out[0..footer_hwid_index + 2]);
    out[footer_hwid_index + 2..footer_hwid_index + 4].copy_from_slice(&crc.to_le_bytes());

    Some(total_len)
}

#[cfg(test)]
mod tests {
    use super::{decode_rf_message, encode_rf_message, RfDecodeError};

    #[test]
    fn rf_roundtrip() {
        let command = [0x34, 0x12, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut rf = [0u8; 64];
        let len = encode_rf_message(&command, 1, &mut rf).expect("rf encode");
        let decoded = decode_rf_message(&rf[..len]).expect("rf decode");
        assert_eq!(decoded.command_slice(), &command);
        assert_eq!(decoded.uart_sel, 1);
    }

    #[test]
    fn rf_bad_crc_rejected() {
        let command = [0x34, 0x12, 0xAA, 0xBB];
        let mut rf = [0u8; 64];
        let len = encode_rf_message(&command, 0, &mut rf).expect("rf encode");
        rf[len - 1] ^= 0x01;
        let result = decode_rf_message(&rf[..len]);
        assert_eq!(result, Err(RfDecodeError::CrcMismatch));
    }
}
