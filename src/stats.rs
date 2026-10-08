//! Screen-time history: what happened each day, and the plain facts the Breaks
//! page draws from it. Pure logic plus JSON (de)serialisation.
//!
//! Nothing here is a score. A day is not graded; it either kept to the rhythm
//! the user chose or it did not, and the page says which, and how often.

use std::collections::BTreeMap;

use chrono::{DateTime, Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::breaks::{BreakKind, EndedSession};
use crate::evening::{self, ClockTime};

/// Days with less use than this are left out of the habits: a quick look at
/// the computer is not a day at it.
const MIN_COUNTED_SECS: u64 = 15 * 60;
/// A stretch of work longer than this share of the target ran over. With the
/// default 50 minutes that is 75: past one or two put-off reminders, which is
/// what finishing what you were doing takes, and into a second one ignored.
/// A share rather than a number of minutes, so it follows the user's target.
pub const OVERRUN_PERCENT: u64 = 150;
/// How many days the habits look back over, today included.
pub const WEEK: u64 = 7;
/// A day starts at this hour rather than at midnight. Past midnight is still
/// the evening before, as far as anyone's sense of "a day" goes; and the
/// evening cutoff ends here, so a night is never split across two days.
pub const DAY_START_HOUR: u32 = 5;

/// The day a local moment belongs to: before 05:00 it is the day before.
pub fn day_of(at: NaiveDateTime) -> NaiveDate {
    let date = at.date();
    if at.time() < day_start_time() {
        date.pred_opt().unwrap_or(date)
    } else {
        date
    }
}

/// When `day` begins, in local time.
pub fn day_start(day: NaiveDate) -> NaiveDateTime {
    day.and_time(day_start_time())
}

fn day_start_time() -> NaiveTime {
    NaiveTime::from_hms_opt(DAY_START_HOUR, 0, 0).expect("a valid hour")
}

/// A Unix timestamp in local time.
pub fn local_time(ts: i64) -> NaiveDateTime {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d: DateTime<Local>| d.naive_local())
        .unwrap_or_default()
}

/// The day a Unix timestamp belongs to, in local time.
pub fn activity_day(ts: i64) -> NaiveDate {
    day_of(local_time(ts))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub start: i64,
    pub end: i64,
    pub active_secs: u64,
    pub target_secs: u64,
    pub kind: BreakKind,
}

impl From<EndedSession> for Session {
    fn from(s: EndedSession) -> Self {
        Self {
            start: s.start,
            end: s.end,
            active_secs: s.active_secs,
            target_secs: s.target_secs,
            kind: s.kind,
        }
    }
}

impl Session {
    /// Whether this stretch ran well past its target; see [`OVERRUN_PERCENT`].
    pub fn overran(&self) -> bool {
        self.target_secs > 0 && self.active_secs * 100 > self.target_secs * OVERRUN_PERCENT
    }
}

/// What became of a break reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The user stepped away until the break was over.
    Rested,
    Snoozed,
    Skipped,
    /// Worked straight through it until it faded away.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    pub at: i64,
    pub outcome: Outcome,
}

/// The user chose to carry on past the cutoff, and said what for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extension {
    pub at: i64,
    pub reason: String,
}

/// A day the evening cutoff was on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveningRecord {
    /// The cutoff in force that day, which the setting may have moved since.
    pub cutoff: ClockTime,
    #[serde(default)]
    pub extensions: Vec<Extension>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DayRecord {
    /// The first and last keyboard or mouse input of the day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_input: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_input: Option<i64>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    /// Present on the days the cutoff was on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evening: Option<EveningRecord>,
}

impl DayRecord {
    /// Notes input at `ts`. True when the record changed enough to be worth
    /// saving: the day's first input, or a later minute than the last one.
    /// Saving every second would mean writing the file every second.
    pub fn saw_input(&mut self, ts: i64) -> bool {
        let mut changed = false;
        if self.first_input.is_none_or(|first| ts < first) {
            self.first_input = Some(ts);
            changed = true;
        }
        match self.last_input {
            Some(last) if ts <= last => {}
            last => {
                changed |= last.is_none_or(|last| ts.div_euclid(60) != last.div_euclid(60));
                self.last_input = Some(ts);
            }
        }
        changed
    }

    /// Keeps the day's cutoff in step with the setting, `None` when it is
    /// off. True when the record changed.
    ///
    /// A day the cutoff was on at all is a day it counts for: shutting down
    /// before it is exactly the evening it is for, and tarsier never sees the
    /// cutoff arrive then. Turning it off before the evening begins takes the
    /// day back out; once the evening is under way the day keeps its record.
    pub fn follow_cutoff(&mut self, day: NaiveDate, cutoff: Option<ClockTime>, now: NaiveDateTime) -> bool {
        match (cutoff, &mut self.evening) {
            (Some(cutoff), Some(record)) if record.cutoff == cutoff => false,
            (Some(cutoff), Some(record)) => {
                record.cutoff = cutoff;
                true
            }
            (Some(cutoff), None) => {
                self.evening = Some(EveningRecord {
                    cutoff,
                    extensions: Vec::new(),
                });
                true
            }
            (None, Some(record)) if now < evening::cutoff_at(day, record.cutoff) => {
                self.evening = None;
                true
            }
            (None, _) => false,
        }
    }

    /// When the user last chose to carry on past the cutoff this day.
    pub fn last_extension(&self) -> Option<&Extension> {
        self.evening.as_ref()?.extensions.last()
    }

    /// Whether the day's computer use ended in time for its cutoff; `None`
    /// when the cutoff was off. A day with no input at all stopped in time.
    pub fn stopped_in_time(&self, day: NaiveDate) -> Option<bool> {
        let cutoff = self.evening.as_ref()?.cutoff;
        Some(
            self.last_input
                .is_none_or(|last| evening::stopped_in_time(day, cutoff, local_time(last))),
        )
    }

    /// The day's sessions, with `open` — a stretch still running — after them.
    pub fn sessions_with<'a>(&'a self, open: Option<&'a Session>) -> impl Iterator<Item = &'a Session> {
        self.sessions.iter().chain(open)
    }

    /// Whether the day had enough use to count toward the habits.
    pub fn counts(&self, open: Option<&Session>) -> bool {
        self.sessions_with(open).map(|s| s.active_secs).sum::<u64>() >= MIN_COUNTED_SECS
    }

    /// Whether no stretch of the day ran over.
    pub fn kept_rhythm(&self, open: Option<&Session>) -> bool {
        !self.sessions_with(open).any(Session::overran)
    }
}

/// So many days out of so many.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub kept: u32,
    pub of: u32,
}

impl Tally {
    fn add(&mut self, kept: bool) {
        self.of += 1;
        self.kept += u32::from(kept);
    }
}

/// The two habits over the last [`WEEK`] days.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Habits {
    /// Days without a stretch that ran over, of the days that counted.
    pub rhythm: Tally,
    /// The longest stretch of the week: its day and its length.
    pub longest: Option<(NaiveDate, u64)>,
    /// Evenings stopped in time, of the evenings the cutoff was on and over.
    pub evenings: Tally,
}

/// The shape `stats.json` is written in. Version 1 had no number, kept days
/// from midnight and counted skips, snoozes and ignored reminders without
/// saying when they happened.
pub const VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Missing in a version 1 file, which reads as 0.
    #[serde(default)]
    pub version: u32,
    /// Keyed by the day, `YYYY-MM-DD`; see [`day_of`].
    #[serde(default)]
    pub days: BTreeMap<NaiveDate, DayRecord>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            version: VERSION,
            days: BTreeMap::new(),
        }
    }
}

impl Stats {
    /// Whether this was read from a file older than [`VERSION`].
    pub fn is_outdated(&self) -> bool {
        self.version < VERSION
    }

    /// Brings a version 1 history up to date. Its days began at midnight, so
    /// every session is filed again under the day it ended in, by `day_of_ts`.
    /// Its tallies of skips, snoozes and ignored reminders carried no time and
    /// cannot become [`Reminder`]s; serde already left them behind.
    pub fn upgrade(self, day_of_ts: impl Fn(i64) -> NaiveDate) -> Self {
        if !self.is_outdated() {
            return self;
        }
        let mut upgraded = Stats::default();
        for session in self.days.into_values().flat_map(|day| day.sessions) {
            upgraded.record(day_of_ts(session.end), session);
        }
        for day in upgraded.days.values_mut() {
            day.sessions.sort_by_key(|s| s.end);
        }
        upgraded
    }

    pub fn day(&self, date: NaiveDate) -> Option<&DayRecord> {
        self.days.get(&date)
    }

    pub fn day_mut(&mut self, date: NaiveDate) -> &mut DayRecord {
        self.days.entry(date).or_default()
    }

    pub fn record(&mut self, date: NaiveDate, session: Session) {
        self.day_mut(date).sessions.push(session);
    }

    /// The habits over the week ending `today`, at local time `now`, with
    /// `open` the stretch still running.
    ///
    /// Tonight counts once its cutoff and the few minutes' grace after it have
    /// passed: before then, there is nothing yet to have kept.
    pub fn habits(&self, today: NaiveDate, now: NaiveDateTime, open: Option<&Session>) -> Habits {
        let mut habits = Habits::default();
        for ago in (0..WEEK).rev() {
            let Some(date) = today.checked_sub_days(Days::new(ago)) else {
                continue;
            };
            let open = if ago == 0 { open } else { None };
            let empty = DayRecord::default();
            let day = self.days.get(&date).unwrap_or(&empty);
            if day.counts(open) {
                habits.rhythm.add(day.kept_rhythm(open));
            }
            if let Some(longest) = day.sessions_with(open).map(|s| s.active_secs).max()
                && habits.longest.is_none_or(|(_, secs)| longest > secs)
            {
                habits.longest = Some((date, longest));
            }
            if let Some(record) = &day.evening {
                let over = evening::cutoff_at(date, record.cutoff) + evening::WRAP_UP_GRACE;
                if ago > 0 || now >= over {
                    habits.evenings.add(day.stopped_in_time(date) == Some(true));
                }
            }
        }
        habits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(active_mins: u64, target_mins: u64) -> Session {
        Session {
            start: 0,
            end: 0,
            active_secs: active_mins * 60,
            target_secs: target_mins * 60,
            kind: BreakKind::Prompted,
        }
    }

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn at(d: u32, h: u32, m: u32) -> NaiveDateTime {
        date(d).and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn a_day_runs_from_five_in_the_morning_to_five_the_next() {
        assert_eq!(day_of(at(8, 5, 0)), date(8));
        assert_eq!(day_of(at(8, 23, 59)), date(8));
        assert_eq!(
            day_of(at(9, 0, 30)),
            date(8),
            "past midnight is still the evening before"
        );
        assert_eq!(day_of(at(9, 4, 59)), date(8));
        assert_eq!(day_of(at(9, 5, 0)), date(9));
        assert_eq!(day_start(date(8)), at(8, 5, 0));
    }

    #[test]
    fn a_stretch_runs_over_past_half_again_its_target() {
        assert!(!session(50, 50).overran());
        assert!(!session(75, 50).overran(), "one or two put-off reminders");
        assert!(session(76, 50).overran());
        assert!(!session(76, 0).overran(), "no target, nothing to run past");
    }

    #[test]
    fn a_short_day_does_not_count_and_one_marathon_breaks_the_rhythm() {
        let mut day = DayRecord::default();
        day.sessions.push(session(10, 50));
        assert!(!day.counts(None), "ten minutes is a look, not a day");
        let open = session(10, 50);
        assert!(day.counts(Some(&open)), "the stretch still running counts too");
        day.sessions.push(session(48, 50));
        assert!(day.kept_rhythm(None));
        day.sessions.push(session(180, 50));
        assert!(!day.kept_rhythm(None));
    }

    #[test]
    fn a_week_of_habits() {
        let mut stats = Stats::default();
        // The 2nd and 3rd are a week and more ago; the rest are in it.
        stats.record(date(1), session(300, 50));
        stats.record(date(3), session(60, 50));
        stats.record(date(4), session(94, 50));
        stats.record(date(5), session(5, 50));
        stats.record(date(6), session(45, 50));
        let nine = ClockTime::new(21, 0);
        for d in [6, 7] {
            let day = stats.day_mut(date(d));
            day.follow_cutoff(date(d), nine, at(d, 9, 0));
        }
        stats.day_mut(date(6)).saw_input(ts(at(6, 20, 50)));
        stats.day_mut(date(7)).saw_input(ts(at(7, 23, 30)));
        stats.day_mut(date(9)).follow_cutoff(date(9), nine, at(9, 9, 0));

        let open = session(30, 50);
        let habits = stats.habits(date(9), at(9, 20, 0), Some(&open));
        // 3rd (60 of 50: fine), 4th (94: over), 6th (45), today (30 so far);
        // the 5th had five minutes, and the 1st is out of the week.
        assert_eq!(habits.rhythm, Tally { kept: 3, of: 4 });
        assert_eq!(habits.longest, Some((date(4), 94 * 60)));
        // The 6th stopped in time, the 7th did not, and tonight is not over.
        assert_eq!(habits.evenings, Tally { kept: 1, of: 2 });
        let later = stats.habits(date(9), at(9, 21, 10), Some(&open));
        assert_eq!(
            later.evenings,
            Tally { kept: 2, of: 3 },
            "no input after the cutoff tonight"
        );
    }

    #[test]
    fn input_is_saved_once_a_minute_but_always_kept() {
        let mut day = DayRecord::default();
        assert!(day.saw_input(120), "the first input of the day");
        assert!(!day.saw_input(150), "the same minute");
        assert_eq!(day.last_input, Some(150), "but still remembered");
        assert!(day.saw_input(185), "a new minute");
        assert!(!day.saw_input(130), "an older input moves nothing");
        assert_eq!((day.first_input, day.last_input), (Some(120), Some(185)));
    }

    #[test]
    fn a_version_one_file_is_filed_again_by_the_new_day() {
        let v1 = r#"{"days":{
            "2026-10-02":{"sessions":[{"start":1,"end":10,"active_secs":60,"target_secs":3000,"kind":"natural"}],
                          "skips":2,"snoozes":1,"ignored":3},
            "2026-10-03":{"sessions":[{"start":20,"end":30,"active_secs":60,"target_secs":3000,"kind":"prompted"}]}
        }}"#;
        let old: Stats = serde_json::from_str(v1).unwrap();
        assert!(old.is_outdated());
        // The second session ended at half past midnight: filed under the 3rd
        // by midnight, it belongs to the evening of the 2nd.
        let new = old.upgrade(|_| date(2));
        assert!(!new.is_outdated());
        assert_eq!(new.days.len(), 1);
        let day = new.day(date(2)).unwrap();
        assert_eq!(day.sessions.iter().map(|s| s.end).collect::<Vec<_>>(), [10, 30]);
        assert!(day.reminders.is_empty(), "the old tallies had no times");
    }

    #[test]
    fn a_fresh_history_is_already_current() {
        assert!(!Stats::default().is_outdated());
        let saved = serde_json::to_string(&Stats::default()).unwrap();
        assert!(!serde_json::from_str::<Stats>(&saved).unwrap().is_outdated());
    }

    fn ts(at: NaiveDateTime) -> i64 {
        Local.from_local_datetime(&at).earliest().unwrap().timestamp()
    }

    #[test]
    fn the_evening_record_follows_the_setting_until_the_evening_begins() {
        let nine = ClockTime::new(21, 0);
        let ten = ClockTime::new(22, 0);
        let mut day = DayRecord::default();
        assert!(day.follow_cutoff(date(8), nine, at(8, 9, 0)));
        assert!(!day.follow_cutoff(date(8), nine, at(8, 9, 1)), "nothing new");
        assert!(day.follow_cutoff(date(8), ten, at(8, 12, 0)));
        assert_eq!(day.evening.as_ref().unwrap().cutoff, ten.unwrap());
        // Off before the evening: the day is not one the cutoff was on.
        assert!(day.follow_cutoff(date(8), None, at(8, 18, 0)));
        assert!(day.evening.is_none());
        // Off once it is under way: the day keeps its record.
        day.follow_cutoff(date(8), nine, at(8, 21, 30));
        assert!(!day.follow_cutoff(date(8), None, at(8, 21, 40)));
        assert!(day.evening.is_some());
    }

    #[test]
    fn a_day_stopped_in_time_by_its_own_cutoff() {
        let mut day = DayRecord::default();
        assert_eq!(day.stopped_in_time(date(8)), None, "the cutoff was off");
        day.follow_cutoff(date(8), ClockTime::new(21, 0), at(8, 9, 0));
        assert_eq!(day.stopped_in_time(date(8)), Some(true), "no input at all");
        day.saw_input(ts(at(8, 20, 40)));
        assert_eq!(day.stopped_in_time(date(8)), Some(true));
        day.saw_input(ts(at(8, 21, 4)));
        assert_eq!(day.stopped_in_time(date(8)), Some(true), "wrapping up");
        day.saw_input(ts(at(9, 0, 20)));
        assert_eq!(day.stopped_in_time(date(8)), Some(false));
    }

    #[test]
    fn json_round_trip() {
        let mut stats = Stats::default();
        stats.record(date(2), session(40, 50));
        stats.day_mut(date(2)).reminders.push(Reminder {
            at: 7,
            outcome: Outcome::Skipped,
        });
        stats.day_mut(date(2)).saw_input(3);
        stats.day_mut(date(3)).evening = Some(EveningRecord {
            cutoff: ClockTime::new(21, 30).unwrap(),
            extensions: vec![Extension {
                at: 9,
                reason: "把部署脚本跑完".into(),
            }],
        });
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"2026-10-02\""));
        assert_eq!(serde_json::from_str::<Stats>(&json).unwrap(), stats);
    }
}
