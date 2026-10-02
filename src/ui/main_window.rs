//! Main window: monitors, breaks, statistics and settings tabs.

use std::collections::HashMap;

use chrono::{Days, Local, TimeZone};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
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
use crate::display::mccs::{VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::display::{self, Feature};
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
    _subscriptions: Vec<Subscription>,
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

    fn render_monitor(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let id = m.id().to_string();
        let inputs = c.inputs_for(m);
        let pair = c.toggle_pair(m);

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

        let input_buttons = h_flex().gap_2().flex_wrap().children(inputs.iter().map(|&code| {
            let controller = self.controller.clone();
            let id = id.clone();
            let active = m.current_input == Some(code);
            Button::new(SharedString::from(format!("in-{idx}-{code}")))
                .small()
                .when(active, |b| b.primary())
                .when(!active, |b| b.outline())
                .label(c.input_label(&id, code))
                .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.switch_input(&id, code, cx)))
        }));

        let pair_picker = |slot: usize| {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .w(px(18.))
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(["A", "B"][slot]),
                )
                .child(h_flex().gap_1().flex_wrap().children(inputs.iter().map(|&code| {
                    let controller = self.controller.clone();
                    let id = id.clone();
                    let selected = pair.is_some_and(|p| p[slot] == code);
                    Button::new(SharedString::from(format!("pair-{idx}-{slot}-{code}")))
                        .xsmall()
                        .when(selected, |b| b.primary())
                        .when(!selected, |b| b.ghost())
                        .label(c.input_label(&id, code))
                        .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.set_toggle_slot(&id, slot, code, cx)))
                })))
        };
        let toggle_hint = match pair {
            Some([a, b]) if a != b => format!(
                "按 {} 或点托盘菜单「切换显示器输入」，在 {} 和 {} 之间来回切换",
                c.config.hotkeys.toggle_input,
                c.input_label(&id, a),
                c.input_label(&id, b)
            ),
            _ => "选择两个输入（例如台式机和笔记本各自连接的接口），之后就能一键来回切换".to_string(),
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
            .child(section_label("输入源", cx))
            .child(input_buttons)
            .when(!m.inputs_reported(), |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("显示器没有上报输入列表，这里列出的是常见接口；可在配置文件 extra_inputs 里补充。"),
                )
            })
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .bg(theme.muted)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Icon::new(Lucide::ArrowLeftRight).size(px(14.)))
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("一键切换")),
                    )
                    .child(pair_picker(0))
                    .child(pair_picker(1))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(toggle_hint)),
            )
            .when(c.config.developer_mode, |el| el.child(render_diagnostics(m, cx)))
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
                v_flex()
                    .child("外观")
                    .child(
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
                    .child(
                        div()
                            .size(px(22.))
                            .rounded_md()
                            .bg(theme.primary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(Lucide::Monitor)
                                    .size(px(13.))
                                    .text_color(theme.primary_foreground),
                            ),
                    )
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
