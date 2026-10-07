//! The Monitors tab: one card per monitor with its brightness and contrast, and
//! the computers it can be switched to.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controls::FEATURES;
use super::{MainWindow, Section};
use crate::controller::{Controller, MonitorEntry};
use crate::display::mccs::VCP_BRIGHTNESS;
use crate::i18n::tr;
use crate::skin::{Control, Mark, Tone};
use crate::ui::kit::{self, skin};

impl MainWindow {
    pub(super) fn render_monitors(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);

        let summary = if c.scanning && c.monitors.is_empty() {
            tr!("Looking for monitors…").to_string()
        } else {
            match c.monitors.len() {
                0 => tr!("No monitors detected").to_string(),
                n => tr!(n = n, "1 monitor with DDC/CI" | "{n} monitors with DDC/CI"),
            }
        };
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(div().flex_1().min_w_0().child(kit::hint(summary, cx)))
            .when(c.config.developer_mode, |el| {
                let controller = self.controller.clone();
                el.child(
                    kit::button("copy-diagnostics", Tone::Ghost, Control::Regular, cx)
                        .icon(Icon::new(Lucide::Copy))
                        .label(tr!("Copy diagnostics"))
                        .on_click(move |_, _, cx| {
                            let report = controller.read(cx).diagnostics_report();
                            cx.write_to_clipboard(ClipboardItem::new_string(report));
                            controller.update(cx, |c, cx| {
                                c.show_notice(tr!("Diagnostics report copied to the clipboard"), cx)
                            });
                        }),
                )
            })
            .child(self.scan_button(c, cx));

        let mut page = v_flex().gap_5().child(header);
        if c.monitors.is_empty() && !c.scanning {
            page = page.child(kit::empty_state(
                Lucide::Monitor,
                tr!("No external monitors supporting DDC/CI were found"),
                tr!(
                    "Turn on DDC/CI in the monitor's on-screen menu, then scan again. Built-in laptop screens do not support DDC/CI."
                ),
                None,
                cx,
            ));
        }
        for (idx, m) in c.monitors.iter().enumerate() {
            page = page.child(self.render_monitor(idx, m, c, cx));
        }
        page.into_any_element()
    }

    fn scan_button(&self, c: &Controller, cx: &App) -> impl IntoElement + use<> {
        let controller = self.controller.clone();
        kit::button("rescan", Tone::Default, Control::Regular, cx)
            .icon(Icon::new(Lucide::RefreshCw))
            .label(if c.scanning {
                tr!("Scanning…")
            } else {
                tr!("Scan again")
            })
            .loading(c.scanning)
            .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.refresh_monitors(cx)))
    }

    fn render_monitor(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &Context<Self>) -> AnyElement {
        let id = m.id();
        let model = m
            .dev
            .caps
            .as_ref()
            .and_then(|caps| caps.model.clone())
            .filter(|model| *model != m.dev.name);

        let features = FEATURES.map(|code| {
            let (name, icon) = if code == VCP_BRIGHTNESS {
                (tr!("Brightness"), Lucide::Sun)
            } else {
                (tr!("Contrast"), Lucide::Contrast)
            };
            let row = h_flex()
                .gap_3()
                .items_center()
                .min_h(px(32.))
                .child(Icon::new(icon).text_color(cx.theme().muted_foreground))
                .child(
                    div()
                        .w(px(84.))
                        .flex_none()
                        .whitespace_nowrap()
                        .child(kit::label(name, cx)),
                );
            match (self.controls.slider(id, code), self.controls.feature_field(id, code)) {
                (Some(slider), Some(field)) => row
                    .child(div().flex_1().min_w_0().child(Slider::new(slider)))
                    .child(div().w(px(112.)).flex_none().child(field.input())),
                _ => row.child(kit::hint(tr!("Not supported by this monitor"), cx)),
            }
        });

        skin(cx)
            .card(cx)
            .id(SharedString::from(format!("monitor-{idx}")))
            .test_support()
            .gap_3()
            .child(kit::card_header(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(Lucide::Monitor))
                    .child(kit::title(m.dev.name.clone(), cx))
                    .children(model.map(|model| kit::hint(model, cx)))
                    .into_any_element(),
                None,
            ))
            .children(features)
            .child(self.render_switching(idx, m, c, cx))
            .when(c.config.developer_mode, |el| el.child(render_diagnostics(m, cx)))
            .into_any_element()
    }

    /// The computers this monitor can be put on, one button each.
    ///
    /// Each button means exactly "put the monitor there". Nothing here is
    /// derived from the monitor's current input: the other computer can change
    /// it at any moment, and a button that names its destination never needs to
    /// know where you are.
    fn render_switching(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &Context<Self>) -> Div {
        let endpoints = c.endpoints(m);
        let hotkey = &c.config.hotkeys.toggle_input;
        let mode = match endpoints.len() {
            0 => None,
            2 => Some(tr!("One-key flip")),
            _ => Some(tr!("Quick-switch panel")),
        };
        let header = kit::card_header(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(Lucide::ArrowLeftRight).size(px(14.)))
                .child(kit::label(tr!("Input switching"), cx))
                .into_any_element(),
            mode.map(|mode| skin(cx).chip(mode, Mark::Plain, cx)).into_iter().chain(
                (!endpoints.is_empty() && !hotkey.is_empty()).then(|| kit::key_caps(hotkey, cx).into_any_element()),
            ),
        );

        let panel = skin(cx).inset(cx).gap_2().child(header);
        if endpoints.is_empty() {
            return panel
                .child(kit::hint(tr!("No computers sharing this monitor are set up yet."), cx))
                .child(
                    h_flex().child(
                        kit::button(
                            SharedString::from(format!("setup-{idx}")),
                            Tone::Primary,
                            Control::Regular,
                            cx,
                        )
                        .label(tr!("Set up switching"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_section(Section::Displays, cx))),
                    ),
                );
        }

        let local = c.local_input(m);
        let numbered = endpoints.len() >= 3;
        let rows = endpoints.iter().enumerate().map(|(ix, &port)| {
            let id = m.id().to_string();
            h_flex()
                .gap_2()
                .items_center()
                .min_h(px(32.))
                .when(numbered, |el| {
                    el.child(div().w(px(14.)).flex_none().child(kit::hint(format!("{}", ix + 1), cx)))
                })
                .child(skin(cx).swatch(skin(cx).identity(ix, cx), cx))
                .child(div().min_w_0().child(kit::label(c.input_label(m.id(), port), cx)))
                .when(local == Some(port), |el| {
                    el.child(skin(cx).chip(tr!("This PC"), Mark::Accent, cx))
                })
                .child(div().flex_1())
                .child(
                    kit::button(
                        SharedString::from(format!("switch-{idx}-{ix}")),
                        Tone::Default,
                        Control::Compact,
                        cx,
                    )
                    .label(tr!("Switch"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.controller.update(cx, |c, cx| c.switch_input(&id, port, cx));
                    })),
                )
        });

        let how = if endpoints.len() == 2 {
            tr!("The hotkey flips between the two without asking which one you are on.")
        } else {
            tr!("The hotkey opens a quick-switch panel; press a number to jump straight there.")
        };
        panel
            .children(rows)
            .child(kit::hint(how, cx))
            // A monitor that reports exactly two inputs works with no setup, but
            // its rows are then named after ports rather than computers.
            .when(!c.endpoints_configured(m), |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(kit::hint(tr!("These are port names, not computer names."), cx))
                        .child(
                            kit::button(
                                SharedString::from(format!("name-{idx}")),
                                Tone::Ghost,
                                Control::Compact,
                                cx,
                            )
                            .label(tr!("Name them"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_section(Section::Displays, cx))),
                        ),
                )
            })
    }
}

/// Developer mode: how this monitor is driven, and what was last sent to it.
fn render_diagnostics(m: &MonitorEntry, cx: &App) -> Div {
    /// Commands shown in the panel; the copied report has the full trace.
    const RECENT: usize = 12;
    let theme = cx.theme();
    let mono = |text: String| div().font_family(theme.mono_font_family.clone()).text_xs().child(text);
    let rows = m.dev.diagnostics.rows().into_iter().map(|(name, value)| {
        h_flex()
            .gap_3()
            .items_start()
            .child(div().w(px(64.)).flex_none().child(kit::hint(name, cx)))
            .child(mono(value).flex_1().min_w_0())
    });
    let trace = m.dev.trace();
    let lines = trace.iter().rev().take(RECENT).rev().map(|entry| {
        mono(crate::display::diagnostics::trace_line(entry))
            .when(entry.result.is_err(), |el| el.text_color(skin(cx).ink(Mark::Poor, cx)))
    });
    skin(cx)
        .inset(cx)
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(Lucide::Bug).size(px(14.)))
                .child(kit::label(tr!("Diagnostics"), cx)),
        )
        .child(mono(m.id().to_string()).text_color(theme.muted_foreground))
        .children(rows)
        .child(kit::label(tr!("Recent commands"), cx))
        .when(trace.is_empty(), |el| el.child(kit::hint(tr!("(none)"), cx)))
        .children(lines)
}
