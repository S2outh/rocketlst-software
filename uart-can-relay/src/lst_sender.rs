use embassy_stm32::{mode::Async, usart::{Error, UartTx}};
use heapless::Vec;
use defmt::Format;

const HEADER_LEN: usize = 8;
const CMD_LEN: usize = HEADER_LEN + 1;
const MAX_MSG_LEN: usize = 256;

const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;

#[repr(u8)]
pub enum LSTCmd {
    Reboot = 0x12,
    GetTelem = 0x17,
}

pub struct LSTSender<'a> {
    uart_tx: UartTx<'a, Async>,
    seq_num: u16,
}
#[derive(Format)]
pub enum SenderError {
    MessageTooLongError,
    UartError(Error),
}

impl<'a> LSTSender<'a> {
    pub fn new(uart_tx: UartTx<'a, Async>) -> Self {
        Self { uart_tx, seq_num: 0 }
    }
    pub fn get_header(&mut self, msg_len: u8, dest: u8) -> [u8; HEADER_LEN] {
        let header = [
            0x22, 0x69,                          // Uart start bytes
            msg_len + 6,                         // packet length (+6 for remaining header)
            0x00, 0x01,                          // Hardware ID (essentially irrelevant)
            (self.seq_num >> 8) as u8, self.seq_num as u8, // SeqNum
            dest,                                // Destination (0x01: LST, 0x11: Relay)
        ];
        self.seq_num = self.seq_num.wrapping_add(1);
        header
    }
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), SenderError> {

        if msg.len() > MAX_MSG_LEN - HEADER_LEN {
            return Err(SenderError::MessageTooLongError)
        }

        let mut packet: Vec<u8, MAX_MSG_LEN> = Vec::new();
        packet.extend_from_slice(&self.get_header(msg.len() as u8, DESTINATION_RELAY)).unwrap();
        packet.extend_from_slice(msg).unwrap();

        self.uart_tx.write(&packet).await.map_err(|e| SenderError::UartError(e))
    }
    pub async fn send_cmd(&mut self, cmd: LSTCmd) -> Result<(), SenderError> {
        let mut packet: Vec<u8, CMD_LEN> = Vec::new();
        packet.extend_from_slice(&self.get_header(1, DESTINATION_LOCAL)).unwrap();
        packet.push(cmd as u8).unwrap();

        self.uart_tx.write(&packet).await.map_err(|e| SenderError::UartError(e))
    }
}
