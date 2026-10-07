# Breaks and scoring

**English** · [简体中文](breaks.zh.md) · [Back to README](../README.md)

## Reminders

**Work time is a plain timer**: every second since your last break counts, with or without keyboard/mouse input (deep reading still strains your eyes). After `work_minutes` of work, tarsier fades a translucent overlay in across every screen, Fadetop-style. It never steals focus and mouse clicks pass straight through, so you can finish what you're doing first.

**The countdown only runs while you are genuinely idle** — jiggling the mouse doesn't count as a break; you have to actually leave the keyboard and mouse. Leaving the keyboard and mouse alone is not treated as leaving — the timer keeps running. Sleeping for longer than `break_minutes` is recorded as a break automatically. If you step away, the reminder still appears and completes by itself once you have been hands-off long enough.

To snooze or skip, use the tray menu or the **Breaks & stats** tab of the main window — which is also where the running timer, today's score and the last seven days are, on one page rather than two.

## Ignoring

If you keep working under the overlay, it fades out on its own at twice `work_minutes`, the session is recorded as **ignored**, and the reminder comes back after `snooze_minutes`.

## Staying out of the way

With `respect_fullscreen` enabled, reminders are held while a fullscreen game, video, or presentation is detected.

## Scoring rules

- A session scoring 100 runs up to 110% of the target. At 150% it is worth 50, and at 190% it reaches 0.
- The daily health score is the length-weighted average of that day's session scores. Days with under 15 minutes of use aren't scored.
- Each break earns 10 / 6 / 2 points from that session's score. Skipping, snoozing, and ignoring cost nothing directly, but they lengthen the session, which lowers the score on its own.
- Consecutive days at or above 80 accumulate as a streak; days you don't use the computer don't break it.

A health score of 80 or above counts as a good day. The data lives in `stats.json`.
