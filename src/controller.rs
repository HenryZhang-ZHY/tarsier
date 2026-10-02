//! Application state shared by the tray, hotkeys and windows. Lives as a
//! GPUI entity for the whole process; windows come and go around it.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use gpui_kit::*;

use crate::breaks::{BreakEvent, BreakTracker, Phase};
use crate::config::{self, Config, MonitorPrefs};
use crate::display::mccs::{self, VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::display::{self, Feature};
use crate::platform;
use crate::stats::{Session, Stats};
use crate::ui::break_overlay::{BreakOverlay, FADE_OUT};

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

pub fn local_date(ts: i64) -> NaiveDate {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d: DateTime<Local>| d.date_naive())
        .unwrap_or_default()
}

impl Controller {
    pub fn init(cx: &mut App) -> Entity<Controller> {
        let config: Config = config::load(&config::config_path());
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
        self.config
            .monitors
            .get(monitor_id)
            .and_then(|p| p.input_names.get(&code).cloned())
            .unwrap_or_else(|| mccs::input_source_name(code))
    }

    /// Effective toggle pair: configured, or the only two inputs a monitor reports.
    pub fn toggle_pair(&self, entry: &MonitorEntry) -> Option<[u8; 2]> {
        self.monitor_prefs(entry.id()).toggle.or_else(|| {
            let reported = entry.dev.input_sources();
            (reported.len() == 2).then(|| [reported[0], reported[1]])
        })
    }

    pub fn set_toggle_slot(&mut self, monitor_id: &str, slot: usize, code: u8, cx: &mut Context<Self>) {
        let current = self
            .monitors
            .iter()
            .find(|m| m.id() == monitor_id)
            .and_then(|m| self.toggle_pair(m));
        let mut pair = current.unwrap_or([code, code]);
        pair[slot] = code;
        self.config.monitors.entry(monitor_id.to_string()).or_default().toggle = Some(pair);
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

    /// Flip every monitor that has a toggle pair to the other input.
    pub fn toggle_inputs(&mut self, cx: &mut Context<Self>) {
        let jobs: Vec<(Arc<display::Monitor>, [u8; 2], Option<u8>)> = self
            .monitors
            .iter()
            .filter_map(|m| Some((m.dev.clone(), self.toggle_pair(m)?, m.current_input)))
            .filter(|(_, [a, b], _)| a != b)
            .collect();
        if jobs.is_empty() {
            self.notice = Some("还没有设置快捷切换的两个输入，请在「显示器」页选择".into());
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
                            let target = if current == Some(a) { b } else { a };
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

    /// Everything needed to debug monitor control on this machine, as text.
    pub fn diagnostics_report(&self) -> String {
        let mut out = format!(
            "# tarsier v{} diagnostics ({} {}, {})

",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            Local::now().format("%Y-%m-%d %H:%M:%S")
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

fn dev_name(monitors: &[MonitorEntry], id: &str) -> String {
    monitors
        .iter()
        .find(|m| m.id() == id)
        .map(|m| m.dev.name.clone())
        .unwrap_or_default()
}
