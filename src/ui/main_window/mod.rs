//! The main window: a title bar with the page tabs, and one scrolling page.
//!
//! - [`monitors`]: brightness, contrast and input switching.
//! - [`breaks`]: the break timer and the scores it produces.
//! - [`settings`]: everything configurable, one section at a time; the
//!   per-monitor computer setup lives in [`displays`].
//!
//! [`controls`] keeps the stateful widgets (sliders, number fields, name
//! fields) in step with the controller, and [`hotkeys`] records shortcuts.

mod breaks;
mod controls;
mod displays;
mod hotkeys;
mod monitors;
mod settings;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{ActiveTheme as _, IconName, TitleBar, h_flex, v_flex};
use gpui_kit::*;

use self::controls::Controls;
use self::displays::Setup;
use self::hotkeys::Recorder;
use crate::controller::Controller;
use crate::i18n::tr;
use crate::skin::{Control, Tone, Voice};
use crate::ui::kit::{self, Choice};

/// The title bar's height; the tab strip is centred inside it.
const TITLE_BAR_HEIGHT: Pixels = px(44.);
/// The page column stops growing here, so a maximised window does not stretch a
/// slider across a whole screen.
const CONTENT_MAX_WIDTH: Pixels = px(880.);

pub fn open(controller: Entity<Controller>, cx: &mut App) -> Option<AnyWindowHandle> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(760.), px(720.)), cx)),
        window_min_size: Some(size(px(640.), px(480.))),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| MainWindow::new(controller, window, cx))
    })
    .map(|(handle, _)| handle)
    .inspect_err(|e| log::error!("failed to open main window: {e}"))
    .ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Monitors,
    Breaks,
    Settings,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Monitors, Self::Breaks, Self::Settings];

    fn key(self) -> &'static str {
        match self {
            Self::Monitors => "monitors",
            Self::Breaks => "breaks",
            Self::Settings => "settings",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Monitors => tr!("Monitors"),
            Self::Breaks => tr!("Breaks"),
            Self::Settings => tr!("Settings"),
        }
    }
}

/// A section of the Settings tab, picked from the list down its left side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Breaks,
    Displays,
    Hotkeys,
    Advanced,
}

pub struct MainWindow {
    controller: Entity<Controller>,
    tab: Tab,
    /// Kept while the Settings tab is left and come back to.
    section: Section,
    /// One scroll position for the page on screen; reset whenever the page
    /// changes, because an offset is only meaningful for the page it was
    /// scrolled on.
    scroll: ScrollHandle,
    controls: Controls,
    /// The first-run flow, per monitor id, for monitors not set up yet.
    setup: HashMap<String, Setup>,
    /// The endpoint whose port chooser is open in the Settings editor, if any.
    port_picker: Option<String>,
    recorder: Recorder,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(controller: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe_in(&controller, window, |this, _, window, cx| {
            this.sync(window, cx);
            cx.notify();
        });
        // "System" follows Windows as it changes; an explicit choice resolves to
        // the same mode and costs nothing but a repaint.
        let appearance = cx.observe_window_appearance(window, |this, _, cx| {
            let config = &this.controller.read(cx).config;
            if config.theme == crate::config::ThemePref::System {
                crate::controller::apply_theme(config.skin, config.theme, cx);
            }
        });
        let recorder = Recorder::new(cx);
        let mut subscriptions = vec![observe, appearance];
        subscriptions.extend(Recorder::subscribe(&recorder, window, cx));

        let mut this = Self {
            controls: Controls::new(&controller, window, cx),
            controller,
            tab: Tab::Monitors,
            section: Section::General,
            scroll: ScrollHandle::new(),
            setup: HashMap::new(),
            port_picker: None,
            recorder,
            _subscriptions: subscriptions,
        };
        this.sync(window, cx);
        this
    }

    /// Brings the wizard state and every stateful widget in line with the
    /// controller.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        displays::sync_setup(&mut self.setup, self.controller.read(cx));
        let controller = self.controller.clone();
        self.controls.sync(&self.setup, &controller, window, cx);
    }

    pub fn show_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        // Leaving the page is leaving a recording, if one was armed.
        self.cancel_recording(cx);
        if self.tab != tab {
            self.tab = tab;
            self.scroll_to_top();
        }
        cx.notify();
    }

    /// Goes to a Settings section from wherever the request came from — "set up
    /// on the Settings tab" has to land on the section it means.
    pub fn open_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.cancel_recording(cx);
        if self.tab != Tab::Settings || self.section != section {
            self.tab = Tab::Settings;
            self.section = section;
            self.scroll_to_top();
        }
        cx.notify();
    }

    fn scroll_to_top(&self) {
        self.scroll.set_offset(point(px(0.), px(0.)));
    }

    fn render_title_bar(&self, cx: &App) -> TitleBar {
        let skin = kit::skin(cx);
        skin.title_bar(TitleBar::new().h(TITLE_BAR_HEIGHT).pl_4(), cx).child(
            h_flex()
                .gap_2()
                .items_center()
                .child(img(brand_icon()).size(px(20.)))
                .child(div().text_sm().font_weight(skin.weight(Voice::Loud)).child("tarsier")),
        )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> Div {
        let view = cx.entity().downgrade();
        kit::segmented(
            "tab",
            Tab::ALL.map(|tab| Choice {
                value: tab,
                key: tab.key(),
                label: tab.label().into(),
            }),
            self.tab,
            true,
            move |tab, _, cx| {
                view.update(cx, |this, cx| this.show_tab(tab, cx)).ok();
            },
            cx,
        )
    }

    fn render_notice(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let text = self.controller.read(cx).notice.clone()?;
        let controller = self.controller.clone();
        let skin = kit::skin(cx);
        Some(
            v_flex().flex_none().child(skin.divider(cx)).child(
                h_flex()
                    .id("notice")
                    .px_5()
                    .py_2()
                    .gap_3()
                    .items_center()
                    .bg(cx.theme().muted)
                    .child(div().flex_1().min_w_0().child(kit::body(text, cx)))
                    .child(
                        kit::button("dismiss-notice", Tone::Ghost, Control::Compact, cx)
                            .icon(IconName::Close)
                            .on_click(move |_, _, cx| controller.update(cx, |c, cx| c.dismiss_notice(cx))),
                    ),
            ),
        )
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let skin = kit::skin(cx);
        let page = match self.tab {
            Tab::Monitors => self.render_monitors(cx),
            Tab::Breaks => self.render_breaks(cx),
            Tab::Settings => self.render_settings(cx),
        };

        v_flex()
            .relative()
            .size_full()
            .font(skin.font(cx))
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                // The scroll area and its scrollbar are siblings in a positioned
                // box, so the bar stays put while the page moves under it.
                skin.canvas(cx)
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("body")
                            .test_support()
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .child(
                                div()
                                    .id("page")
                                    .test_support()
                                    .w_full()
                                    .max_w(CONTENT_MAX_WIDTH)
                                    .mx_auto()
                                    .px_5()
                                    .pt_5()
                                    // Room for the cards' hard shadows at the bottom.
                                    .pb_8()
                                    .child(page),
                            ),
                    )
                    .child(div().absolute().inset_0().child(Scrollbar::vertical(&self.scroll))),
            )
            .children(self.render_notice(cx))
            // The tabs are laid over the title bar rather than placed inside it,
            // so they are centred on the window itself: the bar's own row stops
            // short of the caption buttons, and centring in it would sit the
            // strip half their width to the left. Nothing here takes the pointer
            // except the tabs, so dragging and the caption buttons still work.
            //
            // The strip occludes what is under it. Otherwise the title bar's drag
            // area is still in the hit test there, Windows answers HTCAPTION, and
            // a left click starts a window move instead of reaching the tab.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(TITLE_BAR_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.render_tabs(cx).id("tabs").occlude()),
            )
    }
}

/// The app icon, decoded once so GPUI's image cache keeps hitting the same id.
fn brand_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../assets/icon.png").to_vec(),
        ))
    })
    .clone()
}
