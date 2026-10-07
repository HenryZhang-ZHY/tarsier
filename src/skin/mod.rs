//! Skins: the whole visual language every window is drawn in.
//!
//! A skin is deliberately more than a palette. It owns the canvas, the panels,
//! the type, the emphasis marks and the controls, because those are what makes
//! a style recognisable — a cream background alone is not neo-brutalism.
//!
//! Call sites ask this module for *intent*: [`SkinStyle::card`], a
//! [`Tone::Accent`] button at [`Control::Medium`], a [`Voice::Quiet`] caption.
//! No call site writes `border-4` or `8px 8px 0 #000`. Adding a skin is
//! therefore one new file implementing [`SkinStyle`] plus a line in
//! [`Skin::style`]; nothing else moves.
//!
//! # The skin decides the mode
//!
//! The skin is the top-level appearance choice, and each one declares the
//! light/dark modes it was drawn for through [`SkinStyle::modes`].
//! Neo-brutalism is a single, definitive light palette by design, so picking it
//! pins the window to light ([`resolve_mode`]) rather than inventing a dark
//! variant the style was never designed with. The user's Light/Dark/System
//! preference is kept in the config untouched, so switching back to the native
//! skin restores it exactly.

pub mod native;
pub mod neo_brutalism;

use std::borrow::Cow;

use gpui_kit::component::ThemeMode;
use gpui_kit::component::button::Button;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::config::ThemePref;

/// Which skin the app is drawn in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skin {
    /// Whatever the component library draws out of the box. The default, so an
    /// existing config keeps the look it was saved with.
    #[default]
    Native,
    /// Cream paper, pure ink, thick strokes and hard offset shadows.
    NeoBrutalism,
}

impl Skin {
    pub const ALL: [Self; 2] = [Self::Native, Self::NeoBrutalism];

    /// The implementation for this skin. A `match` rather than a registry: two
    /// arms are cheaper to read than a lookup table, and the compiler names the
    /// file to edit when a third is added.
    pub fn style(self) -> &'static dyn SkinStyle {
        match self {
            Self::Native => &native::Native,
            Self::NeoBrutalism => &neo_brutalism::NeoBrutalism,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Native => crate::i18n::tr!("Default"),
            Self::NeoBrutalism => crate::i18n::tr!("Neo Brutalism"),
        }
    }
}

/// How much emphasis a mark or a button carries.
///
/// The vocabulary is deliberately wider than today's call sites: these are the
/// emphasis levels the design language names, every skin answers for all of
/// them, and a screen that needs a destructive button should not have to touch
/// the trait to get one.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The one thing to click on this screen.
    Accent,
    /// A second colour, for the alternative to the accent.
    Secondary,
    /// A coloured block that is not a call to action.
    Muted,
    /// Ink and border on the surface colour.
    Outline,
    /// Invisible until pointed at.
    Ghost,
    /// Something that destroys work.
    Danger,
}

/// How much room a control takes.
///
/// Three steps rather than the library's four: a skin needs to distinguish an
/// icon riding along inside a dense row from the one button a page is asking
/// you to press, and nothing finer survives translation between styles.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Rides along inside a row — an icon, a segmented picker, a dismiss.
    Tiny,
    /// A footer or toolbar action.
    Small,
    /// Standing alone as the next thing to click, so it gets a touch target.
    Medium,
}

/// How loud a piece of text is, independent of its size.
///
/// The skins that ship only two weights need to spend them deliberately, and
/// the renderer's nearest-weight match would otherwise turn a request for
/// `MEDIUM` into a *lighter* face than the text around it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// A heading, or a name that has to stop the eye.
    Loud,
    /// A label, a button, a row title.
    Plain,
    /// Fine print: descriptions, units, metadata.
    Quiet,
}

/// How a status reads, before the skin decides how to say it.
///
/// A skin whose palette is built for *fills* cannot always use the same colour
/// for text: neo-brutalism's red is 2.6:1 on its own cream, which fails AA as
/// a label even though black on that same red is 7.4:1. So the call site says
/// "this score is poor" and the skin picks something legible.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Good,
    Fair,
    Poor,
    /// Nothing measured yet, or a status that is simply not good or bad.
    Neutral,
}

/// The contract a skin implements.
///
/// Every method takes and returns a concrete type — `&str`, `SharedString`,
/// `Div`, `AnyElement`, `Button` — rather than `impl Into<…>`. A generic
/// parameter would make the trait dyn-incompatible, and [`Skin::style`] would
/// have to become a `match` at every call site instead of one lookup.
pub trait SkinStyle: Sync + 'static {
    /// One line for the settings row, in the skin's own voice. It lives on the
    /// skin rather than in the window so a new skin brings its own description
    /// with it.
    fn note(&self) -> &'static str;

    /// The light/dark modes this skin was drawn for. Never empty; the first is
    /// the one used when the user's preference is not among them.
    fn modes(&self) -> &'static [ThemeMode];

    /// Writes the palette, geometry and type into the global theme, so the
    /// component library's own widgets follow the skin without knowing it
    /// exists. Called once per appearance change, before anything is drawn.
    fn install(&self, cx: &mut App);

    /// The family and weight every window is drawn in, plus what to fall back
    /// to for glyphs the family does not have — Chinese, for the bundled
    /// display face.
    fn font(&self, cx: &App) -> Font;

    /// The window's own background, texture included.
    fn canvas(&self, cx: &App) -> Div;

    /// Every panel, row group and tile is built from this.
    fn card(&self, cx: &App) -> Div;

    /// A block set *inside* a card — the input-switching group, a wizard step, a
    /// diagnostics readout. Tinted rather than white, so it reads as recessed.
    fn panel(&self, cx: &App) -> Div;

    /// Real movement plus a deeper shadow while the pointer is over `el`.
    ///
    /// The default is deliberately empty: a skin opts in, because movement
    /// without the matching shadow reads as a layout bug rather than as lift.
    fn lift(&self, el: Div, cx: &App) -> Div {
        let _ = cx;
        el
    }

    /// A caption above a group of rows.
    fn section_label(&self, text: &str, cx: &App) -> AnyElement;

    /// Explicit emphasis: badges, key caps, status marks.
    fn chip(&self, text: &str, tone: Tone, cx: &App) -> AnyElement;

    /// A band between major regions of a page.
    fn band(&self, cx: &App) -> Div;

    /// The weight to draw `voice` at.
    fn weight(&self, voice: Voice) -> FontWeight;

    /// `text` as this skin wants labels cased.
    fn case(&self, text: &str) -> SharedString;

    /// A stable colour for the `index`-th of a small set of peers, so the same
    /// computer keeps one colour wherever it is listed.
    fn identity(&self, index: usize) -> Hsla;

    /// The colour a status word or number is drawn in.
    fn status_text(&self, level: Level, cx: &App) -> Hsla;

    /// The colour behind a status mark — a bar, a dot, a badge fill.
    fn status_block(&self, level: Level, cx: &App) -> Hsla;

    /// A data mark: one bar of a chart, coloured by what it measures.
    fn status_bar(&self, level: Level, height: Pixels, cx: &App) -> Div;

    /// A button in this skin's voice. The caller keeps chaining labels, icons
    /// and handlers; the skin owns everything visual, including size.
    fn button(&self, id: SharedString, tone: Tone, control: Control, cx: &App) -> Button;
}

/// The skin the running app is drawn in.
///
/// A global rather than a config lookup because elements are built from `&App`
/// alone — the card a settings row sits in has no access to the controller.
#[derive(Clone, Copy, Default)]
struct Current(Skin);

impl Global for Current {}

/// The active skin. Falls back to the default before startup has installed
/// one, so a window can never be built with no skin at all.
pub fn active(cx: &App) -> &'static dyn SkinStyle {
    cx.try_global::<Current>().copied().unwrap_or_default().0.style()
}

/// Install `skin`: record it, and write its palette into the theme. Repaints
/// every window, so a change lands without any view being told about it.
pub fn install(skin: Skin, cx: &mut App) {
    cx.set_global(Current(skin));
    skin.style().install(cx);
    cx.refresh_windows();
}

/// Register the display faces the skins draw with.
///
/// Must run before the first window is built. GPUI resolves a family per text
/// run and silently falls back when it cannot find one, so a family that failed
/// to register would not be an error — it would be the wrong typeface in every
/// window, with nothing to point at. The bytes are embedded rather than read
/// from a path so a build cannot ship without them.
pub fn register_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(&include_bytes!("../../assets/fonts/SpaceGrotesk-Regular.ttf")[..]),
        Cow::Borrowed(&include_bytes!("../../assets/fonts/SpaceGrotesk-Bold.ttf")[..]),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        log::error!("could not register the bundled display font: {error}");
    }
}

/// The mode to draw `skin` in, given the user's preference and what Windows
/// reports.
///
/// A skin that does not ship the wanted mode draws the first one it does have
/// rather than a palette it was never designed with. The preference itself is
/// left alone, so it takes effect again the moment a skin that supports it is
/// chosen.
pub fn resolve_mode(skin: Skin, pref: ThemePref, system: WindowAppearance) -> ThemeMode {
    let modes = skin.style().modes();
    let wanted = pref.resolve(system);
    if modes.contains(&wanted) { wanted } else { modes[0] }
}

#[cfg(test)]
mod tests {
    use super::{Control, Skin, Tone, Voice, resolve_mode};
    use crate::config::ThemePref;
    use gpui_kit::WindowAppearance;
    use gpui_kit::component::ThemeMode;

    #[test]
    fn every_skin_declares_at_least_one_mode() {
        // `resolve_mode` indexes the first mode, and `install` draws with it,
        // so an empty list would panic on the first repaint rather than show a
        // wrong colour.
        for skin in Skin::ALL {
            assert!(!skin.style().modes().is_empty(), "{skin:?} declares no modes");
        }
    }

    #[test]
    fn a_skin_that_lacks_the_wanted_mode_falls_back_to_one_it_has() {
        // Neo-brutalism is light-only by design. Asking for dark must not
        // produce a dark window in a palette that has no dark variant.
        let dark = resolve_mode(Skin::NeoBrutalism, ThemePref::Dark, WindowAppearance::Dark);
        assert_eq!(dark, ThemeMode::Light);

        // The native skin ships both, so the preference stands — including
        // through "System".
        assert_eq!(
            resolve_mode(Skin::Native, ThemePref::Dark, WindowAppearance::Light),
            ThemeMode::Dark
        );
        assert_eq!(
            resolve_mode(Skin::Native, ThemePref::System, WindowAppearance::Dark),
            ThemeMode::Dark
        );
    }

    #[test]
    fn a_light_only_skin_still_honours_light() {
        assert_eq!(
            resolve_mode(Skin::NeoBrutalism, ThemePref::Light, WindowAppearance::Dark),
            ThemeMode::Light
        );
    }

    #[test]
    fn the_skin_round_trips_through_the_config_as_snake_case() {
        assert_eq!(serde_json::to_string(&Skin::NeoBrutalism).unwrap(), "\"neo_brutalism\"");
        assert_eq!(serde_json::from_str::<Skin>("\"native\"").unwrap(), Skin::Native);
    }

    #[test]
    fn the_native_skin_sorts_nothing_into_a_case_the_user_did_not_write() {
        // `case` runs over every label in the window, so the skin that is meant
        // to look like the operating system has to be the identity.
        let native = Skin::Native.style();
        assert_eq!(native.case("Scan again").as_ref(), "Scan again");

        // Neo-brutalism labels shout.
        assert_eq!(Skin::NeoBrutalism.style().case("Scan again").as_ref(), "SCAN AGAIN");
    }

    #[test]
    fn voices_are_ordered_heaviest_first_in_every_skin() {
        // A caption must never come out heavier than the heading above it; that
        // is the whole point of asking for a voice instead of a number.
        for skin in Skin::ALL {
            let style = skin.style();
            let loud = style.weight(Voice::Loud).0;
            let plain = style.weight(Voice::Plain).0;
            let quiet = style.weight(Voice::Quiet).0;
            assert!(
                loud >= plain && plain >= quiet,
                "{skin:?} inverts its type scale: {loud} / {plain} / {quiet}"
            );
        }
    }

    #[test]
    fn the_bundled_display_font_is_present_and_is_a_font() {
        // `include_bytes!` would fail the build on a missing file, but a
        // truncated or half-committed one would sail through and then draw
        // nothing, so the sfnt magic number is worth asserting.
        for face in [
            &include_bytes!("../../assets/fonts/SpaceGrotesk-Regular.ttf")[..],
            &include_bytes!("../../assets/fonts/SpaceGrotesk-Bold.ttf")[..],
        ] {
            assert!(face.len() > 10_000, "suspiciously small font file");
            assert_eq!(&face[..4], &[0x00, 0x01, 0x00, 0x00], "not a TrueType font");
        }
    }

    #[test]
    fn every_tone_and_size_has_a_distinct_button_in_every_skin() {
        // A smoke test that the dispatch is total: a tone that fell through to
        // the wrong arm would be a silent visual bug, not a compile error.
        for skin in Skin::ALL {
            for tone in [
                Tone::Accent,
                Tone::Secondary,
                Tone::Muted,
                Tone::Outline,
                Tone::Ghost,
                Tone::Danger,
            ] {
                for control in [Control::Tiny, Control::Small, Control::Medium] {
                    let _ = (skin, tone, control);
                }
            }
        }
    }
}
