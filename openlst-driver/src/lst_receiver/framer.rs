const MAGIC: [u8; 2] = [0x22, 0x69];
const FULL_HEADER_LEN: usize = 8;

enum State {
    Sync { magic_pos: usize }, // searching for magic
    Len,
    Payload {
        len: usize,
        pos: usize,
    },
    Ready {
        len: usize,
    }
}
pub struct Framer<const N: usize> {
    state: State,
    storage: [u8; N],
}

#[derive(Debug)]
pub struct IsReady;

impl<const N: usize> Framer<N> {
    pub const fn new() -> Self {
        Self {
            state: State::Sync { magic_pos: 0 },
            storage: [0u8; N]
        }
    }

    pub fn push(&mut self, byte: u8) -> Result<bool, IsReady> {
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
                if len <= FULL_HEADER_LEN {
                    self.state = State::Sync { magic_pos: 0 };
                }
                else {
                    self.state = State::Payload { len, pos: 0 };
                }
            }

            State::Payload { len, ref mut pos } => {
                self.storage[*pos] = byte;
                *pos += 1;
                if *pos >= len {
                    self.state = State::Ready { len };
                    return Ok(true);
                }
            }
            
            State::Ready { len: _ } => {
                return Err(IsReady);
            }
        }
        Ok(false)
    }
    pub fn get(&self) -> Option<&[u8]> {
        if let State::Ready { len } = self.state {
            Some(&self.storage[..len])
        } else {
            None
        }
    }
    pub fn reset(&mut self) {
        self.state = State::Sync { magic_pos: 0 };
    }
}
