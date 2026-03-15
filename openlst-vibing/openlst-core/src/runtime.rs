use crate::commands::{handle_command, CommandAction, CommandContext};
use crate::constants::{AUTO_REBOOT_MAX, AUTO_REBOOT_SECONDS, ESP_MAX_PAYLOAD, HWID_LOCAL, MAX_RX_TICKS, MSG_TYPE_RADIO_IN};
use crate::hal::{Interface, RadioHal};
use crate::protocol::{encode_command, parse_command};
use crate::rf::{decode_rf_message, encode_rf_message};
use crate::schedule::{PostponeError, Scheduler};

#[derive(Clone, Copy, Debug)]
pub struct RuntimeConfig {
    pub auto_reboot_seconds: Option<u32>,
    pub auto_reboot_max: Option<u32>,
    pub max_rx_ticks: u32,
    pub forward_uart0_to_rf: bool,
    pub forward_uart1_to_rf: bool,
    pub forward_rf_to_uart1: bool,
    pub ranging_responder_enabled: bool,
    pub rf_ranging_uart: u8,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            auto_reboot_seconds: Some(AUTO_REBOOT_SECONDS),
            auto_reboot_max: Some(AUTO_REBOOT_MAX),
            max_rx_ticks: MAX_RX_TICKS,
            forward_uart0_to_rf: true,
            forward_uart1_to_rf: true,
            forward_rf_to_uart1: true,
            ranging_responder_enabled: true,
            rf_ranging_uart: 1,
        }
    }
}

pub struct RadioRuntime {
    scheduler: Scheduler,
    config: RuntimeConfig,
}

impl RadioRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        let mut scheduler = Scheduler::new(config.auto_reboot_max, config.max_rx_ticks);
        scheduler.init(0, config.auto_reboot_seconds);
        Self { scheduler, config }
    }

    pub fn init<H: RadioHal>(&mut self, hal: &mut H) {
        self.scheduler
            .init(hal.uptime_seconds(), self.config.auto_reboot_seconds);
        hal.radio_listen();
    }

    pub fn tick_1ms<H: RadioHal>(&mut self, hal: &mut H) {
        hal.watchdog_clear();

        let events = self.scheduler.tick_1ms(hal.uptime_seconds());
        if events.reboot_now {
            hal.reboot_now();
            return;
        }
        if events.update_telemetry {
            hal.update_telemetry();
        }
        if events.adc_sample {
            hal.adc_start_sample();
        }
        if events.radio_relisten {
            hal.radio_listen();
        }

        let mut tx = [0u8; ESP_MAX_PAYLOAD];
        let mut tx_rf = [0u8; ESP_MAX_PAYLOAD];
        while let Some(rx) = hal.poll_rx() {
            let mut command_buf = [0u8; ESP_MAX_PAYLOAD];
            let mut rf_uart_sel = 0u8;
            let command_payload = if rx.interface == Interface::Rf {
                let Ok(decoded) = decode_rf_message(rx.payload()) else {
                    continue;
                };
                rf_uart_sel = decoded.uart_sel;
                let command_len = decoded.command_len;
                command_buf[..command_len].copy_from_slice(decoded.command_slice());
                &command_buf[..command_len]
            } else {
                rx.payload()
            };

            let Ok(frame) = parse_command(command_payload) else {
                continue;
            };

            let addressed_to_me = frame.header.system == MSG_TYPE_RADIO_IN
                && (frame.header.hwid == hal.hwid_flash() || frame.header.hwid == HWID_LOCAL);

            if addressed_to_me {
                if rx.interface == Interface::Rf {
                    self.scheduler.note_rx();
                    hal.radio_listen();
                }

                let ctx = CommandContext {
                    hwid_flash: hal.hwid_flash(),
                    telemetry: hal.telemetry_snapshot(),
                    rtc_set: hal.rtc_set(),
                    now: hal.get_time(),
                    callsign: hal.get_callsign(),
                    ranging_responder_enabled: self.config.ranging_responder_enabled,
                };

                let mut reply = handle_command(frame.header, frame.data, &ctx);

                if let Some(action) = reply.action {
                    match action {
                        CommandAction::RebootNow => {
                            hal.reboot_now();
                        }
                        CommandAction::PostponeReboot(delay) => {
                            if let Err(PostponeError::TooLong) =
                                self.scheduler.postpone_reboot(hal.uptime_seconds(), delay)
                            {
                                reply.header.command = crate::protocol::common::NACK;
                                reply.data_len = 0;
                            }
                        }
                        CommandAction::SetTime(time) => hal.set_time(time),
                        CommandAction::SetCallsign(callsign) => hal.set_callsign(callsign),
                        CommandAction::SendRangingAck => {
                            let data = &reply.data[..reply.data_len];
                            if let Ok(length) = encode_command(reply.header, data, &mut tx) {
                                if let Some(rf_len) = encode_rf_message(
                                    &tx[..length],
                                    self.config.rf_ranging_uart,
                                    &mut tx_rf,
                                ) {
                                    hal.send_rf_precise(&tx_rf[..rf_len], self.config.rf_ranging_uart);
                                }
                            }
                        }
                    }
                }

                if !reply.suppress_default_reply {
                    let data = &reply.data[..reply.data_len];
                    if let Ok(length) = encode_command(reply.header, data, &mut tx) {
                        if rx.interface == Interface::Rf {
                            if let Some(rf_len) =
                                encode_rf_message(&tx[..length], rf_uart_sel, &mut tx_rf)
                            {
                                hal.send_rf(&tx_rf[..rf_len], rf_uart_sel);
                            }
                        } else {
                            hal.send(rx.interface, &tx[..length]);
                        }
                    }
                }

                continue;
            }

            match rx.interface {
                Interface::Uart0 if self.config.forward_uart0_to_rf => {
                    if let Some(rf_len) = encode_rf_message(rx.payload(), 0, &mut tx_rf) {
                        hal.send_rf(&tx_rf[..rf_len], 0);
                    }
                }
                Interface::Uart1 if self.config.forward_uart1_to_rf => {
                    if let Some(rf_len) = encode_rf_message(rx.payload(), 1, &mut tx_rf) {
                        hal.send_rf(&tx_rf[..rf_len], 1);
                    }
                }
                Interface::Rf if self.config.forward_rf_to_uart1 => {
                    if let Ok(decoded) = decode_rf_message(rx.payload()) {
                        if decoded.uart_sel == 0 {
                            hal.send(Interface::Uart0, decoded.command_slice());
                        } else {
                            hal.send(Interface::Uart1, decoded.command_slice());
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
