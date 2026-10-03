//! System tray icon and menu, plus global hotkeys. Both create hidden Win32
//! windows on the calling (main) thread; GPUI's message loop pumps them.
//! Events are drained by polling from the GPUI foreground executor.
//!
//! The menu is rebuilt from a [`TrayState`] the controller derives, and only
//! when that state actually changes. A monitor shared by several computers
//! needs the menu to say where each entry goes, which no static menu can do.

use std::cell::RefCell;
use std::str::FromStr;

use anyhow::Result;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::config::Hotkeys;
use crate::i18n::{Language, tr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ShowWindow,
    /// Flip, or ask, depending on how many computers share the monitor.
    ToggleInput,
    /// Switch one monitor straight to a port.
    SwitchTo(String, u8),
    BrightnessUp,
    BrightnessDown,
    BreakNow,
    Snooze,
    Skip,
    TogglePause,
    Quit,
}

/// One computer offered in the switch list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayEndpoint {
    pub port: u8,
    pub name: String,
}

/// The computers sharing one monitor. Split per monitor so a two-monitor
/// setup does not merge into one ambiguous list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayGroup {
    /// The monitor's id. This is what routes a click back to a monitor, and it
    /// is *not* the same string as [`Self::name`] — conflating the two made
    /// every tray switch a silent no-op, because the lookup is by id.
    pub id: String,
    /// The monitor's name, shown only when more than one is listed.
    pub name: String,
    pub endpoints: Vec<TrayEndpoint>,
}

/// Everything the tray shows, derived by the controller. Compared by value so
/// the native menu is only rebuilt when something visible changed.
///
/// There is deliberately no "which one is live" field. Knowing that would mean
/// reading the monitor's current input on a timer, and the answer can be
/// changed by the other computer at any moment — so the menu lists the
/// destinations and never claims to know where you already are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayState {
    pub groups: Vec<TrayGroup>,
    pub paused: bool,
    pub break_active: bool,
    /// The language the menu was last built in. The labels come from `i18n`
    /// like every other string, but the menu is native and only rebuilt when
    /// this state changes, so the language has to be part of the comparison.
    pub language: Language,
    pub tooltip: String,
}

impl TrayState {
    /// Whether the switch list is worth showing at all.
    fn has_switching(&self) -> bool {
        !self.groups.is_empty()
    }

    /// Everything that changes the menu itself. The tooltip ticks once a
    /// minute and must not drag a native menu rebuild along with it.
    fn menu_part(&self) -> (&[TrayGroup], bool, bool, Language) {
        (&self.groups, self.paused, self.break_active, self.language)
    }
}

pub struct Tray {
    icon: TrayIcon,
    /// Menu ids built by the last rebuild, for matching incoming events.
    built: RefCell<Built>,
    last: RefCell<Option<TrayState>>,
}

#[derive(Default)]
struct Built {
    commands: Vec<(MenuId, Command)>,
    switches: Vec<(MenuId, String, u8)>,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let state = TrayState::default();
        let (menu, built) = build_menu(&state).map_err(|e| anyhow::anyhow!("building tray menu: {e}"))?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("tarsier")
            .with_icon(tray_image(state.paused))
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        Ok(Self {
            icon,
            built: RefCell::new(built),
            last: RefCell::new(None),
        })
    }

    /// Shows `state`, rebuilding the native menu only when something on it
    /// actually changed.
    pub fn sync(&self, state: &TrayState) {
        let (same_menu, same_tooltip) = match self.last.borrow().as_ref() {
            Some(last) => (last.menu_part() == state.menu_part(), last.tooltip == state.tooltip),
            None => (false, false),
        };
        if same_menu && same_tooltip {
            return;
        }
        if !same_tooltip {
            let _ = self.icon.set_tooltip(Some(state.tooltip.as_str()));
        }
        if !same_menu {
            match build_menu(state) {
                Ok((menu, built)) => {
                    let _ = self.icon.set_menu(Some(Box::new(menu)));
                    *self.built.borrow_mut() = built;
                }
                Err(e) => log::error!("rebuilding tray menu: {e}"),
            }
            let _ = self.icon.set_icon(Some(tray_image(state.paused)));
        }
        *self.last.borrow_mut() = Some(state.clone());
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
            let id = event.id();
            let built = self.built.borrow();
            if let Some((_, command)) = built.commands.iter().find(|(item, _)| item == id) {
                commands.push(command.clone());
            } else if let Some((_, monitor, port)) = built.switches.iter().find(|(item, _, _)| item == id) {
                commands.push(Command::SwitchTo(monitor.clone(), *port));
            }
        }
        commands
    }
}

/// Appends a plain menu item and remembers which command it stands for.
fn push_item(menu: &Menu, built: &mut Built, id: &str, text: &str, command: Command) -> MenuResult<()> {
    let item = MenuItem::with_id(id, text, true, None);
    built.commands.push((item.id().clone(), command));
    menu.append(&item)
}

/// Menu errors are muda's own type; callers fold them into `anyhow` or a log.
type MenuResult<T> = std::result::Result<T, tray_icon::menu::Error>;

fn build_menu(state: &TrayState) -> MenuResult<(Menu, Built)> {
    let mut built = Built::default();
    let menu = Menu::new();

    push_item(&menu, &mut built, "open", tr!("Open tarsier"), Command::ShowWindow)?;
    menu.append(&PredefinedMenuItem::separator())?;

    if !state.has_switching() {
        push_item(
            &menu,
            &mut built,
            "switch-setup",
            tr!("Set up monitor inputs…"),
            Command::ShowWindow,
        )?;
    } else {
        // Only destinations. There is deliberately no "flip" entry above them:
        // a flip has to read the monitor to know which way to go, and when that
        // reading is missing or lands outside the pair the entry can only do
        // nothing. The hotkey still flips, and asks when it cannot.
        let picker = Submenu::new(tr!("Switch to"), true);
        for group in &state.groups {
            if state.groups.len() > 1 {
                picker.append(&PredefinedMenuItem::separator())?;
                // A disabled item is how the platform draws a section header.
                picker.append(&MenuItem::new(group.name.clone(), false, None))?;
            }
            for endpoint in &group.endpoints {
                let id = MenuId::new(format!("go:{}:{}", group.id, endpoint.port));
                let label = format!(
                    "{}  ·  {}",
                    endpoint.name,
                    crate::display::mccs::input_source_name(endpoint.port)
                );
                let item = MenuItem::with_id(id.clone(), label, true, None);
                picker.append(&item)?;
                built.switches.push((id, group.id.clone(), endpoint.port));
            }
        }
        menu.append(&picker)?;
    }

    menu.append(&PredefinedMenuItem::separator())?;
    push_item(
        &menu,
        &mut built,
        "break-now",
        tr!("Take a break now"),
        Command::BreakNow,
    )?;
    for (id, text, command) in [
        ("snooze", tr!("Snooze this break"), Command::Snooze),
        ("skip", tr!("Skip this break"), Command::Skip),
    ] {
        let item = MenuItem::with_id(id, text, state.break_active, None);
        built.commands.push((item.id().clone(), command));
        menu.append(&item)?;
    }
    push_item(
        &menu,
        &mut built,
        "pause",
        if state.paused {
            tr!("Resume reminders")
        } else {
            tr!("Pause reminders for 1 hour")
        },
        Command::TogglePause,
    )?;
    menu.append(&PredefinedMenuItem::separator())?;
    push_item(&menu, &mut built, "quit", tr!("Quit"), Command::Quit)?;

    Ok((menu, built))
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
                commands.push(cmd.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(port: u8, name: &str) -> TrayEndpoint {
        TrayEndpoint {
            port,
            name: name.to_string(),
        }
    }

    /// A monitor id as Windows reports it — deliberately nothing like the
    /// display name, so a test that confuses the two fails.
    fn group(id: &str, name: &str, endpoints: Vec<TrayEndpoint>) -> TrayGroup {
        TrayGroup {
            id: id.to_string(),
            name: name.to_string(),
            endpoints,
        }
    }

    fn submenu_count(menu: &Menu) -> usize {
        menu.items()
            .into_iter()
            .filter(|item| matches!(item, tray_icon::menu::MenuItemKind::Submenu(_)))
            .count()
    }

    #[test]
    fn a_shared_monitor_lists_destinations_and_nothing_else() {
        let state = TrayState {
            groups: vec![group(
                r"\\?\DISPLAY#GSM5BF6#5&1a2b3c4d&0&UID4353",
                "27GP950",
                vec![endpoint(0x10, "MacBook Pro"), endpoint(0x12, "Desktop")],
            )],
            tooltip: "tarsier".to_string(),
            ..Default::default()
        };
        let (menu, built) = build_menu(&state).unwrap();

        // Every computer is reachable by name, and each is wired to a command
        // that names its own monitor and port.
        assert_eq!(built.switches.len(), 2);
        assert_eq!(built.switches[0].2, 0x10);
        assert_eq!(built.switches[1].2, 0x12);
        // No "flip" entry: it would depend on the monitor's current input,
        // which is the thing this menu deliberately does not consult.
        assert!(
            !built.commands.iter().any(|(_, c)| *c == Command::ToggleInput),
            "the menu should offer destinations, not a state-dependent flip"
        );
        // The list is nested, so the everyday menu does not grow with the
        // number of computers sharing the monitor.
        assert_eq!(submenu_count(&menu), 1);
    }

    #[test]
    fn a_switch_routes_by_monitor_id_not_by_display_name() {
        // `switch_input` looks a monitor up by id. Carrying the display name
        // here instead made every tray switch fail its lookup and return
        // silently — the menu looked dead and nothing said why.
        let id = r"\\?\DISPLAY#GSM5BF6#5&1a2b3c4d&0&UID4353";
        let state = TrayState {
            groups: vec![group(
                id,
                "27GP950",
                vec![endpoint(0x10, "MacBook Pro"), endpoint(0x12, "Desktop")],
            )],
            tooltip: "tarsier".to_string(),
            ..Default::default()
        };
        let (_menu, built) = build_menu(&state).unwrap();
        for (_, routed, _) in &built.switches {
            assert_eq!(routed, id, "the click must carry the id, not the name");
        }
        assert_ne!(id, "27GP950");
    }

    #[test]
    fn three_computers_are_all_listed() {
        let state = TrayState {
            groups: vec![group(
                "m1",
                "27GP950",
                vec![
                    endpoint(0x10, "MacBook Pro"),
                    endpoint(0x12, "Desktop"),
                    endpoint(0x11, "Console"),
                ],
            )],
            tooltip: "tarsier".to_string(),
            ..Default::default()
        };
        let (menu, built) = build_menu(&state).unwrap();
        assert_eq!(built.switches.len(), 3);
        assert_eq!(submenu_count(&menu), 1);
    }

    #[test]
    fn a_monitor_with_nothing_configured_offers_setup_instead() {
        let state = TrayState {
            tooltip: "tarsier".to_string(),
            ..Default::default()
        };
        let (menu, built) = build_menu(&state).unwrap();
        assert!(built.switches.is_empty(), "nothing to switch to");
        assert_eq!(submenu_count(&menu), 0, "no empty submenu");
        assert!(built.commands.iter().any(|(_, c)| *c == Command::ShowWindow));
    }

    #[test]
    fn every_menu_id_is_unique_so_events_cannot_go_astray() {
        let state = TrayState {
            groups: vec![
                group(
                    "monitor-a",
                    "27GP950",
                    vec![endpoint(0x11, "Laptop"), endpoint(0x12, "Desktop")],
                ),
                group(
                    "monitor-b",
                    "U2723QE",
                    vec![endpoint(0x11, "Console"), endpoint(0x0F, "NAS")],
                ),
            ],
            tooltip: "tarsier".to_string(),
            ..Default::default()
        };
        let (_menu, built) = build_menu(&state).unwrap();
        let mut ids: Vec<&MenuId> = built.switches.iter().map(|(id, ..)| id).collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        // Two monitors both use port 0x11; the ids must still differ.
        assert_eq!(ids.len(), before, "menu ids collide across monitors");
    }
}
