//! User settings and data files under `%APPDATA%\tarsier`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::breaks::BreakSettings;
use crate::display::InputProtocolPref;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub breaks: BreakConfig,
    pub hotkeys: Hotkeys,
    /// Brightness change per hotkey press, in percent of the monitor's range.
    pub brightness_step: u32,
    /// Per-monitor preferences keyed by monitor id.
    pub monitors: BTreeMap<String, MonitorPrefs>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            breaks: BreakConfig::default(),
            hotkeys: Hotkeys::default(),
            brightness_step: 10,
            monitors: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BreakConfig {
    pub enabled: bool,
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub snooze_minutes: u32,
    /// Hold reminders while a fullscreen app or presentation is running.
    pub respect_fullscreen: bool,
}

impl Default for BreakConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            work_minutes: 50,
            break_minutes: 5,
            snooze_minutes: 5,
            respect_fullscreen: true,
        }
    }
}

impl BreakConfig {
    pub fn settings(&self) -> BreakSettings {
        BreakSettings {
            work_secs: self.work_minutes.max(1) as u64 * 60,
            break_secs: self.break_minutes.max(1) as u64 * 60,
            snooze_secs: self.snooze_minutes.max(1) as u64 * 60,
        }
    }
}

/// Global hotkeys in `global-hotkey` syntax, e.g. `ctrl+alt+I`; empty disables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub toggle_input: String,
    pub brightness_up: String,
    pub brightness_down: String,
    pub break_now: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle_input: "ctrl+alt+I".into(),
            brightness_up: "ctrl+alt+PageUp".into(),
            brightness_down: "ctrl+alt+PageDown".into(),
            break_now: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorPrefs {
    /// The two inputs the toggle hotkey flips between.
    pub toggle: Option<[u8; 2]>,
    /// Custom labels, e.g. `{"17": "台式机", "15": "笔记本"}` (keys are decimal VCP values).
    pub input_names: BTreeMap<u8, String>,
    /// Extra input codes for monitors that under-report their capabilities.
    pub extra_inputs: Vec<u8>,
    /// Forces how inputs are switched; detected from the monitor when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_protocol: Option<InputProtocolPref>,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("tarsier")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn stats_path() -> PathBuf {
    data_dir().join("stats.json")
}

/// Loads a JSON file, falling back to defaults when missing or unreadable.
pub fn load<T: DeserializeOwned + Default>(path: &PathBuf) -> T {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("ignoring malformed {}: {e}", path.display());
            T::default()
        }),
        Err(_) => T::default(),
    }
}

/// Writes atomically (temp file + rename) so a crash never leaves half a file.
pub fn save<T: Serialize>(path: &PathBuf, value: &T) -> Result<()> {
    fs::create_dir_all(data_dir())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp, path).with_context(|| format!("saving {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let cfg: Config = serde_json::from_str(r#"{"breaks":{"work_minutes":30}}"#).unwrap();
        assert_eq!(cfg.breaks.work_minutes, 30);
        assert_eq!(cfg.breaks.break_minutes, 5);
        assert_eq!(cfg.hotkeys.toggle_input, "ctrl+alt+I");
    }

    #[test]
    fn monitor_prefs_round_trip() {
        let mut cfg = Config::default();
        let prefs = cfg.monitors.entry("m".into()).or_default();
        prefs.toggle = Some([0x0F, 0x11]);
        prefs.input_names.insert(0x11, "笔记本".into());
        prefs.input_protocol = serde_json::from_str(r#"{"kind":"lg","values":{"16":210}}"#).unwrap();
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<Config>(&json).unwrap(), cfg);
    }
}
