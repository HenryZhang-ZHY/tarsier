//! Input switching protocols: what a monitor needs to hear to change input.
//! Inputs are always named by their MCCS value (VCP 0x60, e.g. 0x0F = DP 1)
//! so config and UI stay vendor neutral; a protocol translates as needed.

use anyhow::Result;

use super::channel::VcpChannel;
use super::mccs::VCP_INPUT_SOURCE;

/// The transports a protocol may use for one monitor.
pub struct Channels<'a> {
    pub vcp: &'a dyn VcpChannel,
}

pub trait InputProtocol: Send + Sync {
    /// The active input as an MCCS value.
    fn current(&self, ch: &Channels) -> Result<u8>;
    fn switch(&self, ch: &Channels, input: u8) -> Result<()>;
}

/// The VESA standard: read and write VCP 0x60.
pub struct Mccs;

impl InputProtocol for Mccs {
    fn current(&self, ch: &Channels) -> Result<u8> {
        // Some monitors put garbage in the high byte.
        Ok((ch.vcp.get(VCP_INPUT_SOURCE)?.current & 0xFF) as u8)
    }

    fn switch(&self, ch: &Channels, input: u8) -> Result<()> {
        ch.vcp.set(VCP_INPUT_SOURCE, input as u32)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::display::channel::Feature;

    /// Records writes and answers reads from a fixed value.
    #[derive(Default)]
    pub struct FakeVcp {
        pub input: u32,
        pub writes: Mutex<Vec<(u8, u32)>>,
    }

    impl VcpChannel for FakeVcp {
        fn get(&self, code: u8) -> Result<Feature> {
            assert_eq!(code, VCP_INPUT_SOURCE);
            Ok(Feature {
                current: self.input,
                max: 0,
            })
        }
        fn set(&self, code: u8, value: u32) -> Result<()> {
            self.writes.lock().unwrap().push((code, value));
            Ok(())
        }
        fn capabilities(&self) -> Result<String> {
            Ok(String::new())
        }
    }

    #[test]
    fn mccs_writes_vcp_60() {
        let vcp = FakeVcp::default();
        Mccs.switch(&Channels { vcp: &vcp }, 0x11).unwrap();
        assert_eq!(*vcp.writes.lock().unwrap(), vec![(0x60, 0x11)]);
    }

    #[test]
    fn mccs_ignores_high_byte_of_current() {
        let vcp = FakeVcp {
            input: 0x010F,
            ..Default::default()
        };
        assert_eq!(Mccs.current(&Channels { vcp: &vcp }).unwrap(), 0x0F);
    }
}
