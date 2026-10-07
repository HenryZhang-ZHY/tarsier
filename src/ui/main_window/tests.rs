//! The main window, driven in a headless GPUI window: real layout, real clicks
//! and keystrokes, no pixels.
//!
//! These pin down what users actually see go wrong — a tab strip off centre, a
//! control pushed past the window's edge, a page that will not scroll to its
//! end or opens half scrolled — in both skins and at the narrowest window the
//! app allows.

use gpui_kit::base::test_support;
use gpui_kit::component::TitleBar;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, InputEvent as _, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};

use super::{MainWindow, Section, Tab};
use crate::config::{Config, MonitorPrefs, ThemePref};
use crate::controller::{Controller, MonitorEntry};
use crate::i18n::Language;
use crate::skin::Skin;

/// The narrowest the window can be made, and the size it opens at.
const NARROW: (f32, f32) = (640., 480.);
const DEFAULT: (f32, f32) = (760., 720.);

struct Ui {
    handle: AnyWindowHandle,
    view: Entity<MainWindow>,
    controller: Entity<Controller>,
}

/// Monitor "a" is shared by three computers and set up; monitor "b" reports
/// three inputs and has not been through setup.
fn monitors() -> (Vec<MonitorEntry>, Config) {
    let mut config = Config::default();
    config.monitors.insert(
        "a".into(),
        MonitorPrefs {
            endpoints: vec![0x0F, 0x11, 0x12],
            local_input: Some(0x11),
            input_names: [(0x0F, "Work laptop"), (0x11, "Desktop"), (0x12, "Gaming PC")]
                .into_iter()
                .map(|(p, n)| (p, n.to_string()))
                .collect(),
            ..Default::default()
        },
    );
    let monitors = vec![
        MonitorEntry::fake("a", "DELL U2723QE", &[0x0F, 0x11, 0x12]),
        MonitorEntry::fake("b", "LG 27GP950", &[0x0F, 0x10, 0x11]),
    ];
    (monitors, config)
}

fn open(cx: &mut TestAppContext, config: Config, monitors: Vec<MonitorEntry>, (w, h): (f32, f32)) -> Ui {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::skin::register_fonts(cx);
    });
    let (skin, theme) = (config.skin, config.theme);
    let controller = cx.update(|cx| Controller::for_test(config, monitors, cx));
    cx.update(|cx| crate::controller::apply_theme(skin, theme, cx));
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(w), px(h)),
                })),
                ..TitleBar::window_options()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(controller.clone(), window, cx)),
        )
        .expect("open the main window")
    });
    Ui {
        handle,
        view,
        controller,
    }
}

fn open_with(cx: &mut TestAppContext, skin: Skin, language: Language, window: (f32, f32)) -> Ui {
    let (monitors, mut config) = monitors();
    config.skin = skin;
    config.theme = ThemePref::Light;
    config.language = language;
    open(cx, config, monitors, window)
}

impl Ui {
    /// Runs `f` on a freshly drawn frame.
    fn frame<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            f(window, cx)
        })
        .expect("window is open")
    }

    fn go(&self, cx: &mut TestAppContext, tab: Tab, section: Option<Section>) {
        cx.update_window(self.handle, |_, _, cx| {
            self.view.update(cx, |view, cx| match section {
                Some(section) => view.open_section(section, cx),
                None => view.show_tab(tab, cx),
            })
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn config(&self, cx: &mut TestAppContext) -> Config {
        cx.update(|cx| self.controller.read(cx).config.clone())
    }
}

/// Scrolls the page until `id` is on screen, then clicks it, as a person would.
fn click_into_view(window: &mut Window, id: &'static str, cx: &mut App) {
    let body = window.find("body").bounds();
    let target = window.find(id).bounds();
    let off = target.center().y - body.center().y;
    window.scroll("body", ScrollDelta::Pixels(point(px(0.), -off)), cx);
    window.click(id, cx);
}

/// Every page the window can show.
fn places() -> Vec<(Tab, Option<Section>)> {
    let mut places = vec![(Tab::Monitors, None), (Tab::Breaks, None)];
    places.extend(Section::ALL.map(|s| (Tab::Settings, Some(s))));
    places
}

#[gpui_kit::test]
fn the_tab_strip_is_centred_on_the_window(cx: &mut TestAppContext) {
    for skin in Skin::ALL {
        for language in Language::ALL {
            for window in [NARROW, DEFAULT, (1280., 800.)] {
                let ui = open_with(cx, skin, language, window);
                ui.frame(cx, |window, _| {
                    let first = window.find("tab-monitors").bounds();
                    let last = window.find("tab-settings").bounds();
                    let centre = (first.left() + last.right()) / 2.;
                    let middle = window.viewport_size().width / 2.;
                    assert!(
                        (centre - middle).abs() < px(0.5),
                        "{skin:?}/{language:?} at {window:?}: strip centred at {centre:?}, window at {middle:?}",
                        window = window.viewport_size(),
                    );
                    // And it sits inside the title bar, not under it.
                    assert!(first.top() >= px(0.) && first.bottom() <= super::TITLE_BAR_HEIGHT);
                });
            }
        }
    }
}

#[gpui_kit::test]
fn nothing_runs_off_the_right_edge_of_any_page(cx: &mut TestAppContext) {
    for skin in Skin::ALL {
        for language in Language::ALL {
            let ui = open_with(cx, skin, language, NARROW);
            for (tab, section) in places() {
                ui.go(cx, tab, section);
                ui.frame(cx, |window, _| {
                    let width = window.viewport_size().width;
                    for element in test_support::snapshots(window) {
                        let bounds = element.bounds();
                        assert!(
                            bounds.right() <= width + px(0.5) && bounds.left() >= px(-0.5),
                            "{skin:?}/{language:?} {tab:?}/{section:?}: {:?} spans {:?}..{:?} in a {width:?} window",
                            element.path().last(),
                            bounds.left(),
                            bounds.right(),
                        );
                    }
                });
            }
        }
    }
}

#[gpui_kit::test]
fn every_page_opens_at_its_top(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::NeoBrutalism, Language::En, NARROW);
    for (tab, section) in places() {
        // Scroll a long page as far as it goes, then move on: the next page
        // must not inherit the offset.
        let long = if tab == Tab::Breaks { Tab::Monitors } else { Tab::Breaks };
        ui.go(cx, long, None);
        ui.frame(cx, |window, cx| {
            window.scroll("body", ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
            assert!(window.find("page").bounds().top() < window.find("body").bounds().top());
        });
        ui.go(cx, tab, section);
        ui.frame(cx, |window, _| {
            let body = window.find("body").bounds();
            let page = window.find("page").bounds();
            assert_eq!(page.top(), body.top(), "{tab:?}/{section:?} opened scrolled");
        });
    }
}

#[gpui_kit::test]
fn a_long_page_scrolls_to_its_very_end(cx: &mut TestAppContext) {
    for skin in Skin::ALL {
        let ui = open_with(cx, skin, Language::En, NARROW);
        for (tab, section) in [
            (Tab::Monitors, None),
            (Tab::Breaks, None),
            (Tab::Settings, Some(Section::Displays)),
        ] {
            ui.go(cx, tab, section);
            ui.frame(cx, |window, cx| {
                let body = window.find("body").bounds();
                let page = window.find("page").bounds();
                assert!(
                    page.size.height > body.size.height,
                    "{skin:?} {tab:?}: the fixture should be taller than the window"
                );
                window.scroll("body", ScrollDelta::Pixels(point(px(0.), px(-10_000.))), cx);
                let page = window.find("page").bounds();
                assert!(
                    (page.bottom() - body.bottom()).abs() < px(0.5),
                    "{skin:?} {tab:?}: scrolled to {:?}, the body ends at {:?}",
                    page.bottom(),
                    body.bottom()
                );
            });
        }
    }
}

#[gpui_kit::test]
fn the_wheel_scrolls_the_page_wherever_the_pointer_is(cx: &mut TestAppContext) {
    // A slider or a text field under the pointer must not swallow the wheel,
    // and must not take it as a change of value either.
    let ui = open_with(cx, Skin::Native, Language::En, NARROW);
    let targets: Vec<Bounds<gpui_kit::Pixels>> = ui.frame(cx, |window, _| {
        let body = window.find("body").bounds();
        test_support::snapshots(window)
            .into_iter()
            .filter(|s| {
                let path = format!("{:?}", s.path().last());
                (path.contains("\"slider\"") || path.contains("\"input\""))
                    && s.visible()
                    && body.contains(&s.bounds().center())
            })
            .map(|s| s.bounds())
            .take(2)
            .collect()
    });
    assert_eq!(targets.len(), 2, "the fixture shows a slider and a number field");
    for target in targets {
        let moved = ui.frame(cx, |window, cx| {
            let before = window.find("page").bounds().top();
            window.dispatch_event(
                ScrollWheelEvent {
                    position: target.center(),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-20.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            before - window.find("page").bounds().top()
        });
        assert!(moved > px(0.), "the page did not move under a wheel over {target:?}");
    }
    let brightness = cx.update(|cx| ui.controller.read(cx).monitors[0].brightness.unwrap().current);
    assert_eq!(brightness, 50, "scrolling past a slider changed the brightness");
}

#[gpui_kit::test]
fn the_tabs_are_real_buttons(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::NeoBrutalism, Language::En, DEFAULT);
    ui.frame(cx, |window, cx| {
        assert!(window.try_find("rescan").is_some(), "the window opens on the monitors");
        window.click("tab-settings", cx);
        assert!(
            window.try_find("section-general").is_some(),
            "the settings page is showing"
        );
        assert!(window.try_find("rescan").is_none());
        window.click("section-hotkeys", cx);
        assert!(window.try_find("record-toggle_input").is_some());
        window.click("tab-breaks", cx);
        assert!(window.try_find("break-now").is_some());
    });
}

#[gpui_kit::test]
fn a_switch_button_puts_the_monitor_on_that_computer(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::Native, Language::En, DEFAULT);
    ui.frame(cx, |window, cx| window.click("switch-0-2", cx));
    cx.run_until_parked();
    let current = cx.update(|cx| ui.controller.read(cx).monitors[0].current_input);
    assert_eq!(current, Some(0x12));
}

#[gpui_kit::test]
fn setting_up_a_monitor_from_scratch(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::NeoBrutalism, Language::En, DEFAULT);
    ui.go(cx, Tab::Settings, Some(Section::Displays));
    ui.frame(cx, |window, cx| {
        // Three reported inputs is not three computers, so the flow asks. The
        // input the monitor is showing is assumed to be this computer.
        click_into_view(window, "wizard-next-1", cx);
        assert!(
            window.try_find("wizard-done-1").is_none(),
            "one pick is not enough to go on"
        );
        click_into_view(window, "pick-1-16", cx);
        click_into_view(window, "wizard-next-1", cx);
        click_into_view(window, "suggest-1-1-Laptop", cx);
        click_into_view(window, "wizard-done-1", cx);
    });
    cx.run_until_parked();

    let prefs = ui.config(cx).monitors["b"].clone();
    assert_eq!(prefs.endpoints, vec![0x0F, 0x10]);
    assert_eq!(prefs.local_input, Some(0x0F));
    assert_eq!(prefs.label(0x10), "Laptop");
    assert_eq!(
        prefs.label(0x0F),
        crate::platform::local_hostname(),
        "this computer is named after itself"
    );
    // Set up, the monitor gets the editor instead of the flow.
    ui.frame(cx, |window, _| {
        assert!(window.try_find("wizard-done-1").is_none());
        assert!(window.try_find("port-1-0").is_some());
    });
}

#[gpui_kit::test]
fn recording_a_hotkey(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::Native, Language::En, DEFAULT);
    ui.go(cx, Tab::Settings, Some(Section::Hotkeys));
    ui.frame(cx, |window, cx| {
        window.click("record-break_now", cx);
        assert!(window.try_find("cancel-break_now").is_some(), "the row is waiting");
        // A modifier on its own is not a combination yet.
        window.press("ctrl", cx);
        assert!(window.try_find("cancel-break_now").is_some());
        window.press("ctrl-alt-k", cx);
    });
    assert_eq!(ui.config(cx).hotkeys.break_now, "ctrl+alt+K");

    // Escape gives up without touching the binding.
    ui.frame(cx, |window, cx| {
        window.click("record-break_now", cx);
        window.press("escape", cx);
        assert!(window.try_find("record-break_now").is_some());
    });
    assert_eq!(ui.config(cx).hotkeys.break_now, "ctrl+alt+K");

    // Taking another action's combination takes it away from that action.
    ui.frame(cx, |window, cx| {
        window.click("record-toggle_input", cx);
        window.press("ctrl-alt-k", cx);
    });
    let hotkeys = ui.config(cx).hotkeys;
    assert_eq!(hotkeys.toggle_input, "ctrl+alt+K");
    assert_eq!(hotkeys.break_now, "");
}

#[gpui_kit::test]
fn leaving_the_page_stops_a_recording(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::Native, Language::En, DEFAULT);
    ui.go(cx, Tab::Settings, Some(Section::Hotkeys));
    ui.frame(cx, |window, cx| {
        window.click("record-break_now", cx);
        window.click("tab-monitors", cx);
        window.click("tab-settings", cx);
        assert!(
            window.try_find("record-break_now").is_some(),
            "the row is no longer waiting"
        );
    });
}

#[gpui_kit::test]
fn turning_reminders_off_greys_out_the_break_actions(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::NeoBrutalism, Language::En, DEFAULT);
    ui.go(cx, Tab::Settings, Some(Section::Breaks));
    ui.frame(cx, |window, cx| window.click("breaks-enabled", cx));
    assert!(!ui.config(cx).breaks.enabled);
    ui.go(cx, Tab::Breaks, None);
    ui.frame(cx, |window, cx| {
        window.click("break-now", cx);
        window.click("pause", cx);
    });
    cx.update(|cx| {
        let c = ui.controller.read(cx);
        assert_eq!(c.tracker.phase(), crate::breaks::Phase::Working, "no break was started");
        assert!(!c.is_paused(), "nothing was paused");
    });
}

#[gpui_kit::test]
fn a_light_only_skin_greys_out_the_mode_picker(cx: &mut TestAppContext) {
    let ui = open_with(cx, Skin::Native, Language::En, DEFAULT);
    ui.go(cx, Tab::Settings, Some(Section::General));
    ui.frame(cx, |window, cx| window.click("theme-system", cx));
    assert_eq!(ui.config(cx).theme, ThemePref::System);
    ui.frame(cx, |window, cx| {
        window.click("skin-neo-brutalism", cx);
        // Light only: the mode buttons stay on screen, greyed out, and do nothing.
        window.click("theme-dark", cx);
    });
    assert_eq!(ui.config(cx).skin, Skin::NeoBrutalism);
    assert_eq!(ui.config(cx).theme, ThemePref::System, "the saved preference is kept");
    assert_eq!(cx.update(|cx| crate::skin::current(cx)), Skin::NeoBrutalism);
}

#[gpui_kit::test]
fn an_empty_machine_says_what_to_do(cx: &mut TestAppContext) {
    let ui = open(cx, Config::default(), Vec::new(), DEFAULT);
    ui.frame(cx, |window, _| {
        assert!(window.try_find("rescan").is_some());
        assert!(window.try_find("monitor-0").is_none());
    });
    ui.go(cx, Tab::Settings, Some(Section::Displays));
    ui.frame(cx, |window, _| assert!(window.try_find("export-switching").is_some()));
}
