//! Screen-time history and scoring. Pure logic plus JSON (de)serialisation.
//!
//! Scoring is passive: every work session is graded by how close it stayed to
//! the target interval. A day's score is the activity-weighted average of its
//! sessions, so a single 3-hour marathon hurts more than a short overrun.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::breaks::{BreakKind, EndedSession};
use crate::i18n::translate;

/// Sessions shorter than this don't earn points (but still count for time).
const MIN_REWARDED_SECS: u64 = 10 * 60;
/// Days with less activity than this don't get a score.
const MIN_SCORED_SECS: u64 = 15 * 60;
pub const GOOD_SCORE: u8 = 80;

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DayRecord {
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub skips: u32,
    #[serde(default)]
    pub snoozes: u32,
    /// Reminders worked straight through without resting.
    #[serde(default)]
    pub ignored: u32,
}

impl DayRecord {
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Keyed by local date, `YYYY-MM-DD`.
    #[serde(default)]
    pub days: BTreeMap<NaiveDate, DayRecord>,
}

impl Stats {
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
        loop {
            let Some(prev) = date.checked_sub_days(Days::new(1)) else {
                break;
            };
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
    fn json_round_trip() {
        let mut stats = Stats::default();
        stats.record(date(2), session(40, 50));
        stats.day_mut(date(2)).skips = 1;
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"2026-10-02\""));
        assert_eq!(serde_json::from_str::<Stats>(&json).unwrap(), stats);
    }
}
