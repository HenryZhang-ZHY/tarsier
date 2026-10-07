//! The default skin: the component library's own look, light or dark.
//!
//! It names the theme's tokens and changes almost nothing about them, which is
//! what keeps an older config looking the way it was saved.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, Theme, ThemeMode, TitleBar, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Control, Mark, SkinStyle, Tone, Voice};

const RADIUS: Pixels = px(8.);

pub(super) struct Native;

impl SkinStyle for Native {
    fn note(&self) -> &'static str {
        crate::i18n::tr!("The system font, soft corners and quiet greys, in light or dark")
    }

    fn modes(&self) -> &'static [ThemeMode] {
        &[ThemeMode::Light, ThemeMode::Dark]
    }

    fn install(&self, cx: &mut App) {
        Theme::update(cx, |theme| {
            theme.radius = px(6.);
            theme.radius_lg = RADIUS;
            theme.shadow = true;
            theme.focus_ring = true;
        });
    }

    fn font(&self, cx: &App) -> Font {
        gpui_kit::font(cx.theme().font_family.clone())
    }

    fn canvas(&self, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex().bg(theme.background).text_color(theme.foreground)
    }

    fn title_bar(&self, bar: TitleBar, cx: &App) -> TitleBar {
        let theme = cx.theme();
        bar.bg(theme.background).border_color(theme.border)
    }

    fn card(&self, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .p_4()
            .rounded(RADIUS)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
    }

    fn inset(&self, cx: &App) -> Div {
        v_flex().p_3().rounded(px(6.)).bg(cx.theme().muted)
    }

    fn divider(&self, cx: &App) -> Div {
        div().w_full().h(px(1.)).bg(cx.theme().border)
    }

    fn eyebrow(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().foreground)
            .child(SharedString::from(text.to_owned()))
            .into_any_element()
    }

    fn chip(&self, text: &str, mark: Mark, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let (fg, bg) = match mark {
            Mark::Plain => (theme.muted_foreground, theme.muted),
            Mark::Accent => (theme.primary_foreground, theme.primary),
            other => (self.ink(other, cx), self.fill(other, cx).opacity(0.14)),
        };
        div()
            .flex_none()
            .px_2()
            .py(px(1.))
            .rounded(px(4.))
            .bg(bg)
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .whitespace_nowrap()
            .child(SharedString::from(text.to_owned()))
            .into_any_element()
    }

    fn key_cap(&self, text: &str, cx: &App) -> AnyElement {
        let theme = cx.theme();
        div()
            .flex_none()
            .min_w(px(22.))
            .px_1p5()
            .rounded(px(4.))
            .border_1()
            .border_b_2()
            .border_color(theme.border)
            .bg(theme.background)
            .text_xs()
            .font_family(theme.mono_font_family.clone())
            .text_color(theme.foreground)
            .flex()
            .justify_center()
            .child(SharedString::from(text.to_owned()))
            .into_any_element()
    }

    fn swatch(&self, color: Hsla, cx: &App) -> Div {
        let _ = cx;
        div().flex_none().size(px(10.)).rounded_full().bg(color)
    }

    fn weight(&self, voice: Voice) -> FontWeight {
        match voice {
            Voice::Loud => FontWeight::SEMIBOLD,
            Voice::Plain => FontWeight::MEDIUM,
            Voice::Quiet => FontWeight::NORMAL,
        }
    }

    fn identity(&self, index: usize, cx: &App) -> Hsla {
        const HUES: [f32; 4] = [0.58, 0.08, 0.78, 0.45];
        let lightness = if cx.theme().mode.is_dark() { 0.62 } else { 0.52 };
        hsla(HUES[index % HUES.len()], 0.7, lightness, 1.0)
    }

    fn fill(&self, mark: Mark, cx: &App) -> Hsla {
        let theme = cx.theme();
        match mark {
            Mark::Plain => theme.border,
            Mark::Accent => theme.primary,
            Mark::Good => theme.success,
            Mark::Fair => theme.warning,
            Mark::Poor => theme.danger,
        }
    }

    fn ink(&self, mark: Mark, cx: &App) -> Hsla {
        let theme = cx.theme();
        match mark {
            Mark::Plain => theme.muted_foreground,
            Mark::Accent => theme.foreground,
            other => self.fill(other, cx),
        }
    }

    fn bar(&self, mark: Mark, height: Pixels, cx: &App) -> Div {
        div().w_full().h(height).rounded_t(px(4.)).bg(self.fill(mark, cx))
    }

    fn meter(&self, fraction: f32, mark: Mark, cx: &App) -> Div {
        let fill = match mark {
            Mark::Plain | Mark::Accent => cx.theme().primary,
            other => self.fill(other, cx),
        };
        div()
            .w_full()
            .h(px(6.))
            .rounded_full()
            .overflow_hidden()
            .bg(cx.theme().muted)
            .child(
                div()
                    .h_full()
                    .rounded_full()
                    .w(relative(fraction.clamp(0., 1.)))
                    .bg(fill),
            )
    }

    fn button(&self, id: ElementId, tone: Tone, control: Control, cx: &App) -> Button {
        let _ = cx;
        let button = Button::new(id);
        let button = match tone {
            Tone::Primary => button.primary(),
            Tone::Default => button,
            Tone::Ghost => button.ghost(),
        };
        match control {
            Control::Compact => button.xsmall(),
            Control::Regular => button.small(),
            Control::Prominent => button,
        }
    }

    fn segmented(&self, cx: &App) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded(RADIUS)
            .bg(cx.theme().muted)
    }

    fn segment(&self, id: ElementId, selected: bool, cx: &App) -> Button {
        let theme = cx.theme();
        Button::new(id)
            .ghost()
            .small()
            .px_3()
            .selected(selected)
            .when(selected, |b| {
                b.bg(theme.background).text_color(theme.foreground).shadow_xs()
            })
    }

    fn nav_item(&self, id: ElementId, label: SharedString, selected: bool, cx: &App) -> Button {
        let theme = cx.theme();
        Button::new(id)
            .ghost()
            .w_full()
            .px_3()
            .accessibility_label(label.clone())
            .child(div().flex_1().child(label))
            .selected(selected)
            .when(selected, |b| b.bg(theme.muted).text_color(theme.foreground))
    }
}
