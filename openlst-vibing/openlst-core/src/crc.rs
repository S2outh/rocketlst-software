#[cfg_attr(feature = "crc-lut", allow(dead_code))]
fn crc16_ccitt_false_bitwise(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in data {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[cfg(feature = "crc-lut")]
const fn make_crc_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut j = 0;
        while j < 8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

#[cfg(feature = "crc-lut")]
const CRC16_CCITT_FALSE_TABLE: [u16; 256] = make_crc_table();

#[cfg(feature = "crc-lut")]
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in data {
        let index = ((crc >> 8) as u8) ^ *byte;
        crc = (crc << 8) ^ CRC16_CCITT_FALSE_TABLE[index as usize];
    }
    crc
}

#[cfg(not(feature = "crc-lut"))]
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    crc16_ccitt_false_bitwise(data)
}

#[cfg(test)]
mod tests {
    use super::{crc16_ccitt_false, crc16_ccitt_false_bitwise};

    #[test]
    fn crc_known_vector() {
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29B1);
    }

    #[test]
    fn crc_matches_bitwise_impl() {
        let sample = b"OpenLST CRC parity check";
        assert_eq!(crc16_ccitt_false(sample), crc16_ccitt_false_bitwise(sample));
    }
}
