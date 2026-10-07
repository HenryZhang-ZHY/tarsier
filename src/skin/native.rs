//! The default skin: whatever `gpui-component` draws out of the box.
//!
//! This is the look tarsier shipped before there were skins, written down as a
//! skin so the two share one set of call sites. It adds nothing of its own — it
//! names the theme's own tokens — and it is the reason a config written by an
//! older version still opens looking the way it was saved.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Theme, ThemeMode, v_flex};
use gpui_kit::*;

use super::{Control, Level, SkinStyle, Tone, Voice};

/// The radius the theme's own default uses, so a card here is the card the
/// component library would have drawn.
const RADIUS: Pixels = px(8.);

pub(super) struct Native;

impl SkinStyle for Native {
    fn note(&self) -> &'static str {
        crate::i18n::tr!("The look the app has always had: the system font, soft corners and quiet greys")
    }

    fn modes(&self) -> &'static [ThemeMode] {
        &[ThemeMode::Light, ThemeMode::Dark]
    }

    fn install(&self, cx: &mut App) {
        // The palette is the library's; only the two things this app has always
        // chosen for itself are set here, so an install is idempotent and two
        // windows in a row cannot drift.
        Theme::update(cx, |theme| {
            theme.radius = px(6.);
            theme.radius_lg = RADIUS;
            theme.shadow = true;
            theme.focus_ring = true;
        });
    }

    fn font(&self, cx: &App) -> Font {
        // The family the config has always named, at the weight the window has
        // always used: this skin translates the default look, it does not
        // restyle it.
        gpui_kit::font(cx.theme().font_family.clone())
    }

    fn canvas(&self, cx: &App) -> Div {
        // A column rather than a plain block, because the caller puts the
        // scrolling body inside it and grows it with `flex_1`.
        let theme = cx.theme();
        v_flex().bg(theme.background).text_color(theme.foreground)
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

    fn panel(&self, cx: &App) -> Div {
        v_flex().p_3().rounded_md().bg(cx.theme().muted)
    }

    fn section_label(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(text.to_owned()))
            .into_any_element()
    }

    fn chip(&self, text: &str, tone: Tone, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let (fg, bg) = match tone {
            Tone::Accent => (theme.primary_foreground, theme.primary),
            Tone::Secondary => (theme.secondary_foreground, theme.secondary),
            Tone::Muted => (theme.muted_foreground, theme.muted),
            Tone::Danger => (theme.danger_foreground, theme.danger),
            Tone::Outline | Tone::Ghost => (theme.muted_foreground, theme.background),
        };
        div()
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(bg)
            .text_xs()
            .text_color(fg)
            .child(self.case(text))
            .into_any_element()
    }

    fn band(&self, cx: &App) -> Div {
        div().w_full().h(px(1.)).bg(cx.theme().border)
    }

    fn weight(&self, voice: Voice) -> FontWeight {
        match voice {
            Voice::Loud => FontWeight::SEMIBOLD,
            // The library's own body weight. Asking for MEDIUM here would be a
            // change to the default look, not a translation of it.
            Voice::Plain | Voice::Quiet => FontWeight::NORMAL,
        }
    }

    fn case(&self, text: &str) -> SharedString {
        SharedString::from(text.to_owned())
    }

    fn identity(&self, index: usize) -> Hsla {
        const HUES: [f32; 4] = [0.58, 0.09, 0.78, 0.45];
        hsla(HUES[index % HUES.len()], 0.72, 0.55, 1.0)
    }

    fn status_text(&self, level: Level, cx: &App) -> Hsla {
        self.status_block(level, cx)
    }

    fn status_block(&self, level: Level, cx: &App) -> Hsla {
        let theme = cx.theme();
        match level {
            Level::Good => theme.success,
            Level::Fair => theme.warning,
            Level::Poor => theme.danger,
            Level::Neutral => theme.muted_foreground,
        }
    }

    fn status_bar(&self, level: Level, height: Pixels, cx: &App) -> Div {
        div().w_full().h(height).rounded_md().bg(self.status_block(level, cx))
    }

    fn button(&self, id: SharedString, tone: Tone, control: Control, cx: &App) -> Button {
        let _ = cx;
        let button = Button::new(id);
        let button = match tone {
            Tone::Accent => button.primary(),
            Tone::Secondary => button.secondary(),
            Tone::Danger => button.danger(),
            Tone::Outline => button.outline(),
            Tone::Muted | Tone::Ghost => button.ghost(),
        };
        match control {
            Control::Tiny => button.xsmall(),
            Control::Small => button.small(),
            Control::Medium => button,
        }
    }
}
