
mod framer;
use framer::Framer;

mod ringbuffer;
use ringbuffer::SerialRingbuffer;

use embedded_io_async::Read;

use crate::lst_receiver::ringbuffer::PushErr;

const HEADER_LEN: usize = 5;

const DESTINATION_PTR: usize = 0x04;
const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;

const MAX_LEN: usize = 256;
const UART_RX_BUF_SIZE: usize = 32;

pub struct LSTReceiver<S: Read> {
    uart_rx: S,
    framer: Framer<MAX_LEN>,
    buffer: SerialRingbuffer<u8, {UART_RX_BUF_SIZE * 2}, UART_RX_BUF_SIZE>
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub enum ReceiverError<UartError> {
    ParseError(&'static str),
    ReadError(PushErr<UartError>),
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
        Self { uart_rx, framer: Framer::new(), buffer: SerialRingbuffer::new(0) }
    }
    fn parse_telem(msg: &[u8]) -> Result<LSTTelemetry, ReceiverError<S::Error>> {
        // 62 bytes
        if msg.len() < 62 {
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
    async fn wait_for_msg(&mut self) -> Result<(), ReceiverError<S::Error>> {
        self.framer.reset();
        loop {
            self.buffer.push_from_read(async |b| self.uart_rx.read(b).await).await.map_err(|e| ReceiverError::ReadError(e))?;
            while let Ok(byte) = self.buffer.pop() {
                if self.framer.push(byte).unwrap() {
                    return Ok(());
                }
            }
        }
    }
    pub async fn receive(&mut self) -> Result<LSTMessage<'_>, ReceiverError<S::Error>> {
        self.wait_for_msg().await?;
        let frame = self.framer.get().unwrap();
        return Ok(match frame[DESTINATION_PTR] {
            // msg comming from this lst, not relay
            DESTINATION_LOCAL => Self::parse_local_msg(&frame[HEADER_LEN..])?,
            // msg received from other lst
            DESTINATION_RELAY => LSTMessage::Relay(&frame[HEADER_LEN..]),
            _ => LSTMessage::Unknown(0x00)
        });
    }
}
