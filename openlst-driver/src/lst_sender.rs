use embedded_io_async::Write;
use heapless::Vec;

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

pub struct LSTSender<S: Write> {
    uart_tx: S,
    seq_num: u16,
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SenderError<UartError> {
    MessageTooLongError,
    UartError(UartError),
}

impl<S: Write> LSTSender<S> {
    pub fn new(uart_tx: S) -> Self {
        Self { uart_tx, seq_num: 0 }
    }
    pub fn get_header(&mut self, msg_len: u8, dest: u8) -> [u8; HEADER_LEN] {
        let header = [
            0x22, 0x69,                          // Uart start bytes
            msg_len + 5,                         // packet length (+5 for remaining header)
            0x01, 0x00,                          // Hardware ID = 1 (for the lst to accept commands)
            self.seq_num as u8, (self.seq_num >> 8) as u8, // SeqNum
            dest,                                // Destination (0x01: LST, 0x11: Relay)
        ];
        self.seq_num = self.seq_num.wrapping_add(1);
        header
    }
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), SenderError<S::Error>> {

        if msg.len() > MAX_MSG_LEN - HEADER_LEN {
            return Err(SenderError::MessageTooLongError)
        }

        let mut packet: Vec<u8, MAX_MSG_LEN> = Vec::new();
        packet.extend_from_slice(&self.get_header(msg.len() as u8, DESTINATION_RELAY)).unwrap();
        packet.extend_from_slice(msg).unwrap();

        let mut idx = 0;
        while idx < packet.len() {
            idx += self.uart_tx.write(&packet[idx..]).await.map_err(|e| SenderError::UartError(e))?;
            self.uart_tx.flush().await.map_err(|e| SenderError::UartError(e))?;
        }
        Ok(())
    }
    pub async fn send_cmd(&mut self, cmd: LSTCmd) -> Result<(), SenderError<S::Error>> {
        let mut packet: Vec<u8, CMD_LEN> = Vec::new();
        packet.extend_from_slice(&self.get_header(1, DESTINATION_LOCAL)).unwrap();
        packet.push(cmd as u8).unwrap();
        

        let mut idx = 0;
        while idx < packet.len() {
            idx += self.uart_tx.write(&packet[idx..]).await.map_err(|e| SenderError::UartError(e))?;
            self.uart_tx.flush().await.map_err(|e| SenderError::UartError(e))?;
        }
        Ok(())
    }
}
