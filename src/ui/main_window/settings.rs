//! The Settings tab: a list of sections down the left, the chosen one beside it.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::controls::BREAK_FIELDS;
use super::{MainWindow, Section};
use crate::config::{self, ThemePref};
use crate::i18n::{Language, tr, translate};
use crate::skin::{Control, Skin, Tone};
use crate::ui::kit::{self, Choice, skin};

impl Section {
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Breaks,
        Self::Displays,
        Self::Hotkeys,
        Self::Advanced,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::General => tr!("General"),
            Self::Breaks => tr!("Breaks"),
            Self::Displays => tr!("Displays"),
            Self::Hotkeys => tr!("Hotkeys"),
            Self::Advanced => tr!("Advanced"),
        }
    }

    /// Stable across languages, for element ids.
    pub fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Breaks => "breaks",
            Self::Displays => "displays",
            Self::Hotkeys => "hotkeys",
            Self::Advanced => "advanced",
        }
    }
}

/// How wide the section list is. The section names are short on purpose.
const NAV_WIDTH: Pixels = px(148.);

impl MainWindow {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let nav = v_flex()
            .w(NAV_WIDTH)
            .flex_none()
            .gap_1()
            .children(Section::ALL.map(|section| {
                skin(cx)
                    .nav_item(
                        SharedString::from(format!("section-{}", section.key())).into(),
                        section.label().into(),
                        section == self.section,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_section(section, cx)))
            }));

        let panel = match self.section {
            Section::General => self.render_general_settings(cx),
            Section::Breaks => self.render_break_settings(cx),
            Section::Displays => self.render_display_settings(cx),
            Section::Hotkeys => self.render_hotkey_settings(cx),
            Section::Advanced => self.render_advanced_settings(cx),
        };

        h_flex()
            .gap_5()
            .items_start()
            .child(nav)
            .child(v_flex().flex_1().min_w_0().child(panel))
            .into_any_element()
    }

    fn render_general_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let config = &c.config;

        let pick_skin = {
            let controller = self.controller.clone();
            kit::segmented(
                "skin",
                Skin::ALL.map(|skin| Choice {
                    value: skin,
                    key: skin.key(),
                    label: skin.label().into(),
                }),
                config.skin,
                true,
                move |skin, _, cx| controller.update(cx, |c, cx| c.set_skin(skin, cx)),
                cx,
            )
        };

        // A skin may ship only one mode; the picker then shows the saved choice
        // greyed out and says why, rather than offering buttons that do nothing.
        let fixed_mode = config.skin.style().modes().len() == 1;
        let pick_theme = {
            let controller = self.controller.clone();
            kit::segmented(
                "theme",
                ThemePref::ALL.map(|pref| Choice {
                    value: pref,
                    key: pref.key(),
                    label: pref.label().into(),
                }),
                config.theme,
                !fixed_mode,
                move |pref, _, cx| controller.update(cx, |c, cx| c.set_theme(pref, cx)),
                cx,
            )
        };

        let pick_language = {
            let controller = self.controller.clone();
            kit::segmented(
                "language",
                // A language names itself in its own language: whoever picked
                // the wrong one still has to find the way back.
                Language::ALL.map(|lang| Choice {
                    value: lang,
                    key: lang.key(),
                    label: lang.label().into(),
                }),
                config.language,
                true,
                move |lang, _, cx| controller.update(cx, |c, cx| c.set_language(lang, cx)),
                cx,
            )
        };

        let controller = self.controller.clone();
        skin(cx)
            .card(cx)
            .gap_4()
            .child(skin(cx).eyebrow(tr!("Appearance"), cx))
            .child(kit::setting_row(
                tr!("Skin"),
                Some(config.skin.style().note().into()),
                pick_skin,
                cx,
            ))
            .child(kit::setting_row(
                tr!("Light or dark"),
                Some(if fixed_mode {
                    tr!("This skin is drawn in light only. Your choice comes back with the default skin.").into()
                } else {
                    tr!("System follows the Windows setting as it changes.").into()
                }),
                pick_theme,
                cx,
            ))
            .child(kit::setting_row(
                tr!("Language"),
                Some(tr!("Every window switches right away.").into()),
                pick_language,
                cx,
            ))
            .child(skin(cx).divider(cx))
            .child(kit::toggle_row(
                "autostart",
                tr!("Start at login"),
                tr!("Runs quietly in the tray after you sign in to Windows."),
                c.autostart,
                move |on, cx| controller.update(cx, |c, cx| c.set_autostart(on, cx)),
                cx,
            ))
            .into_any_element()
    }

    fn render_break_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let b = &self.controller.read(cx).config.breaks;
        let (enable, fullscreen) = (self.controller.clone(), self.controller.clone());

        let durations = BREAK_FIELDS.iter().map(|field| {
            kit::setting_row(
                translate(field.label),
                None,
                div().w(px(140.)).children(
                    self.controls
                        .duration(field.key)
                        .map(|number| number.input().suffix(div().pr_1().child(kit::hint(tr!("min"), cx)))),
                ),
                cx,
            )
        });

        let reminders = skin(cx)
            .card(cx)
            .gap_4()
            .child(skin(cx).eyebrow(tr!("Break reminders"), cx))
            .child(kit::toggle_row(
                "breaks-enabled",
                tr!("Remind me to take breaks"),
                tr!("After each stretch of work, a reminder fades in over every screen. It never blocks the keyboard or mouse."),
                b.enabled,
                move |on, cx| enable.update(cx, |c, cx| c.update_config(cx, |cfg| cfg.breaks.enabled = on)),
                cx,
            ))
            .child(kit::toggle_row(
                "respect-fullscreen",
                tr!("Stay quiet in fullscreen"),
                tr!("Holds reminders while you are gaming, watching video or presenting."),
                b.respect_fullscreen,
                move |on, cx| {
                    fullscreen.update(cx, |c, cx| c.update_config(cx, |cfg| cfg.breaks.respect_fullscreen = on))
                },
                cx,
            ))
            .child(skin(cx).divider(cx))
            .children(durations);

        v_flex()
            .gap_5()
            .child(reminders)
            .child(self.render_evening_settings(cx))
            .into_any_element()
    }

    fn render_evening_settings(&self, cx: &mut Context<Self>) -> Div {
        let evening = &self.controller.read(cx).config.evening;
        let enable = self.controller.clone();
        skin(cx)
            .card(cx)
            .gap_4()
            .child(skin(cx).eyebrow(tr!("Evening cutoff"), cx))
            .child(kit::toggle_row(
                "evening-enabled",
                tr!("Stop using the computer in the evening"),
                tr!("From the time you set until 05:00, an overlay over every screen says the day is done. You can always carry on."),
                evening.enabled,
                move |on, cx| enable.update(cx, |c, cx| c.update_config(cx, |cfg| cfg.evening.enabled = on)),
                cx,
            ))
            .child(kit::setting_row(
                tr!("No computer after"),
                Some(
                    tr!("Sleep doctors suggest putting screens away 30 to 60 minutes before bed. Try an hour before you usually go to bed.")
                        .into(),
                ),
                div().id("evening-cutoff").test_support().w(px(140.)).child(self.controls.cutoff().input()),
                cx,
            ))
    }

    fn render_advanced_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let controller = self.controller.clone();
        skin(cx)
            .card(cx)
            .gap_4()
            .child(skin(cx).eyebrow(tr!("Advanced"), cx))
            .child(kit::toggle_row(
                "developer-mode",
                tr!("Developer mode"),
                tr!("Shows DDC/CI diagnostics and recent commands on the Monitors tab, and logs every command."),
                c.config.developer_mode,
                move |on, cx| controller.update(cx, |c, cx| c.set_developer_mode(on, cx)),
                cx,
            ))
            .child(kit::setting_row(
                tr!("Settings folder"),
                Some(config::data_dir().display().to_string().into()),
                kit::button("open-config", Tone::Default, Control::Regular, cx)
                    .label(tr!("Open"))
                    .on_click(|_, _, cx| {
                        let dir = config::data_dir();
                        if let Err(e) = std::fs::create_dir_all(&dir) {
                            log::warn!("creating {}: {e}", dir.display());
                        }
                        let config = config::config_path();
                        cx.reveal_path(if config.exists() { &config } else { &dir });
                    }),
                cx,
            ))
            .child(skin(cx).divider(cx))
            .child(kit::hint(format!("tarsier {}", env!("CARGO_PKG_VERSION")), cx))
            .into_any_element()
    }
}
