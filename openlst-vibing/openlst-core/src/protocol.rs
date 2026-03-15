use crate::constants::ESP_MAX_PAYLOAD;

pub const COMMAND_HEADER_LEN: usize = 6;
pub const COMMAND_DATA_MAX: usize = ESP_MAX_PAYLOAD - COMMAND_HEADER_LEN;

pub mod common {
    pub const ACK: u8 = 0x10;
    pub const NACK: u8 = 0xFF;
    pub const ASCII: u8 = 0x11;
}

pub mod radio {
    pub const REBOOT: u8 = 0x12;
    pub const GET_TIME: u8 = 0x13;
    pub const SET_TIME: u8 = 0x14;
    pub const RANGING: u8 = 0x15;
    pub const RANGING_ACK: u8 = 0x16;
    pub const GET_TELEM: u8 = 0x17;
    pub const TELEM: u8 = 0x18;
    pub const GET_CALLSIGN: u8 = 0x19;
    pub const SET_CALLSIGN: u8 = 0x1A;
    pub const CALLSIGN: u8 = 0x1B;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandHeader {
    pub hwid: u16,
    pub seqnum: u16,
    pub system: u8,
    pub command: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandFrame<'a> {
    pub header: CommandHeader,
    pub data: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    PayloadTooShort,
    PayloadTooLong,
}

impl CommandHeader {
    pub fn encode(&self, out: &mut [u8]) -> Result<(), ProtocolError> {
        if out.len() < COMMAND_HEADER_LEN {
            return Err(ProtocolError::PayloadTooShort);
        }

        out[0..2].copy_from_slice(&self.hwid.to_le_bytes());
        out[2..4].copy_from_slice(&self.seqnum.to_le_bytes());
        out[4] = self.system;
        out[5] = self.command;
        Ok(())
    }
}

pub fn parse_command(payload: &[u8]) -> Result<CommandFrame<'_>, ProtocolError> {
    if payload.len() < COMMAND_HEADER_LEN {
        return Err(ProtocolError::PayloadTooShort);
    }
    if payload.len() > ESP_MAX_PAYLOAD {
        return Err(ProtocolError::PayloadTooLong);
    }

    let header = CommandHeader {
        hwid: u16::from_le_bytes([payload[0], payload[1]]),
        seqnum: u16::from_le_bytes([payload[2], payload[3]]),
        system: payload[4],
        command: payload[5],
    };

    Ok(CommandFrame {
        header,
        data: &payload[COMMAND_HEADER_LEN..],
    })
}

pub fn encode_command(header: CommandHeader, data: &[u8], out: &mut [u8]) -> Result<usize, ProtocolError> {
    if data.len() > COMMAND_DATA_MAX {
        return Err(ProtocolError::PayloadTooLong);
    }
    let total = COMMAND_HEADER_LEN + data.len();
    if out.len() < total {
        return Err(ProtocolError::PayloadTooShort);
    }

    header.encode(out)?;
    out[COMMAND_HEADER_LEN..total].copy_from_slice(data);
    Ok(total)
}
