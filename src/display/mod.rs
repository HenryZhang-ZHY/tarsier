//! Monitor control, layered so new monitors and GPUs plug in without touching
//! the rest of the app:
//!
//! - [`mccs`]: protocol vocabulary and capabilities parsing (pure).
//! - [`channel`]: transports, i.e. how bytes reach a monitor. One impl per OS
//!   API or GPU SDK: [`dxva2`] for standard VCP, [`nvapi`] for raw I²C.
//! - [`ddcci`]: packet encoding for raw I²C transports (pure).
//! - [`input`]: how a monitor wants its input switched, picked per vendor.
//! - [`Monitor`]: the facade everything else uses. It owns bus timing and
//!   retries so transports and protocols stay simple.
//! - [`diagnostics`] / [`trace`]: developer mode. Channels are wrapped in
//!   tracing decorators, so nothing else knows about it.

mod channel;
mod ddcci;
pub mod diagnostics;
mod dxva2;
mod identity;
mod input;
pub mod mccs;
mod nvapi;
mod trace;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;

pub use self::channel::Feature;
use self::channel::{RawDdcChannel, RawDdcProvider, VcpChannel};
use self::diagnostics::Diagnostics;
use self::identity::Identity;
pub use self::input::InputProtocolPref;
use self::input::{Channels, InputProtocol};
use self::mccs::Capabilities;
use self::trace::{Trace, TraceEntry, TracedRaw, TracedVcp};

const RETRIES: usize = 3;
/// MCCS asks hosts to wait at least 50ms between commands.
const COMMAND_GAP: Duration = Duration::from_millis(50);

/// One physical monitor and everything needed to talk to it.
pub struct Monitor {
    /// Stable identifier (device path when available).
    pub id: String,
    pub name: String,
    pub caps: Option<Capabilities>,
    pub diagnostics: Diagnostics,
    trace: Arc<Trace>,
    /// Serialises all traffic to this monitor, whatever the transport.
    bus: Mutex<()>,
    vcp: Box<dyn VcpChannel>,
    raw: Option<Box<dyn RawDdcChannel>>,
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

    /// Recent commands sent to this monitor, oldest first.
    pub fn trace(&self) -> Vec<TraceEntry> {
        self.trace.entries()
    }

    /// Plain-text diagnostics for bug reports.
    pub fn report(&self) -> String {
        diagnostics::monitor_section(&self.id, &self.name, &self.diagnostics, &self.trace())
    }

    fn channels(&self) -> Channels<'_> {
        Channels {
            vcp: self.vcp.as_ref(),
            raw: self.raw.as_deref(),
        }
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

/// Enumerate all monitors that expose a DDC/CI channel. `prefs` forces an
/// input protocol for some monitors, keyed by monitor id.
pub fn enumerate(prefs: &BTreeMap<String, InputProtocolPref>) -> Result<Vec<Monitor>> {
    let providers = raw_providers();
    Ok(dxva2::enumerate()?
        .into_iter()
        .map(|found| build(found, &providers, prefs))
        .collect())
}

/// Raw I²C backends, tried in order. AMD (ADL) or Intel (IGCL) go here.
fn raw_providers() -> Vec<Box<dyn RawDdcProvider>> {
    vec![Box::new(nvapi::NvApi)]
}

fn build(
    found: dxva2::Found,
    providers: &[Box<dyn RawDdcProvider>],
    prefs: &BTreeMap<String, InputProtocolPref>,
) -> Monitor {
    let trace = Arc::new(Trace::default());
    let vcp = TracedVcp {
        inner: Box::new(found.channel),
        trace: trace.clone(),
    };

    let mut raw_backends = Vec::new();
    let mut raw: Option<Box<dyn RawDdcChannel>> = None;
    for provider in providers {
        match provider.open(&found.gdi_name) {
            Ok(channel) => {
                raw_backends.push((provider.name(), Ok(())));
                raw = Some(Box::new(TracedRaw {
                    inner: channel,
                    trace: trace.clone(),
                }));
                break;
            }
            Err(e) => raw_backends.push((provider.name(), Err(format!("{e:#}")))),
        }
    }

    let capabilities = retry(|| {
        let raw = vcp.capabilities();
        thread::sleep(COMMAND_GAP);
        raw
    })
    .map_err(|e| format!("{e:#}"));
    let caps = capabilities.as_deref().ok().map(mccs::parse_capabilities);
    let name = found
        .friendly_name
        .or_else(|| caps.as_ref().and_then(|c| c.model.clone()))
        .unwrap_or(found.description);
    let identity = Identity::from_device_path(&found.id);
    let selected = input::select(identity.as_ref(), caps.as_ref(), prefs.get(&found.id));

    let diagnostics = Diagnostics {
        gdi_name: found.gdi_name,
        adapter: found.adapter,
        identity,
        capabilities,
        input_protocol: selected.protocol.name(),
        input_protocol_reason: selected.reason,
        raw_backends,
    };
    log::info!("{name}: {:?}", diagnostics.rows());
    Monitor {
        id: found.id,
        name,
        caps,
        diagnostics,
        trace,
        bus: Mutex::new(()),
        vcp: Box::new(vcp),
        raw,
        input: selected.protocol,
    }
}

#[cfg(test)]
mod hardware {
    use super::*;

    #[test]
    #[ignore = "talks to real monitors"]
    fn probe() {
        for m in enumerate(&BTreeMap::new()).unwrap() {
            m.current_input();
            println!("{}", m.report());
        }
    }
}
