//! A time-of-day field with −/+ buttons that also accepts typed times.
//!
//! The twin of [`NumberField`](super::number_field::NumberField): typed text is
//! committed on Enter or blur and written back as `HH:MM`, the buttons step by
//! a quarter of an hour, and an outside update never overwrites what the user
//! is typing. Anything that is not an evening time puts the last one back.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState, NumberInput, NumberInputEvent, StepAction};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::evening::ClockTime;

/// What a committed time is handed to.
type Commit = dyn Fn(ClockTime, &mut App);

/// What one press of − or + moves the time by.
const STEP_MINUTES: i32 = 15;

pub struct TimeField {
    state: Entity<InputState>,
    last: Rc<Cell<ClockTime>>,
    _subscriptions: [Subscription; 2],
}

impl TimeField {
    pub fn new<V: 'static>(
        value: ClockTime,
        on_commit: impl Fn(ClockTime, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let state = cx.new(|cx| {
            let mut state = InputState::new(window, cx).default_value(value.to_string());
            // Without this the buttons would add one to the text as a number;
            // with it they emit `NumberInputEvent::Step` for us to handle.
            state.set_step(None, window, cx);
            state
        });
        let last = Rc::new(Cell::new(value));
        let on_commit: Rc<Commit> = {
            let last = last.clone();
            Rc::new(move |time, cx| {
                last.set(time);
                on_commit(time, cx)
            })
        };

        let commit = on_commit.clone();
        let restore = last.clone();
        let typed = cx.subscribe_in(&state, window, move |_, state, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                return;
            }
            match state.read(cx).value().parse::<ClockTime>() {
                Ok(time) if time.is_evening() => apply(state, time, &*commit, window, cx),
                _ => state.update(cx, |s, cx| s.set_value(restore.get().to_string(), window, cx)),
            }
        });

        let commit = on_commit;
        let current = last.clone();
        let stepped = cx.subscribe_in(&state, window, move |_, state, event: &NumberInputEvent, window, cx| {
            let NumberInputEvent::Step(action) = event;
            let from = state
                .read(cx)
                .value()
                .parse::<ClockTime>()
                .ok()
                .filter(|t| t.is_evening())
                .unwrap_or(current.get());
            let next = match action {
                StepAction::Increment => from.step(STEP_MINUTES),
                StepAction::Decrement => from.step(-STEP_MINUTES),
            };
            apply(state, next, &*commit, window, cx);
        });

        Self {
            state,
            last,
            _subscriptions: [typed, stepped],
        }
    }

    /// Reflect an outside change unless the user has typed something not yet
    /// committed.
    pub fn sync(&self, value: ClockTime, window: &mut Window, cx: &mut App) {
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

fn apply(state: &Entity<InputState>, time: ClockTime, commit: &Commit, window: &mut Window, cx: &mut App) {
    let text = time.to_string();
    if state.read(cx).value() != text {
        state.update(cx, |s, cx| s.set_value(text, window, cx));
    }
    commit(time, cx);
}
