//! Developer mode: what tarsier learned about each monitor while setting it
//! up, and a plain-text report to paste into bug reports.
//!
//! The report itself stays English whatever language the interface is in. It is
//! almost entirely identifiers, hex and driver error strings, and its whole
//! purpose is to be pasted into a bug report, where one language for everybody
//! is worth more than a translated label. The copy around it — the panel
//! heading, the buttons — is translated like everything else.

use std::fmt::Write as _;

use super::channel::DisplayTarget;
use super::identity::Identity;
use super::trace::TraceEntry;

/// Facts gathered during enumeration that explain how a monitor is driven.
#[derive(Debug, Clone)]
pub struct Diagnostics {
    pub target: DisplayTarget,
    /// The GPU driving the output, which decides which raw backends can work.
    pub adapter: Option<String>,
    pub identity: Option<Identity>,
    /// The raw capabilities string, or why reading it failed.
    pub capabilities: Result<String, String>,
    pub input_protocol: &'static str,
    pub input_protocol_reason: String,
    /// Each raw I²C backend tried, in order, and why it failed if it did.
    pub raw_backends: Vec<(&'static str, Result<(), String>)>,
}

impl Diagnostics {
    /// Label/value pairs, shared by the UI panel and the text report.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let raw = if self.raw_backends.is_empty() {
            "(no backends)".to_string()
        } else {
            self.raw_backends
                .iter()
                .map(|(name, result)| match result {
                    Ok(()) => format!("{name}: ok"),
                    Err(e) => format!("{name}: {e}"),
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        vec![
            ("gdi", self.target.gdi_name.clone()),
            (
                "target",
                match (self.target.adapter_luid, self.target.target_id) {
                    (Some(luid), Some(id)) => format!("adapter {luid:#x}, target {id}"),
                    _ => "?".into(),
                },
            ),
            ("adapter", self.adapter.clone().unwrap_or_else(|| "?".into())),
            (
                "edid",
                self.identity
                    .as_ref()
                    .map_or("?".into(), |id| format!("{} 0x{:04X}", id.manufacturer, id.product)),
            ),
            (
                "input",
                format!("{} ({})", self.input_protocol, self.input_protocol_reason),
            ),
            ("raw i2c", raw),
            (
                "caps",
                match &self.capabilities {
                    Ok(caps) => caps.trim_end_matches('\0').trim().to_string(),
                    Err(e) => format!("unavailable: {e}"),
                },
            ),
        ]
    }
}

/// One line per traced command, e.g. `14:30:01.123 dxva2 get 0x60 -> current=15 max=18`.
pub fn trace_line(entry: &TraceEntry) -> String {
    let result = match &entry.result {
        Ok(ok) => format!("-> {ok}"),
        Err(e) => format!("FAILED {e}"),
    };
    format!(
        "{} {} {} {result}",
        entry.at.format("%H:%M:%S%.3f"),
        entry.channel,
        entry.op
    )
}

/// The report section for one monitor.
pub fn monitor_section(id: &str, name: &str, diag: &Diagnostics, trace: &[TraceEntry]) -> String {
    let mut out = format!("## {name}\nid: {id}\n");
    for (label, value) in diag.rows() {
        // Indent continuation lines so multi-line values stay readable.
        let _ = writeln!(out, "{label}: {}", value.replace('\n', "\n    "));
    }
    out.push_str("trace:\n");
    if trace.is_empty() {
        out.push_str("  (empty)\n");
    }
    for entry in trace {
        let _ = writeln!(out, "  {}", trace_line(entry));
    }
    out
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::*;

    fn lg_on_intel() -> Diagnostics {
        Diagnostics {
            target: DisplayTarget {
                gdi_name: r"\\.\DISPLAY1".into(),
                adapter_luid: Some(0x1_0000_ABCD),
                target_id: Some(4352),
            },
            adapter: Some("Intel(R) Iris(R) Xe Graphics".into()),
            identity: Some(Identity {
                manufacturer: "GSM".into(),
                product: 0x5BF6,
            }),
            capabilities: Ok("(model(MQ780)vcp(60(0F 11)F4))".into()),
            input_protocol: "lg",
            input_protocol_reason: "quirk: LG side channel".into(),
            raw_backends: vec![("nvapi", Err("NVIDIA_DEVICE_NOT_FOUND".into())), ("igcl", Ok(()))],
        }
    }

    #[test]
    fn section_has_the_facts_needed_to_debug_switching() {
        let trace = [TraceEntry {
            at: Local.with_ymd_and_hms(2026, 10, 2, 14, 30, 1).unwrap(),
            channel: "nvapi",
            op: "write 6E 50 84 03 F4 00 91 4C".into(),
            result: Err("NvAPI_I2CWrite failed".into()),
        }];
        let text = monitor_section("dev-path", "LG SDQHD", &lg_on_intel(), &trace);
        assert_eq!(
            text,
            "## LG SDQHD
id: dev-path
gdi: \\\\.\\DISPLAY1
target: adapter 0x10000abcd, target 4352
adapter: Intel(R) Iris(R) Xe Graphics
edid: GSM 0x5BF6
input: lg (quirk: LG side channel)
raw i2c: nvapi: NVIDIA_DEVICE_NOT_FOUND
    igcl: ok
caps: (model(MQ780)vcp(60(0F 11)F4))
trace:
  14:30:01.000 nvapi write 6E 50 84 03 F4 00 91 4C FAILED NvAPI_I2CWrite failed
"
        );
    }

    #[test]
    fn missing_facts_are_marked() {
        let diag = Diagnostics {
            target: DisplayTarget::default(),
            adapter: None,
            identity: None,
            capabilities: Err("timeout".into()),
            raw_backends: vec![],
            ..lg_on_intel()
        };
        let text = monitor_section("x", "y", &diag, &[]);
        assert!(text.contains("adapter: ?\n"));
        assert!(text.contains("edid: ?\n"));
        assert!(text.contains("raw i2c: (no backends)\n"));
        assert!(text.contains("caps: unavailable: timeout\n"));
        assert!(text.contains("trace:\n  (empty)\n"));
    }
}
