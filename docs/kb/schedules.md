# Schedules

Code: `src/schedule.rs` (pure time logic, `cargo test`), wiring in `src/app/schedules.rs` (`start_ticker`, `run_due`, `release_schedules`), `src/app/panels.rs` (`add_schedule`, `remove_schedule`), and the clock panel `schedules()` in `src/ui/panels.rs`.

- Stored per bot in `state.json` (`Bot.schedules`): prompt, `Repeat` (Daily / Weekdays at HH:MM, every N hours, every N minutes), `anchor` = unix time of the last run (or creation).
- `next_run = repeat.next(anchor)` in local time; weekdays skip Sat/Sun.
- Ticker: first check 3 s after launch, then every 20 s. A due schedule sets `anchor = now`, so any number of missed runs (Mac asleep, app closed) collapses into ONE run.
- A due run pushes `Msg::Scheduled` (amber bubble with the repeat label) and goes through `deliver` → queued if the bot is busy. That wait shares the persisted handoff queue, so it survives a quit.
- Near the plan limit the run does not start and `anchor` stays put, so the schedule remains due (one run, whenever the meter allows). Pause (≥95%) holds every bot of that provider. Throttle (≥90%) holds the others while one bot of that provider is already running. The schedules panel says "waiting". See `usage.rs`.
- Verified 2026-09-30 by injecting an hourly schedule with anchor 2 h old: exactly one run at launch.
- Testing tip: inject into `state.json` while eggbot is closed, and remove it afterwards so it does not keep using the plan.
