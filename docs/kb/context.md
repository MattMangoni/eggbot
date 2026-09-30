# Context management

Bots never reset, so context only grows. Both CLIs auto-compact near the limit (Claude `--autocompact`, Codex automatic + `thread/compact/start`), so nothing crashes — but every turn resends the session (plan usage) and compaction loses detail.

## What eggbot does (decided 2026-10-01)

- **Notes file**: each bot has `~/Library/Application Support/eggbot/bots/<id>/memory` mounted at `/memory`. The role (`NOTES` in `src/main.rs`) tells it to read `/memory/NOTES.md` when a session starts and keep durable facts there. The scratch work folder is `bots/<id>/work` — kept separate so a bot never sees its notes inside `/work` (it improvised `/work/memory/user.txt` when they overlapped).
- **Fresh start** (header button): a turn with `FRESH_START` asks the bot to update its notes; on success eggbot drops the session/thread, resets the context meter and adds a "New session · notes kept" divider.
- **Context meter** (header): Claude = last `message_delta` usage (input + cache read + cache creation + output) vs `result.modelUsage.*.contextWindow`; Codex = `thread/tokenUsage/updated` `last.totalTokens` vs `modelContextWindow`. Amber from 70%.
- **Schedules** run in a throwaway session (`fresh` turn): no resume, and their session id/context are not kept. Verified: answered from NOTES.md, main thread unchanged.
- **Handoffs** use the main session.

## Role changes reaching an existing session (verified with a secret-word test)

- Codex: `thread/resume` and `thread/fork` ignore new `developerInstructions`, and a user message cannot override developer instructions. Fix: fixed `BASE` developer instructions that delegate to `<eggbot-context>` blocks; eggbot sends the role in such a block on a thread's first turn and whenever it changes (`Bot.codex_role`). Verified: new block applied, memory kept.
- Claude: by default the system prompt is snapshotted on the first request (`--system-prompt-snapshot on`), so a later `--append-system-prompt` is ignored on resume. Fix: `--system-prompt-snapshot off` (verified also for sessions recorded with snapshot on).
