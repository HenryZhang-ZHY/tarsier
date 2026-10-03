# Known limitations

**English** · [简体中文](limitations.md) · [Back to README](../README.en.md)

## Monitor control

- Only DDC/CI on external monitors. Built-in laptop panels and monitors behind some USB docks can't be adjusted.
- Many monitors require DDC/CI to be enabled in their OSD menu before tarsier can talk to them.
- Input switching over a vendor-private channel doesn't work on AMD GPUs yet.
- On Intel integrated graphics, every switch over the private channel needs a UAC confirmation — see [Vendor-private channels](private-channels.en.md).

## One monitor, several computers

Install tarsier on each machine and list the computers sharing the monitor on the Monitors tab, with a name for each. Names belong to the port — DisplayPort 2 means the same computer everywhere — so once one machine is set up its config can be copied to the rest.

Most monitors respond to DDC/CI commands on an input even while they're displaying a different one, so `Ctrl+Alt+I` on any of them will switch. If yours ignores commands on the inactive input, you can only switch from whichever computer is currently driving the display.

With **two** computers the hotkey is a blind flip: one press, no thinking. With **three or more** it opens the quick-switch panel so you can name the destination by number. It deliberately does not cycle, because cycling would have to pass through the intermediate machine — two DDC/CI writes, two screen blanks — and on the kind of monitor described above the second hop would never arrive at all.

## Platform

Windows only. Everything beyond DDC/CI — the tray, hotkeys, autostart, the overlay window — calls Win32 directly.
