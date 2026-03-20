#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeSpec {
    pub seconds: u32,
    pub nanoseconds: u32,
}

impl TimeSpec {
    pub const WIRE_LEN: usize = 8;

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < Self::WIRE_LEN {
            return None;
        }

        let seconds = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let nanoseconds = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        Some(Self {
            seconds,
            nanoseconds,
        })
    }

    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < Self::WIRE_LEN {
            return None;
        }
        out[0..4].copy_from_slice(&self.seconds.to_le_bytes());
        out[4..8].copy_from_slice(&self.nanoseconds.to_le_bytes());
        Some(Self::WIRE_LEN)
    }
}
