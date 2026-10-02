//! Input switching protocols: what a monitor needs to hear to change input.
//! Inputs are always named by their MCCS value (VCP 0x60, e.g. 0x0F = DP 1)
//! so config and UI stay vendor neutral; a protocol translates as needed.
//!
//! To support a new monitor family, implement [`InputProtocol`] and add a
//! [`Quirk`] to [`QUIRKS`]. Users can force a protocol per monitor through
//! [`InputProtocolPref`] when detection gets it wrong.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::channel::{RawDdcChannel, VcpChannel};
use super::ddcci::Packet;
use super::identity::Identity;
use super::mccs::{self, Capabilities, VCP_INPUT_SOURCE};

/// The transports a protocol may use for one monitor.
pub struct Channels<'a> {
    pub vcp: &'a dyn VcpChannel,
    /// Present only when a GPU backend can reach this monitor's I²C bus.
    pub raw: Option<&'a dyn RawDdcChannel>,
}

pub trait InputProtocol: Send + Sync {
    /// Short name for logs, e.g. `mccs`.
    fn name(&self) -> &'static str;
    /// The active input as an MCCS value.
    fn current(&self, ch: &Channels) -> Result<u8>;
    fn switch(&self, ch: &Channels, input: u8) -> Result<()>;
}

/// How to switch inputs on one monitor, set in `config.json` as
/// `"input_protocol": {"kind": "lg", "values": {"16": 210}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputProtocolPref {
    pub kind: ProtocolKind,
    /// LG only: MCCS input (decimal key) -> LG input code, merged over the
    /// built-in table.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<u8, u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolKind {
    /// Standard VCP 0x60.
    Mccs,
    /// LG side channel.
    Lg,
}

/// Picks the protocol for a monitor: the user's choice, else the first
/// matching quirk, else the VESA standard.
pub fn select(
    identity: Option<&Identity>,
    caps: Option<&Capabilities>,
    pref: Option<&InputProtocolPref>,
) -> Box<dyn InputProtocol> {
    let model = caps.and_then(|c| c.model.as_deref());
    match pref {
        Some(InputProtocolPref {
            kind: ProtocolKind::Mccs,
            ..
        }) => Box::new(Mccs),
        Some(InputProtocolPref {
            kind: ProtocolKind::Lg,
            values,
        }) => Box::new(LgSideChannel::new(model, values)),
        None => identity
            .and_then(|id| QUIRKS.iter().find(|q| (q.matches)(id, caps)))
            .map_or_else(|| Box::new(Mccs) as Box<dyn InputProtocol>, |q| (q.build)(model)),
    }
}

/// A monitor family whose input switching departs from the standard.
pub struct Quirk {
    pub matches: fn(&Identity, Option<&Capabilities>) -> bool,
    /// Builds the protocol, given the model name from the capabilities string.
    pub build: fn(Option<&str>) -> Box<dyn InputProtocol>,
}

pub const QUIRKS: &[Quirk] = &[
    // Recent LG monitors accept VCP 0x60 writes but ignore them; they switch
    // through VCP 0xF4 on the service address instead (and list 0xF4 in
    // their capabilities).
    Quirk {
        matches: |id, caps| id.manufacturer == "GSM" && caps.is_some_and(|c| c.supports(LG_INPUT_CODE)),
        build: |model| Box::new(LgSideChannel::new(model, &BTreeMap::new())),
    },
];

/// The VESA standard: read and write VCP 0x60.
pub struct Mccs;

impl InputProtocol for Mccs {
    fn name(&self) -> &'static str {
        "mccs"
    }

    fn current(&self, ch: &Channels) -> Result<u8> {
        // Some monitors put garbage in the high byte.
        Ok((ch.vcp.get(VCP_INPUT_SOURCE)?.current & 0xFF) as u8)
    }

    fn switch(&self, ch: &Channels, input: u8) -> Result<()> {
        ch.vcp.set(VCP_INPUT_SOURCE, input as u32)
    }
}

/// LG's service channel: host address 0x50 instead of 0x51, which OS monitor
/// APIs cannot send, so it needs a raw I²C transport.
/// See <https://github.com/rockowitz/ddcutil/wiki/Switching-input-source-on-LG-monitors>.
const LG_HOST_ADDR: u8 = 0x50;
const LG_INPUT_CODE: u8 = 0xF4;

/// MCCS input -> LG input code, as reported for most models.
const LG_VALUES: &[(u8, u8)] = &[(0x0F, 0xD0), (0x10, 0xD1), (0x11, 0x90), (0x12, 0x91)];

/// Per-model corrections to [`LG_VALUES`], keyed by capabilities model name.
const LG_MODEL_VALUES: &[(&str, &[(u8, u8)])] = &[
    // 28MQ780 reports its USB-C port as DP 2.
    ("MQ780", &[(0x10, 0xD2)]),
];

pub struct LgSideChannel {
    values: BTreeMap<u8, u8>,
}

impl LgSideChannel {
    pub fn new(model: Option<&str>, overrides: &BTreeMap<u8, u8>) -> Self {
        let mut values: BTreeMap<u8, u8> = LG_VALUES.iter().copied().collect();
        if let Some((_, fixes)) = LG_MODEL_VALUES.iter().find(|(m, _)| Some(*m) == model) {
            values.extend(fixes.iter().copied());
        }
        values.extend(overrides);
        LgSideChannel { values }
    }
}

impl InputProtocol for LgSideChannel {
    fn name(&self) -> &'static str {
        "lg"
    }

    /// LG still reports the active input correctly on VCP 0x60.
    fn current(&self, ch: &Channels) -> Result<u8> {
        Mccs.current(ch)
    }

    fn switch(&self, ch: &Channels, input: u8) -> Result<()> {
        let Some(&value) = self.values.get(&input) else {
            bail!(
                "不知道 {} 在 LG 私有通道上的编号，请在配置的 input_protocol.values 里补充",
                mccs::input_source_name(input)
            );
        };
        let Some(raw) = ch.raw else {
            bail!("这台 LG 显示器只能由显卡直接发 I²C 命令切换输入，当前显卡不支持（目前支持 NVIDIA）");
        };
        raw.write(&Packet::set_vcp(LG_HOST_ADDR, LG_INPUT_CODE, value as u16))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::display::channel::Feature;

    /// Records writes and answers reads from a fixed value.
    #[derive(Default)]
    struct FakeVcp {
        input: u32,
        writes: Mutex<Vec<(u8, u32)>>,
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

    #[derive(Default)]
    struct FakeRaw(Mutex<Vec<Packet>>);

    impl RawDdcChannel for FakeRaw {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn write(&self, packet: &Packet) -> Result<()> {
            self.0.lock().unwrap().push(packet.clone());
            Ok(())
        }
    }

    const LG_28MQ780_CAPS: &str = "(prot(monitor)type(lcd)model(MQ780)cmds(01 02 03 0C E3 F3)vcp(02 04 05 08 10 12 14(05 08 0B ) 16 18 1A 52 60(11 12 0F 10 ) AC AE B2 B6 C0 C6 C8 C9 D6(01 04) DF 62 8D F4 F5(01 02 03 04) F6(00 01 02) 4D 4E 4F 15(01 06 11 13 14 15 18 19 20 22 23 24 28 29 32 48) F7(00 01 02 03) F8(00 01) F9 EF FA(00 01) FD(00 01) FE(00 01 02) FF)mccs_ver(2.1)mswhql(1))";

    fn lg() -> Identity {
        Identity {
            manufacturer: "GSM".into(),
            product: 0x5BF6,
        }
    }

    fn pref(kind: ProtocolKind) -> InputProtocolPref {
        InputProtocolPref {
            kind,
            values: BTreeMap::new(),
        }
    }

    #[test]
    fn mccs_writes_vcp_60() {
        let vcp = FakeVcp::default();
        Mccs.switch(&Channels { vcp: &vcp, raw: None }, 0x11).unwrap();
        assert_eq!(*vcp.writes.lock().unwrap(), vec![(0x60, 0x11)]);
    }

    #[test]
    fn mccs_ignores_high_byte_of_current() {
        let vcp = FakeVcp {
            input: 0x010F,
            ..Default::default()
        };
        assert_eq!(Mccs.current(&Channels { vcp: &vcp, raw: None }).unwrap(), 0x0F);
    }

    #[test]
    fn lg_monitor_with_f4_gets_side_channel() {
        let caps = mccs::parse_capabilities(LG_28MQ780_CAPS);
        assert_eq!(select(Some(&lg()), Some(&caps), None).name(), "lg");
    }

    #[test]
    fn others_get_the_standard() {
        let caps = mccs::parse_capabilities(LG_28MQ780_CAPS);
        let dell = Identity {
            manufacturer: "DEL".into(),
            product: 0x4321,
        };
        assert_eq!(select(Some(&dell), Some(&caps), None).name(), "mccs");
        // An LG without the side channel.
        let old_lg = mccs::parse_capabilities("(vcp(10 12 60(0F 11)))");
        assert_eq!(select(Some(&lg()), Some(&old_lg), None).name(), "mccs");
        assert_eq!(select(None, Some(&caps), None).name(), "mccs");
    }

    #[test]
    fn preference_overrides_detection() {
        let caps = mccs::parse_capabilities(LG_28MQ780_CAPS);
        assert_eq!(
            select(Some(&lg()), Some(&caps), Some(&pref(ProtocolKind::Mccs))).name(),
            "mccs"
        );
        let lg_pref = pref(ProtocolKind::Lg);
        assert_eq!(select(None, None, Some(&lg_pref)).name(), "lg");
    }

    #[test]
    fn lg_sends_f4_on_host_0x50() {
        let (vcp, raw) = (FakeVcp::default(), FakeRaw::default());
        let ch = Channels {
            vcp: &vcp,
            raw: Some(&raw),
        };
        let lg = LgSideChannel::new(Some("MQ780"), &BTreeMap::new());
        lg.switch(&ch, 0x12).unwrap();
        lg.switch(&ch, 0x10).unwrap();
        let sent = raw.0.lock().unwrap();
        assert_eq!(
            *sent,
            vec![Packet::set_vcp(0x50, 0xF4, 0x91), Packet::set_vcp(0x50, 0xF4, 0xD2)]
        );
        assert!(
            vcp.writes.lock().unwrap().is_empty(),
            "VCP 0x60 is a no-op on these monitors"
        );
    }

    #[test]
    fn lg_values_layer_defaults_model_and_user() {
        let overrides = BTreeMap::from([(0x11, 0x92), (0x1B, 0xD1)]);
        let lg = LgSideChannel::new(Some("MQ780"), &overrides);
        assert_eq!(lg.values[&0x0F], 0xD0, "default");
        assert_eq!(lg.values[&0x10], 0xD2, "model fix");
        assert_eq!(lg.values[&0x11], 0x92, "user override");
        assert_eq!(lg.values[&0x1B], 0xD1, "user addition");
        assert_eq!(LgSideChannel::new(None, &BTreeMap::new()).values[&0x10], 0xD1);
    }

    #[test]
    fn lg_fails_loudly_instead_of_silently() {
        let vcp = FakeVcp::default();
        let lg = LgSideChannel::new(None, &BTreeMap::new());
        let no_raw = Channels { vcp: &vcp, raw: None };
        assert!(lg.switch(&no_raw, 0x11).is_err(), "no raw transport");
        let raw = FakeRaw::default();
        let unknown = Channels {
            vcp: &vcp,
            raw: Some(&raw),
        };
        assert!(lg.switch(&unknown, 0x1B).is_err(), "no LG code for USB-C by default");
        assert!(raw.0.lock().unwrap().is_empty());
    }

    #[test]
    fn preference_json_shape() {
        let pref: InputProtocolPref = serde_json::from_str(r#"{"kind":"lg","values":{"16":210}}"#).unwrap();
        assert_eq!(pref.kind, ProtocolKind::Lg);
        assert_eq!(pref.values, BTreeMap::from([(16, 210)]));
        let pref: InputProtocolPref = serde_json::from_str(r#"{"kind":"mccs"}"#).unwrap();
        assert_eq!(pref.kind, ProtocolKind::Mccs);
        assert_eq!(serde_json::to_string(&pref).unwrap(), r#"{"kind":"mccs"}"#);
    }
}
