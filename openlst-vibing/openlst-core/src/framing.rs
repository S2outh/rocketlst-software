use crate::constants::{ESP_MAX_PAYLOAD, ESP_START_BYTE_0, ESP_START_BYTE_1};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EspState {
    WaitForStart0,
    WaitForStart1,
    WaitForLength,
    ReceiveData,
}

pub struct EspRxParser {
    state: EspState,
    length: usize,
    index: usize,
}

impl Default for EspRxParser {
    fn default() -> Self {
        Self {
            state: EspState::WaitForStart0,
            length: 0,
            index: 0,
        }
    }
}

impl EspRxParser {
    pub fn push(&mut self, byte: u8, out: &mut [u8; ESP_MAX_PAYLOAD]) -> Option<usize> {
        match self.state {
            EspState::WaitForStart0 => {
                if byte == ESP_START_BYTE_0 {
                    self.state = EspState::WaitForStart1;
                }
            }
            EspState::WaitForStart1 => {
                if byte == ESP_START_BYTE_1 {
                    self.state = EspState::WaitForLength;
                } else {
                    self.state = EspState::WaitForStart0;
                }
            }
            EspState::WaitForLength => {
                self.length = byte as usize;
                self.index = 0;
                if self.length == 0 || self.length > ESP_MAX_PAYLOAD {
                    self.state = EspState::WaitForStart0;
                } else {
                    self.state = EspState::ReceiveData;
                }
            }
            EspState::ReceiveData => {
                out[self.index] = byte;
                self.index += 1;
                if self.index == self.length {
                    self.state = EspState::WaitForStart0;
                    return Some(self.length);
                }
            }
        }

        None
    }
}
