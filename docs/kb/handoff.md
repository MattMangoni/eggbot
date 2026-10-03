# Handoff between bots

Code: `src/handoff.rs` (pure: mention parsing, roster, prompt, queue resume; `cargo test`), wiring in `src/app/turns.rs` (`start_turn`, the `Ev::Done` arm of `apply`), `src/app/handoffs.rs` (`deliver`, `hand_off`, `continue_chain`, `pump_queues`), and `src/app/state.rs` (`Saved::migrate` puts an in-flight hop back on load).

- Every turn's role prompt lists the other bots (`handoff::roster`): name, specialty blurb, and when to call them. `@Name` only when that specialty fits the next step better than doing the work yourself — not to narrate, acknowledge, or think out loud. Writing `@Name` anywhere still sends the whole reply.
- The same text lists up to four recent successful handoffs (`Bot.recent`, newest first; a paused chain is not recorded; a deleted bot is left out) and, for each room the bot is in, that room's peers. Prefer those peers for the room's work. There is still no lead or dispatcher.
- When a turn ends OK (not stopped, no error), its reply = bot text written after the hop starts (`reply_from`). An `<eggbot-learn>` block is removed first, so it is not shown, not handed on, and not appended to the room transcript (see `context.md`). A `group` / `shared` bullet inside that block is removed with it and updates the group notes file. A `room` bullet is removed with it and updates that room's memory file. Neither is the handoff.
- `mentions()`: case-insensitive, longest name wins at each `@`, name must end at a non-alphanumeric char, self and duplicates ignored.
- Hops: user turn = 0; each automatic handoff = previous + 1; `> MAX_HOPS (3)` → stored as `Msg::Handoff { paused: true }`, not delivered. "Continue chain" delivers with hops 0.
- Busy receiver → `Bot.queue` (`handoff::Pending`: prompt, hops, fresh session, handoff flag). Popped when its turn ends.
- The queue is saved on the bot in `state.json`. A crash or quit keeps every hop that was waiting.
- The running turn is saved as `Bot.current`. On the next launch, an in-flight `@Name` hop (including Continue chain) is put back at the front of the queue. A user turn or a schedule that was running stays stopped.
- Resume waits until the bot is idle and `docker info` succeeds, then starts the first queued turn (that starts a stopped container). Checked about 3s after launch, then every 20s. While Docker is down the queue stays put. A busy bot keeps the queue until its turn ends. If a turn fails because the engine is down, the hop goes back on the queue instead of starting the next item into the same failure. Any other failure is the usual error: that hop was attempted.
- Usage guardrails (pause at 95%, throttle at 90%) do not clear that queue. `pump_queues` leaves it in `state.json` until the bot is idle, Docker is up, and `may_start` is true. Quitting while a hop is held does not drop it.
- Schedules that were queued behind a busy bot share this queue, so they survive too. A schedule that had already started does not restart.
- A message the user sends while the bot works, or while the throttle holds it behind another bot, also goes on this queue, as `Pending::typed`. The chat shows it as a "Queued" bubble under the running turn; its × removes only that entry (`unqueue`). Stop does not clear or hold the queue. `start_turn` adds the real user bubble when its turn starts, then clears `typed`, so a sign-in retry does not add it twice. Like other queued work, it is saved and starts after a relaunch. A pause still keeps the text in the box.

Observed 2026-09-30 (live test): casual mentions ("@Implementer got my message") also trigger — expected with the "anywhere" rule. Bots tend to stop chains themselves once a task is done.

## Folders in the handoff

`handoff::prompt` compares host paths and names container paths (`/work/<name>`). Scratch folders are per bot and never count as shared. Either bot with no folders gets "They work in a different folder; you cannot see their files."

- Same host path: "You share /work/docs with them…". If the names differ (`/work/docs` vs `/work/docs-2`), both are written.
- The receiver's folder is inside the sender's: the sender can see those files; the receiver cannot see the rest. The closest parent is the one named.
- The sender's folder is inside the receiver's: the reverse.
- A nested path that sits under a folder both already share is not repeated.
- No shared or nested path: the "different folder" line.

## Rooms

Code: `src/room.rs` (roster edits, kickoff prompt, transcript; `cargo test`), wiring in `src/app/rooms.rs` (`start_room`, `show_room`) and `src/app/handoffs.rs` (`hand_off`, `log_room`).

A room is not a new kind of bot. It stores a title, a kickoff, bot ids, and a transcript in `state.json`, plus which member is the facilitator and whether the room is unread.

The kickoff tells the facilitator to call a peer when their specialty fits, and not to `@Name` just to keep them posted. Start delivers the kickoff to the facilitator only, as `Pending::handoff` with hops 0, through `deliver`. The hop still starts with `start_turn`, so `skills::role_text` injects that bot's skills the same way it does for a typed message. A busy facilitator, a pause, or a throttle keeps it on the same persisted queue. `pump_queues` starts it only when the bot is idle, Docker is up, and `may_start` is true. A quit or crash puts an in-flight kickoff back on the queue. Hops stay 0, so the kickoff does not use up the chain limit. The facilitator's reply can `@Name` the other members; those are ordinary handoffs. There is no lead bot. A "Room · …" card in the facilitator's chat opens the room. Each bot's own chat stays its own.

The room view is that conversation in one list: the kickoff text, the facilitator's reply, each `@Name` whose target is still a member, and that member's reply. `Pending.room` is the room id. `room::carry` drops it when the target is not a member, so a handoff that leaves the room does not pull the outsider's reply back in. The same id is stored on the receiver's handoff card, so Continue chain stays on the transcript. The reply recorded is the bot text written after the hop starts (`reply_from`), with any `<eggbot-learn>` block already removed. Tool lines stay in the bot's chat. The room dot turns on when a transcript line arrives while the room is closed, and clears when the room is opened. Rooms that started before the transcript field keep an empty log; new kickoffs append from Start.

Opening the room also shows its memory (`rooms/<id>/NOTES.md`), above the transcript. That file is readable there for anyone who opens the room. It is not the transcript, and the view does not edit it. A member bot on a turn in this room receives it after private notes and any group notes. A private turn does not. See `context.md`.

## Groups

Code: `src/group.rs` (membership, which learn-bullets are shared, the notes section; `cargo test`).

A group is not a room and not a shared transcript. It is a title plus member ids, and one notes file those members all read on their next turn. It does not change `@Name`, the queue, hop limits, pause, or throttle. There is no lead. See `context.md`.
