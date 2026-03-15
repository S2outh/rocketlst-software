use crate::constants::TIMER_COUNT_PERIOD_MS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScheduleEvents {
    pub reboot_now: bool,
    pub update_telemetry: bool,
    pub adc_sample: bool,
    pub radio_relisten: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostponeError {
    TooLong,
}

#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    auto_reboot_at: Option<u32>,
    auto_reboot_max: Option<u32>,
    max_rx_ticks: u32,
    last_rx_ticks: u32,
    countdown_ms: u16,
}

impl Scheduler {
    pub fn new(auto_reboot_max: Option<u32>, max_rx_ticks: u32) -> Self {
        Self {
            auto_reboot_at: None,
            auto_reboot_max,
            max_rx_ticks,
            last_rx_ticks: 0,
            countdown_ms: TIMER_COUNT_PERIOD_MS,
        }
    }

    pub fn init(&mut self, uptime: u32, auto_reboot_seconds: Option<u32>) {
        self.auto_reboot_at = auto_reboot_seconds.map(|delta| uptime.saturating_add(delta));
    }

    pub fn postpone_reboot(&mut self, uptime: u32, postpone: u32) -> Result<(), PostponeError> {
        if let Some(max) = self.auto_reboot_max {
            if postpone > max {
                return Err(PostponeError::TooLong);
            }
        }
        self.auto_reboot_at = Some(uptime.saturating_add(postpone));
        Ok(())
    }

    pub fn note_rx(&mut self) {
        self.last_rx_ticks = 0;
    }

    pub fn tick_1ms(&mut self, uptime: u32) -> ScheduleEvents {
        let mut events = ScheduleEvents::default();

        if let Some(reboot_at) = self.auto_reboot_at {
            if uptime >= reboot_at {
                events.reboot_now = true;
            }
        }

        self.countdown_ms = self.countdown_ms.saturating_sub(1);
        if self.countdown_ms == 0 {
            self.countdown_ms = TIMER_COUNT_PERIOD_MS;
            events.update_telemetry = true;
            events.adc_sample = true;

            self.last_rx_ticks = self.last_rx_ticks.saturating_add(1);
            if self.last_rx_ticks >= self.max_rx_ticks {
                self.last_rx_ticks = 0;
                events.radio_relisten = true;
            }
        }

        events
    }
}
