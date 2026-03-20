use openlst_core::commands::{handle_command, CommandAction, CommandContext};
use openlst_core::constants::{AUTO_REBOOT_MAX, AUTO_REBOOT_SECONDS, MAX_RX_TICKS, MSG_TYPE_RADIO_IN};
use openlst_core::protocol::{common, parse_command, radio, CommandHeader};
use openlst_core::schedule::{PostponeError, Scheduler};
use openlst_core::telemetry::Telemetry;
use openlst_core::time::TimeSpec;

fn run_command(
    header: CommandHeader,
    data: &[u8],
    scheduler: &mut Scheduler,
    uptime: u32,
    rtc_set: &mut bool,
    now: &mut TimeSpec,
    callsign: &mut [u8; 8],
) -> CommandHeader {
    let ctx = CommandContext {
        hwid_flash: 0x1201,
        telemetry: Telemetry::default(),
        rtc_set: *rtc_set,
        now: *now,
        callsign: *callsign,
        ranging_responder_enabled: true,
    };

    let reply = handle_command(header, data, &ctx);
    if let Some(action) = reply.action {
        match action {
            CommandAction::RebootNow => {}
            CommandAction::PostponeReboot(delay) => {
                if let Err(PostponeError::TooLong) = scheduler.postpone_reboot(uptime, delay) {
                    return CommandHeader {
                        command: common::NACK,
                        ..reply.header
                    };
                }
            }
            CommandAction::SetTime(value) => {
                *now = value;
                *rtc_set = true;
            }
            CommandAction::SendRangingAck => {}
            CommandAction::SetCallsign(value) => {
                *callsign = value;
            }
        }
    }

    reply.header
}

fn main() {
    let mut scheduler = Scheduler::new(Some(AUTO_REBOOT_MAX), MAX_RX_TICKS);
    let mut uptime = 0u32;
    scheduler.init(uptime, Some(AUTO_REBOOT_SECONDS));

    let mut rtc_set = false;
    let mut now = TimeSpec::default();
    let mut callsign = [0u8; 8];

    let get_time = CommandHeader {
        hwid: 0x1201,
        seqnum: 1,
        system: MSG_TYPE_RADIO_IN,
        command: radio::GET_TIME,
    };
    let mut packet = [0u8; 6];
    let _ = get_time.encode(&mut packet);
    let frame = parse_command(&packet).expect("parse header");
    let reply = run_command(
        frame.header,
        frame.data,
        &mut scheduler,
        uptime,
        &mut rtc_set,
        &mut now,
        &mut callsign,
    );
    println!("GET_TIME before RTC set -> 0x{:02X}", reply.command);

    let mut set_time_payload = [0u8; 14];
    let set_time = CommandHeader {
        hwid: 0x1201,
        seqnum: 2,
        system: MSG_TYPE_RADIO_IN,
        command: radio::SET_TIME,
    };
    let _ = set_time.encode(&mut set_time_payload[0..6]);
    set_time_payload[6..10].copy_from_slice(&123u32.to_le_bytes());
    set_time_payload[10..14].copy_from_slice(&456u32.to_le_bytes());
    let frame = parse_command(&set_time_payload).expect("parse set_time");
    let reply = run_command(
        frame.header,
        frame.data,
        &mut scheduler,
        uptime,
        &mut rtc_set,
        &mut now,
        &mut callsign,
    );
    println!("SET_TIME -> 0x{:02X}", reply.command);

    for _ in 0..1500 {
        let events = scheduler.tick_1ms(uptime);
        if events.update_telemetry {
            uptime = uptime.saturating_add(1);
        }
        if events.reboot_now {
            println!("Auto reboot event triggered at uptime={}s", uptime);
            break;
        }
    }
}
