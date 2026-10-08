//! Skins: the visual language every window is drawn in.
//!
//! A skin is more than a palette. It owns the surfaces (canvas, cards, inset
//! panels), the marks (chips, key caps, swatches, chart bars), the controls
//! (buttons, segmented pickers, navigation items) and the type weights, because
//! those together are what make a style recognisable.
//!
//! Screens ask for *intent* — "a card", "a primary button", "a stretch that ran long" — and
//! never for a border width or a hex value, so a new skin is one file
//! implementing [`SkinStyle`] plus an arm in [`Skin::style`].
//!
//! # The skin decides the mode
//!
//! Each skin declares the light/dark modes it was drawn for. Neo-brutalism is a
//! single light palette by design, so choosing it pins the window to light
//! ([`resolve_mode`]). The user's Light/Dark/System preference is kept in the
//! config untouched, so switching back restores it.

pub mod native;
pub mod neo_brutalism;

use std::borrow::Cow;

use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ThemeMode, TitleBar};
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::config::ThemePref;
use crate::i18n::tr;

/// Which skin the app is drawn in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skin {
    /// The component library's own look. The default, so a config written
    /// before skins existed opens the way it was saved.
    #[default]
    Native,
    /// Warm paper, black ink, solid strokes and hard offset shadows.
    NeoBrutalism,
}

impl Skin {
    pub const ALL: [Self; 2] = [Self::Native, Self::NeoBrutalism];

    pub fn style(self) -> &'static dyn SkinStyle {
        match self {
            Self::Native => &native::Native,
            Self::NeoBrutalism => &neo_brutalism::NeoBrutalism,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Native => tr!("Default"),
            Self::NeoBrutalism => tr!("Neo Brutalism"),
        }
    }

    /// The skin's name in English, for element ids, which must not change with
    /// the language.
    pub fn key(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::NeoBrutalism => "neo-brutalism",
        }
    }
}

/// How much a button asks to be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The one thing to click in its group.
    Primary,
    /// An ordinary action.
    Default,
    /// An action that should not compete with the content around it.
    Ghost,
}

/// How much room a control takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Rides along inside a dense row.
    Compact,
    /// A toolbar or footer action.
    Regular,
    /// Stands alone as the next thing to do.
    Prominent,
}

/// How loud a piece of text is, independent of its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// A heading, a big number, a name that has to stop the eye.
    Loud,
    /// A label, a row title.
    Plain,
    /// Body copy and fine print.
    Quiet,
}

/// What a mark says about the thing it labels.
///
/// A palette built for *fills* cannot always use the same colour as text (a
/// highlighter red is unreadable on paper), so callers say what they mean and
/// the skin decides whether that is a coloured word or ink on a coloured block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Nothing to say beyond the words; also "no data".
    Plain,
    /// Draws the eye without judging: "recording…", "break in 12 min".
    Accent,
    Good,
    Fair,
    Poor,
}

/// The contract a skin implements.
///
/// Methods take and return concrete types (`Div`, `AnyElement`, `Button`) so
/// the trait stays dyn-compatible and [`Skin::style`] can hand back a
/// `&'static dyn SkinStyle`.
pub trait SkinStyle: Sync + 'static {
    /// One line describing the skin, for the settings row.
    fn note(&self) -> &'static str;

    /// The light/dark modes this skin was drawn for. Never empty; the first is
    /// used when the preferred mode is not among them.
    fn modes(&self) -> &'static [ThemeMode];

    /// Writes the palette and geometry into the global theme, so the component
    /// library's own widgets (sliders, inputs, switches, scrollbars) follow the
    /// skin without knowing it exists. Runs after the mode's base theme loads.
    fn install(&self, cx: &mut App);

    /// The family and resting weight every window is drawn in, with fallbacks
    /// for glyphs the family lacks.
    fn font(&self, cx: &App) -> Font;

    /// The window background behind the scrolling body.
    fn canvas(&self, cx: &App) -> Div;

    /// Colours and rules the window's title bar.
    fn title_bar(&self, bar: TitleBar, cx: &App) -> TitleBar;

    /// A top-level surface: one monitor, one group of settings.
    fn card(&self, cx: &App) -> Div;

    /// A block set inside a card, reading as recessed rather than raised.
    fn inset(&self, cx: &App) -> Div;

    /// A thin rule between the parts of a card.
    fn divider(&self, cx: &App) -> Div;

    /// The small caption above a group of rows.
    fn eyebrow(&self, text: &str, cx: &App) -> AnyElement;

    /// A small label carrying a status or a fact about its neighbour.
    fn chip(&self, text: &str, mark: Mark, cx: &App) -> AnyElement;

    /// One key of a keyboard shortcut.
    fn key_cap(&self, text: &str, cx: &App) -> AnyElement;

    /// A small solid swatch: the colour that identifies a computer, or a legend
    /// entry.
    fn swatch(&self, color: Hsla, cx: &App) -> Div;

    /// The weight to draw `voice` at.
    fn weight(&self, voice: Voice) -> FontWeight;

    /// The colour for the `index`-th of a small set of peers, so one computer
    /// keeps one colour wherever it is listed.
    fn identity(&self, index: usize, cx: &App) -> Hsla;

    /// The fill behind a mark: a chart bar, a badge, a legend swatch.
    fn fill(&self, mark: Mark, cx: &App) -> Hsla;

    /// The colour a status word or number is drawn in.
    fn ink(&self, mark: Mark, cx: &App) -> Hsla;

    /// One stretch on a timeline. The caller places and sizes it.
    fn stretch(&self, mark: Mark, cx: &App) -> Div;

    /// A horizontal progress meter, `fraction` full (clamped to 0..=1).
    fn meter(&self, fraction: f32, mark: Mark, cx: &App) -> Div;

    /// A button. The caller adds the label, icon and handler; the skin owns
    /// everything visual, size included.
    fn button(&self, id: ElementId, tone: Tone, control: Control, cx: &App) -> Button;

    /// Greys `button` out when `disabled`. A hook rather than a plain
    /// `.disabled()` because a skin's own styling is replayed over the
    /// library's disabled look, so a skin that draws shadows has to remove them
    /// here or they show through the faded fill.
    fn disable(&self, button: Button, disabled: bool) -> Button {
        button.disabled(disabled)
    }

    /// The well a segmented picker's options sit in.
    fn segmented(&self, cx: &App) -> Div;

    /// One option of a segmented picker.
    fn segment(&self, id: ElementId, selected: bool, cx: &App) -> Button;

    /// One entry of a vertical navigation list, its label aligned to the start.
    fn nav_item(&self, id: ElementId, label: SharedString, selected: bool, cx: &App) -> Button;
}

/// The skin the running app is drawn in, as a global: elements are built from
/// `&App` alone and cannot reach the controller.
#[derive(Clone, Copy, Default)]
struct Current(Skin);

impl Global for Current {}

/// The active skin, or the default before startup has installed one.
pub fn active(cx: &App) -> &'static dyn SkinStyle {
    current(cx).style()
}

pub fn current(cx: &App) -> Skin {
    cx.try_global::<Current>().copied().unwrap_or_default().0
}

/// Records `skin` and writes its palette into the theme. The theme for the
/// resolved mode must already be loaded; see `controller::apply_theme`.
pub fn install(skin: Skin, cx: &mut App) {
    cx.set_global(Current(skin));
    skin.style().install(cx);
    cx.refresh_windows();
}

/// Registers the display faces the skins draw with. Must run before the first
/// window: GPUI falls back to another family in silence when one is missing.
pub fn register_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(neo_brutalism::FONT_REGULAR),
        Cow::Borrowed(neo_brutalism::FONT_BOLD),
    ];
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        log::error!("could not register the bundled display font: {error}");
    }
}

/// The mode to draw `skin` in, given the preference and what Windows reports.
pub fn resolve_mode(skin: Skin, pref: ThemePref, system: WindowAppearance) -> ThemeMode {
    let modes = skin.style().modes();
    let wanted = pref.resolve(system);
    if modes.contains(&wanted) { wanted } else { modes[0] }
}

/// WCAG contrast ratio between two colours, for the palette tests.
#[cfg(test)]
pub(crate) fn contrast(a: Hsla, b: Hsla) -> f32 {
    fn luminance(color: Hsla) -> f32 {
        let rgb = color.to_rgb();
        let channel = |c: f32| {
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
    }
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::{Skin, Voice, resolve_mode};
    use crate::config::ThemePref;
    use gpui_kit::WindowAppearance;
    use gpui_kit::component::ThemeMode;

    #[test]
    fn a_skin_without_the_wanted_mode_draws_one_it_has() {
        assert_eq!(
            resolve_mode(Skin::NeoBrutalism, ThemePref::Dark, WindowAppearance::Dark),
            ThemeMode::Light
        );
        assert_eq!(
            resolve_mode(Skin::NeoBrutalism, ThemePref::System, WindowAppearance::Dark),
            ThemeMode::Light
        );
        // The native skin ships both, so the preference stands, "System" included.
        assert_eq!(
            resolve_mode(Skin::Native, ThemePref::Dark, WindowAppearance::Light),
            ThemeMode::Dark
        );
        assert_eq!(
            resolve_mode(Skin::Native, ThemePref::System, WindowAppearance::Dark),
            ThemeMode::Dark
        );
        assert_eq!(
            resolve_mode(Skin::Native, ThemePref::System, WindowAppearance::Light),
            ThemeMode::Light
        );
    }

    #[test]
    fn every_skin_orders_its_type_weights_loudest_first() {
        for skin in Skin::ALL {
            let style = skin.style();
            let [loud, plain, quiet] = [Voice::Loud, Voice::Plain, Voice::Quiet].map(|v| style.weight(v).0);
            assert!(
                loud >= plain && plain >= quiet,
                "{skin:?} inverts its type scale: {loud} / {plain} / {quiet}"
            );
        }
    }

    #[test]
    fn skin_ids_are_distinct() {
        let mut keys: Vec<_> = Skin::ALL.iter().map(|s| s.key()).collect();
        keys.dedup();
        assert_eq!(keys.len(), Skin::ALL.len());
    }
}
