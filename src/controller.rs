//! Application state shared by the tray, hotkeys and windows. Lives as a
//! GPUI entity for the whole process; windows come and go around it.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use gpui_kit::component::Theme;
use gpui_kit::*;

use crate::breaks::{BreakEvent, BreakTracker, Phase};
use crate::config::{self, Config, MonitorPrefs};
use crate::display::mccs::{self, VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::display::{self, Feature};
use crate::platform;
use crate::stats::{Session, Stats};
use crate::tray;
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
    pub hotkey_errors: Vec<String>,
    writes: HashMap<(String, u8), WriteSlot>,
    overlays: Vec<(WindowHandle<BreakOverlay>, Entity<BreakOverlay>)>,
    /// One quick-switch panel per display while it is open.
    switch_huds: Vec<WindowHandle<SwitchHud>>,
    /// The window that had focus before the panel took it, to hand it back.
    switch_hud_return_to: Option<isize>,
    last_tick: Instant,
    last_save: Instant,
    stats_dirty: bool,
    pub main_window: Option<AnyWindowHandle>,
}

struct GlobalController(Entity<Controller>);
impl Global for GlobalController {}

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

/// Where a blind flip lands: the other endpoint when the monitor is showing
/// the first, and the first otherwise.
///
/// An input outside the pair — a third device, or a value the monitor reports
/// oddly — also lands on the first.
pub fn flip_destination(pair: [u8; 2], current: Option<u8>) -> u8 {
    if current == Some(pair[0]) { pair[1] } else { pair[0] }
}

pub fn local_date(ts: i64) -> NaiveDate {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d: DateTime<Local>| d.date_naive())
        .unwrap_or_default()
}

impl Controller {
    pub fn init(cx: &mut App) -> Entity<Controller> {
        let mut config: Config = config::load(&config::config_path());
        config.migrate();
        let stats: Stats = config::load(&config::stats_path());
        let tracker = BreakTracker::new(config.breaks.settings(), now_ts());
        crate::logger::set_verbose(config.developer_mode);
        let entity = cx.new(|cx| {
            let mut this = Controller {
                config,
                monitors: Vec::new(),
                scanning: false,
                tracker,
                stats,
                paused_until: None,
                autostart: platform::autostart_enabled(),
                notice: None,
                hotkey_errors: Vec::new(),
                writes: HashMap::new(),
                overlays: Vec::new(),
                switch_huds: Vec::new(),
                switch_hud_return_to: None,
                last_tick: Instant::now(),
                last_save: Instant::now(),
                stats_dirty: false,
                main_window: None,
            };
            this.refresh_monitors(cx);
            this.start_ticker(cx);
            // Runs on tray "退出" and on Windows logoff/shutdown.
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
                let dev = dev.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { dev.set(code, value) })
                    .await;
                let next = this
                    .update(cx, |this, cx| {
                        if let Err(e) = result {
                            this.notice = Some(format!("{}: {e}", dev_name(&this.monitors, &key.0)).into());
                            cx.notify();
                        }
                        let slot = this.writes.get_mut(&key)?;
                        if slot.desired == value {
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
                    None => break,
                }
            }
        })
        .detach();
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
            return;
        };
        let dev = entry.dev.clone();
        entry.current_input = Some(code);
        cx.notify();
        let label = self.input_label(monitor_id, code);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { dev.switch_input(code) })
                .await;
            this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(()) => format!("已切换到 {label}").into(),
                    Err(e) => format!("切换输入失败: {e}").into(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Flip every monitor that is shared by exactly two computers to the other
    /// one. Monitors with three or more are skipped: flipping is only
    /// unambiguous when there is a single other side to flip to.
    pub fn toggle_inputs(&mut self, cx: &mut Context<Self>) {
        let jobs: Vec<(Arc<display::Monitor>, [u8; 2], Option<u8>)> = self
            .monitors
            .iter()
            .filter_map(|m| {
                let ports = self.endpoints(m);
                (ports.len() == 2).then(|| (m.dev.clone(), [ports[0], ports[1]], m.current_input))
            })
            .collect();
        if jobs.is_empty() {
            self.notice = Some("还没有设置要切换的电脑，请在「显示器」页添加".into());
            cx.notify();
            self.show_main_window(cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    jobs.into_iter()
                        .map(|(dev, [a, b], cached)| {
                            // Read fresh: the monitor's own buttons may have changed it.
                            let current = dev.current_input().or(cached);
                            let target = flip_destination([a, b], current);
                            (dev.id.clone(), target, dev.switch_input(target))
                        })
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
                            msgs.push(format!("已切换到 {}", this.input_label(&id, target)));
                        }
                        Err(e) => msgs.push(format!("切换失败: {e}")),
                    }
                }
                this.notice = Some(msgs.join("；").into());
                cx.notify();
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
        self.last_tick = Instant::now();
        if self.paused_until.is_some_and(|t| t <= now) {
            self.paused_until = None;
        }
        let breaks = &self.config.breaks;
        let suppressed =
            !breaks.enabled || self.paused_until.is_some() || (breaks.respect_fullscreen && platform::user_is_busy());
        let idle = platform::idle_secs();
        for event in self.tracker.tick(now, dt, idle, suppressed) {
            match event {
                BreakEvent::PromptBreak => self.open_overlays(cx),
                BreakEvent::BreakFinished => self.close_overlays(cx),
                BreakEvent::BreakIgnored => {
                    self.stats.day_mut(local_date(now)).ignored += 1;
                    self.stats_dirty = true;
                    self.close_overlays(cx);
                }
                BreakEvent::SessionEnded(session) => {
                    self.stats.record(local_date(session.end), Session::from(session));
                    self.stats_dirty = true;
                }
                BreakEvent::Returned => {}
            }
        }
        if self.stats_dirty && self.last_save.elapsed() > Duration::from_secs(30) {
            self.save_stats();
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
        self.stats.day_mut(local_date(now_ts())).snoozes += 1;
        self.stats_dirty = true;
        self.close_overlays(cx);
        cx.notify();
    }

    pub fn skip(&mut self, cx: &mut Context<Self>) {
        self.tracker.skip();
        self.stats.day_mut(local_date(now_ts())).skips += 1;
        self.stats_dirty = true;
        self.close_overlays(cx);
        cx.notify();
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.paused_until = if self.is_paused() { None } else { Some(now_ts() + 3600) };
        if self.is_paused() && matches!(self.tracker.phase(), Phase::Prompted { .. }) {
            self.tracker.snooze();
            self.close_overlays(cx);
        }
        cx.notify();
    }

    /// Today's score including the running session.
    pub fn today_score(&self) -> Option<u8> {
        let today = local_date(now_ts());
        let open = match self.tracker.phase() {
            Phase::Away => None,
            _ => Some((self.tracker.session_active(), self.tracker.settings().work_secs)),
        };
        let empty = Default::default();
        self.stats.day(today).unwrap_or(&empty).score_with(open)
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
        let overlays: Vec<_> = self.overlays.drain(..).collect();
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

    // ---- settings --------------------------------------------------------

    pub fn update_config(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Config)) {
        f(&mut self.config);
        self.tracker.set_settings(self.config.breaks.settings());
        self.save_config();
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
        apply_theme(theme, cx);
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
            self.notice = Some(format!("设置开机启动失败: {e}").into());
        }
        self.autostart = platform::autostart_enabled();
        cx.notify();
    }

    fn save_config(&mut self) {
        if let Err(e) = config::save(&config::config_path(), &self.config) {
            self.notice = Some(format!("保存设置失败: {e}").into());
        }
    }

    pub fn save_stats(&mut self) {
        match config::save(&config::stats_path(), &self.stats) {
            Ok(()) => self.stats_dirty = false,
            Err(e) => log::error!("saving stats: {e}"),
        }
        self.last_save = Instant::now();
    }

    /// Persist everything before exit, closing the running session.
    pub fn shutdown(&mut self) {
        let now = now_ts();
        if let Some(session) = self.tracker.finish(now) {
            self.stats.record(local_date(now), Session::from(session));
        }
        self.save_stats();
    }

    /// Everything about input switching that another computer can use, as text
    /// for the clipboard. See [`config::SwitchingExport`].
    pub fn export_switching(&self) -> String {
        let monitors = self
            .monitors
            .iter()
            .map(|m| (m.dev.name.clone(), self.monitor_prefs(m.id()).portable()))
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
            self.notice = Some("剪贴板里没有 tarsier 的输入切换设置".into());
            cx.notify();
            return 0;
        };
        let targets: Vec<(String, Option<u8>, MonitorPrefs)> = self
            .monitors
            .iter()
            .filter_map(|m| {
                Some((
                    m.id().to_string(),
                    m.current_input,
                    export.monitors.get(&m.dev.name)?.clone(),
                ))
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
            prefs.local_input = current;
            if let Some(port) = prefs.local_input
                && !prefs.is_named(port)
            {
                prefs.input_names.insert(port, platform::local_hostname());
            }
        }
        self.save_config();
        self.notice = Some(
            match matched {
                0 => "剪贴板里的设置和当前接的显示器对不上".to_string(),
                n => format!("已应用到 {n} 台显示器"),
            }
            .into(),
        );
        cx.notify();
        matched
    }

    /// Shows the quick-switch panel. A monitor shared by three or more
    /// computers cannot be served by a blind flip: cycling would have to go
    /// through the intermediate machine — two switches, two screen blanks, and
    /// a detour — and on a monitor that ignores commands on an inactive input
    /// the second one would never arrive. So the user names the destination up
    /// front and tarsier jumps straight there.
    pub fn open_switch_hud(&mut self, cx: &mut Context<Self>) {
        if !self.switch_huds.is_empty() {
            return;
        }
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
                this.switch_huds = handles;
                cx.notify();
            });
        });
        // A stray hotkey should never leave a panel parked over the desktop.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(switch_hud::TIMEOUT).await;
            this.update(cx, |this, cx| this.close_switch_hud(cx)).ok();
        })
        .detach();
    }

    /// Closes the quick-switch panel wherever it is showing, and gives focus
    /// back to whatever had it beforehand. Safe to call when it is not open.
    pub fn close_switch_hud(&mut self, cx: &mut Context<Self>) {
        let handles: Vec<_> = self.switch_huds.drain(..).collect();
        let return_to = self.switch_hud_return_to.take();
        if handles.is_empty() && return_to.is_none() {
            return;
        }
        for handle in handles {
            handle.update(cx, |_, window, _| window.remove_window()).ok();
        }
        // Changing your mind about the switch should not leave your application
        // stranded in the background, so the round trip is invisible.
        if let Some(hwnd) = return_to {
            platform::restore_foreground(hwnd);
        }
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
                    monitor: m.dev.name.clone(),
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
            paused: self.is_paused() || !self.config.breaks.enabled,
            break_active: matches!(self.tracker.phase(), Phase::Prompted { .. }),
            tooltip: self.tooltip_text(),
        }
    }

    /// One line summarising tarsier, shown when hovering the tray icon.
    pub fn tooltip_text(&self) -> String {
        let score = self.today_score().map_or("--".to_string(), |s| s.to_string());
        let state = if !self.config.breaks.enabled {
            "休息提醒已关闭".to_string()
        } else if self.is_paused() {
            "提醒已暂停".to_string()
        } else {
            format!("{} 分钟后休息", self.tracker.until_prompt().div_ceil(60))
        };
        format!("tarsier · {state} · 今日 {score} 分")
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

/// Apply the saved theme preference. Under `System` this must be re-run
/// whenever Windows changes appearance, so `main` also calls it from a
/// window appearance observer.
pub fn apply_theme(pref: config::ThemePref, cx: &mut App) {
    Theme::change(pref.resolve(cx.window_appearance()), None, cx);
}

#[cfg(test)]
mod tests {
    // Deliberately not `use super::*`: that would pull in GPUI's own `test`
    // attribute macro, which shadows the built-in one and recurses.
    use super::flip_destination;

    #[test]
    fn the_flip_only_needs_the_pair_and_where_the_monitor_is() {
        let pair = [0x10, 0x12];
        assert_eq!(flip_destination(pair, Some(0x10)), 0x12);
        assert_eq!(flip_destination(pair, Some(0x12)), 0x10);
        // An input outside the pair, or nothing readable at all, lands on the
        // first. The hotkey reads the monitor fresh at the moment it runs, so
        // this is the only place the current input is consulted — everything
        // the user looks at names a destination, never a state.
        assert_eq!(flip_destination(pair, Some(0x0F)), 0x10);
        assert_eq!(flip_destination(pair, None), 0x10);
    }
}

fn dev_name(monitors: &[MonitorEntry], id: &str) -> String {
    monitors
        .iter()
        .find(|m| m.id() == id)
        .map(|m| m.dev.name.clone())
        .unwrap_or_default()
}
