# Proactive messages

## When a bot reaches out

- A turn ends with a reply that goes to no other bot (plain reply, scheduled run, last bot of a chain).
- A handoff chain pauses at `MAX_HOPS` → "Chain paused" on the waiting bot.
- A turn fails (not stopped by the user) → "<Bot> needs you" with the error.
- Mid-chain replies stay silent; only the end of the chain speaks.

`Eggbot::alert` (`src/app/chat.rs`) decides: nothing if the window is active and the bot is selected; otherwise the bot gets `unread` (sidebar dot, menu bar badge); a macOS notification only when the window is not active.

## Quiet scheduled runs

Scheduled prompts get the `QUIET` hint. A reply of exactly `QUIET` is removed and replaced by a "Nothing to report" divider, with no unread and no notification.

## macOS notifications (src/notify.rs)

- `UNUserNotificationCenter` via `objc2-user-notifications`. It raises an ObjC exception without a bundle id, so `notify` checks `NSBundle.mainBundle.bundleIdentifier` first: `cargo run` has no notifications.
- Permission is asked at launch (once per bundle id). Alert only, no sound.
- Request id `"<bot>:<millis>"`, thread id `bot<id>` (grouped per bot). The delegate parses the id on click and sends `tray::Action::Bot` down the shared channel; the center holds its delegate weakly, so it is leaked on purpose.
- No `willPresent` handler: notifications are only sent while eggbot is in the background.
