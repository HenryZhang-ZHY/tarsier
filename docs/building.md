# Building and running

**English** · [简体中文](building.zh.md) · [Back to README](../README.md)

## Building from source

You need Rust (version pinned in `mise.toml`) plus the MSVC toolchain and Windows SDK.

```bash
cargo build --release
```

The output is `target/release/tarsier.exe`. Release builds set `windows_subsystem = "windows"`, so no console window appears.

During development, `cargo run` keeps the console and prints logs straight to the terminal.

## Tests

```bash
cargo test
```

Besides the unit tests, the main window is tested in a headless GPUI window (gpui-kit's `test-support`): real layout, clicks and keystrokes, no pixels. Those tests pin down what is easy to break by eye — the tab strip centred on the window, nothing pushed past the right edge at the narrowest window, every page scrolling to its end and opening at its top — in both skins and both languages. Another test fails when a `tr!` string has no Chinese translation, or a translation's string is no longer used.

## A separate data directory

Set `TARSIER_HOME` (or pass `--home <dir>`) to keep config, stats and log somewhere other than `%APPDATA%\tarsier`. That copy gets its own single-instance lock and its own autostart entry, so a development build runs beside the installed one without touching its settings:

```powershell
$env:TARSIER_HOME = "$PWD\.dev-home"; cargo run
```

## Command-line flags

| Flag | Effect |
| --- | --- |
| `--background` | Start in the tray without showing the main window. This is what the autostart entry uses. |
| `--home <dir>` | Keep data in `<dir>`, like `TARSIER_HOME`. Autostart passes it for a copy with its own directory. |
| `--raw-ddc-write` | Elevated helper mode; see below. |

## Single instance

tarsier uses a named mutex to stay single-instance. A second launch exits immediately and brings the existing instance's main window to the front.

## The elevated helper (`--raw-ddc-write`)

The Intel driver only lets elevated processes write to I²C. tarsier itself does not run as administrator; when it needs the Intel private channel it launches a short-lived elevated child process that sends that one command and exits.

That path is handled at the very top of `main`: the child never touches the single-instance lock and opens no windows. It performs one DDC/CI write and returns.

## Autostart

tarsier writes to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` rather than registering a scheduled task, so no administrator rights are needed. The Settings toggle maps directly onto that value.
