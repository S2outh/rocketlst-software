use crate::constants::MSG_TYPE_RADIO_OUT;
use crate::protocol::{common, radio, CommandHeader, COMMAND_DATA_MAX};
use crate::telemetry::Telemetry;
use crate::time::TimeSpec;

pub const RANGING_ACK_TYPE: u8 = 1;
pub const RANGING_ACK_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    RebootNow,
    PostponeReboot(u32),
    SetTime(TimeSpec),
    SendRangingAck,
    SetCallsign([u8; 8]),
}

#[derive(Clone, Copy, Debug)]
pub struct CommandContext {
    pub hwid_flash: u16,
    pub telemetry: Telemetry,
    pub rtc_set: bool,
    pub now: TimeSpec,
    pub callsign: [u8; 8],
    pub ranging_responder_enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CommandReply {
    pub header: CommandHeader,
    pub data: [u8; COMMAND_DATA_MAX],
    pub data_len: usize,
    pub action: Option<CommandAction>,
    pub suppress_default_reply: bool,
}

impl CommandReply {
    fn nack_from(input: CommandHeader, hwid_flash: u16) -> Self {
        Self {
            header: CommandHeader {
                hwid: hwid_flash,
                seqnum: input.seqnum,
                system: MSG_TYPE_RADIO_OUT,
                command: common::NACK,
            },
            data: [0; COMMAND_DATA_MAX],
            data_len: 0,
            action: None,
            suppress_default_reply: false,
        }
    }
}

pub fn handle_command(input: CommandHeader, data: &[u8], ctx: &CommandContext) -> CommandReply {
    let mut reply = CommandReply::nack_from(input, ctx.hwid_flash);

    match input.command {
        common::ACK => {
            reply.header.command = common::ACK;
        }
        common::NACK => {
            reply.header.command = common::NACK;
        }
        radio::REBOOT => {
            if data.len() < 4 {
                reply.header.command = common::ACK;
                reply.action = Some(CommandAction::RebootNow);
            } else {
                let postpone = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                reply.header.command = common::ACK;
                reply.action = Some(CommandAction::PostponeReboot(postpone));
            }
        }
        radio::GET_TIME => {
            if ctx.rtc_set {
                reply.header.command = radio::SET_TIME;
                if let Some(len) = ctx.now.encode(&mut reply.data) {
                    reply.data_len = len;
                }
            }
        }
        radio::SET_TIME => {
            if let Some(parsed) = TimeSpec::decode(data) {
                reply.header.command = common::ACK;
                reply.action = Some(CommandAction::SetTime(parsed));
            }
        }
        radio::GET_TELEM => {
            reply.header.command = radio::TELEM;
            if let Some(len) = ctx.telemetry.encode(&mut reply.data) {
                reply.data_len = len;
            }
        }
        radio::SET_CALLSIGN => {
            let mut callsign = [0u8; 8];
            let copy_len = core::cmp::min(callsign.len(), data.len());
            callsign[..copy_len].copy_from_slice(&data[..copy_len]);
            reply.header.command = common::ACK;
            reply.action = Some(CommandAction::SetCallsign(callsign));
        }
        radio::GET_CALLSIGN => {
            reply.header.command = radio::CALLSIGN;
            reply.data[..8].copy_from_slice(&ctx.callsign);
            reply.data_len = 8;
        }
        radio::RANGING => {
            if ctx.ranging_responder_enabled {
                reply.header.command = radio::RANGING_ACK;
                reply.data[0] = RANGING_ACK_TYPE;
                reply.data[1] = RANGING_ACK_VERSION;
                reply.data_len = 2;
                reply.action = Some(CommandAction::SendRangingAck);
                reply.suppress_default_reply = true;
            }
        }
        _ => {}
    }

    reply
}
