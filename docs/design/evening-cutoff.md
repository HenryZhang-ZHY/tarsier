# Design: the evening cutoff, and a rebuilt Breaks page

**English** · [简体中文](evening-cutoff.zh.md)

Status: implemented, 2026-10-08. Where the code departs from this plan is recorded in §9.

## 1. The problem

Using the computer past a certain time in the evening means sleeping badly that night. The user knows when they ought to stop, but when the time comes there is always something in hand, and it slips by.

tarsier looked after the rhythm of the day — how long to work before a break — but had no idea when the day should end.

## 2. Goals

- The user sets a time. tarsier warns once before it, and at the time covers the screens to say it has come.
- To carry on, the user says in a sentence what they still need to do. No hurdle, no limit.
- When each evening ended, and what was said when carrying on, is recorded and shown on the Breaks tab beside the day's rhythm, so the user can see the pattern themselves.
- Take the chance to rebuild the Breaks tab and the history behind it from scratch: from grading the user to showing them their rhythm.

### Not doing

- No shut down, sleep or hibernate on the overlay. What to do with the computer is the user's call.
- No per-weekday rules: one time, every day.
- No anti-cheating: the setting can change at any time and takes effect at once; quitting tarsier costs nothing.
- No dimming the monitors or warming the colours.
- No "how did you sleep?" question in the morning, for now.

## 3. Principles

These came out of the discussion, and decide the details below:

1. **tarsier helps the user build a habit; it does not police them.** It reminds, then respects the decision.
2. **It does not presume what the user does next.** It says how things stand and chooses nothing for them.
3. **The same rhythm every day.** One time, no weekdays and weekends.
4. **Facts, not scores.** "Stopped at 21:52", not "C, 62 points".

## 4. The evening cutoff

### 4.1 The name

In discussion it was called a "curfew". The product does not use that word: a curfew is imposed by someone else. It is the **evening cutoff** (下机时间), and the setting reads as a sentence:

> No computer after [21:00]

### 4.2 The setting

In the **Settings → Breaks** section, under break reminders, because both are the shape of a day.

| Field | Default | Meaning |
| --- | --- | --- |
| `evening.enabled` | `false` | On or off |
| `evening.cutoff` | `"22:00"` | The cutoff, `HH:MM`, from 12:00 to 04:59 |

**Why 22:00.** Sleep medicine names no clock time, only a distance from bed: the American Academy of Sleep Medicine suggests turning electronics off 30 to 60 minutes before, the National Sleep Foundation at least 30 minutes and two hours if possible. So 22:00 is a starting point — an hour before an 11 o'clock bedtime — and the row says so, rather than adding a "bedtime" setting:

> Sleep doctors suggest putting screens away 30 to 60 minutes before bed. Try an hour before you usually go to bed.

These are constants, not settings:

| Constant | Value | Purpose |
| --- | --- | --- |
| `HEADS_UP` | 15 min | How long before the cutoff the heads-up appears |
| `EXTENSION` | 15 min | How long carrying on lasts before the overlay returns |
| `DAY_START_HOUR` | 05:00 | When a day starts, and the cutoff ends |
| `WRAP_UP_GRACE` | 5 min | Stopping within this long after the cutoff is still in time; see §5.3 |

### 4.3 One evening, with a 21:00 cutoff

```
20:45            21:00                       21:15 (if carrying on)   05:00
  │ heads-up       │ overlay                    │ overlay again          │ over
  ▼                ▼                            ▼                        ▼
──┼────────────────┼────────────────────────────┼────────────────────────┼──
```

**20:45: the heads-up.** A small card in the bottom-right of the main display: "Evening cutoff at 21:00", with a countdown ("14 minutes left. Time to start wrapping up."). It takes no focus and stays until 21:00, when the overlay fades in. Its × closes it for the night.

**21:00: the overlay.** Like a break's, a layer fades in over every display, takes no focus and lets clicks through, but darker. It reads:

> **21:00** (large: the time now)
> It is past 21:00, the time you chose to stop using the computer.

On the main display there is one small button, **Keep using** — the only thing on the overlay that takes a click. Unlike a break's, the overlay does not fade away when worked through; it stays until 05:00 unless the user carries on.

**Keep using.** Clicked, the button opens into a field:

> What do you still need to do? [____________] Enter to carry on for 15 minutes · Esc to cancel

It must not be empty; nothing else is checked. The overlay then steps aside for 15 minutes, and comes back quoting the user:

> **21:18**
> At 21:03 you said: "finish the deploy script"

They can carry on again, as often as they like.

### 4.4 Edge cases

| Case | Behaviour |
| --- | --- |
| Turned on, woken or started after the cutoff | The overlay appears at once; within a 15-minute extension, when it runs out |
| Turned on within the 15 minutes before | The heads-up appears at once, unless closed earlier that evening |
| Away from the computer at the heads-up | The card shows anyway, and is there on return |
| Away or asleep while the overlay is up | Nothing; on return, if still within the evening, it is still there |
| Asleep during an extension | The 15 minutes are wall-clock; if they are over on waking, the overlay is back |
| The setting changes during the evening | At once: a later cutoff or turning it off fades the overlay out |
| A cutoff after midnight, e.g. 01:00 | It runs 01:00–05:00, and belongs to the evening before |
| A break reminder comes due under the overlay | Not stacked: the break is held back, as for any suppression |
| During an extension | Break reminders work as usual |
| "Pause for 1 hour" in the tray | Pauses break reminders only; the cutoff has its own "Keep using" |
| `respect_fullscreen` | Does not apply: video and games in the evening are what this is for |
| A game in exclusive fullscreen | May hide the overlay; listed in [Known limitations](../limitations.md) |
| Working under the overlay without "Keep using" | Possible, since clicks pass through; it shows in the evening's last input (§5.2) |

### 4.5 Implementation notes

- **A pure-logic module, `src/evening.rs`,** driven once a second like `breaks.rs`, with no windows in it, so unit tests cover it entirely.
- **Click-through versus a text field.** The full-screen overlay is click-through (`make_click_through`). "Keep using" is a small window of its own over the main display's overlay that is not; it takes focus only when clicked — the user asked for it.
- **The heads-up card** is another small window, after `switch_hud.rs`, that takes no focus. Having a ×, it cannot be click-through; it covers a corner of the screen, which is acceptable.
- The overlay looks like a break's (a full-screen dimmer in the skin's typeface) but is a separate component: the content and the interaction differ, and one component would be mostly branches.

## 5. Data: a new `stats.json`

### 5.1 A day starts at 05:00

Days used to split at midnight: a stretch ending at 00:30 belonged to the next day. That is not how anyone counts a day, and it would split an evening past the cutoff across two. Every record now belongs to a day that starts at **05:00**, which is also when the cutoff ends:

```rust
/// The day a Unix timestamp belongs to: before 05:00, the day before.
pub fn activity_day(ts: i64) -> NaiveDate
```

### 5.2 What a day records

```jsonc
{
  "version": 2,
  "days": {
    "2026-10-08": {
      // The day's first and last keyboard or mouse input (Unix seconds)
      "first_input": 1791432000,
      "last_input": 1791471120,
      // Stretches of work, as before
      "sessions": [
        { "start": 0, "end": 0, "active_secs": 0, "target_secs": 0, "kind": "prompted" }
      ],
      // Each break reminder and what became of it, with its time, instead of three tallies
      "reminders": [
        { "at": 0, "outcome": "rested" }   // rested | snoozed | skipped | ignored
      ],
      // Present on days the cutoff was on
      "evening": {
        "cutoff": "21:00",                 // the cutoff in force that day; the setting may move
        "extensions": [
          { "at": 0, "reason": "finish the deploy script" }
        ]
      }
    }
  }
}
```

- `last_input` is `now - idle`, worked out every second.
- What the user writes when carrying on stays in the local `stats.json`; it is sent nowhere.

### 5.3 What counts as kept

These only state facts on the page; nothing becomes a score.

- **Break rhythm:** no stretch of the day ran past 150% of the target. With the default 50 minutes, the line is at 75. By the reminder rules: the first reminder at 50, ignored it fades at 60, the second comes at 65 and fades at 75. So putting one or two reminders off is not a broken rhythm — what you were doing needs finishing — but working straight through two is an overlong stretch. A share rather than minutes, so the line follows the user's own target.
- **Evening cutoff:** `last_input` no later than the cutoff plus `WRAP_UP_GRACE` (5 minutes). Shutting down takes a few clicks of its own, and finishing the sentence you were typing when the overlay came should not count against you.
- Days the cutoff was off do not count for it; days with under 15 minutes of work count for neither.

### 5.4 Upgrading

- On start, a `stats.json` with no `version` is first copied to `stats.v1.json`, then converted.
- Its `sessions` are filed again by `activity_day(session.end)`.
- The `skips`, `snoozes` and `ignored` tallies have no time and cannot become `reminders`; they are dropped.
- Points, titles and grades were always computed, never stored, and go with the code.

## 6. The new Breaks page

### 6.1 Gone

- The 0–100 health score and S–D grades
- Points and titles ("Sedentary starter" to "Tarsier grandmaster")
- The streak of good days
- "Health 87 today" in the tray tooltip

Why: points rewarded using the computer more; titles only ever went up; grades and a health score are a school report. A streak resets to zero at the first slip, wiping out everything kept before it — exactly the supervisor's frame of mind.

### 6.2 In their place

Three parts, top to bottom:

**1. Now.** The status card as before — time to the next break, on a break, away. With the cutoff on, one more line: "Evening cutoff at 21:00, in 2 h 10 min."

**2. The week.** The body of the page: seven rows of timeline, today on top.

```
          09    12    15    18    21    00   Last use
Today      ██ ███ ██  ███ ██ ██        │
Yesterday  ███ ████ ██ ███   ██ │▓▓          21:40
              Carried on: 21:05 "finish the deploy script"
Tue        ██ ██ ███ ██ █████   ██│          20:50
…
```

- Across is the time of day. The range fits the week: from the earliest work to the latest use or cutoff, never under six hours.
- The blocks are stretches of work; those past 150% of the target are marked in a status colour.
- The vertical line is the cutoff; use after it is underlined.
- Where the user carried on past the cutoff, what they said is under the row.

**3. Two habits**, each a plain statement:

> Break rhythm: Of 6 days at the computer this week, 5 had no stretch of work over 1 h 15 min. The longest stretch was 1 h 34 min, on Tue.
> Evening cutoff: You stopped in time on 4 of 5 evenings this week.

**The tray tooltip** becomes "tarsier · Break in 23 minutes · 3 h 20 min of work today".

### 6.3 Taken off the page

The list of today's sessions: the timeline already shows it.

## 7. Order of work

Each step with its tests, in a commit of its own.

1. `activity_day` and the 05:00 boundary.
2. The `stats.json` v2 format, and the upgrade from v1.
3. `evening.rs`, covering every row of §4.4.
4. The `evening.*` config and the time field on Settings.
5. The heads-up card.
6. The overlay and the "Keep using" window, and holding break reminders back under it.
7. Recording `first_input` / `last_input`, `reminders` and `evening.extensions`.
8. The new Breaks page: the week and the habits, with points, titles, grades and the health score removed.
9. Docs: `breaks`, `configuration`, `limitations` in both languages, and the README.

## 8. Settled

- The heads-up card stays until the cutoff with a countdown, and the user can close it (§4.3).
- No streak; "how many days this week" instead (§6.1).
- The break-rhythm line is at 150% of the target, for the reasons in §5.3.
- The default cutoff is 22:00, with the reason under the setting (§4.2).

## 9. As built

The code follows this plan, except:

- **The time field (§4.2)** is one field (`src/ui/time_field.rs`), not separate hour and minute boxes. Its −/+ step by 15 minutes, and it reads what people type — `2130`, `21`, `21.30`, a full-width colon. Anything outside 12:00–04:59 puts the last value back.
- **The logic (§4.5)** emits no events. `src/evening.rs` is a pure function, `status(now, cutoff, heads-up closed tonight, last time carried on)`, returning what the screen should show this moment; the controller compares it each second and opens or closes windows to match. Waking from sleep, restarting tarsier and changing the setting halfway need no special handling, and the last extension is read from `stats.json`, so a restart keeps its 15 minutes.
- **The returning overlay (§4.3)** says "At 21:03 you said: …", with a time rather than "15 minutes ago", which would be wrong after a night's sleep.
- **Saving `last_input` (§5.2):** it is kept up to date every second, and the file is marked for saving when it moves into a new minute.
- **Small windows' frames.** Windows 11 draws a hairline border and a shadow around small floating windows, which showed as an empty box around the card. The heads-up card and the "Keep using" window have them removed (`platform::remove_frame`); full-screen windows never showed them.
- **The week (§6.2)** has a "Last use" column with the time of each day's last input; today's is blank, since that is a moment ago. What was said when carrying on is shown in full under the row and wraps, rather than being cut short behind a hover.
- **The habits' wording (§6.2)** counts out of the days and evenings that counted this week, not out of a fixed seven: "Of 6 days at the computer this week, 5 had…", "on 4 of 5 evenings".
- **Where the code is:** the cutoff's windows in `src/ui/evening.rs`, the Breaks page in `src/ui/main_window/breaks.rs`, the history in `src/stats.rs`.

## References

- [American Academy of Sleep Medicine: turn electronics off 30–60 minutes before bed](https://aasm.org/americans-are-doomscrolling-at-bedtime-prioritizing-screen-time-over-sleep/)
- [Harvard Health: the National Sleep Foundation's digital curfew](https://content.health.harvard.edu/blog/americans-have-screen-time-until-bedtime/)
