//! Main window: monitors, breaks, statistics and settings tabs.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use chrono::{Days, Local, TimeZone};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::TabBar;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::breaks::{BreakKind, Phase};
use crate::config;
use crate::controller::{Controller, MonitorEntry, local_date, now_ts};
use crate::display::mccs::{self, VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::display::{self, Feature};
use crate::i18n::{tr, translate};
use crate::platform;
use crate::skin::{Control, Level, Tone, Voice};
use crate::stats::{self, GOOD_SCORE};
use crate::ui::format_minutes;
use crate::ui::number_field::{NumberField, Range};

/// The active skin. Every visual decision this file makes goes through it, so
/// the window has one voice and adding a skin never touches this file.
fn skin(cx: &App) -> &'static dyn crate::skin::SkinStyle {
    crate::skin::active(cx)
}

pub fn open(controller: Entity<Controller>, cx: &mut App) -> Option<AnyWindowHandle> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(580.), px(720.)), cx)),
        window_min_size: Some(size(px(540.), px(480.))),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| MainWindow::new(controller, window, cx))
    })
    .map(|(handle, _)| handle)
    .inspect_err(|e| log::error!("failed to open main window: {e}"))
    .ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Monitors,
    Breaks,
    Stats,
    Settings,
}

const TABS: [Tab; 4] = [Tab::Monitors, Tab::Breaks, Tab::Stats, Tab::Settings];

/// Editable break durations: (key, label, range, getter, setter).
type BreakField = (
    &'static str,
    &'static str,
    Range,
    fn(&config::BreakConfig) -> u32,
    fn(&mut config::BreakConfig, u32),
);
const BREAK_FIELDS: [BreakField; 3] = [
    (
        "work",
        "Work",
        Range {
            min: 10,
            max: 180,
            step: 5,
        },
        |b| b.work_minutes,
        |b, v| b.work_minutes = v,
    ),
    (
        "rest",
        "Break",
        Range {
            min: 1,
            max: 30,
            step: 1,
        },
        |b| b.break_minutes,
        |b, v| b.break_minutes = v,
    ),
    (
        "snooze",
        "Snooze",
        Range {
            min: 1,
            max: 30,
            step: 1,
        },
        |b| b.snooze_minutes,
        |b, v| b.snooze_minutes = v,
    ),
];

pub struct MainWindow {
    controller: Entity<Controller>,
    tab: Tab,
    sliders: HashMap<(String, u8), Entity<SliderState>>,
    /// Typed value fields: brightness/contrast keyed `"{monitor}#{code}"`, break durations by name.
    numbers: HashMap<String, NumberField>,
    /// Editable computer names for each endpoint, keyed `endpoint_key`.
    names: HashMap<String, Entity<InputState>>,
    /// The endpoint whose port chooser is expanded, if any. One at a time
    /// keeps the card from turning back into a wall of buttons.
    port_picker: Option<String>,
    /// First-run wizard state, keyed by monitor id.
    setup: HashMap<String, Setup>,
    _subscriptions: Vec<Subscription>,
}

/// The two-step first-run flow for one monitor: tick which ports have a
/// computer on them, then say what each one is called.
#[derive(Default, Clone)]
struct Setup {
    chosen: Vec<u8>,
    naming: bool,
    /// Which of the chosen ports this computer is on.
    local: Option<u8>,
}

/// One-tap names offered while setting up, so the common case needs no typing.
/// Written into the config as the computer's name, in the language it is shown
/// in, so each one is translated where it is drawn.
const NAME_SUGGESTIONS: [&str; 4] = ["Desktop", "Work PC", "Laptop", "Console"];

/// A monitor shared by several computers needs to tell them apart at a glance
/// in three places at once (list, tray, quick-switch panel), so each endpoint
/// keeps one colour wherever it appears. Which colour is the skin's business:
/// the native skin rotates hues, neo-brutalism hands out palette blocks.
fn endpoint_color(index: usize, cx: &App) -> Hsla {
    skin(cx).identity(index)
}

/// A small pill: "This PC" on the computer you are sitting at, and a port's own
/// name where it has to say which one a row means.
fn mark(text: &str, cx: &App) -> AnyElement {
    skin(cx).chip(text, Tone::Outline, cx)
}

/// A hotkey spec like `ctrl+alt+I` drawn as key caps.
fn hotkey_caps(spec: &str, cx: &App) -> AnyElement {
    h_flex()
        .gap_1()
        .children(spec.split('+').filter(|k| !k.trim().is_empty()).map(|key| {
            let label = match key.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => "Ctrl".to_string(),
                "alt" => "Alt".to_string(),
                "shift" => "Shift".to_string(),
                "win" | "super" | "meta" => "Win".to_string(),
                other => other.to_uppercase(),
            };
            skin(cx).chip(&label, Tone::Outline, cx)
        }))
        .into_any_element()
}

/// What the first-run wizard should open on for one monitor, or `None` once it
/// has been through setup.
///
/// A monitor that reports exactly two inputs already says which ports are in
/// play, so there is nothing to tick and the flow opens straight on naming.
/// Anything else starts by asking which ports actually have a computer behind
/// them — a monitor reporting four inputs is not four computers.
///
/// Getting this wrong is not cosmetic: the setup screen is the only place the
/// import shortcut lives, so a monitor that never reaches it has no way to
/// receive another computer's names.
///
/// The test is `== 2`, not `>= 2`, so this stays correct on its own rather than
/// relying on callers never passing a longer list.
fn wizard_seed(configured: bool, known: &[u8], current: Option<u8>) -> Option<Setup> {
    if configured {
        return None;
    }
    let pair_known = known.len() == 2;
    Some(Setup {
        chosen: if pair_known {
            known.to_vec()
        } else {
            current.into_iter().collect()
        },
        naming: pair_known,
        local: current,
    })
}

/// Identity of one endpoint row.
fn endpoint_key(monitor_id: &str, port: u8) -> String {
    format!("{monitor_id}@{port:#04X}")
}

impl MainWindow {
    fn new(controller: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe_in(&controller, window, |this, _, window, cx| {
            this.sync_controls(window, cx);
            cx.notify();
        });
        // Windows fires an appearance change when the user flips the system
        // theme; re-resolve so "System" tracks it without a restart. An
        // explicit light/dark choice resolves to the same mode and costs a
        // repaint, and a skin that ships one mode answers the same either way.
        let appearance = cx.observe_window_appearance(window, |this, _window, cx| {
            let (skin, pref) = {
                let c = this.controller.read(cx);
                (c.config.skin, c.config.theme)
            };
            if pref == config::ThemePref::System {
                crate::controller::apply_theme(skin, pref, cx);
            }
        });
        let mut numbers = HashMap::new();
        let breaks = controller.read(cx).config.breaks.clone();
        for (key, _, range, get, set) in BREAK_FIELDS {
            let controller = controller.clone();
            let field = NumberField::new(
                get(&breaks),
                range,
                move |v, cx| controller.update(cx, |c, cx| c.update_config(cx, |cfg| set(&mut cfg.breaks, v))),
                window,
                cx,
            );
            numbers.insert(key.to_string(), field);
        }
        let mut this = Self {
            controller,
            tab: Tab::Monitors,
            sliders: HashMap::new(),
            numbers,
            names: HashMap::new(),
            port_picker: None,
            setup: HashMap::new(),
            _subscriptions: vec![observe, appearance],
        };
        this.sync_controls(window, cx);
        this
    }

    /// Keep one slider and one number field per (monitor, feature), and the
    /// break duration fields, in step with the controller.
    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let breaks = self.controller.read(cx).config.breaks.clone();
        for (key, _, _, get, _) in BREAK_FIELDS {
            if let Some(field) = self.numbers.get(key) {
                field.sync(get(&breaks), window, cx);
            }
        }

        let features: Vec<(String, u8, Feature)> = self
            .controller
            .read(cx)
            .monitors
            .iter()
            .flat_map(|m| {
                [(VCP_BRIGHTNESS, m.brightness), (VCP_CONTRAST, m.contrast)]
                    .into_iter()
                    .filter_map(|(code, f)| Some((m.id().to_string(), code, f?)))
            })
            .collect();
        self.sliders
            .retain(|key, _| features.iter().any(|(id, code, _)| key.0 == *id && key.1 == *code));
        self.numbers
            .retain(|key, _| !key.contains('#') || features.iter().any(|(id, code, _)| *key == feature_key(id, *code)));
        for (id, code, f) in features {
            let key = (id.clone(), code);
            if let Some(slider) = self.sliders.get(&key) {
                if slider.read(cx).value().start() as u32 != f.current {
                    slider.update(cx, |s, cx| s.set_value(f.current as f32, window, cx));
                }
                if let Some(field) = self.numbers.get(&feature_key(&id, code)) {
                    field.sync(f.current, window, cx);
                }
                continue;
            }
            let field = {
                let controller = self.controller.clone();
                let id = id.clone();
                NumberField::new(
                    f.current,
                    Range {
                        min: 0,
                        max: f.max.max(1),
                        step: 5,
                    },
                    move |v, cx| controller.update(cx, |c, cx| c.set_feature(&id, code, v, cx)),
                    window,
                    cx,
                )
            };
            self.numbers.insert(feature_key(&id, code), field);
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(f.max.max(1) as f32)
                    .step(1.)
                    .default_value(f.current as f32)
            });
            let controller = self.controller.clone();
            let sub = cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
                let (SliderEvent::Change(v) | SliderEvent::Release(v)) = event;
                let value = v.start().round() as u32;
                controller.update(cx, |c, cx| c.set_feature(&id, code, value, cx));
            });
            self._subscriptions.push(sub);
            self.sliders.insert(key, slider);
        }

        self.sync_endpoints(window, cx);
    }

    /// One editable name per configured endpoint, created on demand so typing
    /// is never interrupted by a repaint, plus the first-run wizard state.
    fn sync_endpoints(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Decide the wizard state first. The name fields below depend on it,
        // and a monitor that reports exactly two inputs opens on the naming
        // step straight away, so those fields have to exist on the first paint
        // rather than appearing a second later.
        let (seed, configured): (Vec<(String, Setup)>, Vec<String>) = {
            let c = self.controller.read(cx);
            c.monitors.iter().fold((Vec::new(), Vec::new()), |mut acc, m| {
                let id = m.id().to_string();
                match wizard_seed(c.endpoints_configured(m), &c.endpoints(m), m.current_input) {
                    Some(setup) => acc.0.push((id, setup)),
                    None => acc.1.push(id),
                }
                acc
            })
        };
        for id in configured {
            self.setup.remove(&id);
        }
        for (id, setup) in seed {
            self.setup.entry(id).or_insert(setup);
        }

        // Ports the wizard has ticked but not yet committed. They need name
        // fields too, or the naming step would render empty rows.
        let pending: Vec<(String, Vec<u8>, Option<u8>)> = self
            .setup
            .iter()
            .map(|(id, s)| (id.clone(), s.chosen.clone(), s.local))
            .collect();

        // Read everything up front: building an input re-enters the controller.
        let wanted: Vec<(String, String, u8, String)> = {
            let c = self.controller.read(cx);
            c.monitors
                .iter()
                .flat_map(|m| {
                    let id = m.id().to_string();
                    let prefs = c.monitor_prefs(&id);
                    let mut ports = c.endpoints(m);
                    for (pending_id, chosen, local) in &pending {
                        if pending_id != &id {
                            continue;
                        }
                        for port in chosen {
                            if !ports.contains(port) {
                                ports.push(*port);
                            }
                        }
                        let _ = local;
                    }
                    let draft = pending.iter().find(|(pid, ..)| pid == &id);
                    ports.into_iter().map(move |port| {
                        // This computer's name is never typed: it comes from the
                        // system, including while the wizard is still open.
                        let name = prefs
                            .input_names
                            .get(&port)
                            .filter(|n| !n.trim().is_empty())
                            .cloned()
                            .unwrap_or_else(|| match draft {
                                Some((_, _, local)) if *local == Some(port) => platform::local_hostname(),
                                _ => String::new(),
                            });
                        (endpoint_key(&id, port), id.clone(), port, name)
                    })
                })
                .collect()
        };

        self.names.retain(|key, _| wanted.iter().any(|(k, ..)| k == key));
        for (key, id, port, name) in wanted {
            if let Some(state) = self.names.get(&key) {
                // Don't overwrite what the user is in the middle of typing.
                let state_ref = state.read(cx);
                if !state_ref.focus_handle(cx).is_focused(window) && state_ref.value() != name {
                    let state = state.clone();
                    state.update(cx, |s, cx| s.set_value(name, window, cx));
                }
                continue;
            }
            let controller = self.controller.clone();
            let state = cx.new(|cx| InputState::new(window, cx).default_value(name));
            let sub = cx.subscribe_in(&state, window, move |_, state, event: &InputEvent, _, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let text = state.read(cx).value().to_string();
                controller.update(cx, |c, cx| c.set_input_name(&id, port, text, cx));
            });
            self._subscriptions.push(sub);
            self.names.insert(key, state);
        }
    }

    // ---- monitors tab ------------------------------------------------------

    fn render_monitors(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let theme = cx.theme();
        let refresh = {
            let controller = self.controller.clone();
            skin(cx)
                .button("refresh".into(), Tone::Ghost, Control::Small, cx)
                .icon(Icon::new(Lucide::RefreshCw))
                .label(if c.scanning {
                    tr!("Scanning…")
                } else {
                    tr!("Scan again")
                })
                .loading(c.scanning)
                .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.refresh_monitors(cx)))
        };
        let header = h_flex()
            .justify_between()
            .items_center()
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .text_sm()
                    .child(match c.monitors.len() {
                        0 => tr!("No monitors detected").to_string(),
                        n => tr!(
                            n = n,
                            "Connected to 1 DDC/CI monitor" | "Connected to {n} DDC/CI monitors"
                        ),
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(c.config.developer_mode, |el| {
                        let controller = self.controller.clone();
                        el.child(
                            skin(cx)
                                .button("copy-diagnostics".into(), Tone::Ghost, Control::Small, cx)
                                .icon(Icon::new(Lucide::Copy))
                                .label(tr!("Copy diagnostics report"))
                                .on_click(move |_, _, cx| {
                                    let report = controller.read(cx).diagnostics_report();
                                    cx.write_to_clipboard(ClipboardItem::new_string(report));
                                    controller.update(cx, |c, cx| {
                                        c.notice = Some(tr!("Diagnostics report copied to the clipboard").into());
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                    .child(refresh),
            );

        let mut list = v_flex().gap_4().child(header);
        if c.monitors.is_empty() && !c.scanning {
            list = list.child(
                card(cx)
                    .items_center()
                    .gap_2()
                    .py_8()
                    .child(Icon::new(Lucide::Monitor).size(px(32.)))
                    .child(tr!("No external monitors supporting DDC/CI were found"))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(tr!(
                        "Turn on DDC/CI in the monitor's on-screen menu, then click \"Scan again\". Built-in laptop screens do not support DDC/CI."
                    ))),
            );
        }
        for (idx, m) in c.monitors.iter().enumerate() {
            list = list.child(self.render_monitor(idx, m, c, cx));
        }
        list.into_any_element()
    }

    fn render_monitor(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let id = m.id().to_string();

        let feature_row = |code: u8, label: &'static str, icon: Lucide| {
            let row = h_flex()
                .gap_3()
                .items_center()
                .child(Icon::new(icon).text_color(theme.muted_foreground))
                .child(div().w(px(48.)).child(label));
            match self.sliders.get(&(id.clone(), code)) {
                Some(slider) => row.child(div().flex_1().child(Slider::new(slider))).children(
                    self.numbers
                        .get(&feature_key(&id, code))
                        .map(|f| div().w(px(104.)).child(f.input())),
                ),
                None => row.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(tr!("Not supported")),
                ),
            }
        };

        let model = m
            .dev
            .caps
            .as_ref()
            .and_then(|caps| caps.model.clone())
            .filter(|model| *model != m.dev.name);
        card(cx)
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(Lucide::Monitor))
                    .child(
                        div()
                            .font_weight(skin(cx).weight(Voice::Loud))
                            .child(m.dev.name.clone()),
                    )
                    .children(model.map(|model| div().text_sm().text_color(theme.muted_foreground).child(model))),
            )
            .child(feature_row(VCP_BRIGHTNESS, tr!("Brightness"), Lucide::Sun))
            .child(feature_row(VCP_CONTRAST, tr!("Contrast"), Lucide::Contrast))
            .child(self.render_switching(idx, m, c, cx))
            .when(c.config.developer_mode, |el| el.child(render_diagnostics(m, cx)))
    }

    /// The input-switching block on the Monitors tab.
    ///
    /// One button per computer, each meaning exactly "put the monitor here".
    /// Nothing here is derived from the monitor's current input: the other
    /// computer can change that at any moment, so a button built on it would
    /// be stale until something refreshed it — and a button that says where it
    /// goes never needs to know where you are.
    fn render_switching(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let id = m.id().to_string();
        let hotkey = c.config.hotkeys.toggle_input.clone();
        let endpoints = c.endpoints(m);

        let header = h_flex()
            .gap_2()
            .items_center()
            .child(Icon::new(Lucide::ArrowLeftRight).size(px(14.)))
            .child(
                div()
                    .text_sm()
                    .font_weight(skin(cx).weight(Voice::Plain))
                    .child(if endpoints.len() == 2 {
                        tr!("One-key switching")
                    } else {
                        tr!("Input switching")
                    }),
            )
            .child(div().flex_1())
            .child(hotkey_caps(&hotkey, cx));

        // Nothing to offer at all: this is the one case that needs setup before
        // it can do anything, so it is the one case that sends you to Settings.
        if endpoints.is_empty() {
            return skin(cx)
                .panel(cx)
                .gap_2()
                .child(header)
                .child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(tr!("No computers sharing this monitor are set up yet.")),
                )
                .child(
                    skin(cx)
                        .button(
                            SharedString::from(format!("configure-{idx}")),
                            Tone::Outline,
                            Control::Small,
                            cx,
                        )
                        .label(tr!("Set up on the Settings tab"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tab = Tab::Settings;
                            cx.notify();
                        })),
                )
                .into_any_element();
        }

        let local = c.local_input(m);
        let n = endpoints.len();

        let rows = endpoints.iter().enumerate().map(|(ix, &port)| {
            let here = local == Some(port);
            let id = id.clone();
            let name = c.monitor_prefs(&id).label(port);
            h_flex()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .rounded_md()
                .children((n >= 3).then(|| {
                    div()
                        .w(px(16.))
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(format!("{}", ix + 1))
                }))
                .child(
                    div()
                        .size(px(9.))
                        .rounded_full()
                        .bg(if here { theme.success } else { endpoint_color(ix, cx) }),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(skin(cx).weight(Voice::Plain))
                        .child(name.clone()),
                )
                .when(here, |el| el.child(mark(tr!("This PC"), cx)))
                .child(div().flex_1())
                .child(
                    skin(cx)
                        .button(
                            SharedString::from(format!("go-{idx}-{ix}")),
                            Tone::Outline,
                            Control::Small,
                            cx,
                        )
                        .label(tr!("Switch to {name}", name = name))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.controller.update(cx, |c, cx| c.switch_input(&id, port, cx));
                        })),
                )
        });

        let hint = if n == 2 {
            tr!("Press the hotkey to flip between the two, without looking up which one you are on first.")
        } else {
            tr!(
                "Press the hotkey for the quick-switch panel and jump by number — it never passes through the machine in between."
            )
        };

        skin(cx)
            .panel(cx)
            .gap_2()
            .child(header)
            .children(rows)
            .child(div().text_xs().text_color(theme.muted_foreground).child(hint))
            // A monitor that reports exactly two inputs works with no setup at
            // all, but the rows are then named after the ports rather than the
            // computers, so nudge towards naming them without blocking on it.
            .when(!c.endpoints_configured(m), |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(skin(cx).weight(Voice::Quiet))
                                .text_color(theme.muted_foreground)
                                .child(tr!("These ports have no names yet.")),
                        )
                        .child(
                            skin(cx).button(
                                SharedString::from(format!("name-{idx}")),
                                Tone::Ghost,
                                Control::Tiny,
                                cx,
                            )
                            .label(tr!("Name them on the Settings tab"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tab = Tab::Settings;
                                cx.notify();
                            })),
                        ),
                )
            })
            .when(!m.inputs_reported(), |el| {
                el.child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(tr!("The monitor does not report its input list, so these are the common ports; add more under extra_inputs in the config.")),
                )
            })
            .into_any_element()
    }

    // ---- first-run wizard --------------------------------------------------

    fn render_wizard(
        &self,
        idx: usize,
        m: &MonitorEntry,
        setup: &Setup,
        inputs: &[u8],
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = m.id().to_string();
        let header = |icon: Lucide, text: &'static str| {
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(icon).size(px(14.)))
                .child(div().text_sm().font_weight(skin(cx).weight(Voice::Plain)).child(text))
        };

        if !setup.naming {
            // Step 1: which ports actually have a computer behind them. A
            // monitor reporting four inputs does not mean four computers.
            let chips = inputs.iter().map(|&port| {
                let id = id.clone();
                let on = setup.chosen.contains(&port);
                let here = m.current_input == Some(port);
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        skin(cx)
                            .button(
                                SharedString::from(format!("pick-{idx}-{port}")),
                                if on { Tone::Accent } else { Tone::Outline },
                                Control::Small,
                                cx,
                            )
                            .label(mccs::input_source_name(port))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let entry = this.setup.entry(id.clone()).or_default();
                                match entry.chosen.iter().position(|p| *p == port) {
                                    Some(ix) => {
                                        entry.chosen.remove(ix);
                                    }
                                    None => entry.chosen.push(port),
                                }
                                cx.notify();
                            })),
                    )
                    .when(here, |el| {
                        el.child(div().text_xs().text_color(theme.muted_foreground).child(tr!("This PC")))
                    })
            });
            let count = setup.chosen.len();
            return skin(cx)
                .panel(cx)
                .gap_2()
                .child(header(
                    Lucide::ArrowLeftRight,
                    tr!("How many computers are connected to this monitor?"),
                ))
                .child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(tr!("Select the ports that actually have a computer behind them. Ports that are empty, or that go to a game console or a TV box, can stay unselected.")),
                )
                .child(h_flex().gap_2().flex_wrap().children(chips))
                .child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(match m.current_input {
                            // Only a starting point, read once when the monitor was
                            // enumerated — not a live claim about where it is now.
                            Some(port) => tr!(
                                "Assuming {port} is this computer — click another port if that is wrong",
                                port = mccs::input_source_name(port)
                            ),
                            None => tr!("The monitor reports no inputs, so click whichever port this computer is on").to_string(),
                        }),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            skin(cx).button(
                                SharedString::from(format!("wizard-next-{idx}")),
                                Tone::Accent,
                                Control::Medium,
                                cx,
                            )
                            .label(if count >= 2 {
                                tr!(
                                    n = count,
                                    "Next: name this 1 computer" | "Next: name these {n} computers"
                                )
                            } else {
                                tr!("Pick at least two to switch with one key").to_string()
                            })
                            .disabled(count < 2)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(entry) = this.setup.get_mut(&id) {
                                    entry.naming = true;
                                }
                                cx.notify();
                            })),
                        )
                        .child(
                            // The names live on the monitor, so a machine that
                            // has already been set up can hand them over.
                            skin(cx).button(
                                SharedString::from(format!("wizard-paste-{idx}")),
                                Tone::Outline,
                                Control::Medium,
                                cx,
                            )
                            .label(tr!("Paste from another computer"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let text = cx
                                        .read_from_clipboard()
                                        .and_then(|item| item.text())
                                        .unwrap_or_default();
                                    this.controller.update(cx, |c, cx| c.import_switching(&text, cx));
                                })),
                        ),
                )
                .into_any_element();
        }

        // Step 2: names. This is the whole point — a port number can never tell
        // the user which machine they are looking at.
        let rows = setup.chosen.iter().enumerate().map(|(ix, &port)| {
            let key = endpoint_key(&id, port);
            let here = setup.local == Some(port);
            let suggestions = NAME_SUGGESTIONS.iter().map(|name| {
                let id = id.clone();
                let key = key.clone();
                // The label is also the name written to the config, so both
                // sides of the comparison come from the same translation.
                let name = SharedString::from(translate(*name));
                let label = name.clone();
                let picked = self
                    .names
                    .get(&key)
                    .map(|state| state.read(cx).value() == name)
                    .unwrap_or(false);
                skin(cx)
                    .button(
                        SharedString::from(format!("suggest-{idx}-{ix}-{label}")),
                        if picked { Tone::Accent } else { Tone::Ghost },
                        Control::Tiny,
                        cx,
                    )
                    .label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(state) = this.names.get(&key).cloned() {
                            state.update(cx, |s, cx| s.set_value(name.clone(), window, cx));
                        }
                        this.controller
                            .update(cx, |c, cx| c.set_input_name(&id, port, name.to_string(), cx));
                        cx.notify();
                    }))
            });
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .w(px(18.))
                                .text_xs()
                                .font_weight(skin(cx).weight(Voice::Quiet))
                                .text_color(theme.muted_foreground)
                                .child(format!("{}", ix + 1)),
                        )
                        .child({
                            let id = id.clone();
                            let current_local = setup.local;
                            div()
                                .id(SharedString::from(format!("wizard-local-{idx}-{ix}")))
                                .w(px(22.))
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .hover(|s| s.bg(theme.muted))
                                .child(div().size(px(9.)).rounded_full().bg(if here {
                                    theme.success
                                } else {
                                    endpoint_color(ix, cx)
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(entry) = this.setup.get_mut(&id) {
                                        entry.local = Some(port);
                                    }
                                    let _ = current_local;
                                    cx.notify();
                                }))
                        })
                        .child(div().w(px(158.)).children(self.names.get(&key).map(Input::new)))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(skin(cx).weight(Voice::Quiet))
                                .text_color(theme.muted_foreground)
                                .child(mccs::input_source_name(port)),
                        )
                        .when(here, |el| el.child(mark(tr!("This PC"), cx))),
                )
                .child(h_flex().gap_1().pl_4().children(suggestions))
        });

        skin(cx)
            .panel(cx)
            .gap_3()
            .child(header(Lucide::Laptop, tr!("Give them names")))
            .child(
                div()
                    .text_xs()
                    .font_weight(skin(cx).weight(Voice::Quiet))
                    .text_color(theme.muted_foreground)
                    .child(tr!("The names appear here, in the tray menu, and in the quick-switch panel — they are the only thing that lets you tell the machines apart at a glance.")),
            )
            .children(rows)
            .child(
                div()
                    .text_xs()
                    .font_weight(skin(cx).weight(Voice::Quiet))
                    .text_color(theme.muted_foreground)
                    .child(tr!(
                        "This computer takes its name from the system ({hostname}); for the rest, click a common name. Install tarsier on the other computers too and paste this set of names across, and the 3rd and 4th need no typing at all.",
                        hostname = platform::local_hostname()
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        skin(cx).button(
                            SharedString::from(format!("wizard-back-{idx}")),
                            Tone::Outline,
                            Control::Medium,
                            cx,
                        )
                        .label(tr!("Back"))
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, _, cx| {
                                if let Some(entry) = this.setup.get_mut(&id) {
                                    entry.naming = false;
                                }
                                cx.notify();
                            }
                        })),
                    )
                    .child(
                        skin(cx).button(
                            SharedString::from(format!("wizard-done-{idx}")),
                            Tone::Accent,
                            Control::Medium,
                            cx,
                        )
                        .label(tr!("Done"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(setup) = this.setup.get(&id).cloned() else {
                                    return;
                                };
                                let local = setup
                                    .local
                                    .filter(|p| setup.chosen.contains(p))
                                    .or_else(|| setup.chosen.first().copied());
                                let names: Vec<(u8, String)> = setup
                                    .chosen
                                    .iter()
                                    .map(|port| {
                                        let value = this
                                            .names
                                            .get(&endpoint_key(&id, *port))
                                            .map(|s| s.read(cx).value().to_string())
                                            .unwrap_or_default();
                                        (*port, value)
                                    })
                                    .collect();
                                let ports = setup.chosen.clone();
                                this.controller.update(cx, |c, cx| {
                                    c.set_endpoints(&id, ports, local, cx);
                                    for (port, name) in names {
                                        c.set_input_name(&id, port, name, cx);
                                    }
                                });
                                this.setup.remove(&id);
                                cx.notify();
                            })),
                    )
                    .child(
                        // The same shortcut as the first step, because a
                        // monitor that reports exactly two inputs opens here:
                        // a machine that is already set up can just hand the
                        // names over instead of them being typed again.
                        skin(cx).button(
                            SharedString::from(format!("wizard-paste2-{idx}")),
                            Tone::Outline,
                            Control::Medium,
                            cx,
                        )
                        .label(tr!("Paste from another computer"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let text = cx
                                    .read_from_clipboard()
                                    .and_then(|item| item.text())
                                    .unwrap_or_default();
                                this.controller.update(cx, |c, cx| c.import_switching(&text, cx));
                            })),
                    ),
            )
            .into_any_element()
    }

    // ---- endpoint editor (Settings tab) ------------------------------------

    /// The editable form for one monitor's computers.
    ///
    /// It lives on the Settings tab, where the rest of the configuration is.
    /// The Monitors tab renders the same list read-only, for looking and
    /// switching. The two share the data and the colours but not the markup:
    /// almost every cell differs, a text field against a label, a port chooser
    /// against a port name.
    fn render_endpoint_editor(
        &self,
        idx: usize,
        m: &MonitorEntry,
        c: &Controller,
        endpoints: &[u8],
        inputs: &[u8],
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = m.id().to_string();
        let local = c.local_input(m);
        let n = endpoints.len();

        let rows = endpoints.iter().enumerate().map(|(ix, &port)| {
            let here = local == Some(port);
            let key = endpoint_key(&id, port);
            let picker_open = self.port_picker.as_deref() == Some(key.as_str());

            let strip = picker_open.then(|| {
                let free = inputs.iter().copied().filter(|p| *p == port || !endpoints.contains(p));
                h_flex().gap_1().flex_wrap().pl_4().children(free.map(|p| {
                    let id = id.clone();
                    skin(cx)
                        .button(
                            SharedString::from(format!("setport-{idx}-{ix}-{p}")),
                            if p == port { Tone::Accent } else { Tone::Ghost },
                            Control::Tiny,
                            cx,
                        )
                        .label(mccs::input_source_name(p))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.controller
                                .update(cx, |c, cx| c.set_endpoint_port(&id, port, p, cx));
                            this.port_picker = None;
                            cx.notify();
                        }))
                }))
            });

            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .children((n >= 3).then(|| {
                            div()
                                .w(px(16.))
                                .text_xs()
                                .font_weight(skin(cx).weight(Voice::Quiet))
                                .text_color(theme.muted_foreground)
                                .child(format!("{}", ix + 1))
                        }))
                        .child({
                            let id = id.clone();
                            div()
                                .id(SharedString::from(format!("local-{idx}-{ix}")))
                                .w(px(22.))
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .hover(|s| s.bg(theme.border))
                                .child(div().size(px(9.)).rounded_full().bg(if here {
                                    theme.success
                                } else {
                                    endpoint_color(ix, cx)
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.controller.update(cx, |c, cx| c.set_local_input(&id, port, cx));
                                    cx.notify();
                                }))
                        })
                        .child(div().w(px(158.)).children(self.names.get(&key).map(Input::new)))
                        .when(here, |el| el.child(mark(tr!("This PC"), cx)))
                        .child(div().flex_1())
                        .child({
                            let key = key.clone();
                            skin(cx)
                                .button(
                                    SharedString::from(format!("port-{idx}-{ix}")),
                                    Tone::Outline,
                                    Control::Tiny,
                                    cx,
                                )
                                .label(mccs::input_source_name(port))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.port_picker =
                                        (this.port_picker.as_deref() != Some(key.as_str())).then(|| key.clone());
                                    cx.notify();
                                }))
                        })
                        .children((n > 2).then(|| {
                            let id = id.clone();
                            skin(cx)
                                .button(
                                    SharedString::from(format!("drop-{idx}-{ix}")),
                                    Tone::Ghost,
                                    Control::Tiny,
                                    cx,
                                )
                                .label("✕")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.port_picker = None;
                                    this.controller.update(cx, |c, cx| c.remove_endpoint(&id, port, cx));
                                }))
                        })),
                )
                .children(strip)
        });

        skin(cx)
            .panel(cx)
            .gap_2()
            .children(rows)
            .child({
                let id = id.clone();
                let free = inputs.iter().copied().find(|p| !endpoints.contains(p));
                skin(cx).button(
                    SharedString::from(format!("add-{idx}")),
                    Tone::Ghost,
                    Control::Tiny,
                    cx,
                )
                .label(tr!("+ Add computer"))
                .disabled(free.is_none())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(port) = free {
                        this.controller.update(cx, |c, cx| c.add_endpoint(&id, port, cx));
                    }
                }))
            })
            .when(!m.inputs_reported(), |el| {
                el.child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(tr!("The monitor does not report its input list, so these are the common ports; add more under extra_inputs in the config.")),
                )
            })
            .into_any_element()
    }

    /// One monitor's block in the Settings tab's input card: the first-run
    /// wizard while it has not been through setup, the editable list after.
    fn render_input_settings(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let id = m.id().to_string();
        let inputs = c.inputs_for(m);

        let body: AnyElement = match self.setup.get(&id) {
            // `naming` is checked as well so an import landing mid-wizard
            // takes effect on this paint rather than the next one.
            Some(setup) if setup.naming || !c.endpoints_configured(m) => self.render_wizard(idx, m, setup, &inputs, cx),
            _ => {
                let endpoints = c.endpoints(m);
                if endpoints.is_empty() {
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(tr!("This monitor does not report an input list; add its ports under extra_inputs in the config first."))
                        .into_any_element()
                } else {
                    self.render_endpoint_editor(idx, m, c, &endpoints, &inputs, cx)
                }
            }
        };

        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(skin(cx).weight(Voice::Plain))
                    .child(m.dev.name.clone()),
            )
            .child(body)
            .into_any_element()
    }

    // ---- breaks tab --------------------------------------------------------

    fn render_breaks(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let theme = cx.theme();
        let t = &c.tracker;
        let work = t.settings().work_secs;
        let (title, detail) = match t.phase() {
            _ if !c.config.breaks.enabled => (
                tr!("Break reminders are off").to_string(),
                tr!("Turn them back on from the Settings tab").to_string(),
            ),
            Phase::Away => (
                tr!("You stepped away for a while 👋").to_string(),
                tr!("A fresh timer starts when you come back").to_string(),
            ),
            Phase::Prompted { .. } => (
                tr!("Taking a break").to_string(),
                tr!("Rest for another {time}", time = format_minutes(t.rest_remaining())),
            ),
            Phase::Working => (
                tr!("Working for {time}", time = format_minutes(t.session_active())),
                if c.is_paused() {
                    tr!("Reminders are paused").to_string()
                } else {
                    tr!("Break in {time}", time = format_minutes(t.until_prompt()))
                },
            ),
        };
        let progress = (t.session_active() as f32 / work.max(1) as f32 * 100.0).min(100.0);
        let over = t.session_active() > work;

        let today = local_date(now_ts());
        let empty = Default::default();
        let day = c.stats.day(today).unwrap_or(&empty);
        let score = c.today_score();

        let controller = self.controller.clone();
        let controller2 = self.controller.clone();
        let status = card(cx)
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(Lucide::Timer))
                    .child(div().text_lg().font_weight(skin(cx).weight(Voice::Loud)).child(title)),
            )
            .child(
                Progress::new("work")
                    .value(progress)
                    .when(over, |p| p.color(theme.danger)),
            )
            .child(div().text_sm().text_color(theme.muted_foreground).child(detail))
            .child(if matches!(t.phase(), Phase::Prompted { .. }) {
                let snooze = self.controller.clone();
                let skip = self.controller.clone();
                h_flex()
                    .gap_2()
                    .child(
                        skin(cx)
                            .button("snooze".into(), Tone::Accent, Control::Medium, cx)
                            .label(tr!(
                                n = c.config.breaks.snooze_minutes,
                                "Snooze for 1 min" | "Snooze for {n} min"
                            ))
                            .on_click(move |_, _, cx| snooze.update(cx, |c, cx| c.snooze(cx))),
                    )
                    .child(
                        skin(cx)
                            .button("skip".into(), Tone::Outline, Control::Medium, cx)
                            .label(tr!("Skip this break"))
                            .on_click(move |_, _, cx| skip.update(cx, |c, cx| c.skip(cx))),
                    )
            } else {
                h_flex()
                    .gap_2()
                    .child(
                        skin(cx)
                            .button("break-now".into(), Tone::Accent, Control::Medium, cx)
                            .icon(Icon::new(Lucide::Coffee))
                            .label(tr!("Take a break now"))
                            .disabled(t.phase() != Phase::Working)
                            .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.break_now(cx))),
                    )
                    .child(
                        skin(cx)
                            .button("pause".into(), Tone::Outline, Control::Medium, cx)
                            .label(if c.is_paused() {
                                tr!("Resume reminders")
                            } else {
                                tr!("Pause reminders for 1 hour")
                            })
                            .on_click(move |_, _, cx| controller2.update(cx, |c, cx| c.toggle_pause(cx))),
                    )
            });

        let score_card = card(cx)
            .gap_2()
            .child(section_label(tr!("Today's health score"), cx))
            .child(
                h_flex()
                    .items_end()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(48.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(score_color(score, cx))
                            .child(score.map_or("--".to_string(), |s| s.to_string())),
                    )
                    .child(div().pb_2().text_lg().child(
                        score.map_or(tr!("Scoring starts after 15 minutes of use"), stats::grade),
                    )),
            )
            .child(
                h_flex()
                    .gap_4()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr!(
                        "Screen time {time}",
                        time = format_minutes(day.active_secs() + t.session_active())
                    ))
                    .child(tr!(n = day.breaks(), "1 break" | "{n} breaks"))
                    .child(tr!(
                        "Longest stretch {time}",
                        time = format_minutes(day.longest_secs().max(t.session_active()))
                    ))
                    .child(tr!("+{points} points today", points = day.points())),
            )
            .child(div().text_xs().text_color(theme.muted_foreground).child(tr!(
                "Scoring: a session scores full marks up to 110% of {work} minutes, and loses more the longer it runs past that. Being away from the computer for {rest} minutes counts as a break automatically.",
                work = c.config.breaks.work_minutes,
                rest = c.config.breaks.break_minutes
            )));

        v_flex().gap_4().child(status).child(score_card).into_any_element()
    }

    // ---- stats tab -----------------------------------------------------------

    fn render_stats(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let theme = cx.theme();
        let today = local_date(now_ts());
        let today_score = c.today_score();
        let streak = c.stats.streak(today, today_score);
        let points = c.stats.total_points();
        let (level, level_min, next) = stats::level(points);

        let summary = h_flex()
            .gap_3()
            .child(stat_tile(
                Lucide::Flame,
                tr!(n = streak, "1 day" | "{n} days"),
                tr!("Streak"),
                cx,
            ))
            .child(stat_tile(Lucide::Trophy, format!("{points}"), tr!("Total points"), cx))
            .child(stat_tile(Lucide::Activity, level.to_string(), tr!("Current title"), cx));

        let level_progress = match next {
            Some(next) => (points - level_min) as f32 / (next - level_min) as f32 * 100.0,
            None => 100.0,
        };
        let level_card = card(cx)
            .gap_2()
            .child(h_flex().justify_between().text_sm().child(level).child(
                div().text_color(theme.muted_foreground).child(match next {
                    Some(n) => tr!(
                        n = n - points,
                        "1 more point to level up" | "{n} more points to level up"
                    ),
                    None => tr!("Highest level reached").to_string(),
                }),
            ))
            .child(Progress::new("level").value(level_progress));

        // Last 7 days, oldest first.
        let bar_max = 120.0;
        let bars = h_flex()
            .gap_2()
            .items_end()
            .h(px(bar_max + 40.))
            .children((0..7).rev().map(|ago| {
                let date = today.checked_sub_days(Days::new(ago)).unwrap_or(today);
                let score = if ago == 0 {
                    today_score
                } else {
                    c.stats.day(date).and_then(|d| d.score())
                };
                let height = score.map_or(4.0, |s| (s as f32 / 100.0 * bar_max).max(4.0));
                v_flex()
                    .flex_1()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(skin(cx).weight(Voice::Quiet))
                            .text_color(theme.muted_foreground)
                            .child(score.map_or(String::new(), |s| s.to_string())),
                    )
                    .child(skin(cx).status_bar(level_of(score), px(height), cx))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(if ago == 0 {
                        tr!("Today").to_string()
                    } else {
                        date.format("%m/%d").to_string()
                    }))
            }));

        let empty = Default::default();
        let day = c.stats.day(today).unwrap_or(&empty);
        let mut sessions = v_flex().gap_1();
        if day.sessions.is_empty() {
            sessions = sessions.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr!("No completed sessions today")),
            );
        }
        for s in day.sessions.iter().rev() {
            let fmt = |ts: i64| {
                Local
                    .timestamp_opt(ts, 0)
                    .single()
                    .map(|d| d.format("%H:%M").to_string())
                    .unwrap_or_default()
            };
            let score = s.score();
            sessions = sessions.child(
                h_flex()
                    .gap_3()
                    .text_sm()
                    .py_1()
                    .child(div().w(px(100.)).child(format!("{} – {}", fmt(s.start), fmt(s.end))))
                    .child(div().flex_1().child(format_minutes(s.active_secs)))
                    .child(div().text_color(theme.muted_foreground).child(match s.kind {
                        BreakKind::Natural => tr!("Natural break"),
                        BreakKind::Prompted => tr!("Prompted break"),
                    }))
                    .child(
                        div()
                            .w(px(56.))
                            .text_right()
                            .font_weight(skin(cx).weight(Voice::Plain))
                            .text_color(score_color(Some(score), cx))
                            .child(tr!("{score} points", score = score)),
                    ),
            );
        }
        let skips = (day.skips + day.snoozes + day.ignored > 0).then(|| {
            div().text_xs().text_color(theme.muted_foreground).child(tr!(
                "Today: {skipped} skipped, {snoozed} snoozed, {ignored} ignored",
                skipped = day.skips,
                snoozed = day.snoozes,
                ignored = day.ignored
            ))
        });

        v_flex()
            .gap_4()
            .child(summary)
            .child(level_card)
            .child(
                card(cx)
                    .gap_2()
                    .child(section_label(tr!("Health score, last 7 days"), cx))
                    .child(bars),
            )
            .child(
                card(cx)
                    .gap_1()
                    .child(section_label(tr!("Today's sessions"), cx))
                    .child(sessions)
                    .children(skips),
            )
            .into_any_element()
    }

    // ---- settings tab ----------------------------------------------------------

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let theme = cx.theme();
        let b = c.config.breaks.clone();

        let toggle_row = |id: &'static str,
                          label: &'static str,
                          desc: &'static str,
                          checked: bool,
                          f: fn(&mut Controller, bool, &mut Context<Controller>)| {
            let controller = self.controller.clone();
            h_flex()
                .justify_between()
                .gap_4()
                .child(
                    v_flex()
                        .child(label)
                        .child(div().text_xs().text_color(theme.muted_foreground).child(desc)),
                )
                .child(Switch::new(id).checked(checked).on_click(move |v, _, cx| {
                    let v = *v;
                    controller.update(cx, |c, cx| f(c, v, cx));
                }))
        };

        let duration_row = |(key, label, ..): BreakField| {
            h_flex()
                .justify_between()
                .items_center()
                .child(translate(label))
                .children(self.numbers.get(key).map(|f| {
                    div().w(px(148.)).child(
                        f.input().suffix(
                            div()
                                .pr_1()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(tr!("min")),
                        ),
                    )
                }))
        };

        // Three small buttons rather than a dropdown: every option is visible at
        // a glance, and this is the same primary/outline pair idiom the monitor
        // input pickers use.
        let skin_picker = || {
            let controller = self.controller.clone();
            let current = c.config.skin;
            h_flex()
                .gap_1()
                .children(crate::skin::Skin::ALL.into_iter().map(|pick| {
                    let controller = controller.clone();
                    skin(cx)
                        .button(
                            SharedString::from(format!("skin-{pick:?}")),
                            if pick == current { Tone::Accent } else { Tone::Ghost },
                            Control::Small,
                            cx,
                        )
                        .label(pick.label())
                        .on_click(move |_, _, cx| {
                            controller.update(cx, |c, cx| c.set_skin(pick, cx));
                        })
                }))
        };

        let theme_picker = || {
            let controller = self.controller.clone();
            let current = c.config.theme;
            h_flex()
                .gap_1()
                .children(config::ThemePref::ALL.into_iter().map(|pref| {
                    let controller = controller.clone();
                    skin(cx)
                        .button(
                            SharedString::from(format!("theme-{pref:?}")),
                            if pref == current { Tone::Accent } else { Tone::Ghost },
                            Control::Small,
                            cx,
                        )
                        .label(pref.label())
                        .on_click(move |_, _, cx| {
                            controller.update(cx, |c, cx| c.set_theme(pref, cx));
                        })
                }))
        };

        let language_picker = || {
            let controller = self.controller.clone();
            let current = c.config.language;
            h_flex()
                .gap_1()
                .children(crate::i18n::Language::ALL.into_iter().map(|lang| {
                    let controller = controller.clone();
                    skin(cx)
                        .button(
                            SharedString::from(format!("language-{lang:?}")),
                            if lang == current { Tone::Accent } else { Tone::Ghost },
                            Control::Small,
                            cx,
                        )
                        // A language names itself, in its own language: whoever
                        // picked the wrong one still has to find the way back.
                        .label(lang.label())
                        .on_click(move |_, _, cx| {
                            controller.update(cx, |c, cx| c.set_language(lang, cx));
                        })
                }))
        };

        let skin_row = h_flex()
            .justify_between()
            .gap_4()
            .child(
                v_flex().child(tr!("Skin")).child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(skin(cx).note()),
                ),
            )
            .child(skin_picker());

        // A skin may only ship one mode. Rather than showing a picker whose
        // buttons do nothing, the row says so and states which mode is in use —
        // the saved preference is kept either way, so it returns the moment a
        // skin that honours it is picked.
        let modes = c.config.skin.style().modes();
        let appearance_row = if modes.len() == 1 {
            h_flex()
                .justify_between()
                .gap_4()
                .child(
                    v_flex().child(tr!("Appearance")).child(
                        div()
                            .text_xs()
                            .font_weight(skin(cx).weight(Voice::Quiet))
                            .text_color(theme.muted_foreground)
                            .child(tr!(
                                "A skin decides the look, and some only ever ship one light / dark mode"
                            )),
                    ),
                )
                .child(skin(cx).chip(
                    match modes[0] {
                        ThemeMode::Light => tr!("Light"),
                        ThemeMode::Dark => tr!("Dark"),
                    },
                    Tone::Muted,
                    cx,
                ))
        } else {
            h_flex()
                .justify_between()
                .gap_4()
                .child(
                    v_flex().child(tr!("Appearance")).child(
                        div()
                            .text_xs()
                            .font_weight(skin(cx).weight(Voice::Quiet))
                            .text_color(theme.muted_foreground)
                            .child(tr!("System tracks the Windows light / dark setting as you change it")),
                    ),
                )
                .child(theme_picker())
        };

        let language_row = h_flex()
            .justify_between()
            .gap_4()
            .child(
                v_flex().child(tr!("Language")).child(
                    div()
                        .text_xs()
                        .font_weight(skin(cx).weight(Voice::Quiet))
                        .text_color(theme.muted_foreground)
                        .child(tr!("Switching redraws every window right away")),
                ),
            )
            .child(language_picker());

        let general = card(cx)
            .gap_4()
            .child(section_label(tr!("General"), cx))
            .child(skin_row)
            .child(appearance_row)
            .child(language_row)
            .child(toggle_row(
                "autostart",
                tr!("Start automatically at login"),
                tr!("Runs quietly in the tray after you sign in to Windows"),
                c.autostart,
                |c, v, cx| c.set_autostart(v, cx),
            ))
            .child(toggle_row(
                "developer-mode",
                tr!("Developer mode"),
                tr!("Shows DDC/CI diagnostics and command tracing on the Monitors tab, and logs every command"),
                c.config.developer_mode,
                |c, v, cx| c.set_developer_mode(v, cx),
            ));

        let breaks = card(cx)
            .gap_4()
            .child(section_label(tr!("Break reminders"), cx))
            .child(toggle_row(
                "breaks-enabled",
                tr!("Turn on break reminders"),
                tr!("When the time is up, a full-screen reminder fades in; stepping away counts as a break automatically"),
                b.enabled,
                |c, v, cx| c.update_config(cx, |cfg| cfg.breaks.enabled = v),
            ))
            .child(toggle_row(
                "fullscreen",
                tr!("Stay quiet in fullscreen / presentations"),
                tr!("Holds reminders while you are gaming, watching video, or presenting"),
                b.respect_fullscreen,
                |c, v, cx| c.update_config(cx, |cfg| cfg.breaks.respect_fullscreen = v),
            ))
            .children(BREAK_FIELDS.map(duration_row));

        let hotkeys = &c.config.hotkeys;
        let hk = |label: &'static str, value: &str| {
            h_flex()
                .justify_between()
                .text_sm()
                .child(label)
                .child(div().text_color(theme.muted_foreground).child(if value.is_empty() {
                    tr!("Not set").to_string()
                } else {
                    value.to_string()
                }))
        };
        // Moving the setup to another computer is a whole-app action, not a
        // per-monitor one, so it lives here rather than inside a monitor card.
        let switching_card = {
            let export = self.controller.clone();
            let import = self.controller.clone();
            card(cx)
                .gap_4()
                .child(section_label(tr!("Monitor inputs"), cx))
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    tr!("List the computers sharing each monitor and give them names — the names are the only thing that lets you tell them apart at a glance. Click the dot on the left to mark the one you are sitting at; it only affects what is shown, never switching."),
                ))
                .when(c.monitors.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .font_weight(skin(cx).weight(Voice::Quiet))
                            .text_color(theme.muted_foreground)
                            .child(tr!("No external monitors that support DDC/CI have been detected yet.")),
                    )
                })
                .children(
                    c.monitors
                        .iter()
                        .enumerate()
                        .map(|(idx, m)| self.render_input_settings(idx, m, c, cx)),
                )
                .child(skin(cx).band(cx))
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    tr!("The port-to-name mapping lives on the monitor, so it holds whichever computer you fill it in on. Set it up once and move it to the rest, and you never type it in again."),
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            skin(cx).button("export-switching".into(), Tone::Outline, Control::Small, cx)
                                .icon(Icon::new(Lucide::Copy))
                                .label(tr!("Copy settings"))
                                .on_click(move |_, _, cx| {
                                    let text = export.read(cx).export_switching();
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                    export.update(cx, |c, cx| {
                                        c.notice =
                                            Some(tr!("Copied. Click \"Import settings\" on the other computer.").into());
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            skin(cx).button("import-switching".into(), Tone::Outline, Control::Small, cx)
                                .label(tr!("Import settings"))
                                .on_click(move |_, _, cx| {
                                    let text = cx
                                        .read_from_clipboard()
                                        .and_then(|item| item.text())
                                        .unwrap_or_default();
                                    import.update(cx, |c, cx| {
                                        c.import_switching(&text, cx);
                                    });
                                }),
                        ),
                )
        };

        let hotkey_card = card(cx)
            .gap_2()
            .child(section_label(
                tr!("Global hotkeys (restart after editing the config file)"),
                cx,
            ))
            .child(hk(tr!("Switch monitor input"), &hotkeys.toggle_input))
            .child(hk(tr!("Brightness up"), &hotkeys.brightness_up))
            .child(hk(tr!("Brightness down"), &hotkeys.brightness_down))
            .child(hk(tr!("Take a break now"), &hotkeys.break_now))
            .children(c.hotkey_errors.iter().map(|e| {
                div()
                    .text_xs()
                    .text_color(skin(cx).status_text(Level::Poor, cx))
                    .child(tr!("Registration failed: {e}", e = e))
            }))
            .child(
                h_flex().pt_2().child(
                    skin(cx)
                        .button("open-config".into(), Tone::Outline, Control::Small, cx)
                        .label(tr!("Open config folder"))
                        .on_click(|_, _, cx| {
                            let dir = config::data_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            cx.reveal_path(&config::config_path().exists().then(config::config_path).unwrap_or(dir));
                        }),
                ),
            );

        v_flex()
            .gap_4()
            .child(general)
            .child(breaks)
            .child(switching_card)
            .child(hotkey_card)
            .child(
                div()
                    .text_xs()
                    .font_weight(skin(cx).weight(Voice::Quiet))
                    .text_color(theme.muted_foreground)
                    .child(format!("tarsier v{}", env!("CARGO_PKG_VERSION"))),
            )
            .into_any_element()
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_index = TABS.iter().position(|t| *t == self.tab).unwrap_or(0);
        // One page frame for every tab: the skin's title treatment, then the
        // band. Before this, each tab started wherever its first card happened
        // to, which gave the style nowhere to put its display type.
        let page_title = match self.tab {
            Tab::Monitors => tr!("Monitors"),
            Tab::Breaks => tr!("Breaks"),
            Tab::Stats => tr!("Stats"),
            Tab::Settings => tr!("Settings"),
        };
        let body = match self.tab {
            Tab::Monitors => self.render_monitors(cx),
            Tab::Breaks => self.render_breaks(cx),
            Tab::Stats => self.render_stats(cx),
            Tab::Settings => self.render_settings(cx),
        };
        let notice = self.controller.read(cx).notice.clone();
        let theme = cx.theme();
        let page = v_flex()
            .gap_6()
            .child(skin(cx).display(page_title, cx))
            .child(skin(cx).band(cx))
            .child(body)
            .into_any_element();
        let tabs = TabBar::new("tabs")
            .segmented()
            .small()
            .selected_index(tab_index)
            .child(skin(cx).case(tr!("Monitors")))
            .child(skin(cx).case(tr!("Breaks")))
            .child(skin(cx).case(tr!("Stats")))
            .child(skin(cx).case(tr!("Settings")))
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.tab = TABS[*ix];
                cx.notify();
            }));

        // Self-drawn title bar: brand on the left, tabs in the drag area, native
        // min/max/close hit-testing (snap layouts keep working) on the right.
        // The skin's title-bar colours are the band this sits on.
        let title_bar = TitleBar::new()
            .h(px(46.))
            .pl_4()
            .bg(theme.title_bar)
            .border_color(theme.title_bar_border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(img(brand_icon()).size(px(22.)))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(skin(cx).weight(Voice::Loud))
                            .child(skin(cx).case("tarsier")),
                    ),
            )
            .child(div().pr_2().child(tabs));

        v_flex()
            .size_full()
            .font(skin(cx).font(cx))
            .child(title_bar)
            // The canvas is a layer rather than a background colour so a skin
            // can put a texture behind the content and keep it still while the
            // body scrolls over it.
            .child(
                skin(cx)
                    .canvas(cx)
                    .flex_1()
                    .child(div().id("body").flex_1().overflow_y_scroll().px_4().py_4().child(page)),
            )
            .children(notice.map(|n| {
                let controller = self.controller.clone();
                h_flex()
                    .id("notice")
                    .px_4()
                    .py_2()
                    .gap_2()
                    .justify_between()
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.muted)
                    .text_sm()
                    .child(n)
                    .child(
                        skin(cx)
                            .button("dismiss".into(), Tone::Ghost, Control::Tiny, cx)
                            .icon(IconName::Close)
                            .on_click(move |_, _, cx| {
                                controller.update(cx, |c, cx| {
                                    c.notice = None;
                                    cx.notify();
                                })
                            }),
                    )
            }))
    }
}

fn feature_key(monitor_id: &str, code: u8) -> String {
    format!("{monitor_id}#{code}")
}

/// Developer mode panel: how this monitor is driven and what was sent to it.
fn render_diagnostics(m: &MonitorEntry, cx: &App) -> impl IntoElement {
    /// Commands shown in the panel; the copied report has the full trace.
    const RECENT: usize = 12;
    let theme = cx.theme();
    let mono = |text: String| div().font_family("Consolas").text_xs().child(text);
    let rows = m.dev.diagnostics.rows().into_iter().map(|(label, value)| {
        h_flex()
            .gap_3()
            .items_start()
            .child(
                div()
                    .w(px(56.))
                    .flex_none()
                    .text_xs()
                    .font_weight(skin(cx).weight(Voice::Quiet))
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(mono(value).flex_1().min_w_0())
    });
    let trace = m.dev.trace();
    let lines = trace.iter().rev().take(RECENT).rev().map(|entry| {
        let failed = entry.result.is_err();
        mono(display::diagnostics::trace_line(entry))
            .when(failed, |el| el.text_color(skin(cx).status_text(Level::Poor, cx)))
    });
    // The panel is a recessed block inside the monitor card, so it takes the
    // skin's panel rather than inventing a border of its own.
    skin(cx)
        .panel(cx)
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(Lucide::Bug).size(px(14.)))
                .child(
                    div()
                        .text_sm()
                        .font_weight(skin(cx).weight(Voice::Plain))
                        .child(tr!("Diagnostics")),
                ),
        )
        .child(mono(m.id().to_string()).text_color(theme.muted_foreground))
        .children(rows)
        .child(section_label(tr!("Recent commands"), cx))
        .when(trace.is_empty(), |el| {
            el.child(div().text_xs().text_color(theme.muted_foreground).child(tr!("(none)")))
        })
        .children(lines)
}

fn card(cx: &App) -> Div {
    skin(cx).card(cx)
}

fn section_label(text: &'static str, cx: &App) -> AnyElement {
    skin(cx).section_label(text, cx)
}

/// The status a score reads as, before the skin decides how to say it.
fn level_of(score: Option<u8>) -> Level {
    match score {
        Some(s) if s >= GOOD_SCORE => Level::Good,
        Some(s) if s >= 50 => Level::Fair,
        Some(_) => Level::Poor,
        None => Level::Neutral,
    }
}

/// The app icon, decoded once so GPUI's image cache keeps hitting the same id.
fn brand_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../assets/icon.png").to_vec(),
        ))
    })
    .clone()
}

fn stat_tile(icon: Lucide, value: String, label: &'static str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    // The one place a surface answers the pointer: a summary tile is glanceable
    // rather than clickable, so it gets the style's tactile lift without
    // pretending to be a button.
    skin(cx).lift(
        card(cx)
            .flex_1()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_color(theme.muted_foreground)
                    .text_sm()
                    .child(Icon::new(icon).size(px(14.)))
                    .child(label),
            )
            .child(div().text_xl().font_weight(skin(cx).weight(Voice::Loud)).child(value)),
        cx,
    )
}

fn score_color(score: Option<u8>, cx: &App) -> Hsla {
    skin(cx).status_text(level_of(score), cx)
}

#[cfg(test)]
mod tests {
    // Deliberately not `use super::*`: that would pull in GPUI's own `test`
    // attribute macro, which shadows the built-in one and recurses.
    use super::{Setup, wizard_seed};

    #[test]
    fn a_two_input_monitor_opens_on_naming_not_on_picking() {
        // The monitor already reports which ports are in play, so there is
        // nothing to tick. Opening on the picking step instead would strand
        // the user away from the import shortcut, which lives in this flow.
        let seed = wizard_seed(false, &[0x10, 0x12], Some(0x10)).expect("unconfigured");
        assert!(seed.naming, "goes straight to naming");
        assert_eq!(seed.chosen, vec![0x10, 0x12]);
        assert_eq!(seed.local, Some(0x10));
    }

    #[test]
    fn a_monitor_that_reports_more_ports_starts_by_asking() {
        // Four inputs is not four computers; the user has to say which ones
        // are real, and only the live one can be assumed to be this machine.
        let seed = wizard_seed(false, &[0x0F, 0x10, 0x11, 0x12], Some(0x10)).expect("unconfigured");
        assert!(!seed.naming, "starts on the picking step");
        assert_eq!(seed.chosen, vec![0x10], "only the current input is assumed");
        assert_eq!(seed.local, Some(0x10));

        // Nothing reported at all, and nothing readable: still has to ask.
        let seed = wizard_seed(false, &[], None).expect("unconfigured");
        assert!(!seed.naming);
        assert!(seed.chosen.is_empty());
    }

    #[test]
    fn a_configured_monitor_gets_no_wizard() {
        assert!(wizard_seed(true, &[0x10, 0x12], Some(0x10)).is_none());
        assert!(wizard_seed(true, &[], None).is_none());
    }

    #[test]
    fn a_single_reported_port_is_not_enough_to_skip_picking() {
        // One port cannot be a toggle, so this is not the two-computer case
        // and the name fields would have nothing to name.
        let seed = wizard_seed(false, &[0x10], Some(0x10)).expect("unconfigured");
        assert!(!seed.naming);
        let Setup { chosen, .. } = seed;
        assert_eq!(chosen, vec![0x10]);
    }
}
