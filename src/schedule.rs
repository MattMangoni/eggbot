//! Recurring prompts for a bot. Missed runs collapse into one run when eggbot comes back.

use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Repeat {
    Daily { hour: u32, minute: u32 },
    Weekdays { hour: u32, minute: u32 },
    Hours(u32),
    Minutes(u32),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Schedule {
    pub id: usize,
    pub prompt: String,
    pub repeat: Repeat,
    /// Unix seconds of the last run, or of creation before the first run.
    pub anchor: i64,
}

impl Repeat {
    /// Parses the form value: "09:00" for daily/weekdays, a number ≥ 1 for hours/minutes.
    pub fn parse(kind: usize, value: &str) -> Result<Self, &'static str> {
        let value = value.trim();
        match kind {
            0 | 1 => {
                let t = NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| "Use a time like 09:00")?;
                let (hour, minute) = (t.hour(), t.minute());
                Ok(if kind == 0 { Repeat::Daily { hour, minute } } else { Repeat::Weekdays { hour, minute } })
            }
            _ => match value.parse::<u32>() {
                Ok(n) if n >= 1 => Ok(if kind == 2 { Repeat::Hours(n) } else { Repeat::Minutes(n) }),
                _ => Err("Use a whole number, 1 or more"),
            },
        }
    }

    pub fn label(&self) -> String {
        match *self {
            Repeat::Daily { hour, minute } => format!("Every day at {hour:02}:{minute:02}"),
            Repeat::Weekdays { hour, minute } => format!("Weekdays at {hour:02}:{minute:02}"),
            Repeat::Hours(1) => "Every hour".into(),
            Repeat::Hours(n) => format!("Every {n} hours"),
            Repeat::Minutes(1) => "Every minute".into(),
            Repeat::Minutes(n) => format!("Every {n} minutes"),
        }
    }

    /// The first run time strictly after `after`.
    pub fn next<Tz: TimeZone>(&self, after: DateTime<Tz>) -> DateTime<Tz> {
        let at_time = |hour, minute, weekdays_only: bool| {
            let mut day = after.date_naive();
            loop {
                let weekend = matches!(day.weekday(), Weekday::Sat | Weekday::Sun);
                let candidate = day.and_hms_opt(hour, minute, 0).and_then(|t| after.timezone().from_local_datetime(&t).earliest());
                if let Some(c) = candidate
                    && c > after
                    && !(weekdays_only && weekend)
                {
                    return c;
                }
                day = day.succ_opt().unwrap();
            }
        };
        match *self {
            Repeat::Daily { hour, minute } => at_time(hour, minute, false),
            Repeat::Weekdays { hour, minute } => at_time(hour, minute, true),
            Repeat::Hours(n) => after + Duration::hours(n as i64),
            Repeat::Minutes(n) => after + Duration::minutes(n as i64),
        }
    }
}

impl Schedule {
    pub fn next_run(&self) -> DateTime<Local> {
        self.repeat.next(Local.timestamp_opt(self.anchor, 0).unwrap())
    }

    /// Due at most once, however many runs were missed; the caller moves `anchor` to now.
    pub fn due(&self, now: DateTime<Local>) -> bool {
        self.next_run() <= now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn next_runs() {
        let daily = Repeat::parse(0, "09:00").unwrap();
        assert_eq!(daily.next(at("2026-10-01T08:00:00Z")), at("2026-10-01T09:00:00Z"));
        assert_eq!(daily.next(at("2026-10-01T09:00:00Z")), at("2026-10-02T09:00:00Z"));
        // Friday 10:00 → Monday 09:00
        let weekdays = Repeat::parse(1, "9:00").unwrap();
        assert_eq!(weekdays.next(at("2026-10-02T10:00:00Z")), at("2026-10-05T09:00:00Z"));
        assert_eq!(Repeat::parse(2, "3").unwrap().next(at("2026-10-01T08:00:00Z")), at("2026-10-01T11:00:00Z"));
        assert_eq!(Repeat::parse(3, "15").unwrap().next(at("2026-10-01T08:00:00Z")), at("2026-10-01T08:15:00Z"));
        assert!(Repeat::parse(0, "9am").is_err());
        assert!(Repeat::parse(3, "0").is_err());
    }
}
