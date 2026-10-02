//! Transports: how DDC/CI messages reach a monitor. Each OS API or GPU vendor
//! SDK is one implementation; the rest of the app never names them.

use anyhow::Result;

use super::ddcci::Packet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub current: u32,
    pub max: u32,
}

/// Standard VCP get/set, the subset every OS monitor API offers. Messages
/// always go out with the standard host address (0x51).
pub trait VcpChannel: Send + Sync {
    /// Short backend name for diagnostics, e.g. `dxva2`.
    fn name(&self) -> &'static str;
    fn get(&self, code: u8) -> Result<Feature>;
    fn set(&self, code: u8, value: u32) -> Result<()>;
    /// The raw MCCS capabilities string.
    fn capabilities(&self) -> Result<String>;
}

/// Arbitrary DDC/CI writes on the display's I²C bus. Needed for vendor side
/// channels that OS APIs cannot reach, e.g. a non-standard host address.
pub trait RawDdcChannel: Send + Sync {
    /// Short backend name for diagnostics, e.g. `nvapi`.
    fn name(&self) -> &'static str;
    fn write(&self, packet: &Packet) -> Result<()>;
}

/// Opens raw channels for the displays a backend (usually a GPU driver) drives.
pub trait RawDdcProvider {
    fn name(&self) -> &'static str;
    /// `gdi_name` is the Windows display source, e.g. `\\.\DISPLAY1`. The
    /// error says why this backend cannot reach the display.
    fn open(&self, gdi_name: &str) -> Result<Box<dyn RawDdcChannel>>;
}
