//! Command trace for developer mode. Decorators wrap the real channels and
//! record every message, so transports and protocols stay unaware of it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{DateTime, Local};

use super::channel::{Feature, RawDdcChannel, VcpChannel};
use super::ddcci::Packet;

/// Entries kept per monitor.
const CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub at: DateTime<Local>,
    pub channel: &'static str,
    pub op: String,
    /// What came back, or the error.
    pub result: Result<String, String>,
}

/// The most recent messages sent to one monitor, oldest first.
#[derive(Default)]
pub struct Trace(Mutex<VecDeque<TraceEntry>>);

impl Trace {
    fn record<T>(&self, channel: &'static str, op: String, result: &Result<T>, ok: impl FnOnce(&T) -> String) {
        let mut entries = self.0.lock().unwrap();
        if entries.len() == CAPACITY {
            entries.pop_front();
        }
        let entry = TraceEntry {
            at: Local::now(),
            channel,
            op,
            result: result.as_ref().map(ok).map_err(|e| format!("{e:#}")),
        };
        log::debug!("{}", super::diagnostics::trace_line(&entry));
        entries.push_back(entry);
    }

    pub fn entries(&self) -> Vec<TraceEntry> {
        self.0.lock().unwrap().iter().cloned().collect()
    }
}

pub struct TracedVcp {
    pub inner: Box<dyn VcpChannel>,
    pub trace: Arc<Trace>,
}

impl VcpChannel for TracedVcp {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn get(&self, code: u8) -> Result<Feature> {
        let result = self.inner.get(code);
        self.trace
            .record(self.name(), format!("get 0x{code:02X}"), &result, |f| {
                format!("current={} max={}", f.current, f.max)
            });
        result
    }

    fn set(&self, code: u8, value: u32) -> Result<()> {
        let result = self.inner.set(code, value);
        let op = format!("set 0x{code:02X} = {value} (0x{value:02X})");
        self.trace.record(self.name(), op, &result, |_| "ok".into());
        result
    }

    fn capabilities(&self) -> Result<String> {
        let result = self.inner.capabilities();
        self.trace.record(self.name(), "capabilities".into(), &result, |s| {
            format!("{} bytes", s.len())
        });
        result
    }
}

pub struct TracedRaw {
    pub inner: Box<dyn RawDdcChannel>,
    pub trace: Arc<Trace>,
}

impl RawDdcChannel for TracedRaw {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn write(&self, packet: &Packet) -> Result<()> {
        let result = self.inner.write(packet);
        let op = format!("write {}", hex(packet));
        self.trace.record(self.name(), op, &result, |_| "ok".into());
        result
    }
}

/// The packet as it goes on the wire, e.g. `6E 50 84 03 F4 00 D2 9F`.
fn hex(packet: &Packet) -> String {
    [packet.dest, packet.source]
        .iter()
        .chain(&packet.body)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use anyhow::bail;

    use super::*;

    struct Flaky;

    impl VcpChannel for Flaky {
        fn name(&self) -> &'static str {
            "flaky"
        }
        fn get(&self, _: u8) -> Result<Feature> {
            Ok(Feature { current: 15, max: 18 })
        }
        fn set(&self, _: u8, _: u32) -> Result<()> {
            bail!("no ack")
        }
        fn capabilities(&self) -> Result<String> {
            Ok("(vcp(10))".into())
        }
    }

    struct Sink;

    impl RawDdcChannel for Sink {
        fn name(&self) -> &'static str {
            "sink"
        }
        fn write(&self, _: &Packet) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn records_vcp_traffic_and_passes_results_through() {
        let trace = Arc::new(Trace::default());
        let vcp = TracedVcp {
            inner: Box::new(Flaky),
            trace: trace.clone(),
        };
        assert_eq!(vcp.get(0x60).unwrap().current, 15);
        assert!(vcp.set(0x60, 0x12).is_err());
        vcp.capabilities().unwrap();

        let entries = trace.entries();
        let summary: Vec<_> = entries
            .iter()
            .map(|e| (e.channel, e.op.as_str(), e.result.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("flaky", "get 0x60", Ok("current=15 max=18".to_string())),
                ("flaky", "set 0x60 = 18 (0x12)", Err("no ack".to_string())),
                ("flaky", "capabilities", Ok("9 bytes".to_string())),
            ]
        );
    }

    #[test]
    fn records_raw_packets_as_wire_bytes() {
        let trace = Arc::new(Trace::default());
        let raw = TracedRaw {
            inner: Box::new(Sink),
            trace: trace.clone(),
        };
        raw.write(&Packet::set_vcp(0x50, 0xF4, 0xD2)).unwrap();
        assert_eq!(trace.entries()[0].op, "write 6E 50 84 03 F4 00 D2 9F");
    }

    #[test]
    fn keeps_only_recent_entries() {
        let trace = Arc::new(Trace::default());
        let vcp = TracedVcp {
            inner: Box::new(Flaky),
            trace: trace.clone(),
        };
        for code in 0..=CAPACITY as u8 {
            let _ = vcp.get(code);
        }
        let entries = trace.entries();
        assert_eq!(entries.len(), CAPACITY);
        assert_eq!(entries[0].op, "get 0x01");
    }
}
