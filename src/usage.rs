//! When a plan window is nearly full, hold new turns instead of burning the subscription.
//!
//! Amber (80%) is only a color in the sidebar. Throttle and pause are the actions.
//! A window whose reset time has passed counts as empty, so bots resume even if no new
//! usage event has arrived yet.

use crate::claude::{Provider, Window};

/// Sidebar bars turn amber here. Not a gate.
pub const AMBER: f32 = 0.80;
/// One bot of that provider at a time.
pub const DEFAULT_THROTTLE: f32 = 0.90;
/// No new turns. The one already running finishes.
pub const DEFAULT_PAUSE: f32 = 0.95;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    /// Extra bots wait; schedules wait if one bot of this provider is already going.
    Throttle,
    /// Nothing new starts. Due schedules stay due.
    Pause,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Breach {
    pub level: Level,
    pub label: String,
    pub used: f32,
    /// Unix seconds the window resets; 0 if the CLI did not say.
    pub reset: i64,
}

pub fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
    }
}

/// 0..1, or 0 once `reset` is in the past.
pub fn window_used(w: &Window, now: i64) -> f32 {
    if w.reset > 0 && now >= w.reset { 0. } else { w.used.clamp(0., 1.) }
}

pub fn percent(used: f32) -> u32 {
    (used.clamp(0., 1.) * 100.).round() as u32
}

/// The worst window at or over `throttle`. Pause wins over throttle; ties keep the fuller window.
pub fn breach(windows: &[Window], now: i64, throttle: f32, pause: f32) -> Option<Breach> {
    let mut best: Option<Breach> = None;
    for w in windows {
        let used = window_used(w, now);
        let level = if used >= pause {
            Level::Pause
        } else if used >= throttle {
            Level::Throttle
        } else {
            continue;
        };
        let next = Breach { level, label: w.label.clone(), used, reset: w.reset };
        let replace = match &best {
            None => true,
            Some(b) => rank(next.level) > rank(b.level) || (next.level == b.level && next.used > b.used),
        };
        if replace {
            best = Some(next);
        }
    }
    best
}

fn rank(level: Level) -> u8 {
    match level {
        Level::Ok => 0,
        Level::Throttle => 1,
        Level::Pause => 2,
    }
}

/// True when a due schedule should start (and move its anchor). False leaves it due.
pub fn schedule_action(level: Level, bot_busy: bool, bot_queued: bool, provider_taken: bool) -> bool {
    if level == Level::Pause {
        return false;
    }
    // a turn the user already asked for goes first; the schedule stays due
    if bot_queued && !bot_busy {
        return false;
    }
    // throttle: a sibling that is running or waiting owns the provider
    if level == Level::Throttle && provider_taken && !bot_busy {
        return false;
    }
    true
}

pub fn reset_at(reset: i64) -> Option<String> {
    if reset <= 0 {
        return None;
    }
    chrono::DateTime::from_timestamp(reset, 0).map(|t| t.with_timezone(&chrono::Local).format("%a %H:%M").to_string())
}

pub fn explain(provider: &str, breach: &Breach, pause_at: f32, throttle_at: f32) -> String {
    let pct = percent(breach.used);
    let when = reset_at(breach.reset).map(|t| format!(" Resets {t}.")).unwrap_or_default();
    match breach.level {
        Level::Pause => format!("Paused — {provider} {} is at {pct}% (pauses at {}%). New turns wait, and a draft stays in the box.{when}", breach.label, percent(pause_at)),
        Level::Throttle => format!("Throttled — {provider} {} is at {pct}% (one bot at a time from {}%).{when}", breach.label, percent(throttle_at)),
        Level::Ok => String::new(),
    }
}

/// Sidebar row: why this bot is not starting.
pub fn short(provider: &str, breach: &Breach) -> String {
    let word = match breach.level {
        Level::Pause => "Paused",
        Level::Throttle => "Throttled",
        Level::Ok => "Ok",
    };
    format!("{word} · {provider} {} {}%", breach.label, percent(breach.used))
}

/// One line under the provider's bars.
pub fn meter_line(breach: &Breach) -> String {
    let word = match breach.level {
        Level::Pause => "Paused",
        Level::Throttle => "One at a time",
        Level::Ok => "",
    };
    match reset_at(breach.reset) {
        Some(t) => format!("{word} · {} {}% · resets {t}", breach.label, percent(breach.used)),
        None => format!("{word} · {} {}%", breach.label, percent(breach.used)),
    }
}

/// Settings fields, as whole percents. Throttle must sit strictly below pause.
pub fn parse_limits(throttle: &str, pause: &str) -> Result<(f32, f32), &'static str> {
    let throttle = parse_percent(throttle)?;
    let pause = parse_percent(pause)?;
    if throttle >= pause {
        return Err("Throttle must be lower than pause");
    }
    Ok((throttle as f32 / 100., pause as f32 / 100.))
}

fn parse_percent(text: &str) -> Result<u32, &'static str> {
    match text.trim().parse::<u32>() {
        Ok(n) if (1..=99).contains(&n) => Ok(n),
        _ => Err("Use a whole percent from 1 to 99"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(label: &str, used: f32, reset: i64) -> Window {
        Window { label: label.into(), used, reset }
    }

    #[test]
    fn worst_window_sets_the_guard() {
        let windows = [w("5h", 0.91, 500), w("7d", 0.96, 900)];
        let b = breach(&windows, 100, 0.90, 0.95).unwrap();
        assert_eq!(b.level, Level::Pause);
        assert_eq!(b.label, "7d");

        let b = breach(&[w("5h", 0.91, 500), w("7d", 0.80, 900)], 100, 0.90, 0.95).unwrap();
        assert_eq!(b.level, Level::Throttle);
        assert_eq!(b.label, "5h");

        assert!(breach(&[w("5h", 0.80, 500)], 100, 0.90, 0.95).is_none());
    }

    #[test]
    fn a_reset_window_is_empty() {
        assert!(breach(&[w("5h", 0.99, 50)], 50, 0.90, 0.95).is_none());
        assert_eq!(window_used(&w("5h", 0.99, 0), 50), 0.99);
    }

    #[test]
    fn schedules_wait_for_pause_and_for_a_busy_sibling() {
        assert!(!schedule_action(Level::Pause, false, false, false));
        assert!(!schedule_action(Level::Pause, true, false, true));
        assert!(!schedule_action(Level::Throttle, false, false, true));
        assert!(schedule_action(Level::Throttle, true, false, true));
        assert!(schedule_action(Level::Throttle, false, false, false));
        assert!(!schedule_action(Level::Ok, false, true, false));
        assert!(schedule_action(Level::Ok, true, false, true));
    }

    #[test]
    fn limits_are_whole_percents_with_throttle_below_pause() {
        let (t, p) = parse_limits("90", "95").unwrap();
        assert_eq!(percent(t), 90);
        assert_eq!(percent(p), 95);
        assert!(parse_limits("95", "95").is_err());
        assert!(parse_limits("96", "95").is_err());
        assert!(parse_limits("0", "95").is_err());
        assert!(parse_limits("90", "100").is_err());
        assert!(parse_limits("ninety", "95").is_err());
    }
}
