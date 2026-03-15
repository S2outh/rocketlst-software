use crate::constants::ADC_NUM_CHANNELS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Telemetry {
    pub reserved: u8,
    pub uptime: u32,
    pub uart0_rx_count: u32,
    pub uart1_rx_count: u32,
    pub rx_mode: u8,
    pub tx_mode: u8,
    pub adc: [i16; ADC_NUM_CHANNELS],
    pub last_rssi: i8,
    pub last_lqi: u8,
    pub last_freqest: i8,
    pub packets_sent: u32,
    pub cs_count: u32,
    pub packets_good: u32,
    pub packets_rejected_checksum: u32,
    pub packets_rejected_reserved: u32,
    pub packets_rejected_other: u32,
    pub reserved0: u32,
    pub reserved1: u32,
    pub custom0: u32,
    pub custom1: u32,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            reserved: 0,
            uptime: 0,
            uart0_rx_count: 0,
            uart1_rx_count: 0,
            rx_mode: 0,
            tx_mode: 0,
            adc: [0; ADC_NUM_CHANNELS],
            last_rssi: 0,
            last_lqi: 0,
            last_freqest: 0,
            packets_sent: 0,
            cs_count: 0,
            packets_good: 0,
            packets_rejected_checksum: 0,
            packets_rejected_reserved: 0,
            packets_rejected_other: 0,
            reserved0: 0,
            reserved1: 0,
            custom0: 0,
            custom1: 0,
        }
    }
}

impl Telemetry {
    pub const WIRE_LEN: usize = 76;

    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < Self::WIRE_LEN {
            return None;
        }

        let mut i = 0usize;
        out[i] = self.reserved;
        i += 1;

        for value in [self.uptime, self.uart0_rx_count, self.uart1_rx_count] {
            out[i..i + 4].copy_from_slice(&value.to_le_bytes());
            i += 4;
        }

        out[i] = self.rx_mode;
        i += 1;
        out[i] = self.tx_mode;
        i += 1;

        for value in self.adc {
            out[i..i + 2].copy_from_slice(&value.to_le_bytes());
            i += 2;
        }

        out[i] = self.last_rssi as u8;
        i += 1;
        out[i] = self.last_lqi;
        i += 1;
        out[i] = self.last_freqest as u8;
        i += 1;

        for value in [
            self.packets_sent,
            self.cs_count,
            self.packets_good,
            self.packets_rejected_checksum,
            self.packets_rejected_reserved,
            self.packets_rejected_other,
            self.reserved0,
            self.reserved1,
            self.custom0,
            self.custom1,
        ] {
            out[i..i + 4].copy_from_slice(&value.to_le_bytes());
            i += 4;
        }

        Some(i)
    }
}
