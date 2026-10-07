//! The window's building blocks: text styles, setting rows, pickers and key
//! caps, composed from the active skin's primitives.
//!
//! Screens build from these rather than from raw `div()` styling, so a page has
//! one type scale and one way of laying out a setting, whichever skin draws it.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::*;

use crate::skin::{self, Control, SkinStyle, Tone, Voice};

pub fn skin(cx: &App) -> &'static dyn SkinStyle {
    skin::active(cx)
}

/// A card or section title.
pub fn title(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_base()
        .font_weight(skin(cx).weight(Voice::Loud))
        .child(text.into())
}

/// The name of a row or a control.
pub fn label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_sm()
        .font_weight(skin(cx).weight(Voice::Plain))
        .child(text.into())
}

/// Ordinary running text.
pub fn body(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_sm()
        .font_weight(skin(cx).weight(Voice::Quiet))
        .child(text.into())
}

/// Fine print: what a setting does, why a control is missing.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(skin(cx).weight(Voice::Quiet))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn button(id: impl Into<ElementId>, tone: Tone, control: Control, cx: &App) -> Button {
    skin(cx).button(id.into(), tone, control, cx)
}

/// A button that is greyed out unless `enabled`.
pub fn button_if(id: impl Into<ElementId>, tone: Tone, control: Control, enabled: bool, cx: &App) -> Button {
    let skin = skin(cx);
    skin.disable(skin.button(id.into(), tone, control, cx), !enabled)
}

/// One option of a [`segmented`] picker.
pub struct Choice<T> {
    pub value: T,
    /// Stable across languages, for the element id.
    pub key: &'static str,
    pub label: SharedString,
}

/// A row of mutually exclusive options with the chosen one filled in.
///
/// Used for the main tabs and for every small "pick one" setting, so the two
/// look and behave the same.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &str,
    choices: impl IntoIterator<Item = Choice<T>>,
    selected: T,
    enabled: bool,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let skin = skin(cx);
    let on_pick = Rc::new(on_pick);
    skin.segmented(cx).children(choices.into_iter().map(|choice| {
        let on_pick = on_pick.clone();
        let value = choice.value;
        skin.disable(
            skin.segment(
                SharedString::from(format!("{id}-{}", choice.key)).into(),
                value == selected,
                cx,
            )
            .label(choice.label),
            !enabled,
        )
        .on_click(move |_, window, cx| on_pick(value, window, cx))
    }))
}

/// A setting: its name and what it does on the left, the control on the right.
///
/// The words take whatever width is left and wrap; the control never shrinks.
/// The other way round, a long description pushes the control out of the
/// window.
pub fn setting_row(
    name: impl Into<SharedString>,
    description: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .gap_4()
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(label(name, cx))
                .children(description.map(|d| hint(d, cx))),
        )
        .child(div().flex_none().child(control))
}

/// A setting that is a switch.
pub fn toggle_row(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    checked: bool,
    on_toggle: impl Fn(bool, &mut App) + 'static,
    cx: &App,
) -> Div {
    setting_row(
        name,
        Some(description.into()),
        Switch::new(id)
            .checked(checked)
            .on_click(move |value, _, cx| on_toggle(*value, cx)),
        cx,
    )
}

/// A `global-hotkey` spec such as `ctrl+alt+I`, drawn as key caps.
pub fn key_caps(spec: &str, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .children(key_names(spec).into_iter().map(|key| skin(cx).key_cap(&key, cx)))
}

/// The keys of a hotkey spec, named the way a keyboard prints them.
pub fn key_names(spec: &str) -> Vec<String> {
    spec.split('+')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|key| match key.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "win" | "super" | "meta" | "cmd" => "Win".to_string(),
            "pageup" => "PgUp".to_string(),
            "pagedown" => "PgDn".to_string(),
            _ if key.chars().count() == 1 => key.to_uppercase(),
            _ => key.to_string(),
        })
        .collect()
}

/// A card's heading row: the title, then anything that belongs beside it,
/// pushed to the right.
pub fn card_header(title: AnyElement, trailing: impl IntoIterator<Item = AnyElement>) -> Div {
    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(div().flex_1().min_w_0().child(title))
        .children(trailing)
}

/// What a page shows when it has nothing to list.
pub fn empty_state(icon: impl Into<Icon>, heading: &str, detail: &str, action: Option<Button>, cx: &App) -> Div {
    skin(cx)
        .card(cx)
        .items_center()
        .gap_2()
        .py_8()
        .child(icon.into().size(px(28.)).text_color(cx.theme().muted_foreground))
        .child(label(heading.to_string(), cx))
        .child(div().max_w(px(420.)).text_center().child(hint(detail.to_string(), cx)))
        .children(action.map(|a| div().pt_2().child(a)))
}

#[cfg(test)]
mod tests {
    use super::key_names;

    #[test]
    fn hotkey_specs_are_spelled_the_way_keyboards_print_them() {
        assert_eq!(key_names("ctrl+alt+I"), ["Ctrl", "Alt", "I"]);
        assert_eq!(key_names("control+shift+super+i"), ["Ctrl", "Shift", "Win", "I"]);
        assert_eq!(key_names("ctrl+alt+PageUp"), ["Ctrl", "Alt", "PgUp"]);
        assert_eq!(key_names("F12"), ["F12"]);
        assert!(key_names("").is_empty());
        assert_eq!(key_names(" ctrl + k "), ["Ctrl", "K"], "padding is not a key");
    }
}
