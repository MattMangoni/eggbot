# Handoff between bots

Code: `src/handoff.rs` (pure: mention parsing, roster, prompt, queue resume; `cargo test`), wiring in `src/main.rs` (`start_turn`, `deliver`, `hand_off`, `continue_chain`, `restore_handoffs`, `pump_queues`, `Ev::Done` arm).

- Every turn's role prompt lists the other bots (`handoff::roster`) and warns that `@Name` anywhere sends the whole reply.
- When a turn ends OK (not stopped, no error), its reply = all `Msg::Bot` text since the message that started the turn.
- `mentions()`: case-insensitive, longest name wins at each `@`, name must end at a non-alphanumeric char, self and duplicates ignored.
- Hops: user turn = 0; each automatic handoff = previous + 1; `> MAX_HOPS (3)` → stored as `Msg::Handoff { paused: true }`, not delivered. "Continue chain" delivers with hops 0.
- Busy receiver → `Bot.queue` (`handoff::Pending`: prompt, hops, fresh session, handoff flag). Popped when its turn ends.
- The queue is saved on the bot in `state.json`. A crash or quit keeps every hop that was waiting.
- The running turn is saved as `Bot.current`. On the next launch, an in-flight `@Name` hop (including Continue chain) is put back at the front of the queue. A user turn or a schedule that was running stays stopped.
- Resume waits until the bot is idle and `docker info` succeeds, then starts the first queued turn (that starts a stopped container). Checked about 3s after launch, then every 20s. While Docker is down the queue stays put. A busy bot keeps the queue until its turn ends. If a turn fails because the engine is down, the hop goes back on the queue instead of starting the next item into the same failure. Any other failure is the usual error: that hop was attempted.
- Schedules that were queued behind a busy bot share this queue, so they survive too. A schedule that had already started does not restart.

Observed 2026-09-30 (live test): casual mentions ("@Implementer got my message") also trigger — expected with the "anywhere" rule. Bots tend to stop chains themselves once a task is done.
