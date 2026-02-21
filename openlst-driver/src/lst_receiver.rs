use embedded_io_async::{Read, ReadExactError};

const HEADER_LEN: usize = 5;
const MAGIC: [u8; 2] = [0x22, 0x69];

const DESTINATION_PTR: usize = 0x04;
const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;

const MAX_LEN: usize = 256;

pub struct LSTReceiver<S: Read> {
    uart_rx: S,
    buffer: [u8; MAX_LEN],
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub enum ReceiverError<UartError> {
    ParseError(&'static str),
    ReadError(ReadExactError<UartError>),
    MsgTooShort,
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub struct LSTTelemetry {
    pub uptime: u32,
    pub rssi: i8,
    pub lqi: u8,
    pub packets_sent: u32,
    pub packets_good: u32,
    pub packets_rejected_checksum: u32,
    pub packets_rejected_other: u32,
}
pub enum LSTMessage<'a> {
    Relay(&'a [u8]),
    Telem(LSTTelemetry),
    Ack,
    Nack,
    Unknown(u8),
}

impl<S: Read> LSTReceiver<S> {
    pub const fn new(uart_rx: S) -> Self {
        Self {
            uart_rx,
            buffer: [0; _],
        }
    }
    fn parse_telem(msg: &[u8]) -> Result<LSTTelemetry, ReceiverError<S::Error>> {
        // 62 bytes
        if msg.len() < 62 {
            Err(ReceiverError::ParseError("telem msg too short"))
        } else {
            Ok(LSTTelemetry {
                // u8 reserved: 0
                uptime: u32::from_le_bytes(msg[1..5].try_into().unwrap()),
                // u32 uart0 rx count: 5..9
                // u32 uart1 rx count: 9..13
                // u8 rx mode: 13
                // u8 tx mode: 14
                // i16 * 10 ADC channels: 15..35
                rssi: msg[35] as i8,
                lqi: msg[36] as u8,
                // i8 last frequency estimate: 37
                packets_sent: u32::from_le_bytes(msg[38..42].try_into().unwrap()),
                // u32 cs_count (sending collision count): 42..46
                packets_good: u32::from_le_bytes(msg[46..50].try_into().unwrap()),
                packets_rejected_checksum: u32::from_le_bytes(msg[50..54].try_into().unwrap()),
                packets_rejected_other: u32::from_le_bytes(msg[58..62].try_into().unwrap())
                    + u32::from_le_bytes(msg[54..58].try_into().unwrap()),
                // reserved + custom
            })
        }
    }
    fn parse_local_msg(msg: &[u8]) -> Result<LSTMessage<'_>, ReceiverError<S::Error>> {
        // parsing the available commands from the openlst firmware
        Ok(
            match msg
                .get(0)
                .ok_or(ReceiverError::ParseError("No command byte"))?
            {
                0x10 => LSTMessage::Ack,
                0xFF => LSTMessage::Nack,
                0x18 => LSTMessage::Telem(Self::parse_telem(&msg[1..])?),
                unknown => LSTMessage::Unknown(*unknown),
            },
        )
    }
    pub async fn receive(&mut self) -> Result<LSTMessage<'_>, ReceiverError<S::Error>> {
        // finding framing bytes
        let mut magic_pos = 0;
        loop {
            let mut byte: u8 = 0;
            self.uart_rx
                .read_exact(core::slice::from_mut(&mut byte))
                .await
                .map_err(ReceiverError::ReadError)?;
            if byte == MAGIC[magic_pos] {
                magic_pos += 1;
                if magic_pos == MAGIC.len() {
                    break;
                }
            } else {
                magic_pos = 0;
            }
        }

        // read length
        let mut len: u8 = 0;
        self.uart_rx
            .read_exact(core::slice::from_mut(&mut len))
            .await
            .map_err(ReceiverError::ReadError)?;
        let len = len as usize;

        if len < HEADER_LEN {
            return Err(ReceiverError::MsgTooShort);
        }

        // read packet
        self.uart_rx
            .read_exact(&mut self.buffer[..len])
            .await
            .map_err(ReceiverError::ReadError)?;

        #[cfg(feature = "defmt")]
        defmt::trace!("read lst packet");

        return Ok(match self.buffer[DESTINATION_PTR] {
            // msg comming from this lst, not relay
            DESTINATION_LOCAL => Self::parse_local_msg(&self.buffer[HEADER_LEN..len])?,
            // msg received from other lst
            DESTINATION_RELAY => LSTMessage::Relay(&self.buffer[HEADER_LEN..len]),
            _ => LSTMessage::Unknown(0x00),
        });
    }
}
