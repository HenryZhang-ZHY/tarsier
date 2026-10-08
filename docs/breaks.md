# Breaks, the evening cutoff and the week

**English** · [简体中文](breaks.zh.md) · [Back to README](../README.md)

## Reminders

**Work time is a plain timer**: every second since your last break counts, with or without keyboard/mouse input (deep reading still strains your eyes). After `work_minutes` of work, tarsier fades a translucent overlay in across every screen, Fadetop-style. It never steals focus and mouse clicks pass straight through, so you can finish what you're doing first.

**The countdown only runs while you are genuinely idle** — jiggling the mouse doesn't count as a break; you have to actually leave the keyboard and mouse. Leaving the keyboard and mouse alone is not treated as leaving — the timer keeps running. Sleeping for longer than `break_minutes` is recorded as a break automatically. If you step away, the reminder still appears and completes by itself once you have been hands-off long enough.

To snooze or skip, use the tray menu or the **Breaks** tab of the main window — which is also where the running timer and the week are, on one page.

## Ignoring

If you keep working under the overlay, it fades out on its own once it has been up for twice `break_minutes`, the reminder is recorded as **ignored**, and the reminder comes back after `snooze_minutes`.

## Staying out of the way

With `respect_fullscreen` enabled, reminders are held while a fullscreen game, video, or presentation is detected.

A held-back reminder (paused, turned off, or fullscreen) still notices a break: once a reminder is due, being hands-off for a full `break_minutes` ends the session as a break, exactly as an unsuppressed reminder would have finished. Coming back then starts a fresh timer rather than reminding you at once.

## The evening cutoff

**Settings → Breaks → Evening cutoff**: one switch and one time, "no computer after 22:00". The same time every day.

- **15 minutes before**, a card in the bottom-right corner counts down to it: time to start wrapping up. It takes no focus. Its × closes it for the night.
- **At the cutoff**, a dark layer fades in over every screen with the time, and stays until 05:00. Like a break's, it takes no focus and lets clicks through.
- It offers **nothing to do with the computer** — no shut down, sleep or hibernate button. What you do next is yours to decide.
- The one thing that takes a click is **Keep using**, on the main screen. It asks what you still need to do; once you say, the layer steps aside for 15 minutes, then comes back quoting you. There is no limit and no penalty.

A few things follow from the cutoff being read off the clock rather than counted down:

- Turning the computer on, waking it or starting tarsier after the cutoff shows the layer straight away. Sleeping through the 15 minutes you asked for brings it back as soon as you return.
- Changing the time, or turning the cutoff off, takes effect at once.
- A cutoff after midnight (say 01:00) belongs to the evening before, and lasts until 05:00.
- `respect_fullscreen` does not hold it back: an evening of video or games is exactly what it is for. A game in exclusive fullscreen can still hide it; see [Known limitations](limitations.md).
- While the layer is up, a break reminder that comes due is held back rather than stacked under it. "Pause for 1 hour" pauses break reminders only.

Sleep doctors put no clock time on it, only a distance from bed — the American Academy of Sleep Medicine suggests turning screens off 30 to 60 minutes before. The default, 22:00, is an hour before an 11 o'clock bedtime; set it an hour before yours.

## The week

The **Breaks** tab shows no score. It shows what happened, and lets you see your own rhythm:

- **The week**, a row a day with today on top. Each stretch of work sits where it ran; a stretch past 150% of `work_minutes` is marked. With the default 50 minutes that line is at 75: putting a reminder off once or twice to finish something is not a broken rhythm, working straight through two is. On days the cutoff was on, a line marks it and any use after it is underlined. On the right is when each day's last use was, and under a day is whatever you said when you carried on.
- **Two habits**, in plain numbers:
  - *Break rhythm*: of the days you used the computer this week, how many had no stretch past that line, and which stretch was the longest.
  - *Evening cutoff*: of the evenings it was on, how many ended in time — the last keyboard or mouse input no later than 5 minutes after the cutoff, since shutting down takes a few clicks of its own. A day you shut down early counts as kept; tonight counts once the cutoff and those 5 minutes have passed.

A day runs from **05:00 to 05:00**, so half past midnight still belongs to the evening before. Days with under 15 minutes of work are left out of the habits. There are no points, titles or streaks — one slip does not wipe out the rest of the week.

The data lives in `stats.json`; see the [Configuration reference](configuration.md).
