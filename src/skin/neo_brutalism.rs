//! Neo-brutalism: warm paper, black ink, solid strokes, hard offset shadows.
//!
//! The style lives on restraint as much as on loudness. Structure is ink —
//! every card, button and mark wears a 2px black stroke, and raised surfaces
//! cast a solid ink block with **zero blur**. Colour is spent sparingly:
//!
//! - **One accent**, sunflower yellow, for the thing to press and for "you are
//!   here" (the chosen settings section, a primary button, the slider thumb).
//! - **Status colours only for status**: mint for good, orange for fair, coral
//!   for poor. They are fills behind ink text, never text themselves.
//! - **Identity colours** (sky, pink, lilac, peach) tell computers apart and
//!   appear nowhere else.
//!
//! Everything else is paper, white and ink. Fine print is a warm dark grey: a
//! style that forbids greys entirely leaves body copy and captions at the same
//! weight, and then nothing reads as secondary.
//!
//! GPUI has no transforms, letter spacing or text strokes, so the style's
//! rotated stickers and tracked-out labels are not attempted; uppercase small
//! labels and the ink blocks carry it instead.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    Colorize as _, Disableable as _, Selectable as _, Sizable as _, Size, Theme, ThemeMode, TitleBar, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Control, Mark, SkinStyle, Tone, Voice};

// ---- palette -------------------------------------------------------------

const PAPER: u32 = 0xF4EFE6;
const SURFACE: u32 = 0xFFFFFF;
const INK: u32 = 0x111111;
/// Fine print. 7:1 on white, so it reads as secondary without failing anyone.
const INK_SOFT: u32 = 0x57524A;
/// Neutral fills: an empty chart day, a switch that is off.
const STONE: u32 = 0xD9D1C2;

const ACCENT: u32 = 0xFFD23F;
const ACCENT_HOVER: u32 = 0xF5C518;
const ACCENT_PRESSED: u32 = 0xE0B000;
/// The accent diluted for hover on quiet controls.
const ACCENT_WASH: u32 = 0xFFF1BF;

const MINT: u32 = 0x6FDB9E;
const ORANGE: u32 = 0xFFAE5C;
const CORAL: u32 = 0xFF7564;

const IDENTITY: [u32; 4] = [0x8EC5FF, 0xFF9EC4, 0xC3B1FF, 0xFFC59E];

pub(super) const FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/SpaceGrotesk-Regular.ttf");
pub(super) const FONT_BOLD: &[u8] = include_bytes!("../../assets/fonts/SpaceGrotesk-Bold.ttf");
/// The bundled display face; see `assets/fonts/OFL.txt` for its licence.
const FONT: &str = "Space Grotesk";
/// Space Grotesk has no CJK glyphs, and the UI is drawn in English or Chinese.
const CJK_FALLBACK: &str = "Microsoft YaHei UI";

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

// ---- geometry ------------------------------------------------------------

const STROKE: Pixels = px(2.);
const RADIUS: Pixels = px(4.);
const CARD_RADIUS: Pixels = px(6.);
const CARD_SHADOW: Pixels = px(4.);

/// A solid block of ink, offset down and right, with no blur and no spread.
fn hard(offset: Pixels) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: color(INK),
        offset: point(offset, offset),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: false,
    }]
}

pub(super) struct NeoBrutalism;

impl SkinStyle for NeoBrutalism {
    fn note(&self) -> &'static str {
        crate::i18n::tr!("Warm paper, black ink, solid strokes and hard shadows. Light only, by design")
    }

    fn modes(&self) -> &'static [ThemeMode] {
        &[ThemeMode::Light]
    }

    fn install(&self, cx: &mut App) {
        Theme::update(cx, |theme| {
            theme.radius = RADIUS;
            theme.radius_lg = CARD_RADIUS;
            // The library's soft shadows would sit on top of the ink blocks.
            theme.shadow = false;
            theme.focus_ring = true;
            theme.font_family = FONT.into();

            // A mechanical style snaps rather than eases.
            theme.motion.duration_fast = Duration::from_millis(80);
            theme.motion.duration_normal = Duration::from_millis(120);
            theme.motion.duration_slow = Duration::from_millis(180);

            // `background` is what inputs and popovers fill with, so it is the
            // white of a card; the paper is drawn by `canvas`.
            theme.background = color(SURFACE);
            theme.foreground = color(INK);
            theme.border = color(INK);
            theme.input = color(INK);
            theme.ring = color(INK);
            theme.caret = color(INK);
            theme.selection = color(ACCENT).opacity(0.6);

            // `primary` is structural in the library — a checked switch's track,
            // a focus outline — so it is ink. The accent reaches buttons through
            // `button_primary`.
            theme.primary = color(INK);
            theme.primary_foreground = color(SURFACE);
            theme.primary_hover = color(INK).lighten(0.2);
            theme.primary_active = color(INK);
            theme.secondary = color(ACCENT_WASH);
            theme.secondary_foreground = color(INK);
            theme.secondary_hover = color(ACCENT_WASH);
            theme.secondary_active = color(ACCENT);
            theme.muted = color(PAPER);
            theme.muted_foreground = color(INK_SOFT);
            theme.accent = color(ACCENT_WASH);
            theme.accent_foreground = color(INK);
            theme.link = color(INK);
            theme.link_hover = color(INK);
            theme.link_active = color(INK);

            theme.danger = color(CORAL);
            theme.danger_foreground = color(INK);
            theme.danger_hover = color(CORAL).darken(0.08);
            theme.danger_active = color(CORAL).darken(0.16);
            theme.success = color(MINT);
            theme.success_foreground = color(INK);
            theme.warning = color(ORANGE);
            theme.warning_foreground = color(INK);
            theme.info = color(IDENTITY[0]);
            theme.info_foreground = color(INK);

            theme.button = color(SURFACE);
            theme.button_foreground = color(INK);
            theme.button_hover = color(ACCENT_WASH);
            theme.button_active = color(ACCENT);
            theme.button_primary = color(ACCENT);
            theme.button_primary_foreground = color(INK);
            theme.button_primary_hover = color(ACCENT_HOVER);
            theme.button_primary_active = color(ACCENT_PRESSED);
            theme.button_danger = color(CORAL);
            theme.button_danger_foreground = color(INK);
            theme.button_danger_hover = color(CORAL).darken(0.08);
            theme.button_danger_active = color(CORAL).darken(0.16);

            theme.popover = color(SURFACE);
            theme.popover_foreground = color(INK);
            theme.overlay = color(INK).opacity(0.5);
            theme.title_bar = color(SURFACE);
            theme.title_bar_border = color(INK);
            theme.window_border = color(INK);

            theme.switch = color(STONE);
            theme.switch_thumb = color(SURFACE);
            theme.slider_bar = color(INK);
            theme.slider_thumb = color(ACCENT);
            theme.progress_bar = color(INK);

            theme.scrollbar = color(PAPER).opacity(0.);
            theme.scrollbar_thumb = color(INK_SOFT).opacity(0.55);
            theme.scrollbar_thumb_hover = color(INK);

            theme.chart_1 = color(ACCENT);
            theme.chart_2 = color(IDENTITY[0]);
            theme.chart_3 = color(IDENTITY[1]);
            theme.chart_4 = color(MINT);
            theme.chart_5 = color(ORANGE);
        });
    }

    fn font(&self, cx: &App) -> Font {
        let _ = cx;
        Font {
            family: FONT.into(),
            weight: FontWeight::NORMAL,
            fallbacks: Some(FontFallbacks::from_fonts(vec![CJK_FALLBACK.to_string()])),
            ..Default::default()
        }
    }

    fn canvas(&self, cx: &App) -> Div {
        let _ = cx;
        v_flex().bg(color(PAPER)).text_color(color(INK))
    }

    fn title_bar(&self, bar: TitleBar, cx: &App) -> TitleBar {
        let _ = cx;
        bar.bg(color(SURFACE)).border_b(STROKE).border_color(color(INK))
    }

    fn card(&self, cx: &App) -> Div {
        let _ = cx;
        v_flex()
            .p_4()
            .rounded(CARD_RADIUS)
            .border(STROKE)
            .border_color(color(INK))
            .bg(color(SURFACE))
            .shadow(hard(CARD_SHADOW))
    }

    fn inset(&self, cx: &App) -> Div {
        let _ = cx;
        // Paper inside a white card, with the stroke but no shadow: a well,
        // not another sheet on top.
        v_flex()
            .p_3()
            .rounded(RADIUS)
            .border(STROKE)
            .border_color(color(INK))
            .bg(color(PAPER))
    }

    fn divider(&self, cx: &App) -> Div {
        let _ = cx;
        div().w_full().h(STROKE).bg(color(INK))
    }

    fn eyebrow(&self, text: &str, cx: &App) -> AnyElement {
        let _ = cx;
        // An ink tag with white type: the card's name, stuck on its corner.
        div()
            .flex()
            .child(
                div()
                    .px_2()
                    .py(px(2.))
                    .rounded(px(3.))
                    .bg(color(INK))
                    .text_color(color(SURFACE))
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(SharedString::from(text.to_uppercase())),
            )
            .into_any_element()
    }

    fn chip(&self, text: &str, mark: Mark, cx: &App) -> AnyElement {
        div()
            .flex_none()
            .px_1p5()
            .rounded(px(3.))
            .border(STROKE)
            .border_color(color(INK))
            .bg(match mark {
                Mark::Plain => color(SURFACE),
                other => self.fill(other, cx),
            })
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(color(INK))
            .whitespace_nowrap()
            .child(SharedString::from(text.to_uppercase()))
            .into_any_element()
    }

    fn key_cap(&self, text: &str, cx: &App) -> AnyElement {
        let _ = cx;
        div()
            .flex_none()
            .flex()
            .justify_center()
            .min_w(px(24.))
            .px_1p5()
            .rounded(px(3.))
            .border(STROKE)
            .border_color(color(INK))
            .bg(color(SURFACE))
            .shadow(vec![BoxShadow {
                color: color(INK),
                offset: point(px(0.), px(2.)),
                blur_radius: px(0.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(color(INK))
            .child(SharedString::from(text.to_owned()))
            .into_any_element()
    }

    fn swatch(&self, fill: Hsla, cx: &App) -> Div {
        let _ = cx;
        div()
            .flex_none()
            .size(px(12.))
            .rounded(px(2.))
            .border(STROKE)
            .border_color(color(INK))
            .bg(fill)
    }

    fn weight(&self, voice: Voice) -> FontWeight {
        match voice {
            Voice::Loud | Voice::Plain => FontWeight::BOLD,
            Voice::Quiet => FontWeight::NORMAL,
        }
    }

    fn identity(&self, index: usize, cx: &App) -> Hsla {
        let _ = cx;
        color(IDENTITY[index % IDENTITY.len()])
    }

    fn fill(&self, mark: Mark, cx: &App) -> Hsla {
        let _ = cx;
        color(match mark {
            Mark::Plain => STONE,
            Mark::Accent => ACCENT,
            Mark::Good => MINT,
            Mark::Fair => ORANGE,
            Mark::Poor => CORAL,
        })
    }

    fn ink(&self, mark: Mark, cx: &App) -> Hsla {
        let _ = (mark, cx);
        // Every fill in the palette is too light to be text on white, so a
        // status word stays ink and the block beside it carries the colour.
        color(INK)
    }

    fn bar(&self, mark: Mark, height: Pixels, cx: &App) -> Div {
        div()
            .w_full()
            .h(height)
            .rounded_t(px(3.))
            .border(STROKE)
            .border_color(color(INK))
            .bg(self.fill(mark, cx))
    }

    fn meter(&self, fraction: f32, mark: Mark, cx: &App) -> Div {
        let fill = match mark {
            Mark::Plain => self.fill(Mark::Accent, cx),
            other => self.fill(other, cx),
        };
        let fraction = fraction.clamp(0., 1.);
        div()
            .w_full()
            .h(px(14.))
            .rounded(px(3.))
            .overflow_hidden()
            .border(STROKE)
            .border_color(color(INK))
            .bg(color(PAPER))
            .child(
                div()
                    .h_full()
                    .w(relative(fraction))
                    .bg(fill)
                    // The edge of the fill is a stroke too, unless it is empty
                    // or full, where it would double the frame.
                    .when(fraction > 0. && fraction < 1., |el| {
                        el.border_r(STROKE).border_color(color(INK))
                    }),
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
        let (height, padding, lift) = match control {
            Control::Compact => (px(26.), px(8.), px(2.)),
            Control::Regular => (px(32.), px(12.), px(3.)),
            Control::Prominent => (px(40.), px(16.), px(4.)),
        };
        let size = if control == Control::Prominent {
            Size::Medium
        } else {
            Size::Small
        };
        let ghost = tone == Tone::Ghost;
        button
            .with_size(size)
            .h(height)
            .px(padding)
            .font_weight(FontWeight::BOLD)
            // A ghost keeps an invisible stroke so it lines up with its
            // neighbours, and casts no shadow because it is not a raised sheet.
            .border(STROKE)
            .border_color(if ghost { color(INK).opacity(0.) } else { color(INK) })
            .when(!ghost, |b| b.shadow(hard(lift)))
    }

    fn disable(&self, button: Button, disabled: bool) -> Button {
        // The skin's style is replayed over the disabled look, shadow included,
        // and an ink block under a faded fill reads as a dark smear.
        button.disabled(disabled).when(disabled, |b| {
            b.shadow(Vec::new()).border_color(color(INK_SOFT).opacity(0.5))
        })
    }

    fn segmented(&self, cx: &App) -> Div {
        let _ = cx;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(5.))
            .border(STROKE)
            .border_color(color(INK))
            .bg(color(PAPER))
    }

    fn segment(&self, id: ElementId, selected: bool, cx: &App) -> Button {
        let _ = cx;
        Button::new(id)
            .ghost()
            .small()
            .h(px(26.))
            .px_3()
            .rounded(px(3.))
            .font_weight(FontWeight::BOLD)
            .selected(selected)
            .when(selected, |b| b.bg(color(INK)).text_color(color(SURFACE)))
    }

    fn nav_item(&self, id: ElementId, label: SharedString, selected: bool, cx: &App) -> Button {
        let _ = cx;
        Button::new(id)
            .ghost()
            .w_full()
            .h(px(34.))
            .px_3()
            .accessibility_label(label.clone())
            .child(div().flex_1().child(label))
            .font_weight(FontWeight::BOLD)
            .border(STROKE)
            .border_color(color(INK).opacity(0.))
            .selected(selected)
            .when(selected, |b| {
                b.bg(color(ACCENT))
                    .text_color(color(INK))
                    .border_color(color(INK))
                    .shadow(hard(px(2.)))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{ACCENT, CORAL, IDENTITY, INK, INK_SOFT, MINT, ORANGE, PAPER, STONE, SURFACE, color, hard};
    use crate::skin::contrast;
    use gpui_kit::px;

    #[test]
    fn every_fill_carries_ink_text_at_aaa_contrast() {
        // Status, identity and accent colours are always fills behind ink
        // words, so this is the contrast that has to hold.
        let fills = [PAPER, SURFACE, STONE, ACCENT, MINT, ORANGE, CORAL];
        for fill in fills.into_iter().chain(IDENTITY) {
            let ratio = contrast(color(fill), color(INK));
            assert!(ratio >= 7., "#{fill:06X} carries ink at only {ratio:.1}:1");
        }
    }

    #[test]
    fn fine_print_stays_readable_on_both_surfaces() {
        for surface in [SURFACE, PAPER] {
            let ratio = contrast(color(INK_SOFT), color(surface));
            assert!(ratio >= 4.5, "fine print on #{surface:06X} is {ratio:.1}:1");
        }
    }

    #[test]
    fn white_on_ink_reads_for_the_selected_segment() {
        assert!(contrast(color(SURFACE), color(INK)) >= 7.);
    }

    #[test]
    fn shadows_are_solid_ink_blocks() {
        for offset in [px(2.), px(3.), px(4.)] {
            let [shadow] = hard(offset).try_into().expect("one shadow");
            assert_eq!(shadow.blur_radius, px(0.));
            assert_eq!(shadow.spread_radius, px(0.));
            assert_eq!((shadow.offset.x, shadow.offset.y), (offset, offset));
        }
    }
}
