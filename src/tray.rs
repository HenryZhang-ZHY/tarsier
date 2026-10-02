//! System tray icon and menu, plus global hotkeys. Both create hidden Win32
//! windows on the calling (main) thread; GPUI's message loop pumps them.
//! Events are drained by polling from the GPUI foreground executor.

use std::str::FromStr;

use anyhow::Result;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::config::Hotkeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ShowWindow,
    ToggleInput,
    BrightnessUp,
    BrightnessDown,
    BreakNow,
    Snooze,
    Skip,
    TogglePause,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    pause_item: MenuItem,
    /// Only usable while a break reminder is showing.
    break_items: [MenuItem; 2],
    items: Vec<(MenuItem, Command)>,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let open = MenuItem::new("打开 tarsier", true, None);
        let toggle = MenuItem::new("切换显示器输入", true, None);
        let break_now = MenuItem::new("现在休息", true, None);
        let snooze = MenuItem::new("推迟这次休息", false, None);
        let skip = MenuItem::new("跳过这次休息", false, None);
        let pause = MenuItem::new("暂停提醒 1 小时", true, None);
        let quit = MenuItem::new("退出", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &toggle,
            &break_now,
            &snooze,
            &skip,
            &pause,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("tarsier")
            .with_icon(tray_image(false))
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        let items = vec![
            (open, Command::ShowWindow),
            (toggle, Command::ToggleInput),
            (break_now, Command::BreakNow),
            (snooze.clone(), Command::Snooze),
            (skip.clone(), Command::Skip),
            (pause.clone(), Command::TogglePause),
            (quit, Command::Quit),
        ];
        Ok(Self {
            icon,
            pause_item: pause,
            break_items: [snooze, skip],
            items,
        })
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause_item.set_text(if paused {
            "恢复提醒"
        } else {
            "暂停提醒 1 小时"
        });
        let _ = self.icon.set_icon(Some(tray_image(paused)));
    }

    pub fn set_break_active(&self, active: bool) {
        for item in &self.break_items {
            item.set_enabled(active);
        }
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }

    /// Drains pending tray clicks and menu selections.
    pub fn poll(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                commands.push(Command::ShowWindow);
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some((_, cmd)) = self.items.iter().find(|(item, _)| *item.id() == event.id) {
                commands.push(*cmd);
            }
        }
        commands
    }
}

pub struct Hotkey {
    _manager: GlobalHotKeyManager,
    bindings: Vec<(u32, Command)>,
    pub errors: Vec<String>,
}

impl Hotkey {
    pub fn new(keys: &Hotkeys) -> Result<Self> {
        let manager = GlobalHotKeyManager::new()?;
        let mut bindings = Vec::new();
        let mut errors = Vec::new();
        for (spec, cmd) in [
            (&keys.toggle_input, Command::ToggleInput),
            (&keys.brightness_up, Command::BrightnessUp),
            (&keys.brightness_down, Command::BrightnessDown),
            (&keys.break_now, Command::BreakNow),
        ] {
            if spec.trim().is_empty() {
                continue;
            }
            match HotKey::from_str(spec) {
                Ok(hotkey) => match manager.register(hotkey) {
                    Ok(()) => bindings.push((hotkey.id(), cmd)),
                    Err(e) => errors.push(format!("{spec}: {e}")),
                },
                Err(e) => errors.push(format!("{spec}: {e}")),
            }
        }
        Ok(Self {
            _manager: manager,
            bindings,
            errors,
        })
    }

    pub fn poll(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state() == HotKeyState::Pressed
                && let Some((_, cmd)) = self.bindings.iter().find(|(id, _)| *id == event.id())
            {
                commands.push(*cmd);
            }
        }
        commands
    }
}

/// Draws a 32x32 monitor glyph; grey when reminders are paused.
fn tray_image(paused: bool) -> Icon {
    const N: usize = 32;
    let frame = if paused {
        [140, 140, 150, 255]
    } else {
        [56, 132, 255, 255]
    };
    let screen = if paused {
        [210, 210, 215, 255]
    } else {
        [190, 225, 255, 255]
    };
    let mut rgba = vec![0u8; N * N * 4];
    let mut put = |x: usize, y: usize, c: [u8; 4]| {
        let i = (y * N + x) * 4;
        rgba[i..i + 4].copy_from_slice(&c);
    };
    for y in 0..N {
        for x in 0..N {
            let in_frame = (2..30).contains(&x) && (4..23).contains(&y);
            let corner = (x == 2 || x == 29) && (y == 4 || y == 22);
            let in_screen = (5..27).contains(&x) && (7..20).contains(&y);
            let neck = (14..18).contains(&x) && (23..26).contains(&y);
            let base = (9..23).contains(&x) && (26..28).contains(&y);
            if in_screen {
                put(x, y, screen);
            } else if (in_frame && !corner) || neck || base {
                put(x, y, frame);
            }
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("valid tray icon")
}
