use embassy_stm32::{mode::Async, usart::{Error, UartRx}};
use defmt::Format;

const HEADER_LEN: usize = 8;

const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;


pub struct LSTReceiver<'a> {
    uart_rx: UartRx<'a, Async>,
}
#[derive(Format)]
pub enum ReceiverError {
    ParseError(&'static str),
    UartError(Error),
}
pub struct LSTTelemetry {
    pub uptime: u32,
    pub rssi: i8,
    pub lqi: u8,
    pub packets_sent: u32,
    pub packets_good: u32,
    pub packets_rejected_checksum: u32,
    pub packets_rejected_other: u32,
}
pub enum LSTMessage<'m> {
    Relay(&'m [u8]),
    Telem(LSTTelemetry),
    Ack,
    Nack,
    Unknown,
}

impl<'a> LSTReceiver<'a> {
    pub fn new(uart_rx: UartRx<'a, Async>) -> Self {
        Self { uart_rx }
    }
    fn parse_telem(msg: &[u8]) -> Result<LSTTelemetry, ReceiverError> {
        // 62 bytes
        if msg.len() < 62 {
            Err(ReceiverError::ParseError("telem msg too short"))
        } else {
            Ok(LSTTelemetry {
                uptime: u32::from_be_bytes(msg[1..5].try_into().unwrap()),
                rssi: msg[35] as i8,
                lqi: msg[36] as u8,
                packets_sent: u32::from_be_bytes(msg[38..42].try_into().unwrap()),
                packets_good: u32::from_be_bytes(msg[46..50].try_into().unwrap()),
                packets_rejected_checksum: u32::from_be_bytes(msg[50..54].try_into().unwrap()),
                packets_rejected_other: u32::from_be_bytes(msg[58..62].try_into().unwrap())
                    + u32::from_be_bytes(msg[54..58].try_into().unwrap()),
            })
        }
    }
    fn parse_local_msg<'m>(msg: &[u8]) -> Result<LSTMessage<'m>, ReceiverError> {
        // parsing the available commands from the openlst firmware
        Ok(match msg[0] {
            0x10 => LSTMessage::Ack,
            0xFF => LSTMessage::Nack,
            0x18 => LSTMessage::Telem(Self::parse_telem(&msg[1..])?),
            _ => LSTMessage::Unknown,
        })
    }
    pub async fn receive<'m>(&mut self, buffer: &'m mut [u8]) -> Result<LSTMessage<'m>, ReceiverError> {
        match self.uart_rx.read_until_idle(buffer).await {
            Ok(len) => {
                if len <= HEADER_LEN {
                    // incomplete msg
                    return Err(ReceiverError::ParseError("Message incomplete"));
                }

                // msg comming from this lst, not relay
                Ok(match buffer[7] {
                    DESTINATION_LOCAL => Self::parse_local_msg(&buffer[8..])?,
                    DESTINATION_RELAY => LSTMessage::Relay(&buffer[HEADER_LEN..]),
                    _ => LSTMessage::Unknown
                })
            }
            Err(e) => {
                Err(ReceiverError::UartError(e))
            }
        }
    }
}
