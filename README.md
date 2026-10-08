# tarsier

**English** · [简体中文](README.zh.md)

A small Windows utility for managing monitors and screen time, written in Rust with [GPUI](https://gpui.rs) (using the [gpui-kit](https://gpui-kit.com) component library). Inspired by Twinkle Tray and Fadetop.

It lives in the tray, draws its own title bar, supports light, dark, and follow-system themes, and is drawn in English or Chinese.

## Skins

The window is drawn in one of two visual languages, switchable on the Settings tab:

- **Default** — the look tarsier has always had: the system font, soft corners, quiet greys.
- **Neo Brutalism** — warm paper, black ink, 2px strokes and hard offset shadows, set in a bundled copy of Space Grotesk. Colour is spent sparingly: one yellow accent for the thing to press and "you are here", status colours only for stretches that ran long, and a colour per computer. Light only, by design.

A skin owns the whole visual language, not just the colours — the canvas, the cards, the type weights, the marks and the controls — so the two never mix. Adding one means writing a single file that implements `SkinStyle`; no screen has to be touched, because screens ask for "a card" or "a primary button" rather than for a border width.

The quick-switch panel is drawn in the active skin too. The break overlay keeps its own full-screen dimmer and only takes the skin's typeface.

## Features

- **Brightness / contrast**: adjust external monitors directly over DDC/CI.
- **One-key input switching**: list the computers sharing each monitor and switch with a hotkey or the tray menu. Two computers flip blind; three or more ask which one first. Monitors that ignore the standard command (recent LGs, for instance) fall back to a vendor-private channel automatically.
- **Break reminders**: after a stretch of work, a translucent overlay fades in across every screen, Fadetop-style. It never steals focus and clicks pass straight through. The countdown only runs while you are actually away from the keyboard and mouse.
- **Evening cutoff**: set the time after which you would rather not be at the computer. Fifteen minutes before, a card in the corner says it is coming; at the time, a dark layer covers every screen until 05:00. Carrying on is always one sentence away — say what you still need to do — and nothing on it shuts your computer down for you.
- **The week**: no score, no points. The Breaks tab draws each day as it happened — every stretch of work, the ones that ran long, when the evening ended — and says in plain numbers how often you kept to your rhythm and your cutoff.

## Install

Download `tarsier-<version>-windows-x86_64.zip` from [Releases](https://github.com/HenryZhang-ZHY/tarsier/releases), unzip it anywhere and run `tarsier.exe`. No installer needed.

## Usage

It stays in the tray; double-click the tray icon to open the main window.

| Hotkey | Action |
| --- | --- |
| `Ctrl+Alt+I` | Switch monitor input. Two computers: a blind flip. Three or more: a quick-switch panel, jump by number |
| `Ctrl+Alt+PageUp` / `PageDown` | Brightness ±10% on every monitor |

The window has three tabs — **Monitors**, **Breaks** (the timer and the week it adds up to) and **Settings** — and the Settings tab is split into sections down the left (General, Breaks, Displays, Hotkeys, Advanced), so a setting is one click from its name rather than somewhere in a long column.

**Hotkeys are recorded in the UI**: click Record next to one, press the combination, and it takes effect immediately. No config file, no restart. Everything else lives on the Settings tab, or in `%APPDATA%\tarsier\config.json`.

## Documentation

- [Building and running](docs/building.md) — build from source, command-line flags
- [Configuration reference](docs/configuration.md) — every field in `config.json`
- [Vendor-private channels](docs/private-channels.md) — why recent LGs need the GPU driver, and the UAC prompt
- [Breaks, the evening cutoff and the week](docs/breaks.md) — reminder rules, snooze, ignore, the cutoff, what the week shows
- [Developer mode](docs/developer-mode.md) — DDC/CI diagnostics and command tracing
- [Known limitations](docs/limitations.md) — when a monitor can't be controlled

## License

MIT. Icons from [Lucide](https://lucide.dev) (ISC). The neo-brutalism skin bundles [Space Grotesk](https://fonts.google.com/specimen/Space+Grotesk) (SIL Open Font License 1.1; the licence and its provenance note are in `assets/fonts/`).
