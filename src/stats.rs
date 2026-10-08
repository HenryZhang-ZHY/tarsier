//! Screen-time history and scoring. Pure logic plus JSON (de)serialisation.
//!
//! Scoring is passive: every work session is graded by how close it stayed to
//! the target interval. A day's score is the activity-weighted average of its
//! sessions, so a single 3-hour marathon hurts more than a short overrun.

use std::collections::BTreeMap;

use chrono::{DateTime, Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::breaks::{BreakKind, EndedSession};
use crate::i18n::translate;

/// Sessions shorter than this don't earn points (but still count for time).
const MIN_REWARDED_SECS: u64 = 10 * 60;
/// Days with less activity than this don't get a score.
const MIN_SCORED_SECS: u64 = 15 * 60;
pub const GOOD_SCORE: u8 = 80;
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
    pub fn score(&self) -> u8 {
        session_score(self.active_secs, self.target_secs)
    }

    pub fn points(&self) -> u32 {
        if self.active_secs < MIN_REWARDED_SECS {
            return 0;
        }
        match self.score() {
            s if s >= 95 => 10,
            s if s >= GOOD_SCORE => 6,
            s if s >= 50 => 2,
            _ => 0,
        }
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

    /// How many of the day's reminders ended this way.
    pub fn count(&self, outcome: Outcome) -> usize {
        self.reminders.iter().filter(|r| r.outcome == outcome).count()
    }

    pub fn active_secs(&self) -> u64 {
        self.sessions.iter().map(|s| s.active_secs).sum()
    }

    pub fn breaks(&self) -> usize {
        self.sessions.len()
    }

    pub fn longest_secs(&self) -> u64 {
        self.sessions.iter().map(|s| s.active_secs).max().unwrap_or(0)
    }

    pub fn points(&self) -> u32 {
        self.sessions.iter().map(Session::points).sum()
    }

    /// Score including an optional still-running session `(active, target)`.
    pub fn score_with(&self, open: Option<(u64, u64)>) -> Option<u8> {
        let mut weighted = 0f64;
        let mut total = 0u64;
        let parts = self.sessions.iter().map(|s| (s.active_secs, s.target_secs)).chain(open);
        for (active, target) in parts {
            weighted += session_score(active, target) as f64 * active as f64;
            total += active;
        }
        (total >= MIN_SCORED_SECS).then(|| (weighted / total as f64).round() as u8)
    }

    pub fn score(&self) -> Option<u8> {
        self.score_with(None)
    }
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

    pub fn total_points(&self) -> u32 {
        self.days.values().map(DayRecord::points).sum()
    }

    /// Consecutive days with a good score, ending at `today` (today only
    /// counts once it already qualifies, so the streak isn't broken early).
    pub fn streak(&self, today: NaiveDate, today_score: Option<u8>) -> u32 {
        let good = |s: Option<u8>| s.is_some_and(|s| s >= GOOD_SCORE);
        let mut streak = u32::from(good(today_score));
        let mut date = today;
        while let Some(prev) = date.checked_sub_days(Days::new(1)) {
            match self.days.get(&prev) {
                Some(day) if good(day.score()) => streak += 1,
                // Days without any computer use don't break a streak.
                None => {
                    if self.days.keys().next().is_none_or(|first| prev < *first) {
                        break;
                    }
                }
                Some(day) if day.score().is_none() => {}
                Some(_) => break,
            }
            date = prev;
        }
        streak
    }
}

/// 100 while within 110% of the target, then falling to 0 at 190%.
pub fn session_score(active_secs: u64, target_secs: u64) -> u8 {
    if target_secs == 0 {
        return 100;
    }
    let ratio = active_secs as f64 / target_secs as f64;
    let score = 100.0 - (ratio - 1.1).max(0.0) * 125.0;
    score.clamp(0.0, 100.0).round() as u8
}

pub fn grade(score: u8) -> &'static str {
    match score {
        95.. => "S",
        85.. => "A",
        70.. => "B",
        50.. => "C",
        _ => "D",
    }
}

/// Title for the accumulated points, a small long-term motivator.
///
/// The English name is the source; `level` translates it on the way out, which
/// a `const` table cannot do for itself.
pub fn level(points: u32) -> (&'static str, u32, Option<u32>) {
    const LEVELS: &[(u32, &str)] = &[
        (0, "Sedentary starter"),
        (100, "Stretch apprentice"),
        (300, "Pacing pro"),
        (800, "Eye guardian"),
        (2000, "Health pro"),
        (5000, "Tarsier grandmaster"),
    ];
    let idx = LEVELS.iter().rposition(|(min, _)| points >= *min).unwrap_or(0);
    let next = LEVELS.get(idx + 1).map(|(min, _)| *min);
    (translate(LEVELS[idx].1), LEVELS[idx].0, next)
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
    }

    #[test]
    fn session_score_curve() {
        assert_eq!(session_score(50 * 60, 50 * 60), 100);
        assert_eq!(session_score(55 * 60, 50 * 60), 100);
        assert_eq!(session_score(75 * 60, 50 * 60), 50);
        assert_eq!(session_score(200 * 60, 50 * 60), 0);
        assert_eq!(session_score(10, 0), 100);
    }

    #[test]
    fn day_score_is_activity_weighted() {
        let mut day = DayRecord::default();
        day.sessions.push(session(50, 50)); // 100
        day.sessions.push(session(150, 50)); // 0
        assert_eq!(day.score(), Some(25));
        assert_eq!(day.longest_secs(), 150 * 60);
    }

    #[test]
    fn tiny_days_are_unscored() {
        let mut day = DayRecord::default();
        day.sessions.push(session(5, 50));
        assert_eq!(day.score(), None);
        assert_eq!(day.score_with(Some((20 * 60, 50 * 60))), Some(100));
    }

    #[test]
    fn points_reward_good_breaks() {
        assert_eq!(session(45, 50).points(), 10);
        assert_eq!(session(5, 50).points(), 0);
        assert_eq!(session(60, 50).points(), 6);
        assert_eq!(session(75, 50).points(), 2);
        assert_eq!(session(120, 50).points(), 0);
    }

    #[test]
    fn streak_counts_good_days_and_skips_empty_ones() {
        let mut stats = Stats::default();
        stats.record(date(1), session(40, 50));
        stats.record(date(2), session(200, 50)); // bad
        stats.record(date(3), session(40, 50));
        // date(4) missing: weekend, no usage
        stats.record(date(5), session(40, 50));
        assert_eq!(stats.streak(date(6), None), 2);
        assert_eq!(stats.streak(date(6), Some(90)), 3);
        assert_eq!(stats.streak(date(1), Some(90)), 1);
    }

    #[test]
    fn grades_and_levels() {
        assert_eq!(grade(100), "S");
        assert_eq!(grade(60), "C");
        // Titles read in the language the UI is drawn in; English is the
        // default and the source, so this is what an untranslated run shows.
        assert_eq!(level(0), ("Sedentary starter", 0, Some(100)));
        assert_eq!(level(350).0, "Pacing pro");
        assert_eq!(level(9999).2, None);
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

    #[test]
    fn json_round_trip() {
        let mut stats = Stats::default();
        stats.record(date(2), session(40, 50));
        stats.day_mut(date(2)).reminders.push(Reminder {
            at: 7,
            outcome: Outcome::Skipped,
        });
        stats.day_mut(date(2)).saw_input(3);
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"2026-10-02\""));
        assert_eq!(serde_json::from_str::<Stats>(&json).unwrap(), stats);
    }
}
