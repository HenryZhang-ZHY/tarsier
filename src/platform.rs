//! Small Windows helpers: idle time, "do not disturb" detection, autostart and
//! single-instance handling.

use std::sync::mpsc::Sender;

use anyhow::Result;
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, WAIT_OBJECT_0,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};
use windows::core::HSTRING;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const BACKGROUND_ARG: &str = "--background";

/// The Run-key value this copy owns. A copy with its own data directory gets a
/// value of its own, so turning autostart on or off there never touches the
/// installed copy's entry.
fn run_value() -> String {
    format!("tarsier{}", crate::config::instance_scope())
}

/// This computer's own name, used as the default label for the machine the
/// user is sitting at. There is no way to learn what the *other* computers on a
/// shared monitor are called — a monitor only reports input numbers — so this
/// is the one endpoint name that never has to be typed.
pub fn local_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|name| name.trim().to_string())
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::i18n::translate("This PC").to_string())
}

/// Seconds since the last keyboard or mouse input in this session.
pub fn idle_secs() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return 0;
        }
        // dwTime is a 32-bit tick count; compare in the same width.
        let now = GetTickCount64() as u32;
        (now.wrapping_sub(info.dwTime) / 1000) as u64
    }
}

/// True while a fullscreen game/video or presentation is running, when a
/// break overlay would be intrusive.
pub fn user_is_busy() -> bool {
    unsafe {
        SHQueryUserNotificationState()
            .map(|s| matches!(s, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE))
            .unwrap_or(false)
    }
}

/// The window the user is currently working in.
///
/// A transient panel has to take focus to receive Esc and the number keys;
/// remembering this first lets it hand focus straight back when it closes, so
/// pressing the hotkey and changing your mind costs nothing.
pub fn foreground_window() -> Option<isize> {
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
}

/// Gives focus back to a window remembered by [`foreground_window`]. Windows
/// only honours this for a window that was foreground recently, which is
/// exactly the case for a panel that lived a few seconds.
pub fn restore_foreground(hwnd: isize) {
    let _ = unsafe { SetForegroundWindow(HWND(hwnd as _)) };
}

/// Turns a window into a Fadetop-style overlay: mouse clicks fall through to
/// the windows underneath, it never takes focus, and it stays on top.
pub fn make_click_through(hwnd: isize) {
    let hwnd = HWND(hwnd as _);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let extra = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | extra.0 as isize);
        // A layered window stays invisible until its attributes are set once.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub fn autostart_enabled() -> bool {
    windows_registry::CURRENT_USER
        .open(RUN_KEY)
        .and_then(|k| k.get_string(run_value()))
        .is_ok()
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let key = windows_registry::CURRENT_USER.create(RUN_KEY)?;
    let value = run_value();
    if enabled {
        let exe = std::env::current_exe()?;
        let mut command = format!("\"{}\" {BACKGROUND_ARG}", exe.display());
        if let Some(home) = crate::config::home_override() {
            command.push_str(&format!(" {} \"{}\"", crate::config::HOME_ARG, home.display()));
        }
        key.set_string(&value, command)?;
    } else if key.get_string(&value).is_ok() {
        key.remove_value(&value)?;
    }
    Ok(())
}

/// Held for the process lifetime; a second launch asks this one to show its window.
pub struct SingleInstance {
    _mutex: HANDLE,
}

pub enum Instance {
    Primary(SingleInstance),
    /// Another instance is running and has been asked to show itself.
    Secondary,
}

/// Claims the single-instance mutex. When primary, `on_activate` receives a
/// message every time a later launch wants the window shown.
///
/// `scope` separates instances that keep their files apart (see
/// [`crate::config::HOME_VAR`]): a development build with its own data
/// directory must be able to run beside the installed copy rather than just
/// bringing the installed copy's window forward.
pub fn claim_single_instance(scope: &str, on_activate: Sender<()>) -> Instance {
    let mutex_name = HSTRING::from(format!(r"Local\TarsierSingleInstance{scope}"));
    let event_name = HSTRING::from(format!(r"Local\TarsierActivate{scope}"));
    unsafe {
        let mutex = CreateMutexW(None, true, &mutex_name);
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        let Ok(mutex) = mutex else {
            return Instance::Primary(SingleInstance {
                _mutex: HANDLE::default(),
            });
        };
        if already {
            if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, &event_name) {
                let _ = SetEvent(event);
                let _ = CloseHandle(event);
            }
            let _ = CloseHandle(mutex);
            return Instance::Secondary;
        }
        if let Ok(event) = CreateEventW(None, false, false, &event_name) {
            let event = event.0 as usize;
            std::thread::spawn(move || {
                let event = HANDLE(event as _);
                while WaitForSingleObject(event, INFINITE) == WAIT_OBJECT_0 {
                    if on_activate.send(()).is_err() {
                        break;
                    }
                }
            });
        }
        Instance::Primary(SingleInstance { _mutex: mutex })
    }
}
