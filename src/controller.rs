//! Application state shared by the tray, hotkeys and windows. Lives as a
//! GPUI entity for the whole process; windows come and go around it.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, NaiveDateTime};
use gpui_kit::component::Theme;
use gpui_kit::*;

use crate::breaks::{BreakEvent, BreakKind, BreakTracker, Phase};
use crate::config::{self, Config, MonitorPrefs};
use crate::display::mccs::{self, VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::display::{self, Feature};
use crate::evening;
use crate::i18n::{self, Language, tr};
use crate::platform;
use crate::stats::{self, Outcome, Reminder, Session, Stats, activity_day};
use crate::tray;
use crate::ui;
use crate::ui::break_overlay::{BreakOverlay, FADE_OUT};
use crate::ui::switch_hud::{self, SwitchHud};

pub struct MonitorEntry {
    pub dev: Arc<display::Monitor>,
    pub brightness: Option<Feature>,
    pub contrast: Option<Feature>,
    pub current_input: Option<u8>,
}

impl MonitorEntry {
    pub fn id(&self) -> &str {
        &self.dev.id
    }

    /// Whether the input list came from the monitor itself.
    pub fn inputs_reported(&self) -> bool {
        !self.dev.input_sources().is_empty()
    }

    /// A monitor at half brightness and contrast, on its first input.
    #[cfg(test)]
    pub fn fake(id: &str, name: &str, inputs: &[u8]) -> Self {
        let half = Feature { current: 50, max: 100 };
        MonitorEntry {
            dev: Arc::new(display::Monitor::fake(id, name, inputs)),
            brightness: Some(half),
            contrast: Some(half),
            current_input: inputs.first().copied(),
        }
    }
}

#[derive(Default)]
struct WriteSlot {
    desired: u32,
    inflight: bool,
}

pub struct Controller {
    pub config: Config,
    pub monitors: Vec<MonitorEntry>,
    pub scanning: bool,
    pub tracker: BreakTracker,
    pub stats: Stats,
    pub paused_until: Option<i64>,
    pub autostart: bool,
    /// Last user-facing message (errors, hotkey results).
    pub notice: Option<SharedString>,
    pub hotkey_errors: Vec<tray::HotkeyError>,
    /// The process's registered global hotkeys, when a manager could be made.
    /// Owned here so the Settings tab can re-register in place; the manager
    /// itself lives in `main`, on the thread that created its hidden window.
    pub hotkeys: Option<Rc<RefCell<tray::HotkeyRegistry>>>,
    /// Whether the global hotkeys are released for a recording in progress.
    /// Tracked here as well as in the window, because the window can go away
    /// mid-recording and something has to hand the hotkeys back.
    hotkey_hold: bool,
    writes: HashMap<(String, u8), WriteSlot>,
    overlays: Vec<(WindowHandle<BreakOverlay>, Entity<BreakOverlay>)>,
    /// What the evening cutoff shows right now, worked out every tick.
    pub evening: evening::Status,
    /// The local time `evening` was worked out at, so every window draws the
    /// same minute.
    evening_at: NaiveDateTime,
    /// The day whose heads-up the user closed.
    heads_up_closed: Option<NaiveDate>,
    /// The evening's windows, and what they were opened for.
    evening_windows: EveningWindows,
    /// One quick-switch panel per display while it is open.
    switch_huds: Vec<WindowHandle<SwitchHud>>,
    /// Set from the request until the panels exist: they are built in a
    /// deferred callback, and a second press before then must not build more.
    switch_hud_opening: bool,
    /// Bumped on every open, so a timeout only closes the panel it was set for.
    switch_hud_generation: u64,
    /// The window that had focus before the panel took it, to hand it back.
    switch_hud_return_to: Option<isize>,
    last_tick: Instant,
    last_save: Instant,
    stats_dirty: bool,
    pub main_window: Option<AnyWindowHandle>,
    storage: Storage,
    /// Tests set the time the evening is worked out at.
    #[cfg(test)]
    pub fake_now: Option<i64>,
}

/// Which of the evening's windows are wanted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Shown {
    #[default]
    Nothing,
    HeadsUp,
    Cutoff,
}

impl Shown {
    fn of(status: evening::Status) -> Self {
        match status {
            evening::Status::HeadsUp { .. } => Self::HeadsUp,
            evening::Status::Cutoff { .. } => Self::Cutoff,
            evening::Status::Quiet | evening::Status::KeepingOn { .. } => Self::Nothing,
        }
    }
}

/// The evening's windows. Opened in a deferred callback, so `shown` is set
/// when they are asked for and the handles arrive a moment later.
#[derive(Default)]
struct EveningWindows {
    shown: Shown,
    heads_up: Option<WindowHandle<ui::evening::HeadsUpCard>>,
    overlays: Vec<WindowHandle<ui::evening::CutoffOverlay>>,
    keep_using: Option<WindowHandle<ui::evening::KeepUsing>>,
}

/// A borderless window on `display` that floats above everything and does
/// not take focus when it appears. The caller sets where it goes.
fn popup_options(display: DisplayId) -> WindowOptions {
    WindowOptions {
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(display),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

/// How long a notice stays at the foot of the main window.
const NOTICE_LIFETIME: Duration = Duration::from_secs(8);

struct GlobalController(Entity<Controller>);
impl Global for GlobalController {}

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

/// Where a blind flip lands, or `None` when the monitor is not on either
/// endpoint — it is showing a third device, or the reading cannot be trusted.
///
/// Guessing in that case would send the monitor somewhere the user did not ask
/// for, and guessing from a remembered value would make the flip get stuck on
/// one side. So the caller asks instead.
pub fn flip_destination(pair: [u8; 2], current: Option<u8>) -> Option<u8> {
    match current {
        Some(port) if port == pair[0] => Some(pair[1]),
        Some(port) if port == pair[1] => Some(pair[0]),
        _ => None,
    }
}

/// Reads the history, bringing an older file up to date. The old file is kept
/// beside it first, as `stats.v1.json`: the upgrade drops what the new format
/// has no place for, and that should never be the only copy.
fn load_stats(path: &std::path::Path) -> Stats {
    let loaded: Stats = config::load(path);
    if !loaded.is_outdated() {
        return loaded;
    }
    let backup = path.with_file_name("stats.v1.json");
    if !backup.exists()
        && let Err(e) = std::fs::copy(path, &backup)
    {
        log::error!("keeping {} as {}: {e}", path.display(), backup.display());
    }
    let upgraded = loaded.upgrade(activity_day);
    if let Err(e) = config::save(path, &upgraded) {
        log::error!("saving the upgraded {}: {e:#}", path.display());
    }
    upgraded
}

/// Where the controller keeps what it owns between runs.
#[derive(Clone)]
pub enum Storage {
    Disk {
        config: PathBuf,
        stats: PathBuf,
    },
    /// Nothing is written. Tests use this, so they can never touch the user's
    /// real settings.
    #[cfg_attr(not(test), allow(dead_code))]
    Memory,
}

impl Storage {
    /// The files in [`config::data_dir`].
    pub fn default_disk() -> Self {
        Self::Disk {
            config: config::config_path(),
            stats: config::stats_path(),
        }
    }
}

impl Controller {
    /// The running app's controller: loads the saved state, starts the first
    /// monitor scan and the break ticker, and registers the global handle the
    /// popup windows look it up through.
    pub fn init(cx: &mut App) -> Entity<Controller> {
        let storage = Storage::default_disk();
        let (mut config, stats): (Config, Stats) = match &storage {
            Storage::Disk { config: c, stats: s } => (config::load(c), load_stats(s)),
            Storage::Memory => Default::default(),
        };
        config.migrate();
        crate::logger::set_verbose(config.developer_mode);
        let autostart = platform::autostart_enabled();
        let entity = cx.new(|cx| {
            let mut this = Controller::new(config, stats, storage, autostart);
            this.refresh_monitors(cx);
            this.start_ticker(cx);
            // Runs on tray "Quit" and on Windows logoff/shutdown.
            cx.on_app_quit(|this, _| {
                this.shutdown();
                async {}
            })
            .detach();
            this
        });
        cx.set_global(GlobalController(entity.clone()));
        entity
    }

    /// A controller over the given state, with no side effects: nothing is
    /// scanned, nothing ticks, and nothing is read from disk.
    pub fn new(config: Config, stats: Stats, storage: Storage, autostart: bool) -> Self {
        // Before any window is built: every label is looked up as it is drawn.
        i18n::set_language(config.language);
        let tracker = BreakTracker::new(config.breaks.settings(), now_ts());
        Controller {
            config,
            monitors: Vec::new(),
            scanning: false,
            tracker,
            stats,
            paused_until: None,
            autostart,
            notice: None,
            hotkey_errors: Vec::new(),
            hotkeys: None,
            hotkey_hold: false,
            writes: HashMap::new(),
            overlays: Vec::new(),
            evening: evening::Status::Quiet,
            evening_at: stats::local_time(now_ts()),
            heads_up_closed: None,
            evening_windows: EveningWindows::default(),
            switch_huds: Vec::new(),
            switch_hud_opening: false,
            switch_hud_generation: 0,
            switch_hud_return_to: None,
            last_tick: Instant::now(),
            last_save: Instant::now(),
            stats_dirty: false,
            main_window: None,
            storage,
            #[cfg(test)]
            fake_now: None,
        }
    }

    /// A controller over in-memory state and the given monitors, registered as
    /// the global one, for UI tests.
    #[cfg(test)]
    pub fn for_test(config: Config, monitors: Vec<MonitorEntry>, cx: &mut App) -> Entity<Controller> {
        let entity = cx.new(|_| {
            let mut this = Controller::new(config, Stats::default(), Storage::Memory, false);
            this.monitors = monitors;
            this
        });
        cx.set_global(GlobalController(entity.clone()));
        entity
    }

    pub fn global(cx: &App) -> Entity<Controller> {
        cx.global::<GlobalController>().0.clone()
    }

    // ---- monitors -------------------------------------------------------

    pub fn refresh_monitors(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.notify();
        let prefs: BTreeMap<_, _> = self
            .config
            .monitors
            .iter()
            .filter_map(|(id, p)| Some((id.clone(), p.input_protocol.clone()?)))
            .collect();
        let scan = cx.background_executor().spawn(async move {
            let monitors = display::enumerate(&prefs).unwrap_or_default();
            monitors
                .into_iter()
                .map(|dev| {
                    let dev = Arc::new(dev);
                    MonitorEntry {
                        brightness: dev.get(VCP_BRIGHTNESS).ok(),
                        contrast: dev.get(VCP_CONTRAST).ok(),
                        current_input: dev.current_input(),
                        dev,
                    }
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let monitors = scan.await;
            this.update(cx, |this, cx| {
                this.monitors = monitors;
                this.scanning = false;
                this.writes.clear();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn monitor_prefs(&self, id: &str) -> MonitorPrefs {
        self.config.monitors.get(id).cloned().unwrap_or_default()
    }

    /// Inputs to offer for a monitor: reported ones (or a common fallback) plus extras.
    pub fn inputs_for(&self, entry: &MonitorEntry) -> Vec<u8> {
        let mut inputs = entry.dev.input_sources();
        if inputs.is_empty() {
            inputs = mccs::FALLBACK_INPUTS.to_vec();
        }
        for extra in self.monitor_prefs(entry.id()).extra_inputs {
            if !inputs.contains(&extra) {
                inputs.push(extra);
            }
        }
        if let Some(cur) = entry.current_input
            && !inputs.contains(&cur)
        {
            inputs.push(cur);
        }
        inputs
    }

    pub fn input_label(&self, monitor_id: &str, code: u8) -> String {
        self.monitor_prefs(monitor_id).label(code)
    }

    /// The ports this monitor is shared between, in quick-switch order.
    ///
    /// Falls back to the only two inputs a monitor reports, so one that is
    /// plainly shared by two computers needs no setup at all.
    pub fn endpoints(&self, entry: &MonitorEntry) -> Vec<u8> {
        let configured = self.monitor_prefs(entry.id()).endpoints;
        if !configured.is_empty() {
            return configured;
        }
        let reported = entry.dev.input_sources();
        if reported.len() == 2 { reported } else { Vec::new() }
    }

    /// Whether the user has been through setup for this monitor, as opposed to
    /// the two-input fallback above.
    pub fn endpoints_configured(&self, entry: &MonitorEntry) -> bool {
        !self.monitor_prefs(entry.id()).endpoints.is_empty()
    }

    /// The port this computer is plugged into; display only, never trusted for
    /// switching.
    pub fn local_input(&self, entry: &MonitorEntry) -> Option<u8> {
        self.monitor_prefs(entry.id()).local_input
    }

    /// At least one monitor has three or more computers, so a single hotkey can
    /// no longer say what it will do and has to ask.
    pub fn needs_picker(&self) -> bool {
        self.monitors.iter().any(|m| self.endpoints(m).len() > 2)
    }

    /// Replaces the endpoint list, e.g. when the setup wizard finishes. Names
    /// this computer after itself — the only endpoint name that can be
    /// inferred rather than asked for.
    pub fn set_endpoints(&mut self, monitor_id: &str, ports: Vec<u8>, local: Option<u8>, cx: &mut Context<Self>) {
        let prefs = self.config.monitors.entry(monitor_id.to_string()).or_default();
        prefs.endpoints = ports;
        if local.is_some() {
            prefs.local_input = local;
        }
        if let Some(port) = prefs.local_input
            && !prefs.is_named(port)
        {
            prefs.input_names.insert(port, platform::local_hostname());
        }
        self.save_config();
        cx.notify();
    }

    /// Points an endpoint at a different port. If another endpoint is already
    /// there the two swap, so a port never hosts two computers.
    pub fn set_endpoint_port(&mut self, monitor_id: &str, from: u8, to: u8, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        self.config
            .monitors
            .entry(monitor_id.to_string())
            .or_default()
            .move_endpoint(from, to);
        self.save_config();
        cx.notify();
    }

    /// Adds one computer to a monitor's list. The new row starts unnamed; the
    /// UI focuses it straight away.
    pub fn add_endpoint(&mut self, monitor_id: &str, port: u8, cx: &mut Context<Self>) {
        let prefs = self.config.monitors.entry(monitor_id.to_string()).or_default();
        if prefs.endpoints.contains(&port) {
            return;
        }
        prefs.endpoints.push(port);
        self.save_config();
        cx.notify();
    }

    /// Drops a monitor back to fewer computers. Removing this computer forgets
    /// which port it is on rather than guessing another.
    pub fn remove_endpoint(&mut self, monitor_id: &str, port: u8, cx: &mut Context<Self>) {
        let prefs = self.config.monitors.entry(monitor_id.to_string()).or_default();
        prefs.endpoints.retain(|p| *p != port);
        if prefs.local_input == Some(port) {
            prefs.local_input = None;
        }
        self.save_config();
        cx.notify();
    }

    /// Renames the computer on one port. A blank name falls back to the
    /// monitor's own `DisplayPort 2` style label.
    pub fn set_input_name(&mut self, monitor_id: &str, port: u8, name: String, cx: &mut Context<Self>) {
        let prefs = self.config.monitors.entry(monitor_id.to_string()).or_default();
        if name.trim().is_empty() {
            prefs.input_names.remove(&port);
        } else {
            prefs.input_names.insert(port, name.trim().to_string());
        }
        self.save_config();
        cx.notify();
    }

    /// Marks which port this computer is on. Decoration only — it changes what
    /// the UI says, never where the switch goes.
    pub fn set_local_input(&mut self, monitor_id: &str, port: u8, cx: &mut Context<Self>) {
        let default_name = platform::local_hostname();
        let prefs = self.config.monitors.entry(monitor_id.to_string()).or_default();
        prefs.local_input = Some(port);
        if !prefs.is_named(port) {
            prefs.input_names.insert(port, default_name);
        }
        self.save_config();
        cx.notify();
    }

    /// Update a continuous VCP feature. Writes are coalesced: while one write
    /// is on the wire, later values just replace the pending one.
    pub fn set_feature(&mut self, monitor_id: &str, code: u8, value: u32, cx: &mut Context<Self>) {
        let Some(entry) = self.monitors.iter_mut().find(|m| m.id() == monitor_id) else {
            return;
        };
        let feature = match code {
            VCP_BRIGHTNESS => &mut entry.brightness,
            VCP_CONTRAST => &mut entry.contrast,
            _ => return,
        };
        let mut value = value;
        if let Some(f) = feature {
            value = value.min(f.max);
            if f.current == value {
                return;
            }
            f.current = value;
        }
        let dev = entry.dev.clone();
        let key = (monitor_id.to_string(), code);
        let slot = self.writes.entry(key.clone()).or_default();
        slot.desired = value;
        if slot.inflight {
            return;
        }
        slot.inflight = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                let writer = dev.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { writer.set(code, value) })
                    .await;
                let failed = result.is_err();
                let next = this
                    .update(cx, |this, cx| {
                        if let Err(e) = result {
                            let name = dev_name(&this.monitors, &key.0);
                            this.show_notice(format!("{name}: {e:#}"), cx);
                        }
                        let slot = this.writes.get_mut(&key)?;
                        if slot.desired == value || failed {
                            slot.inflight = false;
                            None
                        } else {
                            Some(slot.desired)
                        }
                    })
                    .ok()
                    .flatten();
                match next {
                    Some(v) => value = v,
                    None => {
                        // The slider already shows the value that was refused.
                        // Read back what the monitor holds, so the UI stops
                        // claiming otherwise and the same value can be retried.
                        if failed {
                            let dev = dev.clone();
                            let actual = cx.background_executor().spawn(async move { dev.get(code).ok() }).await;
                            this.update(cx, |this, cx| this.store_feature(&key.0, code, actual, cx))
                                .ok();
                        }
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn store_feature(&mut self, monitor_id: &str, code: u8, value: Option<Feature>, cx: &mut Context<Self>) {
        let Some(entry) = self.monitors.iter_mut().find(|m| m.id() == monitor_id) else {
            return;
        };
        match code {
            VCP_BRIGHTNESS => entry.brightness = value.or(entry.brightness),
            VCP_CONTRAST => entry.contrast = value.or(entry.contrast),
            _ => return,
        }
        cx.notify();
    }

    /// Change brightness on every monitor by `step_percent` of its range.
    pub fn nudge_brightness(&mut self, up: bool, cx: &mut Context<Self>) {
        let step = self.config.brightness_step.max(1);
        let targets: Vec<(String, u32)> = self
            .monitors
            .iter()
            .filter_map(|m| {
                let f = m.brightness?;
                let delta = (f.max * step / 100).max(1);
                let v = if up {
                    (f.current + delta).min(f.max)
                } else {
                    f.current.saturating_sub(delta)
                };
                Some((m.id().to_string(), v))
            })
            .collect();
        for (id, v) in targets {
            self.set_feature(&id, VCP_BRIGHTNESS, v, cx);
        }
    }

    pub fn switch_input(&mut self, monitor_id: &str, code: u8, cx: &mut Context<Self>) {
        let Some(entry) = self.monitors.iter_mut().find(|m| m.id() == monitor_id) else {
            // Every caller passes a monitor id. A display *name* arriving here
            // failed this lookup and returned silently, which made the whole
            // tray menu look dead; say so rather than swallowing it.
            log::warn!("switch_input: no monitor with id {monitor_id:?}");
            return;
        };
        let dev = entry.dev.clone();
        let before = entry.current_input.replace(code);
        cx.notify();
        let label = self.input_label(monitor_id, code);
        let id = monitor_id.to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { dev.switch_input(code) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.show_notice(tr!("Switched to {label}", label = label), cx),
                Err(e) => {
                    if let Some(entry) = this.monitors.iter_mut().find(|m| m.id() == id) {
                        entry.current_input = before;
                    }
                    this.show_notice(tr!("Switching input failed: {e}", e = format!("{e:#}")), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The switch hotkey: flip a two-computer monitor to its other side, or ask
    /// when flipping is not a question with one answer.
    ///
    /// Asking covers two cases that used to do nothing at all: a monitor with
    /// three or more computers has no single "other one", and a monitor whose
    /// current input cannot be read — or reads as something outside the pair —
    /// has no knowable direction to flip in.
    pub fn toggle_inputs(&mut self, cx: &mut Context<Self>) {
        if self.needs_picker() {
            self.open_switch_hud(cx);
            return;
        }
        let jobs: Vec<(Arc<display::Monitor>, [u8; 2])> = self
            .monitors
            .iter()
            .filter_map(|m| {
                let ports = self.endpoints(m);
                (ports.len() == 2).then(|| (m.dev.clone(), [ports[0], ports[1]]))
            })
            .collect();
        if jobs.is_empty() {
            self.show_notice(
                tr!("No computers are set up to switch between yet. Add them in Settings → Displays."),
                cx,
            );
            self.show_main_window(cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            // Read before writing: whether a flip is even possible depends on
            // where the monitor is, so it is decided here rather than guessed.
            let reads = cx
                .background_executor()
                .spawn(async move {
                    jobs.into_iter()
                        .map(|(dev, pair)| {
                            let current = dev.current_input();
                            (dev, pair, current)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let targets = reads
                .iter()
                .map(|(_, pair, current)| flip_destination(*pair, *current))
                .collect::<Option<Vec<u8>>>();
            let Some(targets) = targets else {
                this.update(cx, |this, cx| {
                    this.show_notice(
                        tr!("The monitor will not say which input it is on, so pick one in the quick-switch panel."),
                        cx,
                    );
                    this.open_switch_hud(cx);
                })
                .ok();
                return;
            };
            let writes = reads
                .into_iter()
                .zip(targets)
                .map(|((dev, _, _), target)| (dev, target))
                .collect::<Vec<_>>();
            let results = cx
                .background_executor()
                .spawn(async move {
                    writes
                        .into_iter()
                        .map(|(dev, target)| (dev.id.clone(), target, dev.switch_input(target)))
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                let mut msgs = Vec::new();
                for (id, target, result) in results {
                    match result {
                        Ok(()) => {
                            if let Some(m) = this.monitors.iter_mut().find(|m| m.id() == id) {
                                m.current_input = Some(target);
                            }
                            msgs.push(tr!("Switched to {label}", label = this.input_label(&id, target)));
                        }
                        Err(e) => msgs.push(tr!("Switching input failed: {e}", e = format!("{e:#}"))),
                    }
                }
                this.show_notice(msgs.join(tr!("; ")), cx);
            })
            .ok();
        })
        .detach();
    }

    // ---- breaks ----------------------------------------------------------

    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn is_paused(&self) -> bool {
        self.paused_until.is_some_and(|t| t > now_ts())
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = now_ts();
        let dt = self.last_tick.elapsed().as_secs_f64().round() as u64;
        if dt == 0 {
            return;
        }
        // Advance by the seconds counted rather than resetting to now: timers
        // that fire a little late would otherwise lose that fraction every
        // tick, and a 50-minute reminder would arrive most of a minute late.
        self.last_tick += Duration::from_secs(dt);
        if self.paused_until.is_some_and(|t| t <= now) {
            self.paused_until = None;
        }
        let idle = platform::idle_secs();
        let last_input = now - idle as i64;
        if self.stats.day_mut(activity_day(last_input)).saw_input(last_input) {
            self.stats_dirty = true;
        }
        self.sync_evening(now, cx);
        let breaks = &self.config.breaks;
        // One overlay at a time: in the evening a break is not the point.
        let suppressed = !breaks.enabled
            || self.paused_until.is_some()
            || matches!(self.evening, evening::Status::Cutoff { .. })
            || (breaks.respect_fullscreen && platform::user_is_busy());
        for event in self.tracker.tick(now, dt, idle, suppressed) {
            match event {
                BreakEvent::PromptBreak => self.open_overlays(cx),
                BreakEvent::BreakFinished => {
                    self.note_reminder(now, Outcome::Rested);
                    self.close_overlays(cx);
                }
                BreakEvent::BreakIgnored => {
                    self.note_reminder(now, Outcome::Ignored);
                    self.close_overlays(cx);
                }
                BreakEvent::SessionEnded(session) => {
                    self.stats.record(activity_day(session.end), Session::from(session));
                    self.stats_dirty = true;
                }
                BreakEvent::Returned => {}
            }
        }
        if self.stats_dirty && self.last_save.elapsed() > Duration::from_secs(30) {
            self.save_stats();
        }
        // A recording holds the global hotkeys released, and the window that was
        // recording is the only thing that hands them back. If it is closed
        // first, nothing would — the hotkeys would stay dead until the next
        // recording or the next launch, with nothing on screen to say why. The
        // tick is the one piece of this program that runs whatever else happens.
        if self.hotkey_hold {
            let window_gone = match self.main_window {
                Some(handle) => handle.update(cx, |_, _, _| ()).is_err(),
                None => true,
            };
            if window_gone {
                log::warn!("the window holding the hotkeys went away; re-registering them");
                self.apply_hotkeys(cx);
            }
        }
        cx.notify();
    }

    pub fn break_now(&mut self, cx: &mut Context<Self>) {
        if self.tracker.break_now() {
            self.open_overlays(cx);
            cx.notify();
        }
    }

    pub fn snooze(&mut self, cx: &mut Context<Self>) {
        self.tracker.snooze();
        self.note_reminder(now_ts(), Outcome::Snoozed);
        self.close_overlays(cx);
        cx.notify();
    }

    pub fn skip(&mut self, cx: &mut Context<Self>) {
        self.tracker.skip();
        self.note_reminder(now_ts(), Outcome::Skipped);
        self.close_overlays(cx);
        cx.notify();
    }

    fn note_reminder(&mut self, at: i64, outcome: Outcome) {
        self.stats
            .day_mut(activity_day(at))
            .reminders
            .push(Reminder { at, outcome });
        self.stats_dirty = true;
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.paused_until = if self.is_paused() { None } else { Some(now_ts() + 3600) };
        if self.is_paused() && matches!(self.tracker.phase(), Phase::Prompted { .. }) {
            self.tracker.snooze();
            self.close_overlays(cx);
        }
        cx.notify();
    }

    /// The stretch of work still running, as a session that ends now.
    pub fn open_session(&self) -> Option<Session> {
        let t = &self.tracker;
        if t.phase() == Phase::Away || t.session_active() == 0 {
            return None;
        }
        Some(Session {
            start: t.session_start(),
            end: now_ts(),
            active_secs: t.session_active(),
            target_secs: t.settings().work_secs,
            kind: BreakKind::Natural,
        })
    }

    /// Today's time at the computer, the stretch still running included.
    pub fn worked_today(&self) -> u64 {
        let open = self.open_session();
        self.stats
            .day(activity_day(now_ts()))
            .map_or(0, |day| day.sessions_with(open.as_ref()).map(|s| s.active_secs).sum())
            .max(open.map_or(0, |s| s.active_secs))
    }

    /// Deferred: the overlay reads this entity while it is being built, and
    /// callers are usually inside `Controller::update`.
    fn open_overlays(&mut self, cx: &mut Context<Self>) {
        self.close_overlays(cx);
        let this = cx.entity();
        cx.defer(move |cx| {
            // The break may already be over (e.g. snoozed) by the time this runs.
            if !matches!(this.read(cx).tracker.phase(), Phase::Prompted { .. }) {
                return;
            }
            let mut handles = Vec::new();
            for display in cx.displays() {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(display.bounds())),
                    titlebar: None,
                    // Fadetop-style: never steal focus from what the user is doing.
                    focus: false,
                    show: true,
                    kind: WindowKind::PopUp,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    display_id: Some(display.id()),
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                };
                // No component `Root` here: it paints the theme background, which
                // would make the translucent layer opaque.
                let mut view = None;
                match cx.open_window(options, |window, cx| {
                    let overlay = cx.new(|cx| BreakOverlay::new(window, cx));
                    view = Some(overlay.clone());
                    overlay
                }) {
                    Ok(handle) => handles.extend(view.map(|v| (handle, v))),
                    Err(e) => log::error!("failed to open break overlay: {e}"),
                }
            }
            this.update(cx, |this, _| this.overlays.extend(handles));
        });
    }

    /// Fade the overlays out, then remove them. Deferred, because the request
    /// may come from code that is updating an overlay window.
    fn close_overlays(&mut self, cx: &mut Context<Self>) {
        let overlays = std::mem::take(&mut self.overlays);
        if overlays.is_empty() {
            return;
        }
        cx.defer(|cx| {
            for (_, view) in &overlays {
                view.update(cx, |o, cx| o.fade_out(cx));
            }
            cx.spawn(async move |cx| {
                cx.background_executor().timer(FADE_OUT).await;
                cx.update(|cx| {
                    for (handle, _) in overlays {
                        handle.update(cx, |_, window, _| window.remove_window()).ok();
                    }
                });
            })
            .detach();
        });
    }

    // ---- evening ---------------------------------------------------------

    /// The time now, or the time a test says it is.
    fn now(&self) -> i64 {
        #[cfg(test)]
        if let Some(now) = self.fake_now {
            return now;
        }
        now_ts()
    }

    /// The local time the evening was last worked out at.
    pub fn evening_now(&self) -> NaiveDateTime {
        self.evening_at
    }

    /// Works out what the evening shows at `now`, keeps the day's record in
    /// step with the setting, and opens or closes windows to match.
    pub fn sync_evening(&mut self, now: i64, cx: &mut Context<Self>) {
        let local = stats::local_time(now);
        let day = stats::day_of(local);
        let cutoff = self.config.evening.enabled.then_some(self.config.evening.cutoff);
        let record = self.stats.day_mut(day);
        if record.follow_cutoff(day, cutoff, local) {
            self.stats_dirty = true;
        }
        let last_extension = record.last_extension().map(|e| stats::local_time(e.at));
        let status = evening::status(local, cutoff, self.heads_up_closed == Some(day), last_extension);
        self.evening_at = local;
        if status != self.evening {
            self.evening = status;
            cx.notify();
        }

        let wanted = Shown::of(status);
        if wanted == self.evening_windows.shown {
            return;
        }
        if wanted == Shown::Cutoff && matches!(self.tracker.phase(), Phase::Prompted { .. }) {
            // The break's overlay gives way; it would only be stacked under.
            self.tracker.snooze();
            self.close_overlays(cx);
        }
        self.close_evening_windows(cx);
        self.evening_windows.shown = wanted;
        match wanted {
            Shown::Nothing => {}
            Shown::HeadsUp => self.open_heads_up(cx),
            Shown::Cutoff => self.open_cutoff(cx),
        }
    }

    /// What the user said the last time they carried on tonight, and when.
    pub fn evening_said(&self) -> Option<(NaiveDateTime, String)> {
        let day = stats::day_of(self.evening_at);
        let extension = self.stats.day(day)?.last_extension()?;
        Some((stats::local_time(extension.at), extension.reason.clone()))
    }

    /// The user closed the heads-up card: not again tonight.
    pub fn close_heads_up(&mut self, cx: &mut Context<Self>) {
        self.heads_up_closed = Some(stats::day_of(self.evening_at));
        self.sync_evening(self.now(), cx);
    }

    /// The user is carrying on past the cutoff, to do `reason`. Nothing
    /// happens without a reason; with one, the overlay steps aside for
    /// [`evening::EXTENSION`].
    pub fn keep_using(&mut self, reason: &str, cx: &mut Context<Self>) {
        let reason = reason.trim();
        if reason.is_empty() || !matches!(self.evening, evening::Status::Cutoff { .. }) {
            return;
        }
        let now = self.now();
        let day = stats::day_of(stats::local_time(now));
        let cutoff = self.config.evening.cutoff;
        let record = self.stats.day_mut(day);
        let evening = record.evening.get_or_insert_with(|| stats::EveningRecord {
            cutoff,
            extensions: Vec::new(),
        });
        evening.extensions.push(stats::Extension {
            at: now,
            reason: reason.to_string(),
        });
        // What someone typed is worth more than a second's wait for the save.
        self.save_stats();
        self.sync_evening(now, cx);
    }

    fn open_heads_up(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity();
        cx.defer(move |cx| {
            if this.read(cx).evening_windows.shown != Shown::HeadsUp {
                return;
            }
            let Some(display) = cx.primary_display() else {
                return;
            };
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(ui::evening::heads_up_bounds(
                    display.visible_bounds(),
                ))),
                ..popup_options(display.id())
            };
            match cx.open_window(options, |window, cx| {
                cx.new(|cx| ui::evening::HeadsUpCard::new(window, cx))
            }) {
                Ok(handle) => this.update(cx, |this, _| this.evening_windows.heads_up = Some(handle)),
                Err(e) => log::error!("failed to open the evening heads-up: {e}"),
            }
        });
    }

    fn open_cutoff(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity();
        cx.defer(move |cx| {
            if this.read(cx).evening_windows.shown != Shown::Cutoff {
                return;
            }
            let mut overlays = Vec::new();
            for display in cx.displays() {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(display.bounds())),
                    ..popup_options(display.id())
                };
                match cx.open_window(options, |window, cx| {
                    cx.new(|cx| ui::evening::CutoffOverlay::new(window, cx))
                }) {
                    Ok(handle) => overlays.push(handle),
                    Err(e) => log::error!("failed to open the evening overlay: {e}"),
                }
            }
            // After the overlays, so it lands on top of them.
            let keep_using = cx.primary_display().and_then(|display| {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(ui::evening::keep_using_bounds(display.bounds()))),
                    ..popup_options(display.id())
                };
                cx.open_window(options, |window, cx| {
                    cx.new(|cx| ui::evening::KeepUsing::new(window, cx))
                })
                .inspect_err(|e| log::error!("failed to open the keep-using box: {e}"))
                .ok()
            });
            this.update(cx, |this, _| {
                this.evening_windows.overlays = overlays;
                this.evening_windows.keep_using = keep_using;
            });
        });
    }

    /// Deferred, like every other close: the request may come from inside one
    /// of these very windows.
    fn close_evening_windows(&mut self, cx: &mut Context<Self>) {
        let windows = std::mem::take(&mut self.evening_windows);
        cx.defer(move |cx| {
            let EveningWindows {
                heads_up,
                overlays,
                keep_using,
                ..
            } = windows;
            let mut all: Vec<AnyWindowHandle> = overlays.into_iter().map(Into::into).collect();
            all.extend(heads_up.map(AnyWindowHandle::from));
            all.extend(keep_using.map(AnyWindowHandle::from));
            for handle in all {
                handle.update(cx, |_, window, _| window.remove_window()).ok();
            }
        });
    }

    // ---- settings --------------------------------------------------------

    pub fn update_config(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Config)) {
        f(&mut self.config);
        self.tracker.set_settings(self.config.breaks.settings());
        self.save_config();
        // A new cutoff, or none, shows at once rather than on the next tick.
        self.sync_evening(self.now(), cx);
        cx.notify();
    }

    pub fn set_developer_mode(&mut self, enabled: bool, cx: &mut Context<Self>) {
        crate::logger::set_verbose(enabled);
        self.update_config(cx, |cfg| cfg.developer_mode = enabled);
    }

    /// Persist the choice and repaint every window. `Theme::change` refreshes
    /// them all, so an open window switches without a restart.
    pub fn set_theme(&mut self, theme: config::ThemePref, cx: &mut Context<Self>) {
        self.update_config(cx, |cfg| cfg.theme = theme);
        apply_theme(self.config.skin, theme, cx);
    }

    /// Persist the choice and redraw every window in the new skin.
    ///
    /// A skin can bring its own mode with it — a light-only skin overrides the
    /// saved light/dark preference — so this re-resolves both rather than
    /// touching one.
    pub fn set_skin(&mut self, skin: crate::skin::Skin, cx: &mut Context<Self>) {
        self.update_config(cx, |cfg| cfg.skin = skin);
        apply_theme(skin, self.config.theme, cx);
    }

    /// Persist the choice and repaint every window in it. A theme is something
    /// a window can re-resolve for itself; a language is not — every label has
    /// to be looked up again, so the windows are redrawn and the tray menu,
    /// which is native, is rebuilt by `TrayState` carrying the language.
    pub fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        i18n::set_language(language);
        self.update_config(cx, |cfg| cfg.language = language);
        cx.refresh_windows();
    }

    /// Records a new combination for one global hotkey, and applies it at once.
    ///
    /// The registration itself lives in `main`, on the thread that owns the
    /// hotkey manager's hidden window, so this writes the config and asks that
    /// registry to re-register everything. Whatever Windows turns down comes
    /// back in [`Self::hotkey_errors`], which is what the Settings tab shows
    /// under the row — a combination another program already owns is a normal
    /// thing to hit, not a bug to swallow.
    pub fn set_hotkey(&mut self, key: &'static str, spec: String, cx: &mut Context<Self>) {
        self.update_config(cx, |cfg| cfg.hotkeys.set_spec(key, spec));
        self.apply_hotkeys(cx);
    }

    /// Releases every global hotkey, or takes them back from the config.
    ///
    /// Recording a combination means pressing it, and a combination that is
    /// still bound would be delivered as well: the screen would switch input in
    /// the middle of being asked which keys should switch input. Anything that
    /// stops recording — a key, Escape, clicking away — has to release the hold
    /// again, which is why this is a pair rather than a one-way switch.
    pub fn hold_hotkeys(&mut self, hold: bool, cx: &mut Context<Self>) {
        let Some(registry) = self.hotkeys.clone() else {
            return;
        };
        if hold {
            self.hotkey_hold = true;
            registry.borrow_mut().clear();
            cx.notify();
        } else {
            self.apply_hotkeys(cx);
        }
    }

    /// Re-registers the configured hotkeys, and remembers what Windows refused.
    fn apply_hotkeys(&mut self, cx: &mut Context<Self>) {
        self.hotkey_hold = false;
        let keys = self.config.hotkeys.clone();
        self.hotkey_errors = match &self.hotkeys {
            Some(registry) => registry.borrow_mut().apply(&keys),
            None => Vec::new(),
        };
        cx.notify();
    }

    /// Everything needed to debug monitor control on this machine, as text.
    pub fn diagnostics_report(&self) -> String {
        let mut out = format!(
            "# tarsier v{} diagnostics ({} {}, {}, elevated: {})

",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            display::elevate::process_elevated()
        );
        for m in &self.monitors {
            out.push_str(&m.dev.report());
            let prefs = serde_json::to_string(&self.monitor_prefs(m.id())).unwrap_or_default();
            out.push_str(&format!(
                "config: {prefs}

"
            ));
        }
        if self.monitors.is_empty() {
            out.push_str(
                "(no DDC/CI monitors found)
",
            );
        }
        out
    }

    pub fn set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Err(e) = platform::set_autostart(enabled) {
            self.show_notice(
                tr!("Could not change the autostart setting: {e}", e = format!("{e:#}")),
                cx,
            );
        }
        self.autostart = platform::autostart_enabled();
        cx.notify();
    }

    fn save_config(&mut self) {
        let Storage::Disk { config: path, .. } = &self.storage else {
            return;
        };
        if let Err(e) = config::save(path, &self.config) {
            // No timer: a settings file that cannot be written stays a problem
            // until something changes, so the notice stays too.
            self.notice = Some(tr!("Could not save settings: {e}", e = format!("{e:#}")).into());
        }
    }

    pub fn save_stats(&mut self) {
        self.last_save = Instant::now();
        let Storage::Disk { stats: path, .. } = &self.storage else {
            self.stats_dirty = false;
            return;
        };
        match config::save(path, &self.stats) {
            Ok(()) => self.stats_dirty = false,
            Err(e) => log::error!("saving stats: {e:#}"),
        }
    }

    /// Persist everything before exit, closing the running session.
    pub fn shutdown(&mut self) {
        let now = now_ts();
        if let Some(session) = self.tracker.finish(now) {
            self.stats.record(activity_day(now), Session::from(session));
        }
        self.save_stats();
    }

    /// The name each connected monitor travels under in an export: its model
    /// name, numbered when two identical monitors would otherwise collide. Both
    /// ends number in enumeration order, which is stable for a fixed set of
    /// cables.
    fn transfer_names(&self) -> Vec<String> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        self.monitors
            .iter()
            .map(|m| {
                let n = seen.entry(m.dev.name.as_str()).or_default();
                *n += 1;
                if *n == 1 {
                    m.dev.name.clone()
                } else {
                    format!("{} ({n})", m.dev.name)
                }
            })
            .collect()
    }

    /// Everything about input switching that another computer can use, as text
    /// for the clipboard. See [`config::SwitchingExport`]. Monitors that have not
    /// been set up are left out, so they cannot overwrite one that has been on
    /// the receiving machine.
    pub fn export_switching(&self) -> String {
        let monitors = self
            .monitors
            .iter()
            .zip(self.transfer_names())
            .filter_map(|(m, name)| {
                let prefs = self.monitor_prefs(m.id());
                (!prefs.endpoints.is_empty()).then(|| (name, prefs.portable()))
            })
            .collect();
        serde_json::to_string_pretty(&config::SwitchingExport {
            version: config::SWITCHING_EXPORT_VERSION,
            monitors,
        })
        .unwrap_or_default()
    }

    /// Applies a [`Self::export_switching`] blob from another computer.
    /// Returns how many of this machine's monitors it matched.
    pub fn import_switching(&mut self, text: &str, cx: &mut Context<Self>) -> usize {
        let Ok(export) = serde_json::from_str::<config::SwitchingExport>(text) else {
            self.show_notice(tr!("The clipboard does not hold tarsier input-switch settings"), cx);
            return 0;
        };
        let targets: Vec<(String, Option<u8>, MonitorPrefs)> = self
            .monitors
            .iter()
            .zip(self.transfer_names())
            .filter_map(|(m, name)| {
                let incoming = export.monitors.get(&name)?;
                // An export from an older version could carry monitors nobody
                // set up; they must not wipe one that is set up here.
                (!incoming.endpoints.is_empty()).then(|| (m.id().to_string(), m.current_input, incoming.clone()))
            })
            .collect();
        let matched = targets.len();
        for (id, current, incoming) in targets {
            let prefs = self.config.monitors.entry(id).or_default();
            prefs.endpoints = incoming.endpoints;
            prefs.input_names = incoming.input_names;
            prefs.extra_inputs = incoming.extra_inputs;
            prefs.input_protocol = incoming.input_protocol;
            // Which port this computer is on cannot be copied — every machine
            // is plugged into a different one — so ask the monitor instead.
            prefs.local_input = current.filter(|port| prefs.endpoints.contains(port));
            if let Some(port) = prefs.local_input
                && !prefs.is_named(port)
            {
                prefs.input_names.insert(port, platform::local_hostname());
            }
        }
        self.save_config();
        self.show_notice(
            if matched == 0 {
                tr!("The copied settings do not match any monitor connected here").to_string()
            } else {
                tr!(n = matched, "Applied to 1 monitor" | "Applied to {n} monitors")
            },
            cx,
        );
        matched
    }

    /// Shows the quick-switch panel. A monitor shared by three or more
    /// computers cannot be served by a blind flip: cycling would have to go
    /// through the intermediate machine — two switches, two screen blanks, and
    /// a detour — and on a monitor that ignores commands on an inactive input
    /// the second one would never arrive. So the user names the destination up
    /// front and tarsier jumps straight there.
    pub fn open_switch_hud(&mut self, cx: &mut Context<Self>) {
        if self.switch_hud_opening || !self.switch_huds.is_empty() {
            return;
        }
        self.switch_hud_opening = true;
        self.switch_hud_generation += 1;
        let generation = self.switch_hud_generation;
        // Remember who had the user's attention before the panel took it.
        self.switch_hud_return_to = platform::foreground_window();
        let this = cx.entity();
        cx.defer(move |cx| {
            let mut handles = Vec::new();
            for display in cx.displays() {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(display.bounds())),
                    titlebar: None,
                    // The hotkey was an explicit request for a chooser, so
                    // taking focus for its lifetime is expected — and it is
                    // what makes Esc and the number keys work.
                    focus: true,
                    show: true,
                    kind: WindowKind::PopUp,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    display_id: Some(display.id()),
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                };
                // No component `Root`: it would paint an opaque background over
                // the whole display. The panel draws its own.
                match cx.open_window(options, |window, cx| cx.new(|cx| SwitchHud::new(window, cx))) {
                    Ok(handle) => handles.push(handle),
                    Err(e) => log::error!("failed to open switch panel: {e}"),
                }
            }
            this.update(cx, |this, cx| {
                this.switch_hud_opening = false;
                this.switch_huds = handles;
                cx.notify();
            });
        });
        // A stray hotkey should never leave a panel parked over the desktop —
        // but this timer belongs to this opening only, not to a panel opened
        // after it was closed.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(switch_hud::TIMEOUT).await;
            this.update(cx, |this, cx| {
                if this.switch_hud_generation == generation {
                    this.close_switch_hud(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Closes the quick-switch panel wherever it is showing, and gives focus
    /// back to whatever had it beforehand. Safe to call when it is not open.
    pub fn close_switch_hud(&mut self, cx: &mut Context<Self>) {
        let handles = std::mem::take(&mut self.switch_huds);
        let return_to = self.switch_hud_return_to.take();
        if handles.is_empty() && return_to.is_none() {
            return;
        }
        // The removal cannot happen here. Every ordinary way of closing the
        // panel — Esc, a number key, clicking a row, clicking outside — runs
        // inside one of these very windows: input reaches a listener through
        // `WindowHandle::update`, which takes the window off the app for the
        // duration. A second `update` on that window therefore fails with
        // "window not found" instead of closing it, and swallowing that error
        // is what left the panel parked on screen. Deferred to the end of the
        // effect cycle, every window is back on the app and the handles work.
        cx.defer(move |cx| {
            for handle in handles {
                if let Err(e) = handle.update(cx, |_, window, _| window.remove_window()) {
                    log::warn!("failed to close the switch panel: {e}");
                }
            }
            // Changing your mind about the switch should not leave your
            // application stranded in the background, so the round trip is
            // invisible. After the panel is gone, or Windows may hand focus
            // back to a window that is still there.
            if let Some(hwnd) = return_to {
                platform::restore_foreground(hwnd);
            }
        });
    }

    /// What the tray menu should show right now. Compared by value on the tray
    /// side, so this can be rebuilt freely.
    ///
    /// Deliberately carries no "which one is live" flag: that would mean
    /// reading the monitor's current input on a timer, and the answer can be
    /// changed by the other computer at any moment. The menu lists the
    /// destinations; it does not pretend to know where you are.
    pub fn tray_state(&self) -> tray::TrayState {
        let groups: Vec<tray::TrayGroup> = self
            .monitors
            .iter()
            .filter_map(|m| {
                let ports = self.endpoints(m);
                if ports.len() < 2 {
                    return None;
                }
                Some(tray::TrayGroup {
                    id: m.id().to_string(),
                    name: m.dev.name.clone(),
                    endpoints: ports
                        .iter()
                        .map(|&port| tray::TrayEndpoint {
                            port,
                            name: self.input_label(m.id(), port),
                        })
                        .collect(),
                })
            })
            .collect();

        tray::TrayState {
            groups,
            paused: self.is_paused(),
            reminders_off: !self.config.breaks.enabled,
            break_active: matches!(self.tracker.phase(), Phase::Prompted { .. }),
            language: self.config.language,
            tooltip: self.tooltip_text(),
        }
    }

    /// One line summarising tarsier, shown when hovering the tray icon.
    pub fn tooltip_text(&self) -> String {
        let state = if !self.config.breaks.enabled {
            tr!("Break reminders are off").to_string()
        } else if self.is_paused() {
            tr!("Reminders are paused").to_string()
        } else {
            match self.tracker.phase() {
                Phase::Prompted { .. } => tr!("On a break").to_string(),
                Phase::Away => tr!("Away").to_string(),
                Phase::Working => {
                    let minutes = self.tracker.until_prompt().div_ceil(60);
                    tr!(n = minutes, "Break in 1 minute" | "Break in {n} minutes")
                }
            }
        };
        match self.worked_today() {
            0 => format!("tarsier · {state}"),
            secs => tr!(
                "tarsier · {state} · {time} of work today",
                state = state,
                time = crate::ui::format_minutes(secs)
            ),
        }
    }

    /// Shows `text` at the foot of the main window for a few seconds.
    pub fn show_notice(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.notice = Some(text.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_LIFETIME).await;
            this.update(cx, |this, cx| {
                // A newer notice has its own timer.
                if this.notice.as_ref() == Some(&text) {
                    this.dismiss_notice(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.notice.take().is_some() {
            cx.notify();
        }
    }

    pub fn show_main_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.main_window
            && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
        {
            return;
        }
        let entity = cx.entity();
        cx.defer(move |cx| {
            let handle = crate::ui::main_window::open(entity.clone(), cx);
            entity.update(cx, |this, _| this.main_window = handle);
        });
    }
}

/// Apply the saved skin and appearance preference.
///
/// Under `System` this must be re-run whenever Windows changes appearance, so
/// `main` also calls it from a window appearance observer.
///
/// The order is load-bearing. `Theme::change` replaces every colour with the
/// registered theme for that mode, so the skin's palette has to be written
/// *after* it or the mode load would undo it — and `install` is where that
/// write lives.
pub fn apply_theme(skin: crate::skin::Skin, pref: config::ThemePref, cx: &mut App) {
    let mode = crate::skin::resolve_mode(skin, pref, cx.window_appearance());
    Theme::change(mode, None, cx);
    crate::skin::install(skin, cx);
}

fn dev_name(monitors: &[MonitorEntry], id: &str) -> String {
    monitors
        .iter()
        .find(|m| m.id() == id)
        .map(|m| m.dev.name.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // Not `use super::*`: that would pull in GPUI's own `test` attribute.
    use std::time::Duration;

    use gpui_kit::{AppContext as _, TestAppContext};

    use super::{Controller, MonitorEntry, NOTICE_LIFETIME, Shown, flip_destination};
    use crate::config::{Config, MonitorPrefs};
    use crate::evening::ClockTime;

    #[test]
    fn a_flip_refuses_to_guess_when_it_cannot_tell_which_way_to_go() {
        let pair = [0x10, 0x12];
        assert_eq!(flip_destination(pair, Some(0x10)), Some(0x12));
        assert_eq!(flip_destination(pair, Some(0x12)), Some(0x10));
        // A third device, or a reading the monitor will not give.
        assert_eq!(flip_destination(pair, Some(0x0F)), None);
        assert_eq!(flip_destination(pair, None), None);
    }

    fn set_up(config: &mut Config, id: &str, endpoints: &[u8], names: &[(u8, &str)]) {
        config.monitors.insert(
            id.to_string(),
            MonitorPrefs {
                endpoints: endpoints.to_vec(),
                input_names: names.iter().map(|(p, n)| (*p, n.to_string())).collect(),
                ..Default::default()
            },
        );
    }

    #[gpui_kit::test]
    fn settings_travel_between_machines_without_clobbering_anything(cx: &mut TestAppContext) {
        // Machine A: two identical monitors set up differently, and one never
        // set up at all.
        let mut config = Config::default();
        set_up(&mut config, "a1", &[0x0F, 0x11], &[(0x0F, "Laptop"), (0x11, "Desktop")]);
        set_up(&mut config, "a2", &[0x10, 0x12], &[(0x10, "NAS"), (0x12, "Console")]);
        let a = cx.update(|cx| {
            Controller::for_test(
                config,
                vec![
                    MonitorEntry::fake("a1", "U2723QE", &[0x0F, 0x11]),
                    MonitorEntry::fake("a2", "U2723QE", &[0x10, 0x12]),
                    MonitorEntry::fake("a3", "LG 27GP950", &[0x0F, 0x11]),
                ],
                cx,
            )
        });
        let export = cx.update(|cx| a.read(cx).export_switching());
        assert!(export.contains("\"U2723QE (2)\""), "identical monitors are told apart");
        assert!(!export.contains("27GP950"), "a monitor nobody set up is not exported");

        // Machine B: the same pair, and its own LG already set up.
        let mut config = Config::default();
        set_up(&mut config, "b3", &[0x0F, 0x11], &[(0x0F, "Work"), (0x11, "Home")]);
        let b = cx.update(|cx| {
            Controller::for_test(
                config,
                vec![
                    MonitorEntry::fake("b1", "U2723QE", &[0x11, 0x0F]),
                    MonitorEntry::fake("b2", "U2723QE", &[0x12, 0x10]),
                    MonitorEntry::fake("b3", "LG 27GP950", &[0x0F, 0x11]),
                ],
                cx,
            )
        });
        let matched = b.update(cx, |c, cx| c.import_switching(&export, cx));
        assert_eq!(matched, 2);
        cx.update(|cx| {
            let c = b.read(cx);
            assert_eq!(c.monitor_prefs("b1").endpoints, vec![0x0F, 0x11]);
            assert_eq!(c.monitor_prefs("b2").label(0x12), "Console");
            // This machine is on whatever input its monitor shows, not on the
            // port the exporting machine was on.
            assert_eq!(c.monitor_prefs("b1").local_input, Some(0x11));
            assert_eq!(c.monitor_prefs("b3").label(0x0F), "Work", "untouched by the import");
        });

        // Something that is not an export changes nothing and says so.
        let matched = b.update(cx, |c, cx| c.import_switching("hello", cx));
        assert_eq!(matched, 0);
        assert!(cx.update(|cx| b.read(cx).notice.is_some()));
    }

    #[gpui_kit::test]
    fn the_tray_routes_by_monitor_id_and_shows_names(cx: &mut TestAppContext) {
        let id = r"\\?\DISPLAY#GSM5BF6#1";
        let mut config = Config::default();
        set_up(&mut config, id, &[0x0F, 0x11], &[(0x11, "Desktop")]);
        config.breaks.enabled = false;
        let c = cx.update(|cx| {
            Controller::for_test(
                config,
                vec![
                    MonitorEntry::fake(id, "27GP950", &[0x0F, 0x11]),
                    // One input: nothing to switch between, so not listed.
                    MonitorEntry::fake("solo", "Laptop panel", &[0x0F]),
                ],
                cx,
            )
        });
        let state = cx.update(|cx| c.read(cx).tray_state());
        assert_eq!(state.groups.len(), 1);
        assert_eq!(state.groups[0].id, id);
        let names: Vec<&str> = state.groups[0].endpoints.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["DisplayPort 1", "Desktop"]);
        assert!(state.reminders_off && !state.paused);
        assert_eq!(
            cx.update(|cx| c.read(cx).tooltip_text()),
            "tarsier · Break reminders are off"
        );
    }

    #[gpui_kit::test]
    fn a_notice_clears_itself_unless_a_newer_one_arrived(cx: &mut TestAppContext) {
        let c = cx.update(|cx| Controller::for_test(Config::default(), Vec::new(), cx));
        c.update(cx, |c, cx| c.show_notice("first", cx));
        cx.executor().advance_clock(NOTICE_LIFETIME / 2);
        c.update(cx, |c, cx| c.show_notice("second", cx));
        // The first notice's timer fires, but the notice is not its own any more.
        cx.executor()
            .advance_clock(NOTICE_LIFETIME / 2 + Duration::from_millis(1));
        assert_eq!(cx.update(|cx| c.read(cx).notice.clone()).as_deref(), Some("second"));
        cx.executor().advance_clock(NOTICE_LIFETIME);
        assert_eq!(cx.update(|cx| c.read(cx).notice.clone()), None);
    }

    #[gpui_kit::test]
    fn removing_this_computer_forgets_where_it_was(cx: &mut TestAppContext) {
        let mut config = Config::default();
        set_up(&mut config, "m", &[0x0F, 0x10, 0x11], &[]);
        config.monitors.get_mut("m").unwrap().local_input = Some(0x10);
        let c =
            cx.update(|cx| Controller::for_test(config, vec![MonitorEntry::fake("m", "M", &[0x0F, 0x10, 0x11])], cx));
        c.update(cx, |c, cx| c.remove_endpoint("m", 0x10, cx));
        let prefs = cx.update(|cx| c.read(cx).monitor_prefs("m"));
        assert_eq!(prefs.endpoints, vec![0x0F, 0x11]);
        assert_eq!(prefs.local_input, None, "no other port is guessed in its place");
    }

    /// Tonight at `h:m`, as a Unix timestamp. Before 05:00 is the next
    /// calendar date, as the evening runs on past midnight.
    fn tonight(h: u32, m: u32) -> i64 {
        use chrono::{Days, Local, TimeZone};
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let date = if h < 5 {
            day.checked_add_days(Days::new(1)).unwrap()
        } else {
            day
        };
        let at = date.and_hms_opt(h, m, 0).unwrap();
        Local.from_local_datetime(&at).earliest().unwrap().timestamp()
    }

    fn evening_at_nine(cx: &mut TestAppContext) -> gpui_kit::Entity<Controller> {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::skin::register_fonts(cx);
        });
        let mut config = Config::default();
        config.evening.enabled = true;
        config.evening.cutoff = ClockTime::new(21, 0).unwrap();
        cx.update(|cx| Controller::for_test(config, Vec::new(), cx))
    }

    fn at(c: &gpui_kit::Entity<Controller>, now: i64, cx: &mut TestAppContext) {
        c.update(cx, |c, cx| {
            c.fake_now = Some(now);
            c.sync_evening(now, cx);
        });
        cx.run_until_parked();
    }

    /// (heads-up card, overlays, keep-using box)
    fn shown(c: &gpui_kit::Entity<Controller>, cx: &mut TestAppContext) -> (bool, usize, bool) {
        cx.update(|cx| {
            let w = &c.read(cx).evening_windows;
            (w.heads_up.is_some(), w.overlays.len(), w.keep_using.is_some())
        })
    }

    #[gpui_kit::test]
    fn an_evening_on_screen(cx: &mut TestAppContext) {
        let c = evening_at_nine(cx);
        at(&c, tonight(20, 30), cx);
        assert_eq!(shown(&c, cx), (false, 0, false));
        at(&c, tonight(20, 50), cx);
        assert_eq!(shown(&c, cx), (true, 0, false), "a heads-up in the corner");
        at(&c, tonight(21, 0), cx);
        let (heads_up, overlays, keep_using) = shown(&c, cx);
        assert!(!heads_up, "the card gives way");
        assert!(overlays > 0, "one overlay per display");
        assert!(keep_using);
        at(&c, tonight(5, 0), cx);
        assert_eq!(shown(&c, cx), (false, 0, false), "a new day");
    }

    #[gpui_kit::test]
    fn a_closed_heads_up_stays_closed(cx: &mut TestAppContext) {
        let c = evening_at_nine(cx);
        at(&c, tonight(20, 50), cx);
        c.update(cx, |c, cx| c.close_heads_up(cx));
        cx.run_until_parked();
        assert_eq!(shown(&c, cx), (false, 0, false));
        at(&c, tonight(20, 55), cx);
        assert_eq!(shown(&c, cx), (false, 0, false), "not again tonight");
    }

    #[gpui_kit::test]
    fn carrying_on_needs_a_reason_and_is_remembered(cx: &mut TestAppContext) {
        let c = evening_at_nine(cx);
        at(&c, tonight(21, 5), cx);
        let keep_using = cx
            .update(|cx| c.read(cx).evening_windows.keep_using)
            .expect("the box is up");
        cx.update_window(keep_using.into(), |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window.click("keep-using", cx);
            window.render_frame(cx);
            // Enter on nothing is not a reason.
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(shown(&c, cx).1 > 0, "still up");

        cx.update_window(keep_using.into(), |_, window, cx| {
            use gpui_kit::test::TestWindowExt as _;
            window.input("  finish the deploy  ", cx);
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(shown(&c, cx), (false, 0, false), "the overlay steps aside");
        let said = cx.update(|cx| c.read(cx).evening_said());
        assert_eq!(said.map(|(_, reason)| reason).as_deref(), Some("finish the deploy"));

        at(&c, tonight(21, 19), cx);
        assert_eq!(shown(&c, cx).1, 0, "for a quarter of an hour");
        at(&c, tonight(21, 21), cx);
        assert!(shown(&c, cx).1 > 0, "then it is back");
    }

    #[gpui_kit::test]
    fn turning_the_cutoff_off_takes_the_overlay_away_at_once(cx: &mut TestAppContext) {
        let c = evening_at_nine(cx);
        at(&c, tonight(22, 0), cx);
        assert!(shown(&c, cx).1 > 0);
        c.update(cx, |c, cx| c.update_config(cx, |cfg| cfg.evening.enabled = false));
        cx.run_until_parked();
        assert_eq!(shown(&c, cx), (false, 0, false));
        assert_eq!(cx.update(|cx| c.read(cx).evening_windows.shown), Shown::Nothing);
    }

    #[gpui_kit::test]
    fn a_break_reminder_gives_way_to_the_evening(cx: &mut TestAppContext) {
        let c = evening_at_nine(cx);
        c.update(cx, |c, cx| c.break_now(cx));
        cx.run_until_parked();
        at(&c, tonight(21, 0), cx);
        cx.update(|cx| {
            let c = c.read(cx);
            assert_eq!(
                c.tracker.phase(),
                crate::breaks::Phase::Working,
                "the break was put off"
            );
            assert!(c.overlays.is_empty(), "its overlay closed");
        });
    }
}
