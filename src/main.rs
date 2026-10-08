#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod breaks;
mod config;
mod controller;
mod display;
mod evening;
mod i18n;
mod logger;
mod platform;
mod skin;
mod stats;
mod tray;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gpui_kit::component::Theme;
use gpui_kit::*;

use crate::controller::Controller;
use crate::platform::Instance;
use crate::tray::{Command, HotkeyRegistry, Tray};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|a| a == config::HOME_ARG)
        && let Some(home) = args.get(at + 1)
    {
        // SAFETY: nothing else is running yet; every later read of the data
        // directory goes through this one variable.
        unsafe { std::env::set_var(config::HOME_VAR, home) };
    }
    // The elevated helper does one DDC/CI write and exits; it must not
    // touch the single-instance lock or open any window.
    if let Some(at) = args.iter().position(|a| a == display::elevate::HELPER_ARG) {
        logger::init();
        std::process::exit(display::run_helper(&args[at + 1..]));
    }

    let (activate_tx, activate_rx) = mpsc::channel();
    let _instance = match platform::claim_single_instance(&config::instance_scope(), activate_tx) {
        Instance::Primary(instance) => instance,
        Instance::Secondary => return,
    };
    logger::init();
    let background = std::env::args().any(|a| a == platform::BACKGROUND_ARG);

    gpui_kit::application().with_assets(ui::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        // Before any window exists: a family GPUI cannot find falls back in
        // silence, so this cannot be deferred until something needs it.
        skin::register_fonts(cx);
        // GPUI quits when the last window closes on Windows by default; we live
        // in the tray, so only the tray's "Quit" (or logoff) ends the process.
        cx.set_quit_mode(QuitMode::Explicit);
        Theme::sync_system_appearance(None, cx);
        Theme::global_mut(cx).font_family = "Microsoft YaHei UI".into();

        let controller = Controller::init(cx);
        // The saved skin and appearance win over the system appearance we just
        // synced.
        let (saved_skin, saved_theme) = {
            let c = controller.read(cx);
            (c.config.skin, c.config.theme)
        };
        controller::apply_theme(saved_skin, saved_theme, cx);
        let tray = Tray::new().inspect_err(|e| log::error!("tray icon: {e}")).ok();
        // Shared with the controller, which re-registers these in place when a
        // combination is recorded on the Settings tab. A hotkey manager owns a
        // hidden window, and Windows only delivers to the thread that made it,
        // so the registry stays on this one and is only ever borrowed briefly.
        let hotkeys = HotkeyRegistry::new()
            .inspect_err(|e| log::error!("hotkeys: {e}"))
            .ok()
            .map(|mut registry| {
                registry.apply(&controller.read(cx).config.hotkeys);
                Rc::new(RefCell::new(registry))
            });
        controller.update(cx, |c, _| {
            c.hotkey_errors = hotkeys
                .as_ref()
                .map(|registry| registry.borrow().errors().to_vec())
                .unwrap_or_default();
            c.hotkeys = hotkeys.clone();
        });

        // Without a tray icon the window is the only way in, so always show it.
        if !background || tray.is_none() {
            controller.update(cx, |c, cx| c.show_main_window(cx));
        }

        cx.spawn(async move |cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let mut commands = Vec::new();
                if let Some(tray) = &tray {
                    commands.extend(tray.poll());
                }
                if let Some(hotkeys) = &hotkeys {
                    commands.extend(hotkeys.borrow().poll());
                }
                while activate_rx.try_recv().is_ok() {
                    commands.push(Command::ShowWindow);
                }
                let controller = controller.clone();
                cx.update(|cx| {
                    for command in commands {
                        run_command(&controller, command, cx);
                    }
                    ticks += 1;
                    if ticks.is_multiple_of(10)
                        && let Some(tray) = &tray
                    {
                        // One derived snapshot drives the whole menu: the label
                        // of the switch entry, which computer is ticked, and
                        // whether the break items are live.
                        let state = controller.read(cx).tray_state();
                        tray.sync(&state);
                    }
                });
            }
        })
        .detach();
    });
}

fn run_command(controller: &Entity<Controller>, command: Command, cx: &mut App) {
    match command {
        Command::ShowWindow => controller.update(cx, |c, cx| c.show_main_window(cx)),
        // Flips a two-computer monitor, and opens the chooser when flipping is
        // not a question with one answer.
        Command::ToggleInput => controller.update(cx, |c, cx| c.toggle_inputs(cx)),
        Command::SwitchTo(monitor, port) => controller.update(cx, |c, cx| c.switch_input(&monitor, port, cx)),
        Command::BrightnessUp => controller.update(cx, |c, cx| c.nudge_brightness(true, cx)),
        Command::BrightnessDown => controller.update(cx, |c, cx| c.nudge_brightness(false, cx)),
        Command::BreakNow => controller.update(cx, |c, cx| c.break_now(cx)),
        Command::Snooze => controller.update(cx, |c, cx| c.snooze(cx)),
        Command::Skip => controller.update(cx, |c, cx| c.skip(cx)),
        Command::TogglePause => controller.update(cx, |c, cx| c.toggle_pause(cx)),
        // Stats are flushed by the controller's on_app_quit hook.
        Command::Quit => cx.quit(),
    }
}
