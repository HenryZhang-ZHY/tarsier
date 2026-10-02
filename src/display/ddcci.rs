//! DDC/CI packet encoding (VESA DDC/CI 1.1) for transports that put raw bytes
//! on the I²C bus. Pure, no I/O.

/// 8-bit I²C write address of a display's DDC/CI endpoint (7-bit 0x37).
pub const DISPLAY_ADDR: u8 = 0x6E;
/// The host source address the standard prescribes.
#[cfg(test)]
pub const HOST_ADDR: u8 = 0x51;

/// One DDC/CI write: the I²C target, the source address byte, then the
/// message body, which ends with the checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub dest: u8,
    pub source: u8,
    pub body: Vec<u8>,
}

impl Packet {
    /// "Set VCP Feature" (opcode 0x03) sent from host address `source`.
    pub fn set_vcp(source: u8, code: u8, value: u16) -> Self {
        let [hi, lo] = value.to_be_bytes();
        // 0x80 | length of the opcode + payload that follows.
        let mut body = vec![0x84, 0x03, code, hi, lo];
        let checksum = body.iter().fold(DISPLAY_ADDR ^ source, |acc, b| acc ^ b);
        body.push(checksum);
        Packet {
            dest: DISPLAY_ADDR,
            source,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_vcp_standard_brightness() {
        // Brightness 50 from the standard host address.
        let p = Packet::set_vcp(HOST_ADDR, 0x10, 50);
        assert_eq!((p.dest, p.source), (0x6E, 0x51));
        assert_eq!(p.body, [0x84, 0x03, 0x10, 0x00, 0x32, 0x9A]);
    }

    #[test]
    fn set_vcp_lg_side_channel() {
        // Same bytes as `writeValueToDisplay.exe 0 0xD2 0xF4 0x50`, known to
        // switch an LG 28MQ780 to USB-C.
        let p = Packet::set_vcp(0x50, 0xF4, 0xD2);
        assert_eq!(p.source, 0x50);
        assert_eq!(p.body, [0x84, 0x03, 0xF4, 0x00, 0xD2, 0x9F]);
    }

    #[test]
    fn checksum_covers_every_byte() {
        let p = Packet::set_vcp(0x50, 0xF4, 0x1234);
        let xor = p.body.iter().fold(p.dest ^ p.source, |acc, b| acc ^ b);
        assert_eq!(xor, 0, "XOR over the whole packet, checksum included, must be 0");
        assert_eq!(&p.body[3..5], [0x12, 0x34]);
    }
}
