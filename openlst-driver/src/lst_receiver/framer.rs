const MAGIC: [u8; 2] = [0xAA, 0x55];

enum State {
    Sync { magic_pos: usize }, // searching for magic
    Len,
    Payload {
        len: usize,
        pos: usize,
    },
}
pub enum Resp {
    Synced(usize),
    Frame(usize)
}
pub struct Framer {
    state: State,
    pub ptr: usize,
}

impl Framer {
    pub fn new() -> Self {
        Self {
            state: State::Sync { magic_pos: 0 },
            ptr: 0
        }
    }

    pub fn push(&mut self, buf: &[u8], len: usize) -> Option<Resp> {
        for byte in &buf[self.ptr..self.ptr+len] {
            self.ptr += 1;
            match self.state {
                State::Sync { ref mut magic_pos } => {
                    if *byte == MAGIC[*magic_pos] {
                        *magic_pos += 1;
                        if *magic_pos == MAGIC.len() {
                            self.state = State::Len;
                            let synced_ptr = self.ptr - MAGIC.len();
                            self.ptr = 0;
                            return Some(Resp::Synced(synced_ptr));
                        }
                    } else {
                        *magic_pos = 0;
                    }
                }

                State::Len => {
                    let len = *byte as usize;
                    self.state = State::Payload { len, pos: 0 };
                }

                State::Payload { len, ref mut pos } => {
                    *pos += 1;

                    if *pos == len {
                        let frame_ptr = self.ptr;
                        self.ptr = 0;
                        return Some(Resp::Frame(frame_ptr));
                    }
                }
            }
        }
        None
    }
}
