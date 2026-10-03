//! Main window: monitors, breaks, statistics and settings tabs.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use chrono::{Days, Local, TimeZone};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
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
use crate::platform;
use crate::stats::{self, GOOD_SCORE};
use crate::ui::format_minutes;
use crate::ui::number_field::{NumberField, Range};

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
        "每工作",
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
        "休息",
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
        "推迟",
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
const NAME_SUGGESTIONS: [&str; 4] = ["台式机", "公司电脑", "笔记本", "游戏机"];

/// A monitor shared by several computers needs to tell them apart at a glance
/// in three places at once (list, tray, quick-switch panel), so each endpoint
/// keeps one hue wherever it appears.
fn endpoint_color(index: usize) -> Hsla {
    const HUES: [f32; 4] = [0.58, 0.09, 0.78, 0.45];
    hsla(HUES[index % HUES.len()], 0.72, 0.55, 1.0)
}

/// A small pill: "本机" on the computer you are sitting at, "正在显示" on the
/// one the monitor is showing right now.
fn mark(text: &str, fg: Hsla, bg: Hsla, border: Hsla) -> impl IntoElement {
    div()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_xs()
        .text_color(fg)
        .child(text.to_string())
}

/// A hotkey spec like `ctrl+alt+I` drawn as key caps.
fn hotkey_caps(spec: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
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
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label)
        }))
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
        // theme; re-resolve so "跟随系统" tracks it without a restart. An
        // explicit light/dark choice resolves to the same mode and costs a
        // repaint.
        let appearance = cx.observe_window_appearance(window, |this, window, cx| {
            let pref = this.controller.read(cx).config.theme;
            if pref == config::ThemePref::System {
                Theme::change(pref.resolve(window.appearance()), Some(window), cx);
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
            Button::new("refresh")
                .ghost()
                .small()
                .icon(Icon::new(Lucide::RefreshCw))
                .label(if c.scanning { "正在检测…" } else { "重新检测" })
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
                        0 => "未检测到显示器".to_string(),
                        n => format!("已连接 {n} 台支持 DDC/CI 的显示器"),
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(c.config.developer_mode, |el| {
                        let controller = self.controller.clone();
                        el.child(
                            Button::new("copy-diagnostics")
                                .ghost()
                                .small()
                                .icon(Icon::new(Lucide::Copy))
                                .label("复制诊断报告")
                                .on_click(move |_, _, cx| {
                                    let report = controller.read(cx).diagnostics_report();
                                    cx.write_to_clipboard(ClipboardItem::new_string(report));
                                    controller.update(cx, |c, cx| {
                                        c.notice = Some("诊断报告已复制到剪贴板".into());
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
                    .child("没有找到支持 DDC/CI 的外接显示器")
                    .child(div().text_sm().text_color(theme.muted_foreground).child(
                        "请在显示器的 OSD 菜单里开启 DDC/CI，然后点击「重新检测」。笔记本内置屏不支持 DDC/CI。",
                    )),
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
                None => row.child(div().text_sm().text_color(theme.muted_foreground).child("不支持")),
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
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(m.dev.name.clone()))
                    .children(model.map(|model| div().text_sm().text_color(theme.muted_foreground).child(model))),
            )
            .child(feature_row(VCP_BRIGHTNESS, "亮度", Lucide::Sun))
            .child(feature_row(VCP_CONTRAST, "对比度", Lucide::Contrast))
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
                    .font_weight(FontWeight::MEDIUM)
                    .child(if endpoints.len() == 2 {
                        "一键切换"
                    } else {
                        "输入切换"
                    }),
            )
            .child(div().flex_1())
            .child(hotkey_caps(&hotkey, cx));

        // Nothing to offer at all: this is the one case that needs setup before
        // it can do anything, so it is the one case that sends you to Settings.
        if endpoints.is_empty() {
            return v_flex()
                .gap_2()
                .p_3()
                .rounded_md()
                .bg(theme.muted)
                .child(header)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("还没设置共用这台显示器的电脑。"),
                )
                .child(
                    Button::new(SharedString::from(format!("configure-{idx}")))
                        .small()
                        .outline()
                        .label("去设置里配置")
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
                        .text_color(theme.muted_foreground)
                        .child(format!("{}", ix + 1))
                }))
                .child(
                    div()
                        .size(px(9.))
                        .rounded_full()
                        .bg(if here { theme.success } else { endpoint_color(ix) }),
                )
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(name.clone()))
                .when(here, |el| {
                    el.child(mark("本机", theme.muted_foreground, theme.background, theme.border))
                })
                .child(div().flex_1())
                .child(
                    Button::new(SharedString::from(format!("go-{idx}-{ix}")))
                        .small()
                        .outline()
                        .label(format!("切换到 {name}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.controller.update(cx, |c, cx| c.switch_input(&id, port, cx));
                        })),
                )
        });

        let hint = if n == 2 {
            "按快捷键在两台之间来回切，不用先看现在在哪台。"
        } else {
            "按快捷键呼出快切面板，按数字直达 —— 不会路过中间那台。"
        };

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(theme.muted)
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
                                .text_color(theme.muted_foreground)
                                .child("这些接口还没有名字。"),
                        )
                        .child(
                            Button::new(SharedString::from(format!("name-{idx}")))
                                .xsmall()
                                .ghost()
                                .label("去设置里起名")
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
                        .text_color(theme.muted_foreground)
                        .child("显示器没有上报输入列表，这里列出的是常见接口；可在配置文件 extra_inputs 里补充。"),
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
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(text))
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
                        Button::new(SharedString::from(format!("pick-{idx}-{port}")))
                            .small()
                            .when(on, |b| b.primary())
                            .when(!on, |b| b.outline())
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
                        el.child(div().text_xs().text_color(theme.muted_foreground).child("本机"))
                    })
            });
            let count = setup.chosen.len();
            return v_flex()
                .gap_2()
                .p_3()
                .rounded_md()
                .bg(theme.muted)
                .child(header(Lucide::ArrowLeftRight, "这台显示器上接着几台电脑？"))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("把真正接着电脑的口点亮。空着的口、接游戏机或电视盒子的口，都可以不选。"),
                )
                .child(h_flex().gap_2().flex_wrap().children(chips))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(match m.current_input {
                            // Only a starting point, read once when the monitor was
                            // enumerated — not a live claim about where it is now.
                            Some(port) => format!(
                                "默认把「{}」算作这台电脑，不对的话点一下换个口。",
                                mccs::input_source_name(port)
                            ),
                            None => "显示器没有上报输入，自己点一下哪个口是这台电脑就行。".to_string(),
                        }),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("wizard-next-{idx}")))
                                .primary()
                                .label(if count >= 2 {
                                    format!("下一步：给这 {count} 台起名")
                                } else {
                                    "至少选两台才能一键切换".to_string()
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
                            Button::new(SharedString::from(format!("wizard-paste-{idx}")))
                                .outline()
                                .label("从另一台电脑粘贴")
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
                let name = SharedString::from(*name);
                let label = name.clone();
                let picked = self
                    .names
                    .get(&key)
                    .map(|state| state.read(cx).value() == name)
                    .unwrap_or(false);
                Button::new(SharedString::from(format!("suggest-{idx}-{ix}-{label}")))
                    .xsmall()
                    .when(picked, |b| b.primary())
                    .when(!picked, |b| b.ghost())
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
                                    endpoint_color(ix)
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
                                .text_color(theme.muted_foreground)
                                .child(mccs::input_source_name(port)),
                        )
                        .when(here, |el| {
                            el.child(
                                div()
                                    .px_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("本机"),
                            )
                        }),
                )
                .child(h_flex().gap_1().pl_4().children(suggestions))
        });

        v_flex()
            .gap_3()
            .p_3()
            .rounded_md()
            .bg(theme.muted)
            .child(header(Lucide::Laptop, "给它们起个名字"))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("名字会出现在这里、托盘菜单和快切面板上 —— 这是唯一能让你一眼认出谁是谁的东西。"),
            )
            .children(rows)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "这台电脑的名字自动取系统里的「{}」，其余的点一下常用名就行。另一台电脑上也装一份 tarsier，把这套名字粘过去，第 3、第 4 台就都不用再填了。",
                        platform::local_hostname()
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!("wizard-back-{idx}")))
                            .outline()
                            .label("上一步")
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
                        Button::new(SharedString::from(format!("wizard-done-{idx}")))
                            .primary()
                            .label("完成")
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
                        Button::new(SharedString::from(format!("wizard-paste2-{idx}")))
                            .outline()
                            .label("从另一台电脑粘贴")
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
                    Button::new(SharedString::from(format!("setport-{idx}-{ix}-{p}")))
                        .xsmall()
                        .when(p == port, |b| b.primary())
                        .when(p != port, |b| b.ghost())
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
                                    endpoint_color(ix)
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.controller.update(cx, |c, cx| c.set_local_input(&id, port, cx));
                                    cx.notify();
                                }))
                        })
                        .child(div().w(px(158.)).children(self.names.get(&key).map(Input::new)))
                        .when(here, |el| {
                            el.child(mark("本机", theme.muted_foreground, theme.background, theme.border))
                        })
                        .child(div().flex_1())
                        .child({
                            let key = key.clone();
                            Button::new(SharedString::from(format!("port-{idx}-{ix}")))
                                .xsmall()
                                .outline()
                                .label(mccs::input_source_name(port))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.port_picker =
                                        (this.port_picker.as_deref() != Some(key.as_str())).then(|| key.clone());
                                    cx.notify();
                                }))
                        })
                        .children((n > 2).then(|| {
                            let id = id.clone();
                            Button::new(SharedString::from(format!("drop-{idx}-{ix}")))
                                .xsmall()
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.port_picker = None;
                                    this.controller.update(cx, |c, cx| c.remove_endpoint(&id, port, cx));
                                }))
                        })),
                )
                .children(strip)
        });

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(theme.muted)
            .children(rows)
            .child({
                let id = id.clone();
                let free = inputs.iter().copied().find(|p| !endpoints.contains(p));
                Button::new(SharedString::from(format!("add-{idx}")))
                    .xsmall()
                    .ghost()
                    .label("+ 添加电脑")
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
                        .text_color(theme.muted_foreground)
                        .child("显示器没有上报输入列表，这里列出的是常见接口；可在配置文件 extra_inputs 里补充。"),
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
                        .text_color(theme.muted_foreground)
                        .child("这台显示器没有上报输入列表，先在配置文件的 extra_inputs 里补上接口。")
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
                    .font_weight(FontWeight::MEDIUM)
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
            _ if !c.config.breaks.enabled => ("休息提醒已关闭".to_string(), "可以在「设置」里重新打开".to_string()),
            Phase::Away => ("你离开了一会儿 👋".to_string(), "回来后会开始新的一轮计时".to_string()),
            Phase::Prompted { .. } => (
                "正在休息".to_string(),
                format!("还需休息 {}", format_minutes(t.rest_remaining())),
            ),
            Phase::Working => (
                format!("已连续工作 {}", format_minutes(t.session_active())),
                if c.is_paused() {
                    "提醒已暂停".to_string()
                } else {
                    format!("{} 后提醒休息", format_minutes(t.until_prompt()))
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
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(title)),
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
                        Button::new("snooze")
                            .primary()
                            .label(format!("推迟 {} 分钟", c.config.breaks.snooze_minutes))
                            .on_click(move |_, _, cx| snooze.update(cx, |c, cx| c.snooze(cx))),
                    )
                    .child(
                        Button::new("skip")
                            .outline()
                            .label("跳过这次")
                            .on_click(move |_, _, cx| skip.update(cx, |c, cx| c.skip(cx))),
                    )
            } else {
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("break-now")
                            .primary()
                            .icon(Icon::new(Lucide::Coffee))
                            .label("现在休息")
                            .disabled(t.phase() != Phase::Working)
                            .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.break_now(cx))),
                    )
                    .child(
                        Button::new("pause")
                            .outline()
                            .label(if c.is_paused() {
                                "恢复提醒"
                            } else {
                                "暂停 1 小时"
                            })
                            .on_click(move |_, _, cx| controller2.update(cx, |c, cx| c.toggle_pause(cx))),
                    )
            });

        let score_card = card(cx)
            .gap_2()
            .child(section_label("今日健康分", cx))
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
                    .child(div().pb_2().text_lg().child(score.map_or("使用 15 分钟后开始评分", stats::grade))),
            )
            .child(
                h_flex()
                    .gap_4()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!("用眼 {}", format_minutes(day.active_secs() + t.session_active())))
                    .child(format!("休息 {} 次", day.breaks()))
                    .child(format!("最长连续 {}", format_minutes(day.longest_secs().max(t.session_active()))))
                    .child(format!("今日积分 +{}", day.points())),
            )
            .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                "评分规则：每段连续工作不超过 {} 分钟的 110% 记满分，超得越多扣得越多。离开电脑 {} 分钟会被自动记为一次休息。",
                c.config.breaks.work_minutes, c.config.breaks.break_minutes
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
            .child(stat_tile(Lucide::Flame, format!("{streak} 天"), "连续达标", cx))
            .child(stat_tile(Lucide::Trophy, format!("{points}"), "累计积分", cx))
            .child(stat_tile(Lucide::Activity, level.to_string(), "当前称号", cx));

        let level_progress = match next {
            Some(next) => (points - level_min) as f32 / (next - level_min) as f32 * 100.0,
            None => 100.0,
        };
        let level_card = card(cx)
            .gap_2()
            .child(h_flex().justify_between().text_sm().child(level).child(
                div().text_color(theme.muted_foreground).child(match next {
                    Some(n) => format!("再得 {} 分升级", n - points),
                    None => "已满级".to_string(),
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
                let color = match score {
                    Some(s) if s >= GOOD_SCORE => theme.success,
                    Some(_) => theme.warning,
                    None => theme.muted,
                };
                v_flex()
                    .flex_1()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(score.map_or(String::new(), |s| s.to_string())),
                    )
                    .child(div().w_full().h(px(height)).rounded_md().bg(color))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(if ago == 0 {
                        "今天".to_string()
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
                    .child("今天还没有完成的工作段"),
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
                        BreakKind::Natural => "自然休息",
                        BreakKind::Prompted => "提醒后休息",
                    }))
                    .child(
                        div()
                            .w(px(56.))
                            .text_right()
                            .text_color(score_color(Some(score), cx))
                            .child(format!("{score} 分")),
                    ),
            );
        }
        let skips = (day.skips + day.snoozes + day.ignored > 0).then(|| {
            div().text_xs().text_color(theme.muted_foreground).child(format!(
                "今天跳过 {} 次、推迟 {} 次、忽略 {} 次提醒",
                day.skips, day.snoozes, day.ignored
            ))
        });

        v_flex()
            .gap_4()
            .child(summary)
            .child(level_card)
            .child(card(cx).gap_2().child(section_label("最近 7 天健康分", cx)).child(bars))
            .child(
                card(cx)
                    .gap_1()
                    .child(section_label("今日工作段", cx))
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
                .child(label)
                .children(self.numbers.get(key).map(|f| {
                    div().w(px(148.)).child(
                        f.input()
                            .suffix(div().pr_1().text_sm().text_color(theme.muted_foreground).child("分钟")),
                    )
                }))
        };

        // Three small buttons rather than a dropdown: every option is visible at
        // a glance, and this is the same primary/outline pair idiom the monitor
        // input pickers use.
        let theme_picker = || {
            let controller = self.controller.clone();
            let current = c.config.theme;
            h_flex()
                .gap_1()
                .children(config::ThemePref::ALL.into_iter().map(|pref| {
                    let controller = controller.clone();
                    Button::new(SharedString::from(format!("theme-{pref:?}")))
                        .xsmall()
                        .when(pref == current, |b| b.primary())
                        .when(pref != current, |b| b.ghost())
                        .label(pref.label())
                        .on_click(move |_, _, cx| {
                            controller.update(cx, |c, cx| c.set_theme(pref, cx));
                        })
                }))
        };

        let theme_row = h_flex()
            .justify_between()
            .gap_4()
            .child(
                v_flex().child("外观").child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("「跟随系统」会随 Windows 的浅色 / 深色设置实时切换"),
                ),
            )
            .child(theme_picker());

        let general = card(cx)
            .gap_4()
            .child(section_label("通用", cx))
            .child(theme_row)
            .child(toggle_row(
                "autostart",
                "开机自动启动",
                "登录 Windows 后在托盘里静默运行",
                c.autostart,
                |c, v, cx| c.set_autostart(v, cx),
            ))
            .child(toggle_row(
                "developer-mode",
                "开发者模式",
                "在「显示器」页显示 DDC/CI 诊断信息和命令记录，并在日志里记录每条命令",
                c.config.developer_mode,
                |c, v, cx| c.set_developer_mode(v, cx),
            ));

        let breaks = card(cx)
            .gap_4()
            .child(section_label("休息提醒", cx))
            .child(toggle_row(
                "breaks-enabled",
                "启用休息提醒",
                "到点后全屏淡入提醒，离开电脑会自动记为休息",
                b.enabled,
                |c, v, cx| c.update_config(cx, |cfg| cfg.breaks.enabled = v),
            ))
            .child(toggle_row(
                "fullscreen",
                "全屏 / 演示时不打扰",
                "玩游戏、看视频或演示 PPT 时推迟提醒",
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
                    "未设置".to_string()
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
                .child(section_label("显示器输入", cx))
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    "给每台显示器列出共用它的电脑并起好名字 —— 名字是唯一能让你一眼认出谁是谁的东西。点左端圆点标记你正坐着的那台，只影响显示，不影响切换。",
                ))
                .when(c.monitors.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("还没有检测到支持 DDC/CI 的外接显示器。"),
                    )
                })
                .children(
                    c.monitors
                        .iter()
                        .enumerate()
                        .map(|(idx, m)| self.render_input_settings(idx, m, c, cx)),
                )
                .child(div().h(px(1.)).bg(theme.border))
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    "「接口 → 电脑名」跟着显示器走，所以在哪台电脑上填都一样。在一台上填好，把它搬到其余几台，就不用再填一遍。",
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("export-switching")
                                .small()
                                .outline()
                                .icon(Icon::new(Lucide::Copy))
                                .label("复制设置")
                                .on_click(move |_, _, cx| {
                                    let text = export.read(cx).export_switching();
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                    export.update(cx, |c, cx| {
                                        c.notice = Some("已复制。在另一台电脑上点「导入设置」。".into());
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Button::new("import-switching")
                                .small()
                                .outline()
                                .label("导入设置")
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
            .child(section_label("全局快捷键（修改配置文件后重启生效）", cx))
            .child(hk("切换显示器输入", &hotkeys.toggle_input))
            .child(hk("调高亮度", &hotkeys.brightness_up))
            .child(hk("调低亮度", &hotkeys.brightness_down))
            .child(hk("现在休息", &hotkeys.break_now))
            .children(
                c.hotkey_errors
                    .iter()
                    .map(|e| div().text_xs().text_color(theme.danger).child(format!("注册失败 {e}"))),
            )
            .child(
                h_flex().pt_2().child(
                    Button::new("open-config")
                        .small()
                        .outline()
                        .label("打开配置文件夹")
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
                    .text_color(theme.muted_foreground)
                    .child(format!("tarsier v{}", env!("CARGO_PKG_VERSION"))),
            )
            .into_any_element()
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_index = TABS.iter().position(|t| *t == self.tab).unwrap_or(0);
        let body = match self.tab {
            Tab::Monitors => self.render_monitors(cx),
            Tab::Breaks => self.render_breaks(cx),
            Tab::Stats => self.render_stats(cx),
            Tab::Settings => self.render_settings(cx),
        };
        let notice = self.controller.read(cx).notice.clone();
        let theme = cx.theme();
        let tabs = TabBar::new("tabs")
            .segmented()
            .small()
            .selected_index(tab_index)
            .child("显示器")
            .child("休息")
            .child("统计")
            .child("设置")
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.tab = TABS[*ix];
                cx.notify();
            }));

        // Self-drawn title bar: brand on the left, tabs in the drag area, native
        // min/max/close hit-testing (snap layouts keep working) on the right.
        let title_bar = TitleBar::new()
            .h(px(46.))
            .pl_4()
            .bg(theme.background)
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(img(brand_icon()).size(px(22.)))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("tarsier")),
            )
            .child(div().pr_2().child(tabs));

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(title_bar)
            .child(div().id("body").flex_1().overflow_y_scroll().px_4().py_4().child(body))
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
                        Button::new("dismiss")
                            .xsmall()
                            .ghost()
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
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(mono(value).flex_1().min_w_0())
    });
    let trace = m.dev.trace();
    let lines = trace.iter().rev().take(RECENT).rev().map(|entry| {
        let failed = entry.result.is_err();
        mono(display::diagnostics::trace_line(entry)).when(failed, |el| el.text_color(theme.danger))
    });
    v_flex()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(Lucide::Bug).size(px(14.)))
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("诊断信息")),
        )
        .child(mono(m.id().to_string()).text_color(theme.muted_foreground))
        .children(rows)
        .child(section_label("最近的命令", cx))
        .when(trace.is_empty(), |el| {
            el.child(div().text_xs().text_color(theme.muted_foreground).child("（暂无）"))
        })
        .children(lines)
}

fn card(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
}

fn section_label(text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text)
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
        .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(value))
}

fn score_color(score: Option<u8>, cx: &App) -> Hsla {
    let theme = cx.theme();
    match score {
        Some(s) if s >= GOOD_SCORE => theme.success,
        Some(s) if s >= 50 => theme.warning,
        Some(_) => theme.danger,
        None => theme.muted_foreground,
    }
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
