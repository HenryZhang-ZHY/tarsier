# Known limitations

**English** · [简体中文](limitations.md) · [Back to README](../README.en.md)

## Monitor control

- Only DDC/CI on external monitors. Built-in laptop panels and monitors behind some USB docks can't be adjusted.
- Many monitors require DDC/CI to be enabled in their OSD menu before tarsier can talk to them.
- Input switching over a vendor-private channel doesn't work on AMD GPUs yet.
- On Intel integrated graphics, every switch over the private channel needs a UAC confirmation — see [Vendor-private channels](private-channels.en.md).

## One monitor, two computers

Install tarsier on both machines and pick the same A / B on each. Most monitors respond to DDC/CI commands on an input even while they're displaying a different one, so `Ctrl+Alt+I` on either computer will switch. If yours ignores commands on the inactive input, you can only switch from whichever computer is currently driving the display.

## Platform

Windows only. Everything beyond DDC/CI — the tray, hotkeys, autostart, the overlay window — calls Win32 directly.
