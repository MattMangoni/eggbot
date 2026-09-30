# Schedules

Code: `src/schedule.rs` (pure time logic, `cargo test`), wiring in `src/main.rs` (`run_due`, `add_schedule`, `remove_schedule`, clock panel `schedules()`).

- Stored per bot in `state.json` (`Bot.schedules`): prompt, `Repeat` (Daily / Weekdays at HH:MM, every N hours, every N minutes), `anchor` = unix time of the last run (or creation).
- `next_run = repeat.next(anchor)` in local time; weekdays skip Sat/Sun.
- Ticker: first check 3 s after launch, then every 20 s. A due schedule sets `anchor = now`, so any number of missed runs (Mac asleep, app closed) collapses into ONE run.
- A due run pushes `Msg::Scheduled` (amber bubble with the repeat label) and goes through `deliver` → queued if the bot is busy.
- Verified 2026-09-30 by injecting an hourly schedule with anchor 2 h old: exactly one run at launch.
- Testing tip: inject into `state.json` while eggbot is closed, and remove it afterwards so it does not keep using the plan.
