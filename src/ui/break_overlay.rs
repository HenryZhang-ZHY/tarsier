//! Fadetop-style break reminder: a translucent layer that slowly fades over
//! every display. It never blocks input — clicks fall through to the windows
//! underneath — so snooze/skip live in the tray menu and the main window.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::*;
use gpui_kit::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::breaks::RESTING_IDLE;
use crate::controller::Controller;
use crate::platform;
use crate::ui::format_clock;

pub const FADE_IN: Duration = Duration::from_secs(4);
pub const FADE_OUT: Duration = Duration::from_millis(900);
/// Darkness of the layer once fully faded in; light enough to keep working.
const MAX_DIM: f32 = 0.72;

const TIPS: &[&str] = &[
    "看看 6 米外的地方，让眼睛的睫状肌放松一下",
    "站起来伸个懒腰，转转脖子和肩膀",
    "去倒杯水，顺便走动走动",
    "闭上眼睛，深呼吸几次",
    "眨眨眼，让眼睛重新湿润起来",
    "活动一下手腕和手指",
];

pub struct BreakOverlay {
    controller: Entity<Controller>,
    tip: &'static str,
    opened: Instant,
    closing: Option<Instant>,
    _observe: Subscription,
}

impl BreakOverlay {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(h) = handle.as_raw()
        {
            platform::make_click_through(h.hwnd.get());
        }
        let controller = Controller::global(cx);
        let observe = cx.observe(&controller, |_, _, cx| cx.notify());
        let seed = controller.read(cx).tracker.session_start().unsigned_abs() as usize;
        Self {
            controller,
            tip: TIPS[seed % TIPS.len()],
            opened: Instant::now(),
            closing: None,
            _observe: observe,
        }
    }

    /// Start fading out; the controller removes the window after `FADE_OUT`.
    pub fn fade_out(&mut self, cx: &mut Context<Self>) {
        self.closing.get_or_insert_with(Instant::now);
        cx.notify();
    }

    /// 0..=1 overall visibility, and whether a fade is still running.
    fn visibility(&self) -> (f32, bool) {
        let fade_in = (self.opened.elapsed().as_secs_f32() / FADE_IN.as_secs_f32()).min(1.0);
        let mut v = ease_in_out(fade_in);
        let mut animating = fade_in < 1.0;
        if let Some(closing) = self.closing {
            let t = (closing.elapsed().as_secs_f32() / FADE_OUT.as_secs_f32()).min(1.0);
            v *= 1.0 - ease_in_out(t);
            animating = t < 1.0;
        }
        (v, animating)
    }
}

impl Render for BreakOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (visibility, animating) = self.visibility();
        if animating {
            window.request_animation_frame();
        }

        let c = self.controller.read(cx);
        let total = c.tracker.settings().break_secs.max(1);
        let remaining = c.tracker.rest_remaining();
        let progress = (total - remaining) as f32 / total as f32 * 100.0;
        let resting = platform::idle_secs() >= RESTING_IDLE;
        let worked = c.tracker.session_active() / 60;

        let fg = hsla(0., 0., 1., visibility);
        let dim = hsla(0., 0., 1., 0.7 * visibility);
        let faint = hsla(0., 0., 1., 0.45 * visibility);

        div()
            .size_full()
            .bg(hsla(220. / 360., 0.35, 0.06, MAX_DIM * visibility))
            .text_color(fg)
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .items_center()
                    .gap_5()
                    .child(Icon::new(Lucide::Coffee).size(px(56.)).text_color(fg))
                    .child(
                        div()
                            .text_size(px(34.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("休息一下吧"),
                    )
                    .child(div().text_color(dim).child(if worked > 0 {
                        format!("你已经连续工作了 {worked} 分钟。{}", self.tip)
                    } else {
                        self.tip.to_string()
                    }))
                    .child(
                        div()
                            .text_size(px(72.))
                            .font_weight(FontWeight::LIGHT)
                            .child(format_clock(remaining)),
                    )
                    .child(
                        div()
                            .w(px(360.))
                            .opacity(visibility)
                            .child(
                                Progress::new("rest")
                                    .value(progress)
                                    .color(hsla(150. / 360., 0.6, 0.6, 1.)),
                            ),
                    )
                    .child(div().text_color(dim).child(if resting {
                        "放松中…离开键盘鼠标，倒计时会自动走完"
                    } else {
                        "检测到键鼠操作，倒计时已暂停"
                    }))
                    .child(
                        div()
                            .mt_6()
                            .text_sm()
                            .text_color(faint)
                            .child("遮罩不会挡住操作 · 需要推迟或跳过，请右键托盘里的 tarsier"),
                    ),
            )
    }
}
