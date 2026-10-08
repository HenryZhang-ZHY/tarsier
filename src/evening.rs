//! The evening cutoff: a time of day after which the user has decided not to
//! use the computer. Pure logic; the controller asks it what the screen should
//! show right now and draws that.
//!
//! - From [`HEADS_UP`] before the cutoff, a small card says it is coming.
//! - From the cutoff until the day ends at 05:00, an overlay says it is here.
//! - The user can carry on by saying what they still need to do; the overlay
//!   then comes back after [`EXTENSION`]. There is no limit and no penalty:
//!   tarsier reminds, the user decides.
//!
//! Nothing here keeps time of its own. What the screen shows follows from the
//! clock, the setting and the record of the evening, so waking from sleep,
//! restarting tarsier or changing the setting halfway through all come out
//! right without being handled one by one.

use std::fmt;
use std::str::FromStr;

use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::stats::{DAY_START_HOUR, day_of, day_start};

/// How long before the cutoff the heads-up appears.
pub const HEADS_UP: TimeDelta = TimeDelta::minutes(15);
/// How long carrying on lasts before the overlay comes back.
pub const EXTENSION: TimeDelta = TimeDelta::minutes(15);
/// Stopping within this long after the cutoff still counts as stopping on
/// time: shutting down takes a few clicks of its own, and nobody should be
/// marked late for finishing the sentence they were typing.
pub const WRAP_UP_GRACE: TimeDelta = TimeDelta::minutes(5);

/// A time of day, minute precision, written `HH:MM`.
///
/// A cutoff falls in the evening or the small hours: from 12:00 to 04:59. The
/// day starts again at 05:00, so a cutoff between then and noon would put the
/// whole working day under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockTime {
    hour: u8,
    minute: u8,
}

/// Where the evening range starts.
const NOON: u32 = 12;
/// Minutes from noon to the end of the range, 04:59 the next morning.
const RANGE_MINUTES: u32 = (24 - NOON + DAY_START_HOUR) * 60;

impl ClockTime {
    pub const fn new(hour: u8, minute: u8) -> Option<Self> {
        if hour < 24 && minute < 60 {
            Some(Self { hour, minute })
        } else {
            None
        }
    }

    pub fn hour(self) -> u8 {
        self.hour
    }

    pub fn minute(self) -> u8 {
        self.minute
    }

    pub fn time(self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.hour.into(), self.minute.into(), 0).expect("a valid time of day")
    }

    /// Whether this can be a cutoff.
    pub fn is_evening(self) -> bool {
        self.minutes_past_noon() < RANGE_MINUTES
    }

    /// `minutes` later (or earlier), kept inside the evening range.
    pub fn step(self, minutes: i32) -> Self {
        let past_noon = (self.minutes_past_noon() as i32 + minutes).clamp(0, RANGE_MINUTES as i32 - 1) as u32;
        let of_day = (past_noon + NOON * 60) % (24 * 60);
        Self {
            hour: (of_day / 60) as u8,
            minute: (of_day % 60) as u8,
        }
    }

    fn minutes_past_noon(self) -> u32 {
        (self.hour as u32 * 60 + self.minute as u32 + 24 * 60 - NOON * 60) % (24 * 60)
    }
}

impl fmt::Display for ClockTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// Reads what a person would type: `21:30`, `9:30`, `21`, `2130`, `21.30`.
impl FromStr for ClockTime {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        let text = text.trim();
        let (hour, minute) = match text.split_once([':', '.', '：']) {
            Some((h, m)) => (h, m),
            None if text.len() > 2 => text.split_at(text.len() - 2),
            None => (text, "0"),
        };
        let digits = |s: &str| !s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(hour) || !digits(minute) {
            return Err(());
        }
        Self::new(hour.parse().map_err(|_| ())?, minute.parse().map_err(|_| ())?).ok_or(())
    }
}

impl Serialize for ClockTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ClockTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse()
            .map_err(|()| serde::de::Error::custom(format!("{text:?} is not a time of day like \"22:00\"")))
    }
}

/// The moment the cutoff falls on the evening of `day`. A cutoff after
/// midnight falls on the next calendar date, but in the same day.
pub fn cutoff_at(day: NaiveDate, cutoff: ClockTime) -> NaiveDateTime {
    let date = if cutoff.hour as u32 >= NOON {
        day
    } else {
        day.checked_add_days(Days::new(1)).unwrap_or(day)
    };
    date.and_time(cutoff.time())
}

/// When the evening of `day` is over, and with it the day.
pub fn night_end(day: NaiveDate) -> NaiveDateTime {
    day_start(day.checked_add_days(Days::new(1)).unwrap_or(day))
}

/// Whether the last input of a day came in time for its cutoff.
pub fn stopped_in_time(day: NaiveDate, cutoff: ClockTime, last_input: NaiveDateTime) -> bool {
    last_input <= cutoff_at(day, cutoff) + WRAP_UP_GRACE
}

/// What the screen should show for the evening right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nothing: the evening is not near, or the cutoff is off.
    Quiet,
    /// The card saying the cutoff is coming at `cutoff`.
    HeadsUp { cutoff: NaiveDateTime },
    /// The overlay: it is past `cutoff`.
    Cutoff { cutoff: NaiveDateTime },
    /// Past the cutoff, but the user said they are carrying on until `until`.
    KeepingOn { until: NaiveDateTime },
}

/// What the evening looks like at `now`.
///
/// - `cutoff`: the setting, `None` when the cutoff is off.
/// - `heads_up_closed`: the user closed tonight's heads-up card.
/// - `last_extension`: when the user last chose to carry on, this day.
pub fn status(
    now: NaiveDateTime,
    cutoff: Option<ClockTime>,
    heads_up_closed: bool,
    last_extension: Option<NaiveDateTime>,
) -> Status {
    let Some(cutoff) = cutoff.filter(|c| c.is_evening()) else {
        return Status::Quiet;
    };
    let day = day_of(now);
    let at = cutoff_at(day, cutoff);
    if now >= at && now < night_end(day) {
        match last_extension.map(|t| t + EXTENSION) {
            Some(until) if now < until => Status::KeepingOn { until },
            _ => Status::Cutoff { cutoff: at },
        }
    } else if now >= at - HEADS_UP && now < at && !heads_up_closed {
        Status::HeadsUp { cutoff: at }
    } else {
        Status::Quiet
    }
}

/// Minutes left until `at`, rounded up, so the card never says 0 while the
/// overlay has not come yet.
pub fn minutes_until(now: NaiveDateTime, at: NaiveDateTime) -> i64 {
    let secs = (at - now).num_seconds().max(0);
    (secs + 59) / 60
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u8, m: u8) -> ClockTime {
        ClockTime::new(h, m).unwrap()
    }

    /// October 2026, `d` the calendar date.
    fn at(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, d)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn nine() -> Option<ClockTime> {
        Some(t(21, 0))
    }

    #[test]
    fn an_evening_goes_quiet_heads_up_cutoff_and_ends_at_five() {
        assert_eq!(status(at(8, 20, 44), nine(), false, None), Status::Quiet);
        let cutoff = at(8, 21, 0);
        assert_eq!(status(at(8, 20, 45), nine(), false, None), Status::HeadsUp { cutoff });
        assert_eq!(status(at(8, 20, 59), nine(), false, None), Status::HeadsUp { cutoff });
        assert_eq!(status(at(8, 21, 0), nine(), false, None), Status::Cutoff { cutoff });
        assert_eq!(
            status(at(9, 1, 30), nine(), false, None),
            Status::Cutoff { cutoff },
            "past midnight is the same evening"
        );
        assert_eq!(status(at(9, 4, 59), nine(), false, None), Status::Cutoff { cutoff });
        assert_eq!(status(at(9, 5, 0), nine(), false, None), Status::Quiet, "a new day");
        assert_eq!(status(at(9, 12, 0), nine(), false, None), Status::Quiet);
    }

    #[test]
    fn switched_off_it_shows_nothing() {
        assert_eq!(status(at(8, 22, 0), None, false, None), Status::Quiet);
        assert_eq!(status(at(8, 20, 50), None, false, None), Status::Quiet);
    }

    #[test]
    fn turning_the_computer_on_late_shows_the_overlay_at_once() {
        // Nothing to catch up on: the evening is read off the clock.
        assert_eq!(
            status(at(8, 23, 10), nine(), false, None),
            Status::Cutoff { cutoff: at(8, 21, 0) }
        );
        assert_eq!(
            status(at(8, 20, 52), nine(), false, None),
            Status::HeadsUp { cutoff: at(8, 21, 0) }
        );
    }

    #[test]
    fn a_closed_heads_up_stays_closed_but_the_cutoff_still_comes() {
        assert_eq!(status(at(8, 20, 50), nine(), true, None), Status::Quiet);
        assert_eq!(
            status(at(8, 21, 0), nine(), true, None),
            Status::Cutoff { cutoff: at(8, 21, 0) }
        );
    }

    #[test]
    fn carrying_on_holds_the_overlay_back_for_a_while() {
        let said = Some(at(8, 21, 3));
        let until = at(8, 21, 18);
        assert_eq!(status(at(8, 21, 4), nine(), false, said), Status::KeepingOn { until });
        assert_eq!(status(at(8, 21, 17), nine(), false, said), Status::KeepingOn { until });
        assert_eq!(
            status(at(8, 21, 18), nine(), false, said),
            Status::Cutoff { cutoff: at(8, 21, 0) }
        );
    }

    #[test]
    fn sleeping_through_the_extension_brings_the_overlay_straight_back() {
        let said = Some(at(8, 21, 3));
        assert_eq!(
            status(at(8, 23, 40), nine(), false, said),
            Status::Cutoff { cutoff: at(8, 21, 0) }
        );
    }

    #[test]
    fn moving_the_cutoff_later_takes_effect_at_once() {
        let ten = Some(t(22, 0));
        assert_eq!(status(at(8, 21, 10), ten, false, None), Status::Quiet);
        assert_eq!(
            status(at(8, 21, 50), ten, false, None),
            Status::HeadsUp { cutoff: at(8, 22, 0) }
        );
    }

    #[test]
    fn a_cutoff_after_midnight_belongs_to_the_evening_before() {
        let one = Some(t(1, 0));
        assert_eq!(status(at(8, 23, 0), one, false, None), Status::Quiet);
        assert_eq!(
            status(at(9, 0, 45), one, false, None),
            Status::HeadsUp { cutoff: at(9, 1, 0) }
        );
        assert_eq!(
            status(at(9, 2, 0), one, false, None),
            Status::Cutoff { cutoff: at(9, 1, 0) }
        );
        assert_eq!(status(at(9, 5, 0), one, false, None), Status::Quiet);
        // Just after midnight: the heads-up starts before it.
        assert_eq!(
            status(at(8, 23, 58), Some(t(0, 10)), false, None),
            Status::HeadsUp { cutoff: at(9, 0, 10) }
        );
    }

    #[test]
    fn stopping_a_few_minutes_late_is_still_on_time() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert!(stopped_in_time(day, t(21, 0), at(8, 20, 30)));
        assert!(stopped_in_time(day, t(21, 0), at(8, 21, 5)));
        assert!(!stopped_in_time(day, t(21, 0), at(8, 21, 6)));
        assert!(!stopped_in_time(day, t(21, 0), at(9, 0, 30)));
    }

    #[test]
    fn the_heads_up_counts_whole_minutes_up() {
        assert_eq!(minutes_until(at(8, 20, 45), at(8, 21, 0)), 15);
        let almost = at(8, 20, 59) + TimeDelta::seconds(30);
        assert_eq!(minutes_until(almost, at(8, 21, 0)), 1);
        assert_eq!(minutes_until(at(8, 21, 1), at(8, 21, 0)), 0);
    }

    #[test]
    fn times_read_the_way_people_type_them() {
        assert_eq!("21:30".parse(), Ok(t(21, 30)));
        assert_eq!("9:05".parse(), Ok(t(9, 5)));
        assert_eq!("21".parse(), Ok(t(21, 0)));
        assert_eq!("2130".parse(), Ok(t(21, 30)));
        assert_eq!("930".parse(), Ok(t(9, 30)));
        assert_eq!(" 21.30 ".parse(), Ok(t(21, 30)));
        assert_eq!(
            "21：30".parse(),
            Ok(t(21, 30)),
            "a full-width colon from a Chinese keyboard"
        );
        for bad in ["", "25:00", "21:60", "abc", "21:3x", "12345", ":30"] {
            assert_eq!(bad.parse::<ClockTime>(), Err(()), "{bad:?}");
        }
        assert_eq!(t(9, 5).to_string(), "09:05");
    }

    #[test]
    fn a_cutoff_falls_between_noon_and_five_in_the_morning() {
        assert!(t(12, 0).is_evening());
        assert!(t(23, 59).is_evening());
        assert!(t(0, 0).is_evening());
        assert!(t(4, 59).is_evening());
        assert!(!t(5, 0).is_evening());
        assert!(!t(11, 59).is_evening());
        // A morning cutoff would cover the whole working day; it is not one.
        assert_eq!(status(at(8, 10, 0), Some(t(9, 0)), false, None), Status::Quiet);
    }

    #[test]
    fn stepping_stays_inside_the_evening() {
        assert_eq!(t(21, 0).step(15), t(21, 15));
        assert_eq!(t(23, 50).step(15), t(0, 5), "across midnight");
        assert_eq!(t(4, 50).step(15), t(4, 59), "not into the morning");
        assert_eq!(t(12, 10).step(-15), t(12, 0), "not before noon");
    }

    #[test]
    fn a_time_is_saved_as_text() {
        assert_eq!(serde_json::to_string(&t(21, 0)).unwrap(), "\"21:00\"");
        assert_eq!(serde_json::from_str::<ClockTime>("\"22:15\"").unwrap(), t(22, 15));
        assert!(serde_json::from_str::<ClockTime>("\"late\"").is_err());
    }
}
