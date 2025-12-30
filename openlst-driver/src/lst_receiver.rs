use embedded_io_async::Read;

mod framer;
use framer::Framer;

const HEADER_LEN: usize = 5;

const DESTINATION_PTR: usize = 0x04;
const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;

const MAX_LEN: usize = 256;

pub struct LSTReceiver<S: Read> {
    uart_rx: S,
    framer: Framer,
    buf: [u8; MAX_LEN],
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
pub enum ReceiverError<UartError> {
    ParseError(&'static str),
    UartError(UartError),
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
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
    pub fn new(uart_rx: S) -> Self {
        Self { uart_rx, framer: Framer::new(), buf: [0u8; MAX_LEN] }
    }
    fn parse_telem(msg: &[u8]) -> Result<LSTTelemetry, ReceiverError<S::Error>> {
        // 62 bytes
        if msg.len() < 55 {
            Err(ReceiverError::ParseError("telem msg too short"))
        } else {
            Ok(LSTTelemetry {
                uptime: u32::from_le_bytes(msg[1..5].try_into().unwrap()),
                rssi: msg[35] as i8,
                lqi: msg[36] as u8,
                packets_sent: u32::from_le_bytes(msg[38..42].try_into().unwrap()),
                packets_good: u32::from_le_bytes(msg[46..50].try_into().unwrap()),
                packets_rejected_checksum: u32::from_le_bytes(msg[50..54].try_into().unwrap()),
                packets_rejected_other: u32::from_le_bytes(msg[58..62].try_into().unwrap())
                    + u32::from_le_bytes(msg[54..58].try_into().unwrap()),
            })
        }
    }
    fn parse_local_msg(msg: &[u8]) -> Result<LSTMessage<'_>, ReceiverError<S::Error>> {
        // parsing the available commands from the openlst firmware
        Ok(match msg[0] {
            0x10 => LSTMessage::Ack,
            0xFF => LSTMessage::Nack,
            0x18 => LSTMessage::Telem(Self::parse_telem(&msg[1..])?),
            unknown => LSTMessage::Unknown(unknown),
        })
    }
    pub async fn receive(&mut self) -> Result<LSTMessage<'_>, ReceiverError<S::Error>> {
        loop {
            let mut read_buf = [0u8; 1];
            self.uart_rx.read(&mut read_buf).await.map_err(|e| ReceiverError::UartError(e))?;
            if let Some(len) = self.framer.push(read_buf[0], &mut self.buf[..]) {
                return Ok(match self.buf[DESTINATION_PTR] {
                    // msg comming from this lst, not relay
                    DESTINATION_LOCAL => Self::parse_local_msg(&self.buf[HEADER_LEN..len])?,
                    // msg received from other lst
                    DESTINATION_RELAY => LSTMessage::Relay(&self.buf[HEADER_LEN..len]),
                    _ => LSTMessage::Unknown(0x00)
                });
            }
        }
    }
    pub fn reset(&mut self) {
        self.framer.reset();
    }
}
