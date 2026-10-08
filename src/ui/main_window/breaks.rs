//! The Breaks tab: where this stretch of work stands, and how the days behind
//! it went. Breaks and their scores share a page because the score is nothing
//! but the sessions the timer above it produces.

use chrono::{Days, Local, TimeZone};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{MainWindow, Section};
use crate::breaks::{BreakKind, Phase};
use crate::controller::{Controller, now_ts};
use crate::i18n::tr;
use crate::skin::{Control, Mark, Tone, Voice};
use crate::stats::{self, GOOD_SCORE, Outcome, activity_day};
use crate::ui::format_minutes;
use crate::ui::kit::{self, skin};

/// How a score reads.
pub fn score_mark(score: Option<u8>) -> Mark {
    match score {
        Some(s) if s >= GOOD_SCORE => Mark::Good,
        Some(s) if s >= 50 => Mark::Fair,
        Some(_) => Mark::Poor,
        None => Mark::Plain,
    }
}

/// The height of the tallest bar in the week chart.
const CHART_HEIGHT: f32 = 112.;

impl MainWindow {
    pub(super) fn render_breaks(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.controller.read(cx);
        v_flex()
            .gap_5()
            .child(self.render_status(c, cx))
            .child(render_summary(c, cx))
            .child(render_today(c, cx))
            .child(render_sessions(c, cx))
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
            .into_any_element()
    }
}

/// Streak, points and title: what a glance at this page is for.
fn render_summary(c: &Controller, cx: &App) -> AnyElement {
    let today = activity_day(now_ts());
    let streak = c.stats.streak(today, c.today_score());
    let points = c.stats.total_points();
    let (level, floor, next) = stats::level(points);
    let to_next = match next {
        Some(next) => tr!(
            n = next - points,
            "1 point to the next title" | "{n} points to the next title"
        ),
        None => tr!("The highest title there is").to_string(),
    };
    let fraction = next.map_or(1., |next| (points - floor) as f32 / (next - floor) as f32);

    let tile = |icon: Lucide, caption: &'static str, value: String| {
        skin(cx)
            .card(cx)
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(icon).size(px(14.)))
                    .child(kit::hint(caption, cx)),
            )
            .child(div().text_xl().font_weight(skin(cx).weight(Voice::Loud)).child(value))
    };

    h_flex()
        .gap_4()
        .items_start()
        .child(tile(
            Lucide::Flame,
            tr!("Streak"),
            tr!(n = streak, "1 day" | "{n} days"),
        ))
        .child(tile(Lucide::Trophy, tr!("Points"), points.to_string()))
        .child(
            tile(Lucide::Activity, tr!("Title"), level.to_string())
                .child(div().pt_1().child(skin(cx).meter(fraction, Mark::Accent, cx)))
                .child(kit::hint(to_next, cx)),
        )
        .into_any_element()
}

/// Today's score and the week it sits in.
fn render_today(c: &Controller, cx: &App) -> AnyElement {
    let t = &c.tracker;
    let today = activity_day(now_ts());
    let empty = Default::default();
    let day = c.stats.day(today).unwrap_or(&empty);
    let score = c.today_score();
    let mark = score_mark(score);

    let headline = h_flex()
        .gap_3()
        .items_center()
        .child(
            div()
                .text_size(px(44.))
                .line_height(px(48.))
                .font_weight(skin(cx).weight(Voice::Loud))
                .text_color(skin(cx).ink(mark, cx))
                .child(score.map_or("—".to_string(), |s| s.to_string())),
        )
        .child(match score {
            Some(s) => v_flex()
                .gap_1()
                .child(h_flex().child(skin(cx).chip(&tr!("Grade {g}", g = stats::grade(s)), mark, cx)))
                .child(kit::hint(tr!("out of 100"), cx)),
            None => v_flex().child(kit::hint(tr!("Scoring starts after 15 minutes of use"), cx)),
        });

    let metric = |caption: &'static str, value: String| {
        v_flex()
            .flex_1()
            .min_w(px(110.))
            .gap_0p5()
            .child(kit::hint(caption, cx))
            .child(kit::label(value, cx))
    };
    let metrics = h_flex()
        .gap_4()
        .flex_wrap()
        .child(metric(
            tr!("Screen time"),
            format_minutes(day.active_secs() + t.session_active()),
        ))
        .child(metric(tr!("Breaks"), day.breaks().to_string()))
        .child(metric(
            tr!("Longest stretch"),
            format_minutes(day.longest_secs().max(t.session_active())),
        ))
        .child(metric(tr!("Points today"), format!("+{}", day.points())));

    // Oldest first. The height and the number carry the score; the colour only
    // says whether the day cleared the good line.
    let bars = h_flex().gap_2().items_end().children((0..7u64).rev().map(|ago| {
        let date = today.checked_sub_days(Days::new(ago)).unwrap_or(today);
        let score = if ago == 0 {
            c.today_score()
        } else {
            c.stats.day(date).and_then(|d| d.score())
        };
        let height = score.map_or(8., |s| (s as f32 / 100. * CHART_HEIGHT).max(8.));
        let mark = match score {
            Some(s) if s >= GOOD_SCORE => Mark::Good,
            Some(_) => Mark::Fair,
            None => Mark::Plain,
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_1()
            .child(kit::hint(score.map(|s| s.to_string()).unwrap_or_default(), cx))
            .child(skin(cx).bar(mark, px(height), cx))
            .child(kit::hint(
                if ago == 0 {
                    tr!("Today").to_string()
                } else {
                    date.format("%m/%d").to_string()
                },
                cx,
            ))
    }));
    let legend = |mark: Mark, text: String| {
        h_flex()
            .gap_1p5()
            .items_center()
            .child(skin(cx).swatch(skin(cx).fill(mark, cx), cx))
            .child(kit::hint(text, cx))
    };

    skin(cx)
        .card(cx)
        .gap_4()
        .child(skin(cx).eyebrow(tr!("Today"), cx))
        .child(headline)
        .child(metrics)
        .child(skin(cx).divider(cx))
        .child(skin(cx).eyebrow(tr!("Last 7 days"), cx))
        .child(div().h(px(CHART_HEIGHT + 44.)).flex().items_end().child(bars.w_full()))
        .child(
            h_flex()
                .gap_4()
                .flex_wrap()
                .child(legend(Mark::Good, tr!("Good ({n}+)", n = GOOD_SCORE)))
                .child(legend(Mark::Fair, tr!("Below {n}", n = GOOD_SCORE)))
                .child(legend(Mark::Plain, tr!("No data").to_string())),
        )
        .child(kit::hint(
            tr!(
                "A session scores full marks up to 110% of {work} minutes and loses more the longer it runs past that. {rest} minutes away from the computer counts as a break.",
                work = c.config.breaks.work_minutes,
                rest = c.config.breaks.break_minutes
            ),
            cx,
        ))
        .into_any_element()
}

/// Every session that ended today, newest first.
fn render_sessions(c: &Controller, cx: &App) -> AnyElement {
    let today = activity_day(now_ts());
    let empty = Default::default();
    let day = c.stats.day(today).unwrap_or(&empty);
    let clock = |ts: i64| {
        Local
            .timestamp_opt(ts, 0)
            .single()
            .map(|d| d.format("%H:%M").to_string())
            .unwrap_or_default()
    };
    let rows = day.sessions.iter().rev().map(|s| {
        let score = s.score();
        h_flex()
            .gap_3()
            .items_center()
            .min_h(px(28.))
            .child(
                div()
                    .w(px(104.))
                    .flex_none()
                    .child(kit::body(format!("{} – {}", clock(s.start), clock(s.end)), cx)),
            )
            .child(
                div()
                    .w(px(88.))
                    .flex_none()
                    .child(kit::body(format_minutes(s.active_secs), cx)),
            )
            .child(div().flex_1().min_w_0().child(kit::hint(
                match s.kind {
                    BreakKind::Natural => tr!("Stepped away"),
                    BreakKind::Prompted => tr!("Reminded"),
                },
                cx,
            )))
            .child(skin(cx).chip(&score.to_string(), score_mark(Some(score)), cx))
    });
    let (skips, snoozes, ignored) = (
        day.count(Outcome::Skipped),
        day.count(Outcome::Snoozed),
        day.count(Outcome::Ignored),
    );
    let tally = (skips + snoozes + ignored > 0).then(|| {
        kit::hint(
            tr!(
                "Today: {skipped} skipped, {snoozed} snoozed, {ignored} ignored",
                skipped = skips,
                snoozed = snoozes,
                ignored = ignored
            ),
            cx,
        )
    });

    skin(cx)
        .card(cx)
        .gap_2()
        .child(skin(cx).eyebrow(tr!("Sessions today"), cx))
        .when(day.sessions.is_empty(), |el| {
            el.child(kit::hint(tr!("No session has ended yet today."), cx))
        })
        .children(rows)
        .children(tally)
        .into_any_element()
}
