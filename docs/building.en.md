# Building and running

**English** · [简体中文](building.md) · [Back to README](../README.en.md)

## Building from source

You need Rust (version pinned in `mise.toml`) plus the MSVC toolchain and Windows SDK.

```bash
cargo build --release
```

The output is `target/release/tarsier.exe`. Release builds set `windows_subsystem = "windows"`, so no console window appears.

During development, `cargo run` keeps the console and prints logs straight to the terminal.

## Command-line flags

| Flag | Effect |
| --- | --- |
| `--background` | Start in the tray without showing the main window. This is what the autostart entry uses. |
| `--raw-ddc-write` | Elevated helper mode; see below. |

## Single instance

tarsier uses a named mutex to stay single-instance. A second launch exits immediately and brings the existing instance's main window to the front.

## The elevated helper (`--raw-ddc-write`)

The Intel driver only lets elevated processes write to I²C. tarsier itself does not run as administrator; when it needs the Intel private channel it launches a short-lived elevated child process that sends that one command and exits.

That path is handled at the very top of `main`: the child never touches the single-instance lock and opens no windows. It performs one DDC/CI write and returns.

## Autostart

tarsier writes to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` rather than registering a scheduled task, so no administrator rights are needed. The Settings toggle maps directly onto that value.
