use embedded_io_async::Write;
use heapless::Vec;

const FULL_HEADER_LEN: usize = 8;
const HEADER_LEN: u8 = 5;
const CMD_LEN: usize = FULL_HEADER_LEN + 1;
const MAX_LEN: usize = 256;

const DESTINATION_RELAY: u8 = 0x11;
const DESTINATION_LOCAL: u8 = 0x01;

#[repr(u8)]
pub enum LSTCmd {
    Reboot = 0x12,
    GetTelem = 0x17,
}

pub struct LSTSender<S: Write> {
    uart_tx: S,
    hwid: u16,
    seq_num: u16,
}
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub enum SenderError<UartError> {
    MessageTooLongError,
    WriteError(UartError),
}

impl<S: Write> LSTSender<S> {
    pub fn new(uart_tx: S, hwid: u16) -> Self {
        Self {
            uart_tx,
            hwid,
            seq_num: 0,
        }
    }
    pub fn get_header(&mut self, msg_len: u8, dest: u8) -> [u8; FULL_HEADER_LEN] {
        let header = [
            0x22,
            0x69,                 // Uart start bytes
            msg_len + HEADER_LEN, // packet length (+5 for remaining header)
            self.hwid as u8,
            (self.hwid >> 8) as u8, // Hardware ID
            self.seq_num as u8,
            (self.seq_num >> 8) as u8, // SeqNum
            dest,                      // Destination (0x01: LST, 0x11: Relay)
        ];
        self.seq_num = self.seq_num.wrapping_add(1);
        header
    }
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), SenderError<S::Error>> {
        if msg.len() > MAX_LEN - FULL_HEADER_LEN {
            return Err(SenderError::MessageTooLongError);
        }

        let mut packet: Vec<u8, MAX_LEN> = Vec::new();
        packet
            .extend_from_slice(&self.get_header(msg.len() as u8, DESTINATION_RELAY))
            .unwrap();
        packet.extend_from_slice(msg).unwrap();

        let mut idx = 0;
        while idx < packet.len() {
            idx += self
                .uart_tx
                .write(&packet[idx..])
                .await
                .map_err(SenderError::WriteError)?;
            self.uart_tx
                .flush()
                .await
                .map_err(SenderError::WriteError)?;
        }
        Ok(())
    }
    pub async fn send_cmd(&mut self, cmd: LSTCmd) -> Result<(), SenderError<S::Error>> {
        let mut packet: Vec<u8, CMD_LEN> = Vec::new();
        packet
            .extend_from_slice(&self.get_header(1, DESTINATION_LOCAL))
            .unwrap();
        packet.push(cmd as u8).unwrap();

        self.uart_tx
            .write_all(&packet)
            .await
            .map_err(SenderError::WriteError)?;

        self.uart_tx
            .flush()
            .await
            .map_err(SenderError::WriteError)?;

        Ok(())
    }
}
