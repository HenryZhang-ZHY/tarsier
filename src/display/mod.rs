//! Monitor control, layered so new monitors and GPUs plug in without touching
//! the rest of the app:
//!
//! - [`mccs`]: protocol vocabulary and capabilities parsing (pure).
//! - [`channel`]: transports, i.e. how bytes reach a monitor. One impl per OS
//!   API or GPU SDK ([`dxva2`]).
//! - [`input`]: how a monitor wants its input switched.
//! - [`Monitor`]: the facade everything else uses. It owns bus timing and
//!   retries so transports and protocols stay simple.

mod channel;
mod dxva2;
mod input;
pub mod mccs;

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::Result;

pub use self::channel::Feature;
use self::channel::VcpChannel;
use self::input::{Channels, InputProtocol};
use self::mccs::Capabilities;

const RETRIES: usize = 3;
/// MCCS asks hosts to wait at least 50ms between commands.
const COMMAND_GAP: Duration = Duration::from_millis(50);

/// One physical monitor and everything needed to talk to it.
pub struct Monitor {
    /// Stable identifier (device path when available).
    pub id: String,
    pub name: String,
    pub caps: Option<Capabilities>,
    /// Serialises all traffic to this monitor, whatever the transport.
    bus: Mutex<()>,
    vcp: Box<dyn VcpChannel>,
    input: Box<dyn InputProtocol>,
}

impl Monitor {
    pub fn get(&self, code: u8) -> Result<Feature> {
        self.on_bus(|| self.vcp.get(code))
    }

    pub fn set(&self, code: u8, value: u32) -> Result<()> {
        self.on_bus(|| self.vcp.set(code, value))
    }

    /// Input sources the monitor claims to support.
    pub fn input_sources(&self) -> Vec<u8> {
        self.caps.as_ref().map(Capabilities::input_sources).unwrap_or_default()
    }

    pub fn current_input(&self) -> Option<u8> {
        self.on_bus(|| self.input.current(&self.channels())).ok()
    }

    /// Switches to `input`, an MCCS input value.
    pub fn switch_input(&self, input: u8) -> Result<()> {
        self.on_bus(|| self.input.switch(&self.channels(), input))
    }

    fn channels(&self) -> Channels<'_> {
        Channels { vcp: self.vcp.as_ref() }
    }

    fn on_bus<T>(&self, mut f: impl FnMut() -> Result<T>) -> Result<T> {
        let _bus = self.bus.lock().unwrap();
        retry(|| {
            let result = f();
            thread::sleep(COMMAND_GAP);
            result
        })
    }
}

fn retry<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut last = None;
    for _ in 0..RETRIES {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

/// Enumerate all monitors that expose a DDC/CI channel.
pub fn enumerate() -> Result<Vec<Monitor>> {
    Ok(dxva2::enumerate()?.into_iter().map(build).collect())
}

fn build(found: dxva2::Found) -> Monitor {
    let caps = retry(|| {
        let raw = found.channel.capabilities();
        thread::sleep(COMMAND_GAP);
        raw
    })
    .ok()
    .map(|raw| mccs::parse_capabilities(&raw));
    let name = found
        .friendly_name
        .or_else(|| caps.as_ref().and_then(|c| c.model.clone()))
        .unwrap_or(found.description);
    Monitor {
        id: found.id,
        name,
        caps,
        bus: Mutex::new(()),
        vcp: Box::new(found.channel),
        input: Box::new(input::Mccs),
    }
}
