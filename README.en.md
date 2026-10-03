# tarsier

**English** · [简体中文](README.md)

A small Windows utility for managing monitors and screen time, written in Rust with [GPUI](https://gpui.rs) (using the [gpui-kit](https://gpui-kit.com) component library). Inspired by Twinkle Tray and Fadetop.

It lives in the tray, draws its own title bar, and supports light, dark, and follow-system themes.

## Features

- **Brightness / contrast**: adjust external monitors directly over DDC/CI.
- **One-key input switching**: list the computers sharing each monitor and switch with a hotkey or the tray menu. Two computers flip blind; three or more ask which one first. Monitors that ignore the standard command (recent LGs, for instance) fall back to a vendor-private channel automatically.
- **Break reminders**: after a stretch of work, a translucent overlay fades in across every screen, Fadetop-style. It never steals focus and clicks pass straight through. The countdown only runs while you are actually away from the keyboard and mouse.
- **Scoring**: every work session is scored by length, with points, titles, and a streak of good days.

## Install

Download `tarsier.exe` from [Releases](https://github.com/HenryZhang-ZHY/tarsier/releases) and run it. No installer needed.

## Usage

It stays in the tray; double-click the tray icon to open the main window.

| Hotkey | Action |
| --- | --- |
| `Ctrl+Alt+I` | Switch monitor input. Two computers: a blind flip. Three or more: a quick-switch panel, jump by number |
| `Ctrl+Alt+PageUp` / `PageDown` | Brightness ±10% on every monitor |

Hotkeys, break durations, and everything else live on the Settings tab of the main window, or in `%APPDATA%\tarsier\config.json`.

## Documentation

- [Building and running](docs/building.en.md) — build from source, command-line flags
- [Configuration reference](docs/configuration.en.md) — every field in `config.json`
- [Vendor-private channels](docs/private-channels.en.md) — why recent LGs need the GPU driver, and the UAC prompt
- [Breaks and scoring](docs/breaks.en.md) — reminder rules, snooze, ignore, how the score is computed
- [Developer mode](docs/developer-mode.en.md) — DDC/CI diagnostics and command tracing
- [Known limitations](docs/limitations.en.md) — when a monitor can't be controlled

## License

MIT. Icons from [Lucide](https://lucide.dev) (ISC).
