//! The evening cutoff on screen: a card in the corner before it, an overlay
//! over every display once it is here, and on the main display a small box for
//! carrying on.
//!
//! None of them offers to shut down, sleep or hibernate. What to do with the
//! computer is the user's to decide, and they know where their power button is.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::controller::Controller;
use crate::evening::{self, Status};
use crate::i18n::tr;
use crate::platform;
use crate::skin::{Control, Tone};
use crate::ui::kit;

pub const FADE_IN: Duration = Duration::from_secs(4);
/// Darker than a break's: this one is not asking for five minutes.
const MAX_DIM: f32 = 0.86;

/// The heads-up card's size, inside which the card itself is inset so a
/// skin's hard shadow has room.
pub const HEADS_UP_SIZE: Size<Pixels> = Size {
    width: px(380.),
    height: px(124.),
};
/// The carry-on box's size, the same way.
pub const KEEP_USING_SIZE: Size<Pixels> = Size {
    width: px(460.),
    height: px(168.),
};

fn clock(at: chrono::NaiveDateTime) -> String {
    at.format("%H:%M").to_string()
}

/// Fades a window in from when it was opened: 0..=1, and whether it still is.
fn fade_in(opened: Instant) -> (f32, bool) {
    let t = (opened.elapsed().as_secs_f32() / FADE_IN.as_secs_f32()).min(1.0);
    (t * t * (3.0 - 2.0 * t), t < 1.0)
}

/// The card in the corner: the cutoff is coming, with how long is left.
pub struct HeadsUpCard {
    controller: Entity<Controller>,
    _observe: Subscription,
}

/// The window's own handle, for the Win32 calls GPUI has no word for.
fn hwnd(window: &Window) -> Option<isize> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

impl HeadsUpCard {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Some(hwnd) = hwnd(window) {
            platform::remove_frame(hwnd);
        }
        let controller = Controller::global(cx);
        let observe = cx.observe(&controller, |_, _, cx| cx.notify());
        Self {
            controller,
            _observe: observe,
        }
    }
}

impl Render for HeadsUpCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let skin = kit::skin(cx);
        let c = self.controller.read(cx);
        let Status::HeadsUp { cutoff } = c.evening else {
            return div().into_any_element();
        };
        let left = evening::minutes_until(c.evening_now(), cutoff);
        let controller = self.controller.clone();
        div()
            .size_full()
            .p_3()
            .font(skin.font(cx))
            .text_color(cx.theme().foreground)
            .child(
                skin.card(cx)
                    .id("heads-up")
                    .test_support()
                    .size_full()
                    .gap_1()
                    .child(kit::card_header(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Icon::new(Lucide::Sunset))
                            .child(kit::title(tr!("Evening cutoff at {time}", time = clock(cutoff)), cx))
                            .into_any_element(),
                        Some(
                            kit::button("close-heads-up", Tone::Ghost, Control::Compact, cx)
                                .icon(IconName::Close)
                                .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.close_heads_up(cx)))
                                .into_any_element(),
                        ),
                    ))
                    .child(kit::body(
                        tr!(
                            n = left,
                            "1 minute left. Time to start wrapping up."
                                | "{n} minutes left. Time to start wrapping up."
                        ),
                        cx,
                    )),
            )
            .into_any_element()
    }
}

/// The layer over one display once the cutoff is here. Like a break's, it
/// never takes focus and clicks fall through it: it says what time it is, and
/// leaves the rest to the user.
pub struct CutoffOverlay {
    controller: Entity<Controller>,
    opened: Instant,
    _observe: Subscription,
}

impl CutoffOverlay {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Some(hwnd) = hwnd(window) {
            platform::make_click_through(hwnd);
        }
        let controller = Controller::global(cx);
        let observe = cx.observe(&controller, |_, _, cx| cx.notify());
        Self {
            controller,
            opened: Instant::now(),
            _observe: observe,
        }
    }
}

impl Render for CutoffOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (visibility, animating) = fade_in(self.opened);
        if animating {
            window.request_animation_frame();
        }
        let c = self.controller.read(cx);
        let now = c.evening_now();
        let cutoff = match c.evening {
            Status::Cutoff { cutoff } => cutoff,
            _ => now,
        };
        let said = c.evening_said();

        let fg = hsla(0., 0., 1., visibility);
        let dim = hsla(0., 0., 1., 0.7 * visibility);
        div()
            .size_full()
            .font(kit::skin(cx).font(cx))
            .bg(hsla(230. / 360., 0.3, 0.05, MAX_DIM * visibility))
            .text_color(fg)
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .items_center()
                    .gap_4()
                    .max_w(px(720.))
                    .child(Icon::new(Lucide::Moon).size(px(48.)).text_color(fg))
                    .child(
                        div()
                            .id("cutoff-clock")
                            .test_support()
                            .text_size(px(88.))
                            .font_weight(FontWeight::LIGHT)
                            .child(clock(now)),
                    )
                    .child(div().text_size(px(20.)).child(tr!(
                        "It is past {time}, the time you chose to stop using the computer.",
                        time = clock(cutoff)
                    )))
                    .children(said.map(|(at, reason)| {
                        div().text_color(dim).text_center().child(tr!(
                            "At {time} you said: “{reason}”",
                            time = clock(at),
                            reason = reason
                        ))
                    })),
            )
    }
}

/// The one thing on the overlay that takes a click: carrying on, by saying
/// what for. A window of its own, because the overlay lets every click through.
pub struct KeepUsing {
    controller: Entity<Controller>,
    input: Entity<InputState>,
    open: bool,
    opened: Instant,
    _subscriptions: [Subscription; 2],
}

impl KeepUsing {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Some(hwnd) = hwnd(window) {
            platform::remove_frame(hwnd);
        }
        let controller = Controller::global(cx);
        let observe = cx.observe(&controller, |_, _, cx| cx.notify());
        let input = cx.new(|cx| InputState::new(window, cx));
        let submit = cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::PressEnter { .. }) {
                return;
            }
            let reason = input.read(cx).value().to_string();
            this.controller.update(cx, |c, cx| c.keep_using(&reason, cx));
        });
        Self {
            controller,
            input,
            open: false,
            opened: Instant::now(),
            _subscriptions: [observe, submit],
        }
    }

    fn expand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        // Typing needs the keyboard, and the user just asked for it by
        // clicking; until then the overlay took nothing from them.
        window.activate_window();
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn collapse(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }
}

impl Render for KeepUsing {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (visibility, animating) = fade_in(self.opened);
        if animating {
            window.request_animation_frame();
        }
        let skin = kit::skin(cx);
        let content = if self.open {
            skin.card(cx)
                .id("keep-using-form")
                .w_full()
                .gap_2()
                .child(kit::label(tr!("What do you still need to do?"), cx))
                .child(
                    div()
                        .id("keep-using-reason")
                        .test_support()
                        .child(Input::new(&self.input)),
                )
                .child(kit::hint(tr!("Enter to carry on for 15 minutes · Esc to cancel"), cx))
                .into_any_element()
        } else {
            kit::button("keep-using", Tone::Default, Control::Prominent, cx)
                .label(tr!("Keep using"))
                .on_click(cx.listener(|this, _, window, cx| this.expand(window, cx)))
                .into_any_element()
        };
        div()
            .id("keep-using-root")
            .size_full()
            .p_3()
            .flex()
            .items_center()
            .justify_center()
            .font(skin.font(cx))
            .text_color(cx.theme().foreground)
            .opacity(visibility)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.collapse(cx);
                    cx.stop_propagation();
                }
            }))
            .child(content)
    }
}

/// Where the heads-up card goes on a display: the bottom right of the area
/// windows can use, clear of the taskbar.
pub fn heads_up_bounds(area: Bounds<Pixels>) -> Bounds<Pixels> {
    let margin = px(12.);
    Bounds {
        origin: point(
            area.right() - HEADS_UP_SIZE.width - margin,
            area.bottom() - HEADS_UP_SIZE.height - margin,
        ),
        size: HEADS_UP_SIZE,
    }
}

/// Where the carry-on box goes: centred, below the overlay's words.
pub fn keep_using_bounds(area: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: point(
            area.left() + (area.size.width - KEEP_USING_SIZE.width) / 2.,
            area.top() + area.size.height * 0.68,
        ),
        size: KEEP_USING_SIZE,
    }
}
