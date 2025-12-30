const MAGIC: [u8; 2] = [0x22, 0x69];

enum State {
    Sync { magic_pos: usize }, // searching for magic
    Len,
    Payload {
        len: usize,
        pos: usize,
    },
}
pub struct Framer {
    state: State,
}

impl Framer {
    pub fn new() -> Self {
        Self {
            state: State::Sync { magic_pos: 0 },
        }
    }

    pub fn push(&mut self, byte: u8, buf: &mut [u8]) -> Option<usize> {
        match self.state {
            State::Sync { ref mut magic_pos } => {
                if byte == MAGIC[*magic_pos] {
                    *magic_pos += 1;
                    if *magic_pos == MAGIC.len() {
                        self.state = State::Len;
                    }
                } else {
                    *magic_pos = 0;
                }
            }

            State::Len => {
                let len = byte as usize;
                if len == 0 {
                    self.state = State::Sync { magic_pos: 0 };
                }
                else {
                    self.state = State::Payload { len, pos: 0 };
                }
            }

            State::Payload { len, ref mut pos } => {
                buf[*pos] = byte;
                *pos += 1;
                if *pos >= len {
                    self.state = State::Sync { magic_pos: 0 };
                    return Some(len);
                }
            }
        }
        None
    }
    pub fn reset(&mut self) {
        self.state = State::Sync { magic_pos: 0 };
    }
}
