//! Recording global hotkeys on the Settings tab.
//!
//! Click Record, press the combination, and it is written to the config and
//! registered at once. While a row waits, the global hotkeys are released —
//! otherwise pressing the combination being recorded would also run it — and
//! anything that ends the wait (a key, Escape, clicking away, leaving the
//! window) hands them back.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::MainWindow;
use crate::config::HOTKEY_FIELDS;
use crate::i18n::{tr, translate};
use crate::skin::{Control, Mark, Tone};
use crate::tray::HotkeyProblem;
use crate::ui::kit::{self, skin};

pub struct Recorder {
    /// The config field waiting for a combination, if any.
    armed: Option<&'static str>,
    /// Holds keyboard focus while armed, so the keystroke lands here.
    focus: FocusHandle,
}

impl Recorder {
    pub fn new(cx: &mut Context<MainWindow>) -> Self {
        Self {
            armed: None,
            focus: cx.focus_handle(),
        }
    }

    /// Ends a recording when focus or the window goes elsewhere: a row left
    /// armed behind the user's back would leave the hotkeys released.
    pub fn subscribe(recorder: &Self, window: &mut Window, cx: &mut Context<MainWindow>) -> [Subscription; 2] {
        [
            cx.on_blur(&recorder.focus, window, |this, _, cx| this.cancel_recording(cx)),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.cancel_recording(cx);
                }
            }),
        ]
    }
}

impl MainWindow {
    fn start_recording(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.update(cx, |c, cx| c.hold_hotkeys(true, cx));
        self.recorder.armed = Some(key);
        window.focus(&self.recorder.focus, cx);
        cx.notify();
    }

    /// Stops waiting and hands the global hotkeys back. Safe when idle.
    pub(super) fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        if self.recorder.armed.take().is_some() {
            self.controller.update(cx, |c, cx| c.hold_hotkeys(false, cx));
            cx.notify();
        }
    }

    fn capture(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(field) = self.recorder.armed else {
            return;
        };
        if event.keystroke.key == "escape" {
            self.cancel_recording(cx);
            return;
        }
        // A modifier on its own is not a combination yet.
        let Some(spec) = hotkey_spec(&event.keystroke) else {
            return;
        };
        self.recorder.armed = None;
        // Writing the config re-registers everything, which also ends the hold.
        self.controller.update(cx, |c, cx| c.set_hotkey(field, spec, cx));
        cx.notify();
    }

    pub(super) fn render_hotkey_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        let rows = HOTKEY_FIELDS.iter().map(|field| {
            let key = field.key;
            let spec = (field.get)(&c.config.hotkeys).to_string();
            let armed = self.recorder.armed == Some(key);
            let error = c.hotkey_errors.iter().find(|e| e.key == key);

            let shown: AnyElement = if armed {
                skin(cx).chip(tr!("Press a combination…"), Mark::Accent, cx)
            } else if spec.is_empty() {
                kit::hint(tr!("Not set"), cx).into_any_element()
            } else {
                kit::key_caps(&spec, cx).into_any_element()
            };
            let controls = h_flex().gap_2().items_center().child(shown).map(|el| {
                if armed {
                    el.child(
                        kit::button(
                            SharedString::from(format!("cancel-{key}")),
                            Tone::Ghost,
                            Control::Compact,
                            cx,
                        )
                        .label(tr!("Cancel"))
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_recording(cx))),
                    )
                } else {
                    el.child(
                        kit::button(
                            SharedString::from(format!("record-{key}")),
                            Tone::Default,
                            Control::Compact,
                            cx,
                        )
                        .label(tr!("Record"))
                        .on_click(cx.listener(move |this, _, window, cx| this.start_recording(key, window, cx))),
                    )
                    .when(!spec.is_empty(), |el| {
                        el.child(
                            kit::button(
                                SharedString::from(format!("clear-{key}")),
                                Tone::Ghost,
                                Control::Compact,
                                cx,
                            )
                            .label(tr!("Clear"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.controller.update(cx, |c, cx| c.set_hotkey(key, String::new(), cx));
                            })),
                        )
                    })
                }
            });
            kit::setting_row(
                translate(field.label),
                error.map(|e| match &e.problem {
                    HotkeyProblem::Taken => tr!("Another program is already using this combination.").into(),
                    HotkeyProblem::Invalid => {
                        tr!("This is not a combination Windows understands. Record it again.").into()
                    }
                    HotkeyProblem::Refused(reason) => {
                        tr!("Windows refused this combination: {reason}", reason = reason.clone()).into()
                    }
                }),
                controls,
                cx,
            )
        });

        skin(cx)
            .card(cx)
            .id("hotkeys")
            // The panel the keystroke lands on. It is always in the tree, so
            // the focus handle has somewhere to sit the moment recording starts.
            .track_focus(&self.recorder.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.capture(event, cx)))
            .gap_4()
            .child(skin(cx).eyebrow(tr!("Hotkeys"), cx))
            .child(kit::hint(
                tr!("Click Record and press the combination you want. It works straight away, in every application."),
                cx,
            ))
            .children(rows)
            .into_any_element()
    }
}

/// A GPUI keystroke as a `global-hotkey` spec, or `None` for a press that
/// cannot be part of a combination — a modifier on its own.
///
/// The two libraries name keys differently, and Windows adds a trap: for the
/// digit row and the punctuation keys GPUI reports the *shifted* character and
/// clears the shift flag, so Shift+1 arrives as `"!"`. That is turned back into
/// the key it is printed on, with the shift put back.
pub fn hotkey_spec(keystroke: &Keystroke) -> Option<String> {
    let (key, shifted) = spec_key(&keystroke.key)?;
    let m = &keystroke.modifiers;
    let mut spec = String::new();
    // GPUI's `platform` is the Windows key, which `global-hotkey` calls `super`.
    for (held, name) in [
        (m.control, "ctrl"),
        (m.alt, "alt"),
        (m.shift || shifted, "shift"),
        (m.platform, "super"),
    ] {
        if held {
            spec.push_str(name);
            spec.push('+');
        }
    }
    spec.push_str(&key);
    Some(spec)
}

/// The `global-hotkey` name for a key GPUI reported, and whether the character
/// carries a shift of its own.
fn spec_key(key: &str) -> Option<(String, bool)> {
    if matches!(
        key,
        "" | "control" | "alt" | "shift" | "platform" | "function" | "capslock"
    ) {
        return None;
    }
    let named = match key {
        "space" => Some("Space"),
        "tab" => Some("Tab"),
        "enter" => Some("Enter"),
        "escape" => Some("Escape"),
        "backspace" => Some("Backspace"),
        "delete" => Some("Delete"),
        "insert" => Some("Insert"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        _ => None,
    };
    if let Some(named) = named {
        return Some((named.to_string(), false));
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
        && (1..=24).contains(&n)
    {
        return Some((format!("F{n}"), false));
    }
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    // Shifted symbols, each folded back onto the key it shares.
    const SHIFTED: [(char, char); 21] = [
        ('!', '1'),
        ('@', '2'),
        ('#', '3'),
        ('$', '4'),
        ('%', '5'),
        ('^', '6'),
        ('&', '7'),
        ('*', '8'),
        ('(', '9'),
        (')', '0'),
        ('~', '`'),
        ('_', '-'),
        ('+', '='),
        ('{', '['),
        ('}', ']'),
        ('|', '\\'),
        (':', ';'),
        ('"', '\''),
        ('<', ','),
        ('>', '.'),
        ('?', '/'),
    ];
    match c {
        'a'..='z' | 'A'..='Z' => Some((c.to_ascii_uppercase().to_string(), false)),
        '0'..='9' | '`' | '-' | '=' | '[' | ']' | '\\' | ';' | '\'' | ',' | '.' | '/' => Some((c.to_string(), false)),
        _ => SHIFTED
            .iter()
            .find(|(symbol, _)| *symbol == c)
            .map(|(_, base)| (base.to_string(), true)),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use global_hotkey::hotkey::HotKey;
    use gpui_kit::{Keystroke, Modifiers};

    use super::hotkey_spec;

    fn pressed(key: &str, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: None,
        }
    }

    fn ctrl_alt() -> Modifiers {
        Modifiers {
            control: true,
            alt: true,
            ..Default::default()
        }
    }

    #[test]
    fn every_recordable_key_produces_a_spec_the_hotkey_manager_parses() {
        // The recorder writes the spec and `global-hotkey` reads it; a name only
        // one of them knows is a combination the UI accepts and Windows refuses.
        let keys = "i a z 1 9 f1 f12 f24 pageup pagedown up down left right home end insert delete backspace \
                    enter tab space escape ` - = [ ] \\ ; ' , . / ! @ # $ % ^ & * ( ) ~ _ + { } | : \" < > ?";
        let all = Modifiers {
            control: true,
            alt: true,
            shift: true,
            platform: true,
            function: false,
        };
        for key in keys.split_whitespace() {
            for modifiers in [Modifiers::none(), ctrl_alt(), all] {
                let spec = hotkey_spec(&pressed(key, modifiers)).unwrap_or_else(|| panic!("{key} produced no spec"));
                assert!(
                    HotKey::from_str(&spec).is_ok(),
                    "{key} recorded as {spec:?}, which does not parse"
                );
            }
        }
    }

    #[test]
    fn a_shifted_symbol_is_written_as_the_key_it_is_printed_on() {
        assert_eq!(hotkey_spec(&pressed("!", Modifiers::none())).unwrap(), "shift+1");
        assert_eq!(hotkey_spec(&pressed("_", ctrl_alt())).unwrap(), "ctrl+alt+shift+-");
        // A letter keeps its own shift flag, and a digit keeps its own key.
        assert_eq!(hotkey_spec(&pressed("i", Modifiers::shift())).unwrap(), "shift+I");
        assert_eq!(hotkey_spec(&pressed("7", Modifiers::none())).unwrap(), "7");
        let win = Modifiers {
            platform: true,
            ..Default::default()
        };
        assert_eq!(hotkey_spec(&pressed("i", win)).unwrap(), "super+I");
        assert_eq!(hotkey_spec(&pressed("pageup", ctrl_alt())).unwrap(), "ctrl+alt+PageUp");
    }

    #[test]
    fn a_modifier_on_its_own_is_not_a_combination_yet() {
        for key in ["", "control", "alt", "shift", "platform", "function", "capslock"] {
            assert!(hotkey_spec(&pressed(key, ctrl_alt())).is_none(), "{key:?} was taken");
        }
        // A key neither library names is refused rather than written down.
        assert!(hotkey_spec(&pressed("intlbackslash", ctrl_alt())).is_none());
        assert!(hotkey_spec(&pressed("é", ctrl_alt())).is_none());
    }
}
