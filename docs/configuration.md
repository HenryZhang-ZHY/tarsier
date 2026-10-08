# Configuration reference

**English** · [简体中文](configuration.zh.md) · [Back to README](../README.md)

"Settings folder" on **Settings → Advanced** opens `%APPDATA%\tarsier\` (or wherever `TARSIER_HOME` points; see [Building and running](building.md)). A file there that does not parse — a typo from hand-editing, or one written by a newer version — is renamed to `*.json.broken` and tarsier starts from defaults, so the original is never overwritten:

| File | Contents |
| --- | --- |
| `config.json` | Settings |
| `stats.json` | What each day held: stretches of work, reminders, first and last input, the evening cutoff and anything said when carrying on past it |
| `tarsier.log` | Log file |

Anything you can change in the UI is written back to `config.json`. Everything below can also be edited by hand; the fields that have no UI are marked, and those need a restart to take effect. The file is JSON, missing fields fall back to defaults, and a malformed file is ignored with a warning in the log.

## Top-level fields

### `hotkeys`

Hotkeys in `global-hotkey` syntax; an empty string disables one. These are the four rows under **Settings → Hotkeys**, and recording one there is the usual way to set it: the change is registered immediately, with no restart.

```json
{
  "hotkeys": {
    "toggle_input": "ctrl+alt+I",
    "brightness_up": "ctrl+alt+PageUp",
    "brightness_down": "ctrl+alt+PageDown",
    "break_now": ""
  }
}
```

Forms like `"ctrl+alt+I"` or `"ctrl+shift+F12"`. One combination does one thing: recording one that another row already holds clears the other row, because Windows refuses a combination that is already registered — including by tarsier itself. A registration failure shows its reason under the rows.

### `brightness_step`

How much brightness a hotkey press changes, in percent. Defaults to `10`. **No UI** — edit the file and restart.

### `developer_mode`

Developer mode, also switchable from the UI. See [Developer mode](developer-mode.md).

### `language`

`"en"` (the default) or `"zh"`, also switchable from the UI. Every window and the tray menu are drawn in it. English is the source language, so a string with no Chinese translation shows up in English rather than as a missing label.

### `skin`

Which visual language the main window is drawn in, also switchable from the UI.

| Value | Look |
| --- | --- |
| `"native"` (the default) | Whatever the component library draws out of the box: the system font, soft corners, quiet greys. This is the look tarsier had before there were skins. |
| `"neo_brutalism"` | Warm paper, black ink, solid strokes, hard offset shadows, one yellow accent, and the bundled Space Grotesk face. |

A skin is more than a palette: it owns the canvas, the panels, the type and the controls. It is also the **top-level** appearance choice, because a skin can be drawn for only some modes. Neo-brutalism is a single, definitive light palette by design, so choosing it pins the window to light and the Settings tab reports that instead of offering a picker.

### `theme`

`"light"`, `"dark"`, or `"system"` (the default), also switchable from the UI. It is honoured by any skin that ships both modes. A skin that ships one — neo-brutalism — draws that one whatever this says, and the value is kept, so switching back to a skin that honours it restores the choice exactly. Under `"system"` the value tracks the Windows light / dark setting live.

### `breaks`

```json
{
  "breaks": {
    "enabled": true,
    "work_minutes": 50,
    "break_minutes": 5,
    "snooze_minutes": 5,
    "respect_fullscreen": true
  }
}
```

When `respect_fullscreen` is true, reminders are held while a fullscreen game, video, or presentation is detected. See [Breaks, the evening cutoff and the week](breaks.md).

### `evening`

```json
{
  "evening": {
    "enabled": false,
    "cutoff": "22:00"
  }
}
```

The evening cutoff, also on **Settings → Breaks**. `cutoff` is a time of day from `"12:00"` to `"04:59"`: the day starts again at 05:00, and a morning cutoff would cover the whole working day. A value that is not such a time falls back to `"22:00"` with a warning in the log, rather than costing the rest of the file. See [Breaks, the evening cutoff and the week](breaks.md#the-evening-cutoff).

## `stats.json`

Days are keyed `YYYY-MM-DD` and run from 05:00 to 05:00. The file carries `"version": 2`. A file from before that — no version, days from midnight, skips and snoozes as bare counts — is copied to `stats.v1.json` on the first start, then converted: its sessions are filed under the day they ended in, and the counts, which say nothing about when, are dropped.

## Per-monitor settings: `monitors.<id>`

The key is the monitor id, which the diagnostics panel on the Monitors tab shows. Every field is optional.

### `endpoints`

The computers sharing this monitor, in order, as decimal VCP values:

```json
{ "monitors": { "DEL41A3": { "endpoints": [16, 18, 17] } } }
```

15 = DisplayPort 1, 17 = HDMI 1. Which value maps to which physical port follows the monitor's own reporting and differs between models.

**The order matters**: with three or more computers it is both the number key in the quick-switch panel and the index shown at the start of each row.

With two, the hotkey is a blind flip: it reads the monitor's current input once, when pressed, and switches to the other one. So their order is irrelevant. From three upwards the hotkey opens the quick-switch panel and jumps straight to the chosen port. It deliberately does not cycle: most monitors stop answering this machine's DDC/CI once they are showing a different input, so the second hop would often never arrive.

> Older versions described the same thing as `toggle: [15, 17]`. It is upgraded to `endpoints` on load, and `input_names` is kept as is.

### `local_input`

Which port **this** computer is plugged into. It only drives the "this computer" mark in the UI: every button in the interface says where it will *send* the monitor, never where the monitor currently is, so switching does not depend on this at all.

```json
{ "local_input": 16 }
```

Leave it out and everything still works; the UI just will not mark which row is the machine you are sitting at.

> The UI deliberately never shows which computer the monitor is on. That value can only be had by polling the monitor, and the other computer can change it at any moment — so rather than display a state that may already be wrong, tarsier only offers "switch to X" buttons. A button that names its destination cannot lie.

### `input_names`

What each computer is called, keyed by decimal VCP value:

```json
{ "input_names": { "15": "Desktop", "17": "Laptop" } }
```

The names appear in the main window, the tray menu and the quick-switch panel. They are the only thing that makes "which one is which" answerable, and they replace the old A / B letters. The port-to-name mapping is absolute — DisplayPort 2 means the same machine on every computer — so one set of names holds everywhere.

Edit them directly in the main window; there is no need to hand-write JSON.

### `extra_inputs`

Input values a monitor fails to report. Some monitors connect over USB-C but never advertise 27 (0x1B); adding it here makes it selectable:

```json
{ "extra_inputs": [27] }
```

### `input_protocol`

Forces how inputs are switched. You normally leave this out — tarsier detects it.

- `{"kind": "mccs"}`: the standard VCP 0x60.
- `{"kind": "lg", "values": {"16": 210}}`: the LG private channel. `values` maps decimal MCCS input values onto LG's numbers and overrides the built-in table.

Background: [Vendor-private channels](private-channels.md).

## Full example

```json
{
  "hotkeys": {
    "toggle_input": "ctrl+alt+I",
    "brightness_up": "ctrl+alt+PageUp",
    "brightness_down": "ctrl+alt+PageDown",
    "break_now": ""
  },
  "brightness_step": 10,
  "developer_mode": false,
  "language": "en",
  "breaks": {
    "enabled": true,
    "work_minutes": 50,
    "break_minutes": 5,
    "snooze_minutes": 5,
    "respect_fullscreen": true
  },
  "monitors": {
    "DEL41A3": {
      "endpoints": [16, 18, 17],
      "local_input": 16,
      "input_names": { "16": "Laptop", "17": "Desktop", "18": "Console" },
      "extra_inputs": [27]
    }
  }
}
```
