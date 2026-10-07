# tarsier

**English** · [简体中文](README.zh.md)

A small Windows utility for managing monitors and screen time, written in Rust with [GPUI](https://gpui.rs) (using the [gpui-kit](https://gpui-kit.com) component library). Inspired by Twinkle Tray and Fadetop.

It lives in the tray, draws its own title bar, supports light, dark, and follow-system themes, and is drawn in English or Chinese.

## Skins

The window is drawn in one of two visual languages, switchable on the Settings tab:

- **Default** — the look tarsier has always had: the system font, soft corners, quiet greys.
- **Neo Brutalism** — cream paper, pure ink, thick black strokes and hard offset shadows, set in a bundled copy of Space Grotesk. Light only, by design: the style is a single definitive light palette rather than a theme that happens to be bright.

A skin owns the whole visual language, not just the colours — the canvas and its grid, the panels, the type scale, the badges and the buttons — so the two never mix. Adding one means writing a single file that implements `SkinStyle`; no screen has to be touched, because screens ask for "a card" or "an accent button" rather than for a border width.

The break overlay and the quick-switch panel take the palette from the active skin but keep their own layouts: one is a full-screen dimmer, the other a compact popup, and neither reads as a card.

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

The window has three tabs — **Monitors**, **Breaks & stats** and **Settings** — and the Settings tab is split into sections down the left (General, Breaks, Displays, Hotkeys, Advanced), so a setting is one click from its name rather than somewhere in a long column.

**Hotkeys are recorded in the UI**: click Record next to one, press the combination, and it takes effect immediately. No config file, no restart. Everything else lives on the Settings tab, or in `%APPDATA%\tarsier\config.json`.

## Documentation

- [Building and running](docs/building.md) — build from source, command-line flags
- [Configuration reference](docs/configuration.md) — every field in `config.json`
- [Vendor-private channels](docs/private-channels.md) — why recent LGs need the GPU driver, and the UAC prompt
- [Breaks and scoring](docs/breaks.md) — reminder rules, snooze, ignore, how the score is computed
- [Developer mode](docs/developer-mode.md) — DDC/CI diagnostics and command tracing
- [Known limitations](docs/limitations.md) — when a monitor can't be controlled

## License

MIT. Icons from [Lucide](https://lucide.dev) (ISC). The neo-brutalism skin bundles [Space Grotesk](https://fonts.google.com/specimen/Space+Grotesk) (SIL Open Font License 1.1; the licence and its provenance note are in `assets/fonts/`).
