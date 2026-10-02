//! VESA MCCS (Monitor Control Command Set) helpers: VCP codes, input source
//! names and the DDC/CI capabilities string parser. Pure logic, no OS calls.

use std::collections::BTreeMap;

pub const VCP_BRIGHTNESS: u8 = 0x10;
pub const VCP_CONTRAST: u8 = 0x12;
pub const VCP_INPUT_SOURCE: u8 = 0x60;

/// Input sources offered when a monitor does not report its own list.
pub const FALLBACK_INPUTS: &[u8] = &[0x0F, 0x10, 0x11, 0x12, 0x1B];

/// Human readable name of an MCCS input source value (VCP 0x60).
pub fn input_source_name(code: u8) -> String {
    let name = match code {
        0x01 => "VGA 1",
        0x02 => "VGA 2",
        0x03 => "DVI 1",
        0x04 => "DVI 2",
        0x05 => "Composite 1",
        0x06 => "Composite 2",
        0x07 => "S-Video 1",
        0x08 => "S-Video 2",
        0x09 => "Tuner 1",
        0x0A => "Tuner 2",
        0x0B => "Tuner 3",
        0x0C => "Component 1",
        0x0D => "Component 2",
        0x0E => "Component 3",
        0x0F => "DisplayPort 1",
        0x10 => "DisplayPort 2",
        0x11 => "HDMI 1",
        0x12 => "HDMI 2",
        0x13 => "HDMI 3",
        0x19 => "USB-C 2",
        0x1A => "USB-C 3",
        0x1B => "USB-C",
        _ => return format!("Input 0x{code:02X}"),
    };
    name.to_string()
}

/// Parsed DDC/CI capabilities string, e.g.
/// `(prot(monitor)type(lcd)model(U2723QE)vcp(10 12 60(0F 11 1B)))`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Capabilities {
    pub model: Option<String>,
    /// VCP code -> allowed values (empty when the code is continuous).
    pub vcp: BTreeMap<u8, Vec<u8>>,
}

impl Capabilities {
    pub fn supports(&self, code: u8) -> bool {
        self.vcp.contains_key(&code)
    }

    pub fn input_sources(&self) -> Vec<u8> {
        self.vcp.get(&VCP_INPUT_SOURCE).cloned().unwrap_or_default()
    }
}

pub fn parse_capabilities(raw: &str) -> Capabilities {
    let raw = raw.trim().trim_end_matches('\0');
    let body = strip_outer_parens(raw);
    let mut caps = Capabilities::default();
    for (key, value) in top_level_entries(body) {
        match key.to_ascii_lowercase().as_str() {
            "model" => caps.model = Some(value.trim().to_string()).filter(|m| !m.is_empty()),
            "vcp" => caps.vcp = parse_vcp_list(value),
            _ => {}
        }
    }
    caps
}

fn strip_outer_parens(s: &str) -> &str {
    let s = s.trim();
    match (s.strip_prefix('('), s.ends_with(')')) {
        (Some(inner), true) => &inner[..inner.len() - 1],
        _ => s,
    }
}

/// Splits `key(value)key2(value2)` into pairs, honouring nested parens.
fn top_level_entries(s: &str) -> Vec<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut entries = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'(' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key = s[key_start..i].trim();
        let value_start = i + 1;
        let mut depth = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let value_end = i.min(bytes.len());
        entries.push((key, &s[value_start..value_end]));
        i += 1;
    }
    entries
}

/// Parses `10 12 14(05 08 0B) 60(0F 11)`; tolerates missing spaces such as `60(0F11)`.
fn parse_vcp_list(s: &str) -> BTreeMap<u8, Vec<u8>> {
    let mut map = BTreeMap::new();
    let mut chars = s.chars().filter(|c| !c.is_whitespace()).peekable();
    let mut current: Option<u8> = None;
    let mut hex = String::new();
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                let mut values = Vec::new();
                let mut depth = 1;
                let mut buf = String::new();
                for c in chars.by_ref() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        c if c.is_ascii_hexdigit() && depth == 1 => {
                            buf.push(c);
                            if buf.len() == 2 {
                                values.push(u8::from_str_radix(&buf, 16).unwrap());
                                buf.clear();
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(code) = current.take() {
                    map.insert(code, values);
                }
            }
            c if c.is_ascii_hexdigit() => {
                hex.push(c);
                if hex.len() == 2 {
                    let code = u8::from_str_radix(&hex, 16).unwrap();
                    map.insert(code, Vec::new());
                    current = Some(code);
                    hex.clear();
                }
            }
            _ => {}
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dell_capabilities() {
        let raw = "(prot(monitor)type(lcd)model(U2723QE)cmds(01 02 03 07 0C E3 F3)vcp(02 04 05 08 10 12 14(01 04 05 06 08 09 0B 0C) 16 18 1A 52 60(1B 0F 11 ) AA(01 02 04 ) AC AE B2 B6 C6 C8 C9 CC(02 03 04 06 09 0A 0D 0E) D6(01 04 05) DC(00 03 05 ) DF E0 E1 E2(00 1D 02 04 0E 12 14 23 24 27) F0(00 08) F1 F2 FD)mswhql(1)asset_eep(40)mccs_ver(2.1))";
        let caps = parse_capabilities(raw);
        assert_eq!(caps.model.as_deref(), Some("U2723QE"));
        assert!(caps.supports(VCP_BRIGHTNESS));
        assert!(caps.supports(VCP_CONTRAST));
        assert_eq!(caps.input_sources(), vec![0x1B, 0x0F, 0x11]);
        assert_eq!(caps.vcp[&0xAA], vec![0x01, 0x02, 0x04]);
        assert!(caps.vcp[&0x10].is_empty());
    }

    #[test]
    fn tolerates_missing_spaces_and_nul() {
        let caps = parse_capabilities("(vcp(1012 60(0F1112))model(X))\0");
        assert_eq!(caps.input_sources(), vec![0x0F, 0x11, 0x12]);
        assert!(caps.supports(0x10) && caps.supports(0x12));
        assert_eq!(caps.model.as_deref(), Some("X"));
    }

    #[test]
    fn ignores_vcpname_key() {
        let caps = parse_capabilities("(vcp(10 60(11 12))vcpname(10(Brightness)))");
        assert_eq!(caps.vcp.len(), 2);
    }

    #[test]
    fn empty_or_garbage_yields_empty_caps() {
        assert_eq!(parse_capabilities(""), Capabilities::default());
        assert!(parse_capabilities("garbage").vcp.is_empty());
    }

    #[test]
    fn names_inputs() {
        assert_eq!(input_source_name(0x11), "HDMI 1");
        assert_eq!(input_source_name(0x0F), "DisplayPort 1");
        assert_eq!(input_source_name(0xD0), "Input 0xD0");
    }
}
