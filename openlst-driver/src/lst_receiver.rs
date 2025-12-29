use embedded_io_async::Read;
use core::ops::Range;

mod framer;
use framer::{Framer, Resp};

const HEADER_LEN: usize = 8;

const DESTINATION_PTR: usize = 0x07;
const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;

const MAX_LEN: usize = 256;

pub struct LSTReceiver<S: Read> {
    uart_rx: S,
    framer: Framer,
    buf: [u8; MAX_LEN],
    remaining_range: Range<usize>
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug, Derive))]
pub enum ReceiverError<UartError> {
    ParseError(&'static str),
    UartError(UartError),
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug, Derive))]
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
        Self { uart_rx, framer: Framer::new(), buf: [0; MAX_LEN], remaining_range: 0..0 }
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
        self.buf.copy_within(self.remaining_range.clone(), 0);
        let mut additional_len = self.remaining_range.len();
        loop {
            let strt_pos = self.framer.ptr;
            let uart_len = self.uart_rx.read(&mut self.buf[strt_pos..]).await.map_err(|e| ReceiverError::UartError(e))?;
            let len = uart_len + additional_len;
            additional_len = 0;
            if let Some(result) = self.framer.push(&self.buf, len) {
                match result {
                    Resp::Synced(ptr) => {
                        self.buf.copy_within(ptr..strt_pos+len, 0);
                    }
                    Resp::Frame(ptr) => {
                        self.remaining_range = ptr..strt_pos+len;

                        return Ok(match self.buf[DESTINATION_PTR] {
                            // msg comming from this lst, not relay
                            DESTINATION_LOCAL => Self::parse_local_msg(&self.buf[HEADER_LEN..])?,
                            // msg received from other lst
                            DESTINATION_RELAY => LSTMessage::Relay(&self.buf[HEADER_LEN..]),
                            _ => LSTMessage::Unknown(0x00)
                        });
                    }
                }
            }
        }
    }
}
