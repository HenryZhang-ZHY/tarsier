//! The window's stateful widgets — sliders, number fields and name fields —
//! created on demand and kept in step with the controller.
//!
//! Each widget is stored with the subscription that writes it back, so a
//! monitor that disappears takes its listeners with it.

use std::collections::HashMap;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;

use super::MainWindow;
use super::displays::{Setup, endpoint_key};
use crate::config::BreakConfig;
use crate::controller::Controller;
use crate::display::mccs::{VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::platform;
use crate::ui::number_field::{NumberField, Range};

/// An editable break duration, shared by the Settings section and the summary
/// on the Breaks tab.
pub struct BreakField {
    pub key: &'static str,
    /// English source text; translated where it is drawn.
    pub label: &'static str,
    pub range: Range,
    pub get: fn(&BreakConfig) -> u32,
    pub set: fn(&mut BreakConfig, u32),
}

pub const BREAK_FIELDS: [BreakField; 3] = [
    BreakField {
        key: "work",
        label: "Work",
        range: Range {
            min: 10,
            max: 180,
            step: 5,
        },
        get: |b| b.work_minutes,
        set: |b, v| b.work_minutes = v,
    },
    BreakField {
        key: "rest",
        label: "Break",
        range: Range {
            min: 1,
            max: 30,
            step: 1,
        },
        get: |b| b.break_minutes,
        set: |b, v| b.break_minutes = v,
    },
    BreakField {
        key: "snooze",
        label: "Snooze",
        range: Range {
            min: 1,
            max: 30,
            step: 1,
        },
        get: |b| b.snooze_minutes,
        set: |b, v| b.snooze_minutes = v,
    },
];

/// The DDC/CI features that get a slider, in the order they are drawn.
pub const FEATURES: [u8; 2] = [VCP_BRIGHTNESS, VCP_CONTRAST];

type FeatureKey = (String, u8);

struct FeatureControl {
    slider: Entity<SliderState>,
    field: NumberField,
    max: u32,
    _write_back: Subscription,
}

pub struct Controls {
    features: HashMap<FeatureKey, FeatureControl>,
    durations: HashMap<&'static str, NumberField>,
    names: HashMap<String, (Entity<InputState>, Subscription)>,
}

impl Controls {
    pub fn new(controller: &Entity<Controller>, window: &mut Window, cx: &mut Context<MainWindow>) -> Self {
        let breaks = controller.read(cx).config.breaks.clone();
        let durations = BREAK_FIELDS
            .iter()
            .map(|field| {
                let controller = controller.clone();
                let set = field.set;
                let number = NumberField::new(
                    (field.get)(&breaks),
                    field.range,
                    move |v, cx| controller.update(cx, |c, cx| c.update_config(cx, |cfg| set(&mut cfg.breaks, v))),
                    window,
                    cx,
                );
                (field.key, number)
            })
            .collect();
        Self {
            features: HashMap::new(),
            durations,
            names: HashMap::new(),
        }
    }

    pub fn slider(&self, monitor: &str, code: u8) -> Option<&Entity<SliderState>> {
        self.features.get(&(monitor.to_string(), code)).map(|f| &f.slider)
    }

    pub fn feature_field(&self, monitor: &str, code: u8) -> Option<&NumberField> {
        self.features.get(&(monitor.to_string(), code)).map(|f| &f.field)
    }

    pub fn duration(&self, key: &str) -> Option<&NumberField> {
        self.durations.get(key)
    }

    pub fn name(&self, monitor: &str, port: u8) -> Option<&Entity<InputState>> {
        self.names.get(&endpoint_key(monitor, port)).map(|(state, _)| state)
    }

    pub fn sync(
        &mut self,
        setup: &HashMap<String, Setup>,
        controller: &Entity<Controller>,
        window: &mut Window,
        cx: &mut Context<MainWindow>,
    ) {
        let breaks = controller.read(cx).config.breaks.clone();
        for field in &BREAK_FIELDS {
            if let Some(number) = self.durations.get(field.key) {
                number.sync((field.get)(&breaks), window, cx);
            }
        }
        self.sync_features(controller, window, cx);
        self.sync_names(setup, controller, window, cx);
    }

    fn sync_features(&mut self, controller: &Entity<Controller>, window: &mut Window, cx: &mut Context<MainWindow>) {
        let wanted: Vec<(FeatureKey, u32, u32)> = controller
            .read(cx)
            .monitors
            .iter()
            .flat_map(|m| {
                let id = m.id().to_string();
                FEATURES.into_iter().filter_map(move |code| {
                    let feature = match code {
                        VCP_BRIGHTNESS => m.brightness,
                        _ => m.contrast,
                    }?;
                    Some(((id.clone(), code), feature.current, feature.max.max(1)))
                })
            })
            .collect();

        // A rescan can report a different range for the same monitor, and a
        // slider cannot change its range, so that is a new control.
        self.features
            .retain(|key, control| wanted.iter().any(|(k, _, max)| k == key && *max == control.max));

        for (key, current, max) in wanted {
            if let Some(control) = self.features.get(&key) {
                if control.slider.read(cx).value().start().round() as u32 != current {
                    control
                        .slider
                        .update(cx, |s, cx| s.set_value(current as f32, window, cx));
                }
                control.field.sync(current, window, cx);
                continue;
            }
            let (id, code) = key.clone();
            let field = {
                let controller = controller.clone();
                let id = id.clone();
                NumberField::new(
                    current,
                    Range { min: 0, max, step: 5 },
                    move |v, cx| controller.update(cx, |c, cx| c.set_feature(&id, code, v, cx)),
                    window,
                    cx,
                )
            };
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(max as f32)
                    .step(1.)
                    .default_value(current as f32)
            });
            let write_back = {
                let controller = controller.clone();
                cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
                    let (SliderEvent::Change(v) | SliderEvent::Release(v)) = event;
                    let value = v.start().round() as u32;
                    controller.update(cx, |c, cx| c.set_feature(&id, code, value, cx));
                })
            };
            self.features.insert(
                key,
                FeatureControl {
                    slider,
                    field,
                    max,
                    _write_back: write_back,
                },
            );
        }
    }

    /// One name field per computer the window can show: the configured ones,
    /// plus whatever the first-run flow has ticked but not committed yet.
    fn sync_names(
        &mut self,
        setup: &HashMap<String, Setup>,
        controller: &Entity<Controller>,
        window: &mut Window,
        cx: &mut Context<MainWindow>,
    ) {
        let wanted: Vec<(String, String, u8, String)> = {
            let c = controller.read(cx);
            c.monitors
                .iter()
                .flat_map(|m| {
                    let id = m.id().to_string();
                    let prefs = c.monitor_prefs(&id);
                    let draft = setup.get(&id);
                    let mut ports = c.endpoints(m);
                    for port in draft.map(|s| s.chosen.as_slice()).unwrap_or_default() {
                        if !ports.contains(port) {
                            ports.push(*port);
                        }
                    }
                    ports
                        .into_iter()
                        .map(|port| {
                            // This computer's name is never typed: it comes from
                            // the system, the wizard included.
                            let name = if prefs.is_named(port) {
                                prefs.label(port)
                            } else if draft.and_then(|s| s.local) == Some(port) {
                                platform::local_hostname()
                            } else {
                                String::new()
                            };
                            (endpoint_key(&id, port), id.clone(), port, name)
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };

        self.names.retain(|key, _| wanted.iter().any(|(k, ..)| k == key));
        for (key, id, port, name) in wanted {
            if let Some((state, _)) = self.names.get(&key) {
                // Never overwrite what the user is in the middle of typing.
                let editing = state.read(cx).focus_handle(cx).is_focused(window);
                if !editing && state.read(cx).value() != name {
                    state.update(cx, |s, cx| s.set_value(name, window, cx));
                }
                continue;
            }
            let state = cx.new(|cx| InputState::new(window, cx).default_value(name));
            let controller = controller.clone();
            let commit = cx.subscribe_in(&state, window, move |_, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    controller.update(cx, |c, cx| c.set_input_name(&id, port, text, cx));
                }
            });
            self.names.insert(key, (state, commit));
        }
    }
}
