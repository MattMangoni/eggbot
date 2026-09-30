# Handoff between bots

Code: `src/handoff.rs` (pure: mention parsing, roster, prompt; `cargo test`), wiring in `src/main.rs` (`start_turn`, `deliver`, `hand_off`, `continue_chain`, `Ev::Done` arm).

- Every turn's role prompt lists the other bots (`handoff::roster`) and warns that `@Name` anywhere sends the whole reply.
- When a turn ends OK (not stopped, no error), its reply = all `Msg::Bot` text since the message that started the turn.
- `mentions()`: case-insensitive, longest name wins at each `@`, name must end at a non-alphanumeric char, self and duplicates ignored.
- Hops: user turn = 0; each automatic handoff = previous + 1; `> MAX_HOPS (3)` → stored as `Msg::Handoff { paused: true }`, not delivered. "Continue chain" delivers with hops 0.
- Busy receiver → `Bot.queue` (in memory only); popped when its turn ends.

Observed 2026-09-30 (live test): casual mentions ("@Implementer got my message") also trigger — expected with the "anywhere" rule. Bots tend to stop chains themselves once a task is done.
