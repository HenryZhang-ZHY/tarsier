//! Break tracking state machine (Fadetop-style), driven once per second by
//! the caller with "seconds since last keyboard/mouse input". Pure logic.
//!
//! - Work time is mechanical: every second of a session counts, whether or not
//!   there was input (reading or thinking still strains the eyes).
//! - Only sleep/hibernation (a tick gap of `break_secs`) ends a session as a
//!   natural break. Being idle or locked does not: the timer keeps running and
//!   the reminder finishes by itself once the user has been hands-off long
//!   enough.
//! - Once a session reaches `work_secs` of activity a break is prompted. The
//!   prompted break only counts down while the user is actually hands-off,
//!   so wiggling the mouse does not "complete" a break.
//! - The reminder never blocks input. Working straight through it for
//!   `IGNORE_FACTOR` x `break_secs` counts as ignoring it; it fades away and
//!   comes back after `snooze_secs`.

use serde::{Deserialize, Serialize};

/// Back from a break once input is this recent.
const RETURN_IDLE: u64 = 60;
/// During a prompted break, idle at least this long counts as resting.
pub const RESTING_IDLE: u64 = 3;
/// Larger tick gaps are treated as time away (sleep, hibernation, hang).
const MAX_TICK: u64 = 5;
/// A prompted break left unrested this many break-lengths is ignored.
pub const IGNORE_FACTOR: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakSettings {
    pub work_secs: u64,
    pub break_secs: u64,
    pub snooze_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakKind {
    /// User walked away on their own.
    Natural,
    /// User rested after a reminder.
    Prompted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Working,
    /// Prompted break in progress: `rested` seconds of hands-off time out of
    /// `elapsed` wall-clock seconds since the reminder appeared.
    Prompted {
        rested: u64,
        elapsed: u64,
    },
    /// Session ended, waiting for the user to come back.
    Away,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakEvent {
    PromptBreak,
    /// A prompted break finished (rested enough or user left); close the overlay.
    BreakFinished,
    /// The user kept working through the reminder; close it, remind again later.
    BreakIgnored,
    SessionEnded(EndedSession),
    /// User came back after a break; a fresh session starts.
    Returned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndedSession {
    pub start: i64,
    pub end: i64,
    pub active_secs: u64,
    pub target_secs: u64,
    pub kind: BreakKind,
}

#[derive(Debug)]
pub struct BreakTracker {
    settings: BreakSettings,
    phase: Phase,
    session_start: i64,
    session_active: u64,
    next_prompt_at: u64,
}

impl BreakTracker {
    pub fn new(settings: BreakSettings, now: i64) -> Self {
        Self {
            settings,
            phase: Phase::Working,
            session_start: now,
            session_active: 0,
            next_prompt_at: settings.work_secs,
        }
    }

    pub fn settings(&self) -> BreakSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: BreakSettings) {
        if settings.work_secs != self.settings.work_secs {
            self.next_prompt_at = settings.work_secs;
        }
        self.settings = settings;
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn session_active(&self) -> u64 {
        self.session_active
    }

    pub fn session_start(&self) -> i64 {
        self.session_start
    }

    /// Active seconds left until the next reminder.
    pub fn until_prompt(&self) -> u64 {
        self.next_prompt_at.saturating_sub(self.session_active)
    }

    /// Seconds of rest still needed in a prompted break.
    pub fn rest_remaining(&self) -> u64 {
        match self.phase {
            Phase::Prompted { rested, .. } => self.settings.break_secs.saturating_sub(rested),
            _ => self.settings.break_secs,
        }
    }

    /// Advance by `dt` wall-clock seconds; `idle` is seconds since last input.
    /// `suppressed` holds back reminders (paused, fullscreen app, presenting).
    pub fn tick(&mut self, now: i64, dt: u64, idle: u64, suppressed: bool) -> Vec<BreakEvent> {
        let mut events = Vec::new();
        let (dt, idle) = if dt > MAX_TICK { (dt, idle.max(dt)) } else { (dt, idle) };
        match self.phase {
            Phase::Working => {
                if dt > MAX_TICK && dt >= self.settings.break_secs {
                    self.end_session(now - dt as i64, BreakKind::Natural, &mut events);
                } else {
                    self.session_active += dt.min(MAX_TICK);
                    if self.session_active >= self.next_prompt_at {
                        if !suppressed {
                            self.phase = Phase::Prompted { rested: 0, elapsed: 0 };
                            events.push(BreakEvent::PromptBreak);
                        } else if idle >= self.settings.break_secs {
                            // Unsuppressed, this is a reminder that finishes the
                            // moment it opens, because the user is already a full
                            // break away. Held back, it must still count — or the
                            // time away is scored as work and the reminder lands
                            // the second they come back.
                            self.end_session(now, BreakKind::Natural, &mut events);
                        }
                    }
                }
            }
            Phase::Prompted { rested, elapsed } => {
                let rested = if idle >= RESTING_IDLE { rested + dt } else { rested };
                let elapsed = elapsed + dt;
                if idle < RESTING_IDLE {
                    // The overlay doesn't block input, so working through it is work.
                    self.session_active += dt.min(MAX_TICK);
                }
                if rested >= self.settings.break_secs || idle >= self.settings.break_secs {
                    events.push(BreakEvent::BreakFinished);
                    // After a sleep the session ended when the lid closed, not
                    // when it opened — which may be the next day.
                    let end = if dt > MAX_TICK { now - dt as i64 } else { now };
                    self.end_session(end, BreakKind::Prompted, &mut events);
                } else if elapsed >= self.settings.break_secs * IGNORE_FACTOR {
                    events.push(BreakEvent::BreakIgnored);
                    self.phase = Phase::Working;
                    self.next_prompt_at = self.session_active + self.settings.snooze_secs;
                } else {
                    self.phase = Phase::Prompted { rested, elapsed };
                }
            }
            Phase::Away => {
                if idle < self.settings.break_secs.min(RETURN_IDLE) {
                    self.start_session(now - idle as i64);
                    events.push(BreakEvent::Returned);
                }
            }
        }
        events
    }

    /// Postpone a prompted break.
    pub fn snooze(&mut self) {
        if matches!(self.phase, Phase::Prompted { .. }) {
            self.phase = Phase::Working;
        }
        self.next_prompt_at = self.session_active + self.settings.snooze_secs;
    }

    /// Dismiss a prompted break; next reminder after a full work interval.
    pub fn skip(&mut self) {
        if matches!(self.phase, Phase::Prompted { .. }) {
            self.phase = Phase::Working;
        }
        self.next_prompt_at = self.session_active + self.settings.work_secs;
    }

    /// Start a prompted break right away (from the tray / UI).
    pub fn break_now(&mut self) -> bool {
        if self.phase == Phase::Working {
            self.phase = Phase::Prompted { rested: 0, elapsed: 0 };
            true
        } else {
            false
        }
    }

    /// Close the running session, e.g. on app exit; returns it if non-trivial.
    pub fn finish(&mut self, now: i64) -> Option<EndedSession> {
        if self.phase == Phase::Away || self.session_active == 0 {
            return None;
        }
        Some(EndedSession {
            start: self.session_start,
            end: now,
            active_secs: self.session_active,
            target_secs: self.settings.work_secs,
            kind: BreakKind::Natural,
        })
    }

    fn end_session(&mut self, end: i64, kind: BreakKind, events: &mut Vec<BreakEvent>) {
        if self.session_active > 0 {
            events.push(BreakEvent::SessionEnded(EndedSession {
                start: self.session_start,
                end,
                active_secs: self.session_active,
                target_secs: self.settings.work_secs,
                kind,
            }));
        }
        self.phase = Phase::Away;
        self.session_active = 0;
    }

    fn start_session(&mut self, now: i64) {
        self.phase = Phase::Working;
        self.session_start = now;
        self.session_active = 0;
        self.next_prompt_at = self.settings.work_secs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: BreakSettings = BreakSettings {
        work_secs: 600,
        break_secs: 120,
        snooze_secs: 60,
    };

    /// Runs `secs` one-second ticks where `idle_at(i)` gives the idle value.
    fn run(
        t: &mut BreakTracker,
        now: &mut i64,
        secs: u64,
        idle_at: impl Fn(u64) -> u64,
        suppressed: bool,
    ) -> Vec<BreakEvent> {
        let mut all = Vec::new();
        for i in 0..secs {
            *now += 1;
            all.extend(t.tick(*now, 1, idle_at(i), suppressed));
        }
        all
    }

    fn sessions(events: &[BreakEvent]) -> Vec<EndedSession> {
        events
            .iter()
            .filter_map(|e| match e {
                BreakEvent::SessionEnded(s) => Some(*s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn prompts_after_work_interval_of_activity() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        let ev = run(&mut t, &mut now, 599, |_| 0, false);
        assert!(ev.is_empty());
        let ev = run(&mut t, &mut now, 1, |_| 0, false);
        assert_eq!(ev, vec![BreakEvent::PromptBreak]);
        assert_eq!(t.phase(), Phase::Prompted { rested: 0, elapsed: 0 });
    }

    #[test]
    fn short_idle_still_counts_as_work() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        // Reading without touching input for 100s (< break_secs) is still work.
        run(&mut t, &mut now, 100, |i| i, false);
        assert_eq!(t.session_active(), 100);
        assert_eq!(t.phase(), Phase::Working);
    }

    #[test]
    fn long_idle_is_not_a_break() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 500, |i| i, false);
        assert_eq!(t.phase(), Phase::Working);
        assert_eq!(t.session_active(), 500);
    }

    #[test]
    fn leaving_unlocked_prompts_then_finishes_the_break() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 500, |_| 0, false);
        // Away from the desk: the timer keeps running, the prompt fires...
        let ev = run(&mut t, &mut now, 200, |i| 1000 + i, false);
        assert!(ev.contains(&BreakEvent::PromptBreak));
        // ...and the break completes on its own since the user is hands-off.
        assert!(ev.contains(&BreakEvent::BreakFinished));
        assert_eq!(t.phase(), Phase::Away);
    }

    #[test]
    fn prompted_break_counts_only_hands_off_time() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 600, |_| 0, false);
        // Working through the reminder: never rests, and it counts as work.
        run(&mut t, &mut now, 100, |_| 0, false);
        assert_eq!(t.rest_remaining(), 120);
        assert_eq!(t.session_active(), 700);
        // Hands off: idle grows; seconds with idle >= RESTING_IDLE count.
        let ev = run(&mut t, &mut now, 130, |i| i + 1, false);
        assert!(ev.contains(&BreakEvent::BreakFinished));
        let s = sessions(&ev);
        assert_eq!(s[0].kind, BreakKind::Prompted);
        assert_eq!(s[0].active_secs, 702);
    }

    #[test]
    fn working_through_the_reminder_ignores_it() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 600, |_| 0, false);
        let ev = run(&mut t, &mut now, 239, |_| 0, false);
        assert!(ev.is_empty());
        let ev = run(&mut t, &mut now, 1, |_| 0, false);
        assert_eq!(ev, vec![BreakEvent::BreakIgnored]);
        assert_eq!(t.phase(), Phase::Working);
        assert_eq!(t.until_prompt(), 60);
        assert_eq!(sessions(&ev).len(), 0);
    }

    #[test]
    fn snooze_and_skip_reschedule() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 600, |_| 0, false);
        t.snooze();
        assert_eq!(t.phase(), Phase::Working);
        assert_eq!(t.until_prompt(), 60);
        let ev = run(&mut t, &mut now, 60, |_| 0, false);
        assert_eq!(ev, vec![BreakEvent::PromptBreak]);
        t.skip();
        assert_eq!(t.until_prompt(), 600);
    }

    #[test]
    fn suppression_defers_prompt_until_lifted() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        let ev = run(&mut t, &mut now, 700, |_| 0, true);
        assert!(ev.is_empty());
        let ev = run(&mut t, &mut now, 1, |_| 0, false);
        assert_eq!(ev, vec![BreakEvent::PromptBreak]);
    }

    #[test]
    fn sleep_gap_counts_as_away() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 100, |_| 0, false);
        now += 3600;
        // After resume GetLastInputInfo may report small idle; the gap wins.
        let ev = t.tick(now, 3600, 0, false);
        let s = sessions(&ev);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].active_secs, 100);
        assert_eq!(t.session_active(), 0);
    }

    #[test]
    fn a_due_break_taken_while_reminders_are_held_back_still_counts() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        // Paused, and the reminder comes due while the user is at the desk.
        let ev = run(&mut t, &mut now, 650, |_| 0, true);
        assert!(ev.is_empty());
        // Then they walk away for longer than a break.
        let ev = run(&mut t, &mut now, 200, |i| i, true);
        let s = sessions(&ev);
        assert_eq!(s.len(), 1, "the time away ended the session");
        assert_eq!(s[0].kind, BreakKind::Natural);
        assert_eq!(t.phase(), Phase::Away);
        // Coming back starts afresh instead of reminding at once.
        let ev = run(&mut t, &mut now, 1, |_| 0, false);
        assert_eq!(ev, vec![BreakEvent::Returned]);
        assert_eq!(t.until_prompt(), SETTINGS.work_secs);
    }

    #[test]
    fn idle_before_the_break_is_due_is_still_work_even_when_held_back() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        let ev = run(&mut t, &mut now, 300, |i| i, true);
        assert!(ev.is_empty());
        assert_eq!(t.session_active(), 300);
    }

    #[test]
    fn sleeping_through_a_reminder_ends_the_session_when_the_sleep_began() {
        let mut now = 1_000;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 600, |_| 0, false);
        assert!(matches!(t.phase(), Phase::Prompted { .. }));
        let slept_at = now;
        now += 8 * 3600;
        let ev = t.tick(now, 8 * 3600, 0, false);
        assert!(ev.contains(&BreakEvent::BreakFinished));
        assert_eq!(sessions(&ev)[0].end, slept_at);
    }

    #[test]
    fn coming_back_starts_a_fresh_session() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 100, |_| 0, false);
        now += 3600;
        t.tick(now, 3600, 3600, false);
        assert_eq!(t.phase(), Phase::Away);
        // Still away: nothing happens.
        assert!(run(&mut t, &mut now, 10, |_| 4000, false).is_empty());
        // Input again: a new session starts from the moment of that input.
        let ev = t.tick(now + 1, 1, 0, false);
        assert_eq!(ev, vec![BreakEvent::Returned]);
        assert_eq!(t.phase(), Phase::Working);
        assert_eq!(t.session_start(), now + 1);
        assert_eq!(t.session_active(), 0);
    }

    #[test]
    fn a_new_work_length_moves_the_next_reminder() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 100, |_| 0, false);
        t.set_settings(BreakSettings {
            work_secs: 300,
            ..SETTINGS
        });
        assert_eq!(t.until_prompt(), 200);
        // Changing only the break length leaves the reminder where it was.
        t.set_settings(BreakSettings {
            work_secs: 300,
            break_secs: 60,
            ..SETTINGS
        });
        assert_eq!(t.until_prompt(), 200);
    }

    #[test]
    fn break_now_and_finish() {
        let mut now = 0;
        let mut t = BreakTracker::new(SETTINGS, now);
        run(&mut t, &mut now, 30, |_| 0, false);
        assert!(t.break_now());
        assert!(!t.break_now());
        let f = t.finish(now).unwrap();
        assert_eq!(f.active_secs, 30);
    }
}
