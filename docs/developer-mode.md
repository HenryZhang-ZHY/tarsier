# Developer mode

**English** · [简体中文](developer-mode.zh.md) · [Back to README](../README.md)

Turn it on from the Settings tab, or set `developer_mode` to `true` in `config.json`.

Once enabled:

- Each monitor on the Monitors tab gains a diagnostics panel showing:
  - the GPU driving it
  - its EDID
  - the capability string it reports
  - which input protocol was chosen, and why
  - the result from each raw I²C backend, with a reason when one fails
  - the most recent commands and their results
- The log records every DDC/CI command, not just the failures.
- The "Copy diagnostics report" button copies all of the above plus the relevant config as text.

## Filing an issue

Reproduce the problem, hit "Copy diagnostics report", and paste the text into the issue. It carries nothing personal — just the monitor's EDID, its capability string, and the matching `monitors.<id>` config fragment.

The report header states the tarsier version, system architecture, current time, and whether the process is running elevated — that last one matters when debugging Intel UAC problems.
