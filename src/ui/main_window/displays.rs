//! Settings → Displays: which computers share each monitor, and what they are
//! called.
//!
//! A monitor that has not been set up gets a two-step flow — tick the ports
//! that have a computer behind them, then name each one. After that it gets an
//! editor: rename, re-port, add or remove a computer. Either way the names can
//! be carried to the other computers through the clipboard, so they are only
//! ever typed once.

use std::collections::HashMap;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::MainWindow;
use crate::controller::{Controller, MonitorEntry};
use crate::display::mccs;
use crate::i18n::{tr, translate};
use crate::platform;
use crate::skin::{Control, Mark, Tone};
use crate::ui::kit::{self, skin};

/// The first-run flow for one monitor.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Setup {
    /// Ports ticked as having a computer behind them, in the order ticked.
    pub chosen: Vec<u8>,
    /// On the second step, naming them.
    pub naming: bool,
    /// Which of the chosen ports this computer is on.
    pub local: Option<u8>,
}

/// One computer's row: its number and colour, its name field, and its
/// controls. The controls are one group that drops to its own line, pushed
/// right, when the window is too narrow for the row.
fn computer_row(ix: usize, numbered: bool, name: Option<Input>, controls: Div, cx: &App) -> Div {
    h_flex()
        .w_full()
        .flex_wrap()
        .gap_2()
        .items_center()
        .child(
            h_flex()
                .flex_none()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .w(px(14.))
                        .when(numbered, |el| el.child(kit::hint(format!("{}", ix + 1), cx))),
                )
                .child(skin(cx).swatch(skin(cx).identity(ix, cx), cx)),
        )
        // One width for every row, so the names line up; it only gives way when
        // the window is too narrow for it.
        .child(div().w(px(220.)).flex_shrink_1().min_w(px(140.)).children(name))
        .child(controls.flex_none().ml_auto())
}

/// One-tap names offered while naming, so the common case needs no typing.
/// Saved in the language they are shown in, so translated where drawn.
const NAME_SUGGESTIONS: [&str; 4] = ["Desktop", "Work PC", "Laptop", "Console"];

/// Identity of one computer row, stable across repaints.
pub fn endpoint_key(monitor_id: &str, port: u8) -> String {
    format!("{monitor_id}@{port:#04X}")
}

/// Where the first-run flow opens for one monitor, or `None` once it is set up.
///
/// A monitor reporting exactly two inputs already says which ports are in play,
/// so it opens straight on naming. Anything else starts by asking: four inputs
/// is not four computers.
pub fn wizard_seed(configured: bool, known: &[u8], current: Option<u8>) -> Option<Setup> {
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

/// Starts the flow for monitors that need it, keeps it where the user left it,
/// and drops it for monitors that have been set up or have gone away.
pub fn sync_setup(setup: &mut HashMap<String, Setup>, c: &Controller) {
    setup.retain(|id, _| c.monitors.iter().any(|m| m.id() == id));
    for m in &c.monitors {
        match wizard_seed(c.endpoints_configured(m), &c.endpoints(m), m.current_input) {
            Some(seed) => {
                setup.entry(m.id().to_string()).or_insert(seed);
            }
            None => {
                setup.remove(m.id());
            }
        }
    }
}

impl MainWindow {
    pub(super) fn render_display_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        v_flex()
            .gap_5()
            .child(kit::hint(
                tr!("List the computers sharing each monitor and give them names; the names are what the tray menu and the quick-switch panel show. Marking which one you are sitting at only changes what is shown, never where a switch goes."),
                cx,
            ))
            .when(c.monitors.is_empty(), |el| {
                el.child(kit::empty_state(
                    Lucide::Monitor,
                    tr!("No monitors to set up"),
                    tr!("Monitors that support DDC/CI appear here once they are detected."),
                    None,
                    cx,
                ))
            })
            .children(
                c.monitors
                    .iter()
                    .enumerate()
                    .map(|(idx, m)| self.render_monitor_setup(idx, m, c, cx)),
            )
            .child(self.render_transfer(cx))
            .into_any_element()
    }

    fn render_monitor_setup(&self, idx: usize, m: &MonitorEntry, c: &Controller, cx: &Context<Self>) -> AnyElement {
        let inputs = c.inputs_for(m);
        let endpoints = c.endpoints(m);
        let (mode, mark) = match (self.setup.contains_key(m.id()), endpoints.len()) {
            (true, _) | (_, 0) => (tr!("Not set up"), Mark::Fair),
            (_, 2) => (tr!("One-key flip"), Mark::Good),
            _ => (tr!("Quick-switch panel"), Mark::Good),
        };

        let body = match self.setup.get(m.id()) {
            Some(setup) if setup.naming => self.render_naming(idx, m, setup, cx),
            Some(setup) => self.render_picking(idx, m, setup, &inputs, cx),
            None if endpoints.is_empty() => kit::hint(
                tr!("This monitor does not report its inputs. Add its ports under extra_inputs in the config file."),
                cx,
            )
            .into_any_element(),
            None => self.render_editor(idx, m, c, &endpoints, &inputs, cx),
        };

        let footer = (endpoints.len() >= 3).then(|| {
            let controller = self.controller.clone();
            h_flex().child(
                kit::button(
                    SharedString::from(format!("preview-{idx}")),
                    Tone::Ghost,
                    Control::Compact,
                    cx,
                )
                .label(tr!("Preview the quick-switch panel"))
                .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.open_switch_hud(cx))),
            )
        });

        skin(cx)
            .card(cx)
            .gap_3()
            .child(kit::card_header(
                kit::title(m.dev.name.clone(), cx).into_any_element(),
                Some(skin(cx).chip(mode, mark, cx)),
            ))
            .child(body)
            .children(footer)
            .into_any_element()
    }

    /// Step one: which ports have a computer behind them.
    fn render_picking(
        &self,
        idx: usize,
        m: &MonitorEntry,
        setup: &Setup,
        inputs: &[u8],
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = m.id().to_string();
        let ports = inputs.iter().map(|&port| {
            let id = id.clone();
            let on = setup.chosen.contains(&port);
            h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    kit::button(
                        SharedString::from(format!("pick-{idx}-{port}")),
                        if on { Tone::Primary } else { Tone::Default },
                        Control::Regular,
                        cx,
                    )
                    .when(on, |b| b.icon(Icon::new(IconName::Check)))
                    .label(mccs::input_source_name(port))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let entry = this.setup.entry(id.clone()).or_default();
                        match entry.chosen.iter().position(|p| *p == port) {
                            Some(ix) => {
                                entry.chosen.remove(ix);
                            }
                            None => entry.chosen.push(port),
                        }
                        this.setup_changed(window, cx);
                    })),
                )
                .when(m.current_input == Some(port), |el| {
                    el.child(skin(cx).chip(tr!("This PC"), Mark::Accent, cx))
                })
        });

        let count = setup.chosen.len();
        let next = {
            let id = id.clone();
            kit::button_if(
                SharedString::from(format!("wizard-next-{idx}")),
                Tone::Primary,
                Control::Prominent,
                count >= 2,
                cx,
            )
            .label(if count >= 2 {
                tr!(n = count, "Name this computer" | "Name these {n} computers")
            } else {
                tr!("Pick at least two").to_string()
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(entry) = this.setup.get_mut(&id) {
                    entry.naming = true;
                }
                cx.notify();
            }))
        };

        skin(cx)
            .inset(cx)
            .gap_3()
            .child(kit::label(tr!("Which ports have a computer behind them?"), cx))
            .child(kit::hint(
                tr!("Leave out ports that are empty or go to a game console or TV box."),
                cx,
            ))
            .child(h_flex().gap_2().flex_wrap().children(ports))
            .child(kit::hint(
                match m.current_input {
                    Some(port) => tr!(
                        "{port} looks like this computer, because the monitor is showing it now.",
                        port = mccs::input_source_name(port)
                    ),
                    None => tr!("The monitor does not say which input it is showing.").to_string(),
                },
                cx,
            ))
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .items_center()
                    .child(next)
                    .child(self.paste_button(SharedString::from(format!("wizard-paste-{idx}")), Tone::Ghost, cx)),
            )
            .into_any_element()
    }

    /// Step two: a name for each ticked port.
    fn render_naming(&self, idx: usize, m: &MonitorEntry, setup: &Setup, cx: &Context<Self>) -> AnyElement {
        let id = m.id().to_string();
        let rows = setup.chosen.iter().enumerate().map(|(ix, &port)| {
            let state = self.controls.name(&id, port);
            let suggestions = NAME_SUGGESTIONS.iter().map(|name| {
                let name = SharedString::from(translate(name));
                let picked = state.is_some_and(|s| s.read(cx).value() == name);
                let (id, state) = (id.clone(), state.cloned());
                kit::button(
                    SharedString::from(format!("suggest-{idx}-{ix}-{name}")),
                    if picked { Tone::Primary } else { Tone::Ghost },
                    Control::Compact,
                    cx,
                )
                .label(name.clone())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(state) = &state {
                        state.update(cx, |s, cx| s.set_value(name.clone(), window, cx));
                    }
                    this.controller
                        .update(cx, |c, cx| c.set_input_name(&id, port, name.to_string(), cx));
                }))
            });
            let mark_local = {
                let id = id.clone();
                cx.listener(
                    move |this: &mut Self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>| {
                        if let Some(entry) = this.setup.get_mut(&id) {
                            entry.local = Some(port);
                        }
                        this.setup_changed(window, cx);
                    },
                )
            };
            v_flex()
                .gap_1p5()
                .child(computer_row(
                    ix,
                    true,
                    state.map(Input::new),
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(kit::hint(mccs::input_source_name(port), cx))
                        .child(self.local_mark(
                            SharedString::from(format!("wizard-local-{idx}-{ix}")),
                            setup.local == Some(port),
                            mark_local,
                            cx,
                        )),
                    cx,
                ))
                .child(h_flex().gap_1().flex_wrap().pl(px(42.)).children(suggestions))
        });
        let rows: Vec<_> = rows.collect();

        let back = {
            let id = id.clone();
            kit::button(
                SharedString::from(format!("wizard-back-{idx}")),
                Tone::Default,
                Control::Prominent,
                cx,
            )
            .label(tr!("Back"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(entry) = this.setup.get_mut(&id) {
                    entry.naming = false;
                }
                cx.notify();
            }))
        };
        let done = kit::button(
            SharedString::from(format!("wizard-done-{idx}")),
            Tone::Primary,
            Control::Prominent,
            cx,
        )
        .label(tr!("Done"))
        .on_click(cx.listener(move |this, _, _, cx| this.finish_setup(&id, cx)));

        skin(cx)
            .inset(cx)
            .gap_3()
            .child(kit::label(tr!("Give each computer a name"), cx))
            .child(kit::hint(
                tr!("This computer is named after the system ({hostname}). For the others, type a name or pick a common one.", hostname = platform::local_hostname()),
                cx,
            ))
            .children(rows)
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .items_center()
                    .child(done)
                    .child(back)
                    .child(self.paste_button(SharedString::from(format!("wizard-paste2-{idx}")), Tone::Ghost, cx)),
            )
            .into_any_element()
    }

    /// The flow lives in the window, not the controller, so a change to it has
    /// to bring the name fields along by hand: a port ticked a moment ago needs
    /// a field before the naming step can show it.
    fn setup_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync(window, cx);
        cx.notify();
    }

    /// Commits the flow: the chosen ports become the monitor's computers, with
    /// the names typed so far.
    fn finish_setup(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(setup) = self.setup.get(id).cloned() else {
            return;
        };
        let local = setup
            .local
            .filter(|p| setup.chosen.contains(p))
            .or_else(|| setup.chosen.first().copied());
        let names: Vec<(u8, String)> = setup
            .chosen
            .iter()
            .map(|&port| {
                let name = self
                    .controls
                    .name(id, port)
                    .map(|s| s.read(cx).value().to_string())
                    .unwrap_or_default();
                (port, name)
            })
            .collect();
        self.setup.remove(id);
        self.controller.update(cx, |c, cx| {
            for (port, name) in names {
                c.set_input_name(id, port, name, cx);
            }
            c.set_endpoints(id, setup.chosen, local, cx);
        });
    }

    /// The editable list of a monitor's computers, once it is set up.
    fn render_editor(
        &self,
        idx: usize,
        m: &MonitorEntry,
        c: &Controller,
        endpoints: &[u8],
        inputs: &[u8],
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = m.id().to_string();
        let local = c.local_input(m);
        let n = endpoints.len();

        let rows = endpoints.iter().enumerate().map(|(ix, &port)| {
            let key = endpoint_key(&id, port);
            let picker_open = self.port_picker.as_deref() == Some(key.as_str());
            let mark_local = {
                let id = id.clone();
                cx.listener(
                    move |this: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                        this.controller.update(cx, |c, cx| c.set_local_input(&id, port, cx));
                    },
                )
            };

            let choose_port = picker_open.then(|| {
                let free = inputs.iter().copied().filter(|p| *p == port || !endpoints.contains(p));
                h_flex().gap_1().flex_wrap().pl(px(42.)).children(free.map(|p| {
                    let id = id.clone();
                    kit::button(
                        SharedString::from(format!("set-port-{idx}-{ix}-{p}")),
                        if p == port { Tone::Primary } else { Tone::Default },
                        Control::Compact,
                        cx,
                    )
                    .label(mccs::input_source_name(p))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.port_picker = None;
                        this.controller
                            .update(cx, |c, cx| c.set_endpoint_port(&id, port, p, cx));
                    }))
                }))
            });

            v_flex()
                .gap_1p5()
                .child(computer_row(
                    ix,
                    n >= 3,
                    self.controls.name(&id, port).map(Input::new),
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(self.local_mark(
                            SharedString::from(format!("local-{idx}-{ix}")),
                            local == Some(port),
                            mark_local,
                            cx,
                        ))
                        .child({
                            let key = key.clone();
                            kit::button(
                                SharedString::from(format!("port-{idx}-{ix}")),
                                Tone::Default,
                                Control::Compact,
                                cx,
                            )
                            .label(mccs::input_source_name(port))
                            .icon(Icon::new(if picker_open {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.port_picker =
                                    (this.port_picker.as_deref() != Some(key.as_str())).then(|| key.clone());
                                cx.notify();
                            }))
                        })
                        .when(n > 2, |el| {
                            let id = id.clone();
                            el.child(
                                kit::button(
                                    SharedString::from(format!("remove-{idx}-{ix}")),
                                    Tone::Ghost,
                                    Control::Compact,
                                    cx,
                                )
                                .icon(IconName::Close)
                                .tooltip(tr!("Remove this computer"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.port_picker = None;
                                    this.controller.update(cx, |c, cx| c.remove_endpoint(&id, port, cx));
                                })),
                            )
                        }),
                    cx,
                ))
                .children(choose_port)
        });
        let rows: Vec<_> = rows.collect();

        let free = inputs.iter().copied().find(|p| !endpoints.contains(p));
        let add = kit::button_if(
            SharedString::from(format!("add-{idx}")),
            Tone::Ghost,
            Control::Compact,
            free.is_some(),
            cx,
        )
        .icon(IconName::Plus)
        .label(tr!("Add a computer"))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(port) = free {
                this.controller.update(cx, |c, cx| c.add_endpoint(&id, port, cx));
            }
        }));

        skin(cx)
            .inset(cx)
            .gap_2()
            .children(rows)
            .child(h_flex().child(add))
            .when(!m.inputs_reported(), |el| {
                el.child(kit::hint(
                    tr!("The monitor does not report its inputs, so these are the common ports. Add others under extra_inputs in the config file."),
                    cx,
                ))
            })
            .into_any_element()
    }

    /// "This PC" on the computer you are at, and a way to say so on the others.
    fn local_mark(
        &self,
        id: SharedString,
        here: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        if here {
            skin(cx).chip(tr!("This PC"), Mark::Accent, cx)
        } else {
            kit::button(id, Tone::Ghost, Control::Compact, cx)
                .label(tr!("This is me"))
                .on_click(on_click)
                .into_any_element()
        }
    }

    fn paste_button(&self, id: SharedString, tone: Tone, cx: &App) -> impl IntoElement + use<> {
        let controller = self.controller.clone();
        kit::button(id, tone, Control::Regular, cx)
            .icon(Icon::new(Lucide::ClipboardPaste))
            .label(tr!("Paste from another computer"))
            .on_click(move |_, _, cx| {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                controller.update(cx, |c, cx| {
                    c.import_switching(&text, cx);
                });
            })
    }

    /// Carrying the names to another computer, which then needs no typing.
    fn render_transfer(&self, cx: &App) -> AnyElement {
        let export = self.controller.clone();
        skin(cx)
            .card(cx)
            .gap_3()
            .child(skin(cx).eyebrow(tr!("Other computers"), cx))
            .child(kit::hint(
                tr!("Set the names up once, copy them here, and paste them on each of the other computers."),
                cx,
            ))
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        kit::button("export-switching", Tone::Default, Control::Regular, cx)
                            .icon(Icon::new(Lucide::Copy))
                            .label(tr!("Copy these settings"))
                            .on_click(move |_, _, cx| {
                                let text = export.read(cx).export_switching();
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                export.update(cx, |c, cx| {
                                    c.show_notice(tr!("Copied. Paste them on the other computer."), cx)
                                });
                            }),
                    )
                    .child(self.paste_button("import-switching".into(), Tone::Default, cx)),
            )
            .into_any_element()
    }
}
