//! User settings and data files under `%APPDATA%\tarsier`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use gpui_kit::WindowAppearance;
use gpui_kit::component::ThemeMode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::breaks::BreakSettings;
use crate::display::InputProtocolPref;
use crate::i18n::{Language, tr};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub breaks: BreakConfig,
    pub hotkeys: Hotkeys,
    /// Brightness change per hotkey press, in percent of the monitor's range.
    pub brightness_step: u32,
    /// Per-monitor preferences keyed by monitor id.
    pub monitors: BTreeMap<String, MonitorPrefs>,
    /// Shows DDC/CI diagnostics in the UI and logs at debug level.
    pub developer_mode: bool,
    /// Light, dark, or follow the system.
    pub theme: ThemePref,
    /// The language every window is drawn in.
    pub language: Language,
}

/// Which appearance the UI uses. `System` tracks Windows and follows it live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePref {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemePref {
    /// Label for the settings row, in picker order.
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => tr!("Light"),
            Self::Dark => tr!("Dark"),
            Self::System => tr!("System"),
        }
    }

    pub const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::System];

    /// The mode to apply, given what the system currently reports. GPUI maps
    /// Windows' `AppsUseLightTheme` to a `WindowAppearance`, so `System` needs
    /// no registry reading of our own.
    pub fn resolve(self, system: WindowAppearance) -> ThemeMode {
        match self {
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
            Self::System => system.into(),
        }
    }
}

impl Config {
    /// Brings a config written by an older version up to date. Idempotent, and
    /// called once after loading.
    ///
    /// A monitor used to describe its two inputs as an unordered `toggle` pair.
    /// That pair is now an ordered `endpoints` list — same information, but the
    /// order is meaningful once a monitor is shared by three computers.
    pub fn migrate(&mut self) {
        for prefs in self.monitors.values_mut() {
            if prefs.endpoints.is_empty()
                && let Some([a, b]) = prefs.toggle.take()
            {
                prefs.endpoints.push(a);
                if b != a {
                    prefs.endpoints.push(b);
                }
            }
            // `local_input` did not exist; the toggle's first slot is the best
            // guess at which port this computer was on, and it is only ever
            // used for display, so a wrong guess costs nothing.
            if prefs.local_input.is_none() {
                prefs.local_input = prefs.endpoints.first().copied();
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            breaks: BreakConfig::default(),
            hotkeys: Hotkeys::default(),
            brightness_step: 10,
            monitors: BTreeMap::new(),
            developer_mode: false,
            theme: ThemePref::default(),
            language: Language::default(),
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
    /// The computers this monitor is shared between, in quick-switch order.
    /// Order matters: with three or more it is the order of the number keys.
    pub endpoints: Vec<u8>,
    /// Which input this computer is plugged into. Display only — switching
    /// reads the monitor's own current input and never trusts this.
    pub local_input: Option<u8>,
    /// Superseded by [`Self::endpoints`]; read for compatibility, upgraded by
    /// [`Config::migrate`] and never written back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle: Option<[u8; 2]>,
    /// Custom labels, e.g. `{"17": "Desktop", "15": "Laptop"}` (keys are decimal VCP values).
    /// These are the computer names shown in the UI, tray and quick-switch panel.
    pub input_names: BTreeMap<u8, String>,
    /// Extra input codes for monitors that under-report their capabilities.
    pub extra_inputs: Vec<u8>,
    /// Forces how inputs are switched; detected from the monitor when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_protocol: Option<InputProtocolPref>,
}

impl MonitorPrefs {
    /// The name to show for an input: the user's, else the MCCS default.
    pub fn label(&self, port: u8) -> String {
        self.input_names
            .get(&port)
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| crate::display::mccs::input_source_name(port))
    }

    /// Whether the user gave this port a name of their own.
    pub fn is_named(&self, port: u8) -> bool {
        self.input_names.get(&port).is_some_and(|n| !n.trim().is_empty())
    }

    /// The part of a monitor's settings that is a property of the *monitor*
    /// rather than of this computer, for handing to another machine.
    ///
    /// `local_input` is deliberately dropped: every computer is plugged into a
    /// different port, so the receiving machine works that out for itself.
    pub fn portable(&self) -> MonitorPrefs {
        MonitorPrefs {
            endpoints: self.endpoints.clone(),
            local_input: None,
            toggle: None,
            input_names: self.input_names.clone(),
            extra_inputs: self.extra_inputs.clone(),
            input_protocol: self.input_protocol.clone(),
        }
    }

    /// Points one endpoint at a different port. If another computer is already
    /// there the two swap, so a port never hosts two computers. Names belong to
    /// the computer rather than the socket, so they travel with it.
    pub fn move_endpoint(&mut self, from: u8, to: u8) {
        if from == to {
            return;
        }
        let Some(leaving_ix) = self.endpoints.iter().position(|p| *p == from) else {
            return;
        };
        if let Some(arriving_ix) = self.endpoints.iter().position(|p| *p == to) {
            self.endpoints[arriving_ix] = from;
        }
        self.endpoints[leaving_ix] = to;

        let leaving = self.input_names.remove(&from);
        let arriving = self.input_names.remove(&to);
        if let Some(name) = arriving {
            self.input_names.insert(from, name);
        }
        if let Some(name) = leaving {
            self.input_names.insert(to, name);
        }
        if self.local_input == Some(from) {
            self.local_input = Some(to);
        }
    }
}

/// The endpoint list and names of every monitor, carried to another computer
/// through the clipboard.
///
/// Keyed by monitor *name* rather than by the `monitors` key: that key is a
/// Windows device path, which depends on the machine's graphics topology and
/// therefore does not survive the trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchingExport {
    pub version: u32,
    pub monitors: BTreeMap<String, MonitorPrefs>,
}

pub const SWITCHING_EXPORT_VERSION: u32 = 1;

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
    fn theme_pref_resolves_against_the_system() {
        let sys_dark = WindowAppearance::Dark;
        assert_eq!(ThemePref::default(), ThemePref::System);
        assert_eq!(ThemePref::System.resolve(sys_dark), ThemeMode::Dark);
        assert_eq!(ThemePref::System.resolve(WindowAppearance::Light), ThemeMode::Light);
        // An explicit choice ignores the system.
        assert_eq!(ThemePref::Light.resolve(sys_dark), ThemeMode::Light);
        assert_eq!(ThemePref::Dark.resolve(WindowAppearance::Light), ThemeMode::Dark);
    }

    #[test]
    fn theme_pref_round_trips_as_snake_case() {
        let json = serde_json::to_string(&ThemePref::System).unwrap();
        assert_eq!(json, r#""system""#);
        let cfg: Config = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(cfg.theme, ThemePref::Dark);
        // Older config files without the key follow the system.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.theme, ThemePref::System);
    }

    #[test]
    fn monitor_prefs_round_trip() {
        let mut cfg = Config::default();
        let prefs = cfg.monitors.entry("m".into()).or_default();
        prefs.endpoints = vec![0x0F, 0x11, 0x12];
        prefs.local_input = Some(0x11);
        prefs.input_names.insert(0x11, "Laptop".into());
        prefs.input_protocol = serde_json::from_str(r#"{"kind":"lg","values":{"16":210}}"#).unwrap();
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<Config>(&json).unwrap(), cfg);
    }

    #[test]
    fn old_toggle_pair_becomes_endpoints() {
        let mut cfg: Config = serde_json::from_str(
            r#"{"monitors":{"DEL41A3":{"toggle":[15,17],"input_names":{"17":"Desktop","15":"Laptop"}}}}"#,
        )
        .unwrap();
        cfg.migrate();
        let prefs = &cfg.monitors["DEL41A3"];
        assert_eq!(prefs.endpoints, vec![15, 17], "order is kept");
        assert_eq!(prefs.local_input, Some(15));
        assert_eq!(prefs.label(17), "Desktop", "names survive the upgrade");
        assert_eq!(prefs.label(15), "Laptop");
        // The legacy key is consumed rather than left behind.
        assert_eq!(prefs.toggle, None);
        assert!(!serde_json::to_string(prefs).unwrap().contains("toggle"));
    }

    #[test]
    fn migration_is_idempotent_and_keeps_newer_config() {
        let mut cfg: Config =
            serde_json::from_str(r#"{"monitors":{"m":{"endpoints":[16,18,17],"toggle":[15,17]}}}"#).unwrap();
        cfg.migrate();
        assert_eq!(cfg.monitors["m"].endpoints, vec![16, 18, 17], "configured list wins");
        // A degenerate old pair must not produce a duplicated endpoint.
        let mut cfg: Config = serde_json::from_str(r#"{"monitors":{"m":{"toggle":[15,15]}}}"#).unwrap();
        cfg.migrate();
        assert_eq!(cfg.monitors["m"].endpoints, vec![15]);
    }

    #[test]
    fn labels_fall_back_to_the_mccs_name() {
        let mut prefs = MonitorPrefs::default();
        assert_eq!(prefs.label(0x10), "DisplayPort 2");
        assert!(!prefs.is_named(0x10));
        prefs.input_names.insert(0x10, "  ".into());
        assert_eq!(prefs.label(0x10), "DisplayPort 2", "blank names are ignored");
        prefs.input_names.insert(0x10, "Desktop".into());
        assert_eq!(prefs.label(0x10), "Desktop");
        assert!(prefs.is_named(0x10));
    }

    #[test]
    fn moving_an_endpoint_takes_its_name_along() {
        let mut prefs = MonitorPrefs {
            endpoints: vec![0x0F, 0x10, 0x11],
            local_input: Some(0x0F),
            input_names: BTreeMap::from([(0x0Fu8, "Laptop".to_string()), (0x10u8, "Desktop".to_string())]),
            ..Default::default()
        };
        // Onto a free port: the name follows the computer, the local mark too.
        prefs.move_endpoint(0x0F, 0x12);
        assert_eq!(prefs.endpoints, vec![0x12, 0x10, 0x11]);
        assert_eq!(prefs.local_input, Some(0x12));
        assert_eq!(prefs.label(0x12), "Laptop");
        assert!(!prefs.is_named(0x0F), "the old port is left clean");

        // Onto an occupied one: the two swap, so no port hosts two computers.
        prefs.move_endpoint(0x12, 0x10);
        assert_eq!(prefs.endpoints, vec![0x10, 0x12, 0x11]);
        assert_eq!(prefs.label(0x10), "Laptop");
        assert_eq!(prefs.label(0x12), "Desktop");
        assert_eq!(prefs.local_input, Some(0x10));

        // Unknown ports and no-ops leave everything alone.
        let before = prefs.clone();
        prefs.move_endpoint(0x99, 0x11);
        prefs.move_endpoint(0x11, 0x11);
        assert_eq!(prefs, before);
    }

    #[test]
    fn a_portable_copy_drops_what_is_local_to_this_machine() {
        let prefs = MonitorPrefs {
            endpoints: vec![0x0F, 0x10],
            local_input: Some(0x0F),
            input_names: BTreeMap::from([(0x0Fu8, "Laptop".to_string())]),
            extra_inputs: vec![0x1B],
            ..Default::default()
        };
        let carried = prefs.portable();
        assert_eq!(carried.endpoints, prefs.endpoints);
        assert_eq!(carried.input_names, prefs.input_names);
        assert_eq!(carried.extra_inputs, vec![0x1B]);
        assert_eq!(
            carried.local_input, None,
            "which port this computer is on cannot travel to another one"
        );

        // And it has to survive the clipboard round trip intact.
        let export = SwitchingExport {
            version: SWITCHING_EXPORT_VERSION,
            monitors: BTreeMap::from([("27GP950".to_string(), carried)]),
        };
        let back: SwitchingExport = serde_json::from_str(&serde_json::to_string(&export).unwrap()).unwrap();
        assert_eq!(back.version, SWITCHING_EXPORT_VERSION);
        assert_eq!(back.monitors["27GP950"].endpoints, vec![0x0F, 0x10]);
        assert_eq!(back.monitors["27GP950"].label(0x0F), "Laptop");
        assert_eq!(back.monitors["27GP950"].local_input, None);
    }
}
