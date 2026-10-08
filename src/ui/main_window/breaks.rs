//! The Breaks tab: where this stretch of work stands, how the week went, and
//! the two habits it adds up to.
//!
//! Nothing on it is a score. The week is drawn as it happened — when each
//! stretch of work ran, which ones ran long, when each evening ended — and the
//! habits are said in plain numbers, so the user can see their own rhythm
//! rather than be graded on it.

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{MainWindow, Section};
use crate::breaks::Phase;
use crate::controller::Controller;
use crate::evening::{self, Status};
use crate::i18n::{tr, translate};
use crate::skin::{Control, Mark, Tone, Voice};
use crate::stats::{self, DayRecord, OVERRUN_PERCENT, Session, WEEK};
use crate::ui::format_minutes;
use crate::ui::kit::{self, skin};

/// English is the source, translated where drawn; Monday first, as chrono
/// numbers them.
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// The width of the day names down the left of the week.
const DAY_COLUMN: Pixels = px(76.);
/// The width of the "last use" times down the right.
const LAST_COLUMN: Pixels = px(52.);
const TRACK_HEIGHT: Pixels = px(18.);

impl MainWindow {
    pub(super) fn render_breaks(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        v_flex()
            .gap_5()
            .child(self.render_status(c, cx))
            .child(render_week(c, cx))
            .child(render_habits(c, cx))
            .into_any_element()
    }

    fn render_status(&self, c: &Controller, cx: &Context<Self>) -> AnyElement {
        let t = &c.tracker;
        let enabled = c.config.breaks.enabled;
        let paused = c.is_paused();
        let phase = t.phase();

        let (heading, detail) = match phase {
            _ if !enabled => (
                tr!("Break reminders are off").to_string(),
                tr!("Turn them back on in Settings to get a reminder after each stretch of work.").to_string(),
            ),
            Phase::Away => (
                tr!("You stepped away").to_string(),
                tr!("A fresh timer starts when you come back.").to_string(),
            ),
            Phase::Prompted { .. } => (
                tr!("Taking a break").to_string(),
                tr!("The countdown runs while you are away from the keyboard and mouse.").to_string(),
            ),
            Phase::Working => (
                tr!("Working for {time}", time = format_minutes(t.session_active())),
                if paused {
                    tr!("Reminders are paused for the next hour.").to_string()
                } else {
                    tr!("When the time is up, a reminder fades in over every screen. Stepping away counts as a break on its own.")
                        .to_string()
                },
            ),
        };

        // The one number this card exists for, beside the heading.
        let countdown = match phase {
            _ if !enabled || paused => None,
            Phase::Prompted { .. } => Some(tr!("{time} left", time = format_minutes(t.rest_remaining()))),
            Phase::Working => Some(tr!("Break in {time}", time = format_minutes(t.until_prompt()))),
            Phase::Away => None,
        };

        let work = t.settings().work_secs.max(1);
        let progress = t.session_active() as f32 / work as f32;
        let over = t.session_active() > work;

        let actions = if matches!(phase, Phase::Prompted { .. }) {
            let (snooze, skip) = (self.controller.clone(), self.controller.clone());
            h_flex()
                .gap_3()
                .flex_wrap()
                .child(
                    kit::button("snooze", Tone::Primary, Control::Prominent, cx)
                        .label(tr!(
                            n = c.config.breaks.snooze_minutes,
                            "Snooze for 1 min" | "Snooze for {n} min"
                        ))
                        .on_click(move |_, _, cx| snooze.update(cx, |c, cx| c.snooze(cx))),
                )
                .child(
                    kit::button("skip", Tone::Default, Control::Prominent, cx)
                        .label(tr!("Skip this break"))
                        .on_click(move |_, _, cx| skip.update(cx, |c, cx| c.skip(cx))),
                )
        } else {
            let (now, pause) = (self.controller.clone(), self.controller.clone());
            h_flex()
                .gap_3()
                .flex_wrap()
                .child(
                    kit::button_if(
                        "break-now",
                        Tone::Primary,
                        Control::Prominent,
                        enabled && phase == Phase::Working,
                        cx,
                    )
                    .icon(Icon::new(Lucide::Coffee))
                    .label(tr!("Take a break now"))
                    .on_click(move |_, _, cx| now.update(cx, |c, cx| c.break_now(cx))),
                )
                .child(
                    kit::button_if("pause", Tone::Default, Control::Prominent, enabled, cx)
                        .label(if paused {
                            tr!("Resume reminders")
                        } else {
                            tr!("Pause for 1 hour")
                        })
                        .on_click(move |_, _, cx| pause.update(cx, |c, cx| c.toggle_pause(cx))),
                )
        };

        let b = &c.config.breaks;
        let cadence = h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(kit::hint(
                tr!(
                    "Every {work} min of work, a {rest} min break. Snoozing waits {snooze} min.",
                    work = b.work_minutes,
                    rest = b.break_minutes,
                    snooze = b.snooze_minutes
                ),
                cx,
            ))
            .child(
                kit::button("change-cadence", Tone::Ghost, Control::Compact, cx)
                    .label(tr!("Change"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_section(Section::Breaks, cx))),
            );

        skin(cx)
            .card(cx)
            .gap_3()
            .child(kit::card_header(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(Lucide::Timer))
                    .child(div().text_lg().font_weight(skin(cx).weight(Voice::Loud)).child(heading))
                    .into_any_element(),
                countdown.map(|text| skin(cx).chip(&text, Mark::Accent, cx)),
            ))
            .when(enabled, |el| {
                el.child(skin(cx).meter(progress, if over { Mark::Poor } else { Mark::Accent }, cx))
            })
            .child(kit::body(detail, cx))
            .child(actions)
            .child(skin(cx).divider(cx))
            .child(cadence)
            .children(evening_line(c).map(|text| kit::hint(text, cx)))
            .into_any_element()
    }
}

/// Where tonight's cutoff stands, when it is on.
fn evening_line(c: &Controller) -> Option<String> {
    let evening = &c.config.evening;
    if !evening.enabled {
        return None;
    }
    let now = c.evening_now();
    let at = evening::cutoff_at(stats::day_of(now), evening.cutoff);
    let time = evening.cutoff.to_string();
    Some(match c.evening {
        Status::Cutoff { .. } | Status::KeepingOn { .. } => tr!("Evening cutoff at {time}: it is here.", time = time),
        _ => {
            let left = (at - now).num_seconds().max(0) as u64;
            tr!(
                "Evening cutoff at {time}, in {left}.",
                time = time,
                left = format_minutes(left)
            )
        }
    })
}

/// A day as a person names it: today, yesterday, or its weekday.
fn day_name(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        tr!("Today").to_string()
    } else if Some(date) == today.pred_opt() {
        tr!("Yesterday").to_string()
    } else {
        translate(WEEKDAYS[date.weekday().num_days_from_monday() as usize]).to_string()
    }
}

/// Hours since the start of `day` (05:00), the week's one horizontal scale.
fn hours_into(day: NaiveDate, at: NaiveDateTime) -> f32 {
    (at - stats::day_start(day)).num_seconds() as f32 / 3600.
}

fn hours_of(day: NaiveDate, ts: i64) -> f32 {
    hours_into(day, stats::local_time(ts))
}

/// The hours the week's rows show, from the earliest work to the latest use
/// or cutoff, in whole hours since 05:00. Never under six hours, so a quiet
/// week is not stretched into a few fat bars.
pub fn axis(spans: impl IntoIterator<Item = (f32, f32)>) -> (u32, u32) {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for (start, end) in spans {
        lo = lo.min(start);
        hi = hi.max(end);
    }
    if lo > hi {
        // Nothing yet: 08:00 to 23:00.
        return (3, 18);
    }
    let lo = lo.floor().clamp(0., 23.) as u32;
    let hi = (hi.ceil().clamp(1., 24.) as u32).max(lo + 1);
    if hi - lo >= 6 {
        (lo, hi)
    } else {
        let hi = (lo + 6).min(24);
        (hi - 6, hi)
    }
}

/// One day of the week, in hours since its 05:00.
struct Row {
    date: NaiveDate,
    /// Each stretch of work and whether it ran over.
    stretches: Vec<(f32, f32, bool)>,
    cutoff: Option<f32>,
    last_input: Option<f32>,
    /// The time of day of the last input, for the right-hand column.
    last_clock: Option<String>,
    said: Vec<String>,
}

fn row(date: NaiveDate, day: &DayRecord, open: Option<&Session>, today: NaiveDate) -> Row {
    let clock = |ts: i64| stats::local_time(ts).format("%H:%M").to_string();
    Row {
        date,
        stretches: day
            .sessions_with(open)
            .map(|s| (hours_of(date, s.start), hours_of(date, s.end), s.overran()))
            .collect(),
        cutoff: day
            .evening
            .as_ref()
            .map(|e| hours_into(date, evening::cutoff_at(date, e.cutoff))),
        last_input: day.last_input.map(|ts| hours_of(date, ts)),
        // Today's last input is a moment ago; "last use" means once a day is done.
        last_clock: day.last_input.filter(|_| date != today).map(clock),
        said: day
            .evening
            .iter()
            .flat_map(|e| &e.extensions)
            .map(|e| tr!("{time} “{reason}”", time = clock(e.at), reason = e.reason.clone()))
            .collect(),
    }
}

/// The week, a row a day, today on top.
fn render_week(c: &Controller, cx: &App) -> AnyElement {
    let now = c.evening_now();
    let today = stats::day_of(now);
    let open = c.open_session();
    let empty = DayRecord::default();
    let rows: Vec<Row> = (0..WEEK)
        .filter_map(|ago| today.checked_sub_days(Days::new(ago)))
        .map(|date| {
            let day = c.stats.day(date).unwrap_or(&empty);
            row(date, day, open.as_ref().filter(|_| date == today), today)
        })
        .collect();
    let has_data = rows.iter().any(|r| !r.stretches.is_empty() || r.last_input.is_some());

    let (lo, hi) = axis(rows.iter().flat_map(|r| {
        let mut spans: Vec<(f32, f32)> = r.stretches.iter().map(|&(a, b, _)| (a, b)).collect();
        spans.extend(r.cutoff.map(|h| (h, h)));
        spans.extend(r.last_input.map(|h| (h, h)));
        spans
    }));
    let span = (hi - lo) as f32;
    let x = move |h: f32| ((h - lo as f32) / span).clamp(0., 1.);

    let step = if hi - lo > 12 { 3 } else { 2 };
    let ticks = (lo..=hi)
        .filter(|h| (h + stats::DAY_START_HOUR).is_multiple_of(step))
        .map(|h| {
            div()
                .absolute()
                .top_0()
                .left(relative(x(h as f32)))
                .ml(px(-12.))
                .w(px(24.))
                .text_center()
                .child(kit::hint(format!("{:02}", (h + stats::DAY_START_HOUR) % 24), cx))
        });
    let header = h_flex()
        .gap_3()
        .child(div().w(DAY_COLUMN).flex_none())
        .child(div().flex_1().min_w_0().relative().h(px(16.)).children(ticks))
        .child(
            div()
                .w(LAST_COLUMN)
                .flex_none()
                .text_right()
                .child(kit::hint(tr!("Last use"), cx)),
        );

    let skin = skin(cx);
    let work_fill = skin.fill(Mark::Accent, cx);
    let over_fill = skin.fill(Mark::Poor, cx);
    let late_fill = skin.fill(Mark::Fair, cx);
    let line = cx.theme().foreground.opacity(0.7);

    let lines = rows.iter().map(|r| {
        let bars = r.stretches.iter().map(|&(a, b, overran)| {
            let (a, b) = (x(a), x(b));
            skin.stretch(if overran { Mark::Poor } else { Mark::Accent }, cx)
                .absolute()
                .top(px(3.))
                .bottom(px(3.))
                .left(relative(a))
                .w(relative((b - a).max(0.)))
                .min_w(px(2.))
        });
        let late = r
            .cutoff
            .zip(r.last_input)
            .filter(|(cutoff, last)| last > cutoff)
            .map(|(cutoff, last)| {
                div()
                    .absolute()
                    .bottom_0()
                    .h(px(3.))
                    .left(relative(x(cutoff)))
                    .w(relative((x(last) - x(cutoff)).max(0.)))
                    .bg(late_fill)
            });
        let cutoff = r.cutoff.map(|h| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(x(h)))
                .w(px(2.))
                .bg(line)
        });
        let track = div()
            .flex_1()
            .min_w_0()
            .relative()
            .h(TRACK_HEIGHT)
            .rounded(px(3.))
            .bg(cx.theme().muted)
            .children(bars)
            .children(late)
            .children(cutoff);
        v_flex()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .w(DAY_COLUMN)
                            .flex_none()
                            .child(kit::label(day_name(r.date, today), cx).text_xs()),
                    )
                    .child(track)
                    .child(
                        div()
                            .w(LAST_COLUMN)
                            .flex_none()
                            .text_right()
                            .child(kit::hint(r.last_clock.clone().unwrap_or_default(), cx)),
                    ),
            )
            .when(!r.said.is_empty(), |el| {
                el.child(
                    div()
                        .pl(DAY_COLUMN + px(12.))
                        .pr(LAST_COLUMN + px(12.))
                        .child(kit::hint(tr!("Carried on: {said}", said = r.said.join(" · ")), cx)),
                )
            })
    });

    let legend = |fill: Hsla, text: String| {
        h_flex()
            .gap_1p5()
            .items_center()
            .child(skin.swatch(fill, cx))
            .child(kit::hint(text, cx))
    };
    let limit = c.config.breaks.work_minutes as u64 * OVERRUN_PERCENT / 100 * 60;

    skin.card(cx)
        .id("week")
        .test_support()
        .gap_3()
        .child(skin.eyebrow(tr!("This week"), cx))
        .when(!has_data, |el| {
            el.child(kit::hint(tr!("The week fills in as you use the computer."), cx))
        })
        .child(header)
        .children(lines)
        .child(
            h_flex()
                .gap_4()
                .flex_wrap()
                .child(legend(work_fill, tr!("Work stretch").to_string()))
                .child(legend(over_fill, tr!("Over {time}", time = format_minutes(limit))))
                .child(legend(line, tr!("Evening cutoff").to_string()))
                .child(legend(late_fill, tr!("Use after the cutoff").to_string())),
        )
        .into_any_element()
}

/// The two habits, each said as a plain fact about the week.
fn render_habits(c: &Controller, cx: &App) -> AnyElement {
    let now = c.evening_now();
    let today = stats::day_of(now);
    let open = c.open_session();
    let habits = c.stats.habits(today, now, open.as_ref());
    let limit = c.config.breaks.work_minutes as u64 * OVERRUN_PERCENT / 100 * 60;

    let mut rhythm = if habits.rhythm.of == 0 {
        tr!("Nothing to count yet: a day counts once it has 15 minutes of work.").to_string()
    } else {
        tr!(
            n = habits.rhythm.of,
            "Of 1 day at the computer this week, {kept} had no stretch of work over {limit}."
                | "Of {n} days at the computer this week, {kept} had no stretch of work over {limit}.",
            kept = habits.rhythm.kept,
            limit = format_minutes(limit)
        )
    };
    if let Some((date, secs)) = habits.longest {
        rhythm.push(' ');
        let time = format_minutes(secs);
        rhythm.push_str(&if date == today {
            tr!("The longest stretch was {time}, today.", time = time)
        } else if Some(date) == today.pred_opt() {
            tr!("The longest stretch was {time}, yesterday.", time = time)
        } else {
            tr!(
                "The longest stretch was {time}, on {day}.",
                time = time,
                day = day_name(date, today)
            )
        });
    }

    let evening = &c.config.evening;
    let evenings = if habits.evenings.of > 0 {
        tr!(
            n = habits.evenings.of,
            "You stopped in time on {kept} of 1 evening this week."
                | "You stopped in time on {kept} of {n} evenings this week.",
            kept = habits.evenings.kept
        )
    } else if evening.enabled {
        tr!(
            "No evening to count yet. Tonight counts once {time} has passed.",
            time = evening.cutoff.to_string()
        )
    } else {
        tr!("Off. Turn it on in Settings, and your evenings show up here.").to_string()
    };

    let habit = |icon: Lucide, name: &'static str, text: String| {
        h_flex()
            .gap_3()
            .items_start()
            .child(
                div()
                    .pt_0p5()
                    .child(Icon::new(icon).size(px(16.)).text_color(cx.theme().muted_foreground)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(kit::label(name, cx))
                    .child(kit::body(text, cx)),
            )
    };

    skin(cx)
        .card(cx)
        .gap_4()
        .child(skin(cx).eyebrow(tr!("Habits"), cx))
        .child(habit(Lucide::Timer, tr!("Break rhythm"), rhythm))
        .child(habit(Lucide::Moon, tr!("Evening cutoff"), evenings))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::axis;

    #[test]
    fn the_week_spans_from_its_first_work_to_its_last_use() {
        assert_eq!(axis([(3.5, 6.0), (4.0, 16.2)]), (3, 17));
        assert_eq!(axis([]), (3, 18), "an empty week shows a plain day");
        assert_eq!(axis([(10.0, 11.0)]), (10, 16), "never narrower than six hours");
        assert_eq!(axis([(20.0, 23.5)]), (18, 24), "nor past the end of the day");
        assert_eq!(axis([(-1.0, 30.0)]), (0, 24), "nor outside it");
    }
}
