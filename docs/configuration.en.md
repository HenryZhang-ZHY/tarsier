# Configuration reference

**English** · [简体中文](configuration.md) · [Back to README](../README.en.md)

"Open config folder" on the Settings tab points at `%APPDATA%\tarsier\`:

| File | Contents |
| --- | --- |
| `config.json` | Settings |
| `stats.json` | Per-day work session records |
| `tarsier.log` | Log file |

Anything you can change in the UI is written back to `config.json`. The fields below can **only** be edited by hand; restart for changes to take effect. The file is JSON, missing fields fall back to defaults, and a malformed file is ignored with a warning in the log.

## Top-level fields

### `hotkeys`

Hotkeys in `global-hotkey` syntax; an empty string disables one.

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

Forms like `"ctrl+alt+I"` or `"ctrl+shift+F12"`. A registration failure shows its reason on the Settings tab.

### `brightness_step`

How much brightness a hotkey press changes, in percent. Defaults to `10`.

### `developer_mode`

Developer mode, also switchable from the UI. See [Developer mode](developer-mode.en.md).

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

When `respect_fullscreen` is true, reminders are held while a fullscreen game, video, or presentation is detected. See [Breaks and scoring](breaks.en.md).

## Per-monitor settings: `monitors.<id>`

The key is the monitor id, which the diagnostics panel on the Monitors tab shows. Every field is optional.

### `toggle`

The two inputs the hotkey flips between, as decimal VCP values:

```json
{ "monitors": { "DEL41A3": { "toggle": [15, 17] } } }
```

15 = DisplayPort 1, 17 = HDMI 1. Which value maps to which physical port follows the monitor's own reporting and differs between models.

### `input_names`

Custom labels, keyed by decimal VCP value:

```json
{ "input_names": { "15": "Desktop", "17": "Laptop" } }
```

The labels appear on the Monitors tab and in the tray menu.

### `extra_inputs`

Input values a monitor fails to report. Some monitors connect over USB-C but never advertise 27 (0x1B); adding it here makes it selectable:

```json
{ "extra_inputs": [27] }
```

### `input_protocol`

Forces how inputs are switched. You normally leave this out — tarsier detects it.

- `{"kind": "mccs"}`: the standard VCP 0x60.
- `{"kind": "lg", "values": {"16": 210}}`: the LG private channel. `values` maps decimal MCCS input values onto LG's numbers and overrides the built-in table.

Background: [Vendor-private channels](private-channels.en.md).

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
  "breaks": {
    "enabled": true,
    "work_minutes": 50,
    "break_minutes": 5,
    "snooze_minutes": 5,
    "respect_fullscreen": true
  },
  "monitors": {
    "DEL41A3": {
      "toggle": [15, 17],
      "input_names": { "15": "Desktop", "17": "Laptop" },
      "extra_inputs": [27]
    }
  }
}
```
