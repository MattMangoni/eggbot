# Context management

Bots never reset, so context only grows. Both CLIs auto-compact near the limit (Claude `--autocompact`, Codex automatic + `thread/compact/start`), so nothing crashes — but every turn resends the session (plan usage) and compaction loses detail.

## What eggbot does (decided 2026-10-01)

- **Notes file**: each bot has `~/Library/Application Support/eggbot/bots/<id>/memory` mounted at `/memory`. The file is `/memory/NOTES.md`. The scratch work folder is `bots/<id>/work`, mounted at `/work` only when the bot has no folders. User folders mount at `/work/<name>` (see `sandbox.md`), so notes still never appear inside a project mount (a bot improvised `/work/memory/user.txt` when they overlapped).
- **Injected every turn** (`memory::context`, and `group::notes_for` when the bot is in a group): that text is the notes argument of `skills::role_text`, after skills and the roster and before the folders note. Private notes are the notes rule plus a capped copy of the file (4,000 characters). A group adds a labeled section after that, also capped, and only for a bot who is a member. Claude receives the whole role on `--append-system-prompt` every turn. Codex receives it in `<eggbot-context>` whenever the text changed, including after notes or skills change. The bot follows that copy even if it never opens the file. A note cannot close the Codex wrapper (`</eggbot-context>` is neutralized).
- **How notes grow** (`memory::extract`, `memory::learn`): the model edits `/memory/NOTES.md`, or ends a successful reply with one `<eggbot-learn>` block (`- fact:`, `- preference:`, `- lesson:`, `- forget:`). eggbot merges those bullets into the file (deduped, 16 per section, newest batch on top), strips the block from the chat, and does not include it in a handoff or in the room transcript. A line with no prefix is a lesson. Forget removes an exact bullet, not a substring. Freeform notes already in the file are kept. Nothing is written when the turn errors or is stopped. The next turn reads the file again, including after a restart.
- **Group notes** (`src/group.rs`): a group is a title and member bot ids in `state.json`. It is not a room (no kickoff, no transcript, no facilitator). Its file is `groups/<id>/NOTES.md` under Application Support, same Facts / Preferences / Lessons shape, and it is not mounted in the container. On each turn a member's notes argument is the private block plus one section per group they belong to. A bot outside the group does not get that section. A bullet grows the group file only when it uses a `group` or `shared` prefix in the same `<eggbot-learn>` block (`- group preference: …`, or `- group Reviewers preference: …` when the bot is in more than one group). A bullet without that prefix stays private. Forget on one store does not clear the other. An unnamed group bullet is saved only when the bot is in exactly one group; a name that matches no group of theirs is dropped. There is no lead, and `@Name` is not how notes move. The block is still stripped before the chat, the handoff, and the room transcript.
- **Fresh start** (header button): a turn asks the bot to update its notes (the file, or one `<eggbot-learn>` block); on success eggbot drops the session/thread, resets the context meter and adds a "New session · notes kept" divider.
- **Context meter** (header): Claude = last `message_delta` usage (input + cache read + cache creation + output) vs `result.modelUsage.*.contextWindow`; Codex = `thread/tokenUsage/updated` `last.totalTokens` vs `modelContextWindow`. Amber from 70%.
- **Schedules** run in a throwaway session (`fresh` turn): no resume, and their session id/context are not kept. Verified: answered from NOTES.md, main thread unchanged.
- **Handoffs** use the main session.

## Role changes reaching an existing session (verified with a secret-word test)

- Codex: `thread/resume` and `thread/fork` ignore new `developerInstructions`, and a user message cannot override developer instructions. Fix: fixed `BASE` developer instructions that delegate to `<eggbot-context>` blocks; eggbot sends the role in such a block on a thread's first turn and whenever it changes (`Bot.codex_role`). Verified: new block applied, memory kept.
- Claude: by default the system prompt is snapshotted on the first request (`--system-prompt-snapshot on`), so a later `--append-system-prompt` is ignored on resume. Fix: `--system-prompt-snapshot off` (verified also for sessions recorded with snapshot on).

## Instructions for all bots

Settings holds one text every bot gets after its role, roster and notes rule (`Saved.shared`, None = `SHARED`, the old style line). It is part of the role string, so it reaches running sessions like a role edit: Claude re-reads `--append-system-prompt` every turn; Codex sees a changed role and sends a new `<eggbot-context>` block once.

## Skills

Each bot has `skills: [{name, body}]` in `state.json` (`src/skills.rs`). Hatch copies the preset's defaults (Reviewer, Implementer, Designer; Custom starts empty). The bot then owns that list: the Skills link under the composer adds, edits, and removes on any bot. They are not a shared library and they are not files in the container.

`skills::role_text` inserts them after the bot's role and before the roster. The notes argument after the roster is the durable-notes block, then the folders note. No skills means that piece is empty, so the rest of the role string is unchanged. A later edit changes the role string, so the next turn delivers it: Claude via `--append-system-prompt`, Codex via a new `<eggbot-context>` block when `codex_role` differs. A direct user instruction wins over a skill; that line is in the section itself.
