//! Neo-brutalism: cream paper, pure ink, thick strokes, hard offset shadows.
//!
//! The style is a rebellion against soft, borderless, gradient-lit interfaces.
//! Structure is not implied here, it is enforced: every surface this skin owns
//! carries a black stroke, and every shadow is a solid block of ink at a 45°
//! offset with **zero blur** — `BoxShadow` with `blur_radius: 0` and
//! `spread_radius: 0`, which is the only kind of shadow this file ever builds.
//!
//! # What this style asks for that GPUI cannot give
//!
//! Three of the source design's signatures have no equivalent in GPUI's style
//! model. Each is replaced rather than faked, because a half-working imitation
//! reads as a bug:
//!
//! - **Rotation** ("stickers slapped on at angles"). `Style` has no transform,
//!   so nothing can be rotated. The layered-collage feel comes from hard
//!   shadows, offset blocks and asymmetry instead.
//! - **Letter spacing** (`tracking-widest`). `TextStyle` has no tracking.
//!   Uppercase plus heavy weight plus a colour block carries the label instead.
//! - **Text stroke** (`-webkit-text-stroke`, hollow display type). Not
//!   available. Display type is solid ink beside a colour block, which the
//!   source design also names as the alternative.
//!
//! # Where the palette is mapped onto the theme
//!
//! The component library's widgets are themed through tokens, not through
//! per-widget styling, so the translation lives entirely in
//! [`NeoBrutalism::install`]. Three mappings are worth knowing about, because
//! they are what make the widgets come out right:
//!
//! - `primary` is set to **ink**, not to the red. The library uses `primary`
//!   for a button's *stroke*, a checked switch's track and a tab bar's
//!   indicator — all structural here, so all black. The red reaches the primary
//!   button through `button_primary`, its *fill*, which the library reads
//!   separately. That one split is what yields a red button with a black
//!   outline.
//! - `muted_foreground` is set to **ink** as well. The style forbids greys, so
//!   secondary text recedes by size and case instead of by going grey.
//! - `shadow` is turned **off**. The library paints a blurred `shadow_xs` under
//!   buttons when it is on, which would land on top of the hard ink block this
//!   skin attaches by hand.
//!
//! Contrast is checked against the cream canvas: ink on cream is 19.6:1, ink on
//! red 7.4:1, ink on yellow 14.8:1, ink on violet 12.5:1. The red itself is
//! 2.6:1 on cream, so it is never used as text — [`SkinStyle::status_text`]
//! returns ink and lets a coloured block carry the meaning.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{Colorize as _, Sizable as _, Size as ControlSize, Theme, ThemeMode, v_flex};
use gpui_kit::*;

use super::{Control, Level, SkinStyle, Tone, Voice};

// ---- the palette ---------------------------------------------------------

/// The canvas: warm, paper-like, softer than white.
const CREAM: u32 = 0xFFFDF5;
/// Structure: every stroke, every shadow, every word.
const INK: u32 = 0x000000;
/// The accent and the primary action.
const RED: u32 = 0xFF6B6B;
/// The second colour, and the title bar.
const YELLOW: u32 = 0xFFD93D;
/// The third colour: card headers and quiet fills.
const VIOLET: u32 = 0xC4B5FD;
/// Contrast panels. Cards are white on the cream canvas.
const WHITE: u32 = 0xFFFFFF;
/// Not in the source palette, which names only three highlighter colours, but a
/// monitor that is *this* computer has to read as good rather than as an
/// accident of the colour cycle. A marker green keeps the unmixed-paint feel.
const GREEN: u32 = 0x4ADE80;

/// The bundled display face. See `assets/fonts/OFL.txt` for its licence.
const FONT: &str = "Space Grotesk";
/// Space Grotesk has no CJK glyphs, and this UI is drawn in English or Chinese.
/// Without a fallback the Chinese window would be a page of tofu.
const CJK_FALLBACK: &str = "Microsoft YaHei UI";

fn ink() -> Hsla {
    rgb(INK).into()
}

fn cream() -> Hsla {
    rgb(CREAM).into()
}

fn white() -> Hsla {
    rgb(WHITE).into()
}

fn red() -> Hsla {
    rgb(RED).into()
}

fn yellow() -> Hsla {
    rgb(YELLOW).into()
}

fn violet() -> Hsla {
    rgb(VIOLET).into()
}

fn green() -> Hsla {
    rgb(GREEN).into()
}

// ---- geometry ------------------------------------------------------------

/// The signature stroke. Cards, chips and standing buttons all wear it.
const STROKE: Pixels = px(4.);
/// The stroke on a small mark, where 4px would close the counter of a letter.
const STROKE_THIN: Pixels = px(2.);
/// Card ink block. [`SkinStyle::lift`] deepens it to `LIFTED` on hover, and the
/// two are a pair: changing one without the other makes the hover read as a
/// glitch rather than as lift.
const SHADOW: Pixels = px(8.);
const LIFTED: Pixels = px(12.);
/// How far a lifted surface travels. It matches the shadow growth, so the block
/// appears to slide out from under the card.
const LIFT_TRAVEL: Pixels = px(3.);

/// The spacing of the graph-paper grid, and how many lines of it are drawn. The
/// grid is fixed rather than measured: measuring it would need the layout pass
/// the layer exists to sit behind. This many lines covers 2400px each way, and
/// the layer clips whatever a larger window would have wanted.
const GRID_PITCH: f32 = 48.;
const GRID_LINES: usize = 50;

/// A solid block of ink, offset bottom-right, with no blur and no spread.
fn hard(offset: Pixels) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: ink(),
        offset: point(offset, offset),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// The graph-paper background, drawn as real lines.
///
/// GPUI can repeat neither an image nor a gradient, so a tiled 20px halftone
/// would be roughly a thousand elements repainted every frame. A 48px grid is a
/// hundred lines for a window this size, which the renderer does not notice.
fn grid() -> Div {
    let line = ink().alpha(0.09);
    let mut layer = div().absolute().inset_0().overflow_hidden();
    for i in 0..GRID_LINES {
        let offset = px(i as f32 * GRID_PITCH);
        layer = layer
            .child(div().absolute().top_0().bottom_0().left(offset).w(px(1.)).bg(line))
            .child(div().absolute().left_0().right_0().top(offset).h(px(1.)).bg(line));
    }
    layer
}

pub(super) struct NeoBrutalism;

impl SkinStyle for NeoBrutalism {
    fn note(&self) -> &'static str {
        crate::i18n::tr!("Cream paper, pure ink, thick black strokes and hard offset shadows. Light only by design")
    }

    /// Light only, and deliberately so. The style is a single, definitive light
    /// palette; a dark variant would be an invention rather than a translation,
    /// so [`super::resolve_mode`] pins the window here instead.
    fn modes(&self) -> &'static [ThemeMode] {
        &[ThemeMode::Light]
    }

    fn install(&self, cx: &mut App) {
        // Start from the library's light palette, so every field this skin does
        // not name stays a sane value rather than an unset one.
        Theme::change(ThemeMode::Light, None, cx);
        Theme::update(cx, |theme| {
            // Square everywhere, and no soft elevation anywhere.
            theme.radius = px(0.);
            theme.radius_lg = px(0.);
            theme.shadow = false;
            // Focus rings stay on. A keyboard user has to be able to see where
            // they are, and a black ring on cream is the loudest thing on the
            // page — the style and the accessibility requirement agree here.
            theme.focus_ring = true;
            theme.font_family = FONT.into();

            // Fast and direct. The style is mechanical, so nothing eases in and
            // out over a quarter of a second; a control snaps.
            theme.motion.duration_fast = Duration::from_millis(80);
            theme.motion.duration_normal = Duration::from_millis(120);
            theme.motion.duration_slow = Duration::from_millis(180);

            theme.background = cream();
            theme.foreground = ink();
            theme.border = ink();
            theme.input = ink();
            theme.ring = ink();
            theme.caret = ink();
            // A marker stroke behind selected text.
            theme.selection = yellow();

            // Structural accents are ink; see the module note on `primary`.
            theme.primary = ink();
            theme.primary_foreground = white();
            // The accent's own interaction states, for the components that read
            // them instead of the button tokens.
            theme.primary_hover = red().darken(0.08);
            theme.primary_active = red().darken(0.16);
            theme.secondary = yellow();
            theme.secondary_foreground = ink();
            theme.muted = violet();
            // No greys: secondary text recedes by size and case instead.
            theme.muted_foreground = ink();
            theme.accent = yellow();
            theme.accent_foreground = ink();
            theme.danger = red();
            theme.danger_foreground = ink();
            theme.success = green();
            theme.success_foreground = ink();
            theme.warning = yellow();
            theme.warning_foreground = ink();
            theme.info = violet();
            theme.info_foreground = ink();

            // A link carries ink rather than the accent: red text on cream is
            // 2.6:1, which fails AA. The heavy underline does the work.
            theme.link = ink();
            theme.link_hover = ink();
            theme.link_active = ink();

            // The split that makes a red button with a black outline.
            //
            // Every hover is a snap to a different block and every press is that
            // same block a step darker: the style has no room for a translate,
            // so the press has to read as "pushed in" rather than as "changed
            // state", and only a value step says that.
            theme.button = white();
            theme.button_foreground = ink();
            theme.button_hover = violet();
            theme.button_active = violet().darken(0.2);
            theme.button_primary = red();
            theme.button_primary_foreground = ink();
            theme.button_primary_hover = red().darken(0.08);
            theme.button_primary_active = red().darken(0.16);
            theme.button_secondary = yellow();
            theme.button_secondary_foreground = ink();
            theme.button_secondary_hover = yellow().darken(0.08);
            theme.button_secondary_active = yellow().darken(0.16);
            theme.button_danger = red();
            theme.button_danger_foreground = ink();
            theme.button_danger_hover = red().darken(0.08);
            theme.button_danger_active = red().darken(0.16);

            // Panels. Cards and menus are white on the cream canvas.
            theme.popover = white();
            theme.popover_foreground = ink();
            theme.overlay = ink().alpha(0.6);
            theme.group_box = violet();
            theme.group_box_foreground = ink();
            theme.accordion = white();
            theme.skeleton = violet();
            theme.drag_border = ink();
            theme.drop_target = yellow();
            theme.description_list_label = violet();
            theme.description_list_label_foreground = ink();

            // The title bar is a yellow band: the one place colour blocking gets
            // to shout without stealing attention from the content, framing the
            // window like a poster.
            theme.title_bar = yellow();
            theme.title_bar_border = ink();
            theme.status_bar = cream();
            theme.status_bar_border = ink();

            // Tabs: a cream well inside the yellow band, with the selected tab
            // as a solid ink block and white words on it.
            theme.tab_bar = yellow();
            theme.tab_bar_segmented = cream();
            theme.tab = cream();
            theme.tab_foreground = ink();
            theme.tab_active = ink();
            theme.tab_active_foreground = white();

            // Controls. A black thumb on violet is the one pair in the palette
            // that still reads at nine pixels.
            theme.switch = violet();
            theme.switch_thumb = ink();
            theme.slider_bar = violet();
            theme.slider_thumb = ink();
            theme.progress_bar = red();

            theme.scrollbar = cream();
            theme.scrollbar_thumb = ink();
            theme.scrollbar_thumb_hover = red();

            // Through `colors`, not the deref: `Theme` has a `list` field of its
            // own — the list widget's settings — which shadows the colour.
            theme.colors.list = white();
            theme.list_hover = yellow();
            theme.list_active = yellow();
            theme.list_even = cream();
            theme.list_head = violet();
            theme.table = white();
            theme.table_hover = yellow();
            theme.table_active = yellow();
            theme.table_even = cream();
            theme.table_head = violet();
            theme.table_head_foreground = ink();
            theme.table_row_border = ink();

            theme.sidebar = yellow();
            theme.sidebar_foreground = ink();
            theme.sidebar_border = ink();
            theme.sidebar_accent = red();
            theme.sidebar_accent_foreground = ink();
            theme.sidebar_primary = red();
            theme.sidebar_primary_foreground = ink();

            // Charts: the palette itself, in order of loudness.
            theme.chart_1 = red();
            theme.chart_2 = yellow();
            theme.chart_3 = violet();
            theme.chart_4 = ink();
            theme.chart_5 = green();
            theme.chart_bullish = green();
            theme.chart_bearish = red();
            theme.chart_grid = ink().alpha(0.35);

            theme.window_border = ink();
        });
    }

    fn font(&self, cx: &App) -> Font {
        let _ = cx;
        Font {
            family: FONT.into(),
            // Bold is the resting weight; only `Voice::Quiet` drops to the
            // regular face, which is why both weights are bundled.
            weight: FontWeight::BOLD,
            fallbacks: Some(FontFallbacks::from_fonts(vec![CJK_FALLBACK.to_string()])),
            ..Default::default()
        }
    }

    fn canvas(&self, cx: &App) -> Div {
        // The grid is the first child so every later sibling paints over it,
        // and it lives in the window rather than in the scrolling body, so the
        // paper stays put while the content slides across it. Sizing is the
        // caller's: this only says what the layer is made of.
        let _ = cx;
        v_flex().relative().bg(cream()).text_color(ink()).child(grid())
    }

    fn card(&self, cx: &App) -> Div {
        let _ = cx;
        v_flex()
            .p_4()
            .rounded_none()
            .border(STROKE)
            .border_color(ink())
            // White on cream, so a panel reads as a sheet laid on the paper.
            .bg(white())
            .shadow(hard(SHADOW))
    }

    fn panel(&self, cx: &App) -> Div {
        let _ = cx;
        // Violet rather than white: a recessed block on a card, with a thinner
        // stroke than the card it sits in so the two do not compete.
        v_flex()
            .p_3()
            .rounded_none()
            .border(STROKE_THIN)
            .border_color(ink())
            .bg(violet())
    }

    fn lift(&self, el: Div, cx: &App) -> Div {
        let _ = cx;
        // `relative` plus an inset offset moves the card without moving its
        // neighbours: GPUI has no transform, and a margin would reflow the page
        // out from under the pointer.
        el.relative()
            .hover(|style| style.top(-LIFT_TRAVEL).shadow(hard(LIFTED)))
    }

    fn display(&self, text: &str, cx: &App) -> AnyElement {
        let _ = cx;
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_2xl()
                    .line_height(px(26.))
                    .font_weight(FontWeight::BLACK)
                    .text_color(ink())
                    .child(self.case(text)),
            )
            // The red bar under the title: a solid block, not a rule.
            .child(div().h(px(6.)).w(px(56.)).bg(red()))
            .into_any_element()
    }

    fn section_label(&self, text: &str, cx: &App) -> AnyElement {
        let _ = cx;
        div()
            .px_2()
            .py_0p5()
            .border(STROKE_THIN)
            .border_color(ink())
            // Yellow, not the violet the panels use: a caption frequently sits
            // *inside* a violet panel, and two violets told apart only by a
            // stroke read as one block.
            .bg(yellow())
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(ink())
            .child(self.case(text))
            .into_any_element()
    }

    fn chip(&self, text: &str, tone: Tone, cx: &App) -> AnyElement {
        let _ = cx;
        let fill = match tone {
            Tone::Accent | Tone::Danger => red(),
            Tone::Secondary => yellow(),
            Tone::Muted => violet(),
            Tone::Outline | Tone::Ghost => cream(),
        };
        div()
            .px_2()
            .py_0p5()
            .rounded_none()
            .border(STROKE_THIN)
            .border_color(ink())
            .bg(fill)
            // Even a badge gets an ink block. It is what makes it a sticker
            // rather than a label.
            .shadow(hard(px(2.)))
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(ink())
            .child(self.case(text))
            .into_any_element()
    }

    fn band(&self, cx: &App) -> Div {
        let _ = cx;
        // Hazard stripes: the one repeating pattern GPUI ships.
        div()
            .w_full()
            .h(px(8.))
            .border_y(STROKE_THIN)
            .border_color(ink())
            .bg(pattern_slash(ink(), 0.35, 0.35))
    }

    fn weight(&self, voice: Voice) -> FontWeight {
        match voice {
            // Space Grotesk tops out at Bold, so 900 degrades to 700 in the
            // renderer. The intent is written down anyway: a family swapped in
            // later that *does* ship a black weight gets it for free.
            Voice::Loud => FontWeight::BLACK,
            Voice::Plain => FontWeight::BOLD,
            // The one honest use of the regular face. Fine print at 11px bold
            // is a grey mush, and the style would rather be readable there than
            // loud.
            Voice::Quiet => FontWeight::NORMAL,
        }
    }

    fn case(&self, text: &str) -> SharedString {
        // Uppercase stands in for the letter spacing GPUI cannot do. Chinese
        // has no case, so a translated label passes through unchanged, which is
        // the right outcome rather than a compromise.
        text.to_uppercase().into()
    }

    fn identity(&self, index: usize) -> Hsla {
        // White is in the cycle because a fourth computer needs a colour that is
        // not a variation on the other three, and a white block with an ink
        // stroke is a colour in this palette.
        const FILLS: [u32; 4] = [RED, YELLOW, VIOLET, WHITE];
        rgb(FILLS[index % FILLS.len()]).into()
    }

    fn status_text(&self, level: Level, cx: &App) -> Hsla {
        let _ = (level, cx);
        // Always ink. Every colour in the palette is too light to be text on
        // cream, so a status is a coloured block with black words on it, and
        // the words themselves never leave the structural colour.
        ink()
    }

    fn status_block(&self, level: Level, cx: &App) -> Hsla {
        let _ = cx;
        match level {
            Level::Good => green(),
            Level::Fair => yellow(),
            Level::Poor => red(),
            Level::Neutral => violet(),
        }
    }

    fn status_bar(&self, level: Level, height: Pixels, cx: &App) -> Div {
        // A bar is a block here like everything else: ink stroke, square
        // corners, and the colour inside carries the reading.
        div()
            .w_full()
            .h(height)
            .rounded_none()
            .border(STROKE_THIN)
            .border_color(ink())
            .bg(self.status_block(level, cx))
    }

    fn button(&self, id: SharedString, tone: Tone, control: Control, cx: &App) -> Button {
        // The variant decides the *fill*; the skin decides everything else.
        // `Tone::Ghost` is a custom variant rather than the library's ghost so
        // that it can snap to yellow under the pointer, which is this style's
        // hover idiom — the library's own ghost has no hover state at all.
        let button = match tone {
            Tone::Accent => Button::new(id).primary(),
            Tone::Secondary => Button::new(id).secondary(),
            Tone::Danger => Button::new(id).danger(),
            Tone::Outline => Button::new(id),
            Tone::Muted => Button::new(id).custom(
                ButtonCustomVariant::new(cx)
                    .color(violet())
                    .foreground(ink())
                    .hover(violet().darken(0.08))
                    .active(violet().darken(0.16))
                    .shadow(false),
            ),
            Tone::Ghost => Button::new(id).custom(
                ButtonCustomVariant::new(cx)
                    // The card's own colour, so a resting ghost button is a
                    // rectangle you cannot see, and a pointed-at one snaps to
                    // yellow.
                    .color(white())
                    .foreground(ink())
                    .hover(yellow())
                    .active(yellow().darken(0.2))
                    .shadow(false),
            ),
        };

        let (button, stroke, offset) = match control {
            Control::Tiny => (button.with_size(ControlSize::XSmall).px_2(), STROKE_THIN, px(2.)),
            Control::Small => (button.with_size(ControlSize::Small).px_3(), STROKE_THIN, px(3.)),
            // The standing call to action gets a touch target's height and the
            // full stroke: at 20px tall, 4px of border is most of the button.
            Control::Medium => (
                button.with_size(ControlSize::Size(px(48.))).h(px(44.)).px_4(),
                STROKE,
                px(4.),
            ),
        };

        // A `Button` refines its own style last, so these win over the 1px
        // stroke and soft elevation the component would otherwise apply — which
        // is how a library widget ends up wearing this style's stroke instead
        // of the other way round.
        //
        // The press is the one signature this cannot have. `active` lives on
        // `StatefulInteractiveElement`, which `Button` does not implement, and
        // a `Button` has a single hover slot that the component fills itself,
        // so a skin cannot add either state. What the component *does* offer is
        // an active-state fill drawn from the theme, so the press arrives as
        // the block darkening under the pointer — which is why `install` gives
        // every `button_*_active` token a visibly darker value than its hover.
        button
            .rounded_none()
            .font_weight(FontWeight::BOLD)
            .border(stroke)
            .border_color(ink())
            .shadow(hard(offset))
    }
}

#[cfg(test)]
mod tests {
    // Deliberately not `use super::*`: that would pull in GPUI's own `test`
    // attribute macro, which shadows the built-in one and recurses.
    use super::{
        CREAM, GREEN, INK, LIFTED, NeoBrutalism, RED, SHADOW, SkinStyle as _, VIOLET, WHITE, YELLOW, hard, ink,
    };
    use gpui_kit::component::ThemeMode;
    use gpui_kit::px;

    #[test]
    fn ink_blocks_have_no_blur_and_no_spread() {
        // The whole style rests on this. A blurred shadow is the one thing that
        // would make it look like every other interface.
        for offset in [px(2.), px(3.), px(4.), SHADOW, LIFTED] {
            let shadow = hard(offset);
            assert_eq!(shadow.len(), 1);
            assert_eq!(shadow[0].blur_radius, px(0.));
            assert_eq!(shadow[0].spread_radius, px(0.));
            assert_eq!(shadow[0].offset.x, offset);
            assert_eq!(shadow[0].offset.y, offset);
            assert_eq!(shadow[0].color, ink());
        }
    }

    #[test]
    fn the_skin_is_light_only() {
        assert_eq!(NeoBrutalism.modes(), &[ThemeMode::Light]);
    }
    #[test]
    fn the_lifted_card_shadow_is_deeper_than_the_resting_one() {
        // `lift` names its own size rather than reading `SHADOW`, so the pair
        // has to be kept in order deliberately.
        assert!(LIFTED > SHADOW);
        assert!(hard(LIFTED)[0].offset.x > hard(SHADOW)[0].offset.x);
    }

    #[test]
    fn every_palette_colour_is_legible_as_a_block_behind_ink_text() {
        // Every colour here is a fill behind ink text, never text itself, so
        // this is the contrast that has to hold. 7:1 is the AAA body bar and it
        // is the one this style can meet, because black on a highlighter is
        // always a big jump.
        for fill in [RED, YELLOW, VIOLET, WHITE, CREAM, GREEN] {
            let ratio = contrast(fill, INK);
            assert!(ratio >= 7., "#{fill:06X} carries ink at only {ratio:.1}:1");
        }
    }

    #[test]
    fn the_accent_stays_unusable_as_text_on_the_canvas() {
        // Documents the constraint `status_text` exists to work around. If this
        // ever starts passing, the workaround can be reconsidered.
        let ratio = contrast(RED, CREAM);
        assert!(
            ratio < 4.5,
            "red on cream is {ratio:.1}:1, which would pass AA for text"
        );
    }

    /// WCAG relative luminance of a `0xRRGGBB` colour.
    fn luminance(color: u32) -> f32 {
        fn channel(value: u32) -> f32 {
            let c = (value & 0xFF) as f32 / 255.;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let r = channel(color >> 16);
        let g = channel(color >> 8);
        let b = channel(color);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast(a: u32, b: u32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }
}
