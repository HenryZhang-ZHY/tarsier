//! An integer field with −/+ buttons that also accepts typed values.
//!
//! Typed text is committed on Enter or blur, clamped to the range and written
//! back normalised; the buttons step immediately. While the field has focus,
//! outside updates don't overwrite what the user is typing.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState, MaskPattern, NumberInput, NumberInputEvent, StepAction};
use gpui_kit::component::*;
use gpui_kit::*;

pub struct NumberField {
    state: Entity<InputState>,
    /// Last committed or synced value, restored when the text doesn't parse.
    last: Rc<Cell<u32>>,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone, Copy)]
pub struct Range {
    pub min: u32,
    pub max: u32,
    pub step: u32,
}

impl Range {
    pub fn clamp(&self, v: i64) -> u32 {
        v.clamp(self.min as i64, self.max as i64) as u32
    }
}

impl NumberField {
    pub fn new<V: 'static>(
        value: u32,
        range: Range,
        on_commit: impl Fn(u32, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let state = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .mask_pattern(MaskPattern::Number {
                    separator: None,
                    fraction: None,
                })
                .default_value(value.to_string());
            // The number mask installs a silent built-in step of 1; drop it so
            // the buttons emit `NumberInputEvent::Step` and we step + commit.
            state.set_step(None, window, cx);
            state
        });
        let last = Rc::new(Cell::new(value));
        let on_commit: Rc<dyn Fn(u32, &mut App)> = {
            let last = last.clone();
            Rc::new(move |v, cx| {
                last.set(v);
                on_commit(v, cx)
            })
        };

        let commit = on_commit.clone();
        let restore = last.clone();
        let typed = cx.subscribe_in(&state, window, move |_, state, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                return;
            }
            let text = state.read(cx).value();
            match text.trim().parse::<f64>() {
                Ok(v) => apply(state, range.clamp(v.round() as i64), &*commit, window, cx),
                Err(_) => state.update(cx, |s, cx| s.set_value(restore.get().to_string(), window, cx)),
            }
        });

        let commit = on_commit;
        let stepped = cx.subscribe_in(&state, window, move |_, state, event: &NumberInputEvent, window, cx| {
            let NumberInputEvent::Step(action) = event;
            let current = state
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .unwrap_or(range.min as f64)
                .round() as i64;
            let next = match action {
                StepAction::Increment => current + range.step as i64,
                StepAction::Decrement => current - range.step as i64,
            };
            apply(state, range.clamp(next), &*commit, window, cx);
        });

        Self {
            state,
            last,
            _subscriptions: [typed, stepped],
        }
    }

    /// Reflect an outside change (slider, hotkey) unless the user has typed
    /// something not yet committed.
    pub fn sync(&self, value: u32, window: &mut Window, cx: &mut App) {
        let state = self.state.read(cx);
        let editing = state.focus_handle(cx).is_focused(window) && state.value() != self.last.get().to_string();
        let text = value.to_string();
        if !editing && state.value() != text {
            self.last.set(value);
            self.state.update(cx, |s, cx| s.set_value(text, window, cx));
        }
    }

    pub fn input(&self) -> NumberInput {
        NumberInput::new(&self.state).small()
    }
}

fn apply(state: &Entity<InputState>, value: u32, commit: &dyn Fn(u32, &mut App), window: &mut Window, cx: &mut App) {
    let text = value.to_string();
    if state.read(cx).value() != text {
        state.update(cx, |s, cx| s.set_value(text, window, cx));
    }
    commit(value, cx);
}

#[cfg(test)]
mod tests {
    use super::Range;

    #[test]
    fn clamps_to_range() {
        let r = Range {
            min: 10,
            max: 180,
            step: 5,
        };
        assert_eq!(r.clamp(3), 10);
        assert_eq!(r.clamp(999), 180);
        assert_eq!(r.clamp(42), 42);
        assert_eq!(r.clamp(-5), 10);
    }
}
