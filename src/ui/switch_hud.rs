//! Quick-switch panel: the answer to a monitor shared by three or more
//! computers.
//!
//! A blind flip only works when there is a single other side to flip to. With
//! three computers, reaching the third by cycling would mean going *through*
//! the second: two DDC/CI writes, two screen blanks, and a detour through a
//! machine the user did not ask for. On a monitor that ignores commands on an
//! inactive input (see `docs/limitations.md`) the second hop would not even
//! arrive. So the hotkey asks instead of guessing, and jumps straight there.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::controller::Controller;

/// Long enough to read and choose, short enough that a stray hotkey does not
/// leave a panel parked on screen.
pub const TIMEOUT: Duration = Duration::from_secs(6);

pub struct SwitchHud {
    controller: Entity<Controller>,
    focus: FocusHandle,
    _observe: Subscription,
}

impl SwitchHud {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let controller = Controller::global(cx);
        let observe = cx.observe(&controller, |_, _, cx| cx.notify());
        let focus = cx.focus_handle();
        // Esc and the number keys only reach us if we hold focus. The user
        // asked for this panel by pressing the hotkey, so taking focus for its
        // six-second life is what they expect.
        window.focus(&focus, cx);
        Self {
            controller,
            focus,
            _observe: observe,
        }
    }
}

/// One computer sharing one monitor, flattened for the panel.
struct Entry {
    /// Monitor id, for the switch itself.
    id: String,
    /// Monitor name, for the header when more than one is involved.
    monitor: String,
    port: u8,
    name: String,
    current: bool,
}

fn panel_entries(controller: &Controller) -> Vec<Entry> {
    controller
        .monitors
        .iter()
        .flat_map(|m| {
            let ports = controller.endpoints(m);
            let id = m.id().to_string();
            let monitor = m.dev.name.clone();
            let current = m.current_input;
            ports.into_iter().map(move |port| Entry {
                id: id.clone(),
                monitor: monitor.clone(),
                port,
                name: controller.input_label(&id, port),
                current: current == Some(port),
            })
        })
        .collect()
}

impl Render for SwitchHud {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entries = panel_entries(self.controller.read(cx));
        let multi = {
            let mut ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
            ids.dedup();
            ids.len() > 1
        };

        let rows = entries.iter().enumerate().map(|(ix, entry)| {
            let id = entry.id.clone();
            let port = entry.port;
            h_flex()
                .id(SharedString::from(format!("hud-{ix}")))
                .gap_3()
                .items_center()
                .px_2()
                .py_1()
                .rounded_md()
                .when(entry.current, |el| el.bg(theme.muted))
                .hover(|s| s.bg(theme.muted))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.controller.update(cx, |c, cx| {
                        c.close_switch_hud(cx);
                        c.switch_input(&id, port, cx);
                    });
                }))
                .child(
                    div()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{}", ix + 1)),
                )
                .child(div().text_sm().child(entry.name.clone()))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if entry.current {
                            "正在显示".to_string()
                        } else {
                            crate::display::mccs::input_source_name(entry.port)
                        }),
                )
                .when(multi, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(entry.monitor.clone()),
                    )
                })
        });

        v_flex().size_full().flex().items_center().justify_center().child(
            v_flex()
                .id("switch-hud")
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    let key = event.keystroke.key.as_str();
                    if key == "escape" {
                        this.controller.update(cx, |c, cx| c.close_switch_hud(cx));
                        return;
                    }
                    let Ok(index) = key.parse::<usize>() else {
                        return;
                    };
                    if index == 0 {
                        return;
                    }
                    // Same order as the rendered rows, so the printed number
                    // is always the key that reaches it.
                    let target = panel_entries(this.controller.read(cx))
                        .into_iter()
                        .nth(index - 1)
                        .map(|e| (e.id, e.port));
                    if let Some((id, port)) = target {
                        this.controller.update(cx, |c, cx| {
                            c.close_switch_hud(cx);
                            c.switch_input(&id, port, cx);
                        });
                    }
                }))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.controller.update(cx, |c, cx| c.close_switch_hud(cx));
                }))
                .w(px(300.))
                .p_3()
                .gap_1()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .pt_2()
                        .child(Icon::new(Lucide::ArrowLeftRight).size(px(14.)))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("切换显示器输入"),
                        ),
                )
                .children(rows)
                .child(
                    h_flex()
                        .justify_between()
                        .pt_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("按数字键直达")
                        .child("Esc 取消"),
                ),
        )
    }
}
