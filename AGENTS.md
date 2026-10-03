# eggbot

A native macOS app (Rust + GPUI) that hosts always-on AI bots. Each bot has a role (Reviewer, Implementer, Designer, custom), runs on the user's Claude or ChatGPT subscription through the official CLIs, and works inside its own Docker container.

## Working with Matteo

- Ask every open question; never decide on Matteo's behalf. Always give a recommendation with the options.
- Stop at the end of each phase (and on any surprise) and check in before continuing.
- Simplicity first: no speculative abstractions, reuse before writing.
- Record what you learn in `docs/kb/` as you go (decisions, API findings, traps).

## Commands

- `cargo run` — build and open the app.
- `cargo build` — compile only. First build takes minutes (GPUI).
- `cargo test` — unit tests for the pure logic (parsers, routing, state.json migration).
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` — CI runs both. `rustfmt.toml` keeps short builder chains on one line.
- `scripts/bundle.sh` — release build → `eggbot.app` (id `com.digitalmaze.eggbot`, ad-hoc signed) in `~/Applications`. Notifications only work from the bundle.
- App data: `~/Library/Application Support/eggbot/` (`state.json`, `bots/<id>/work` scratch, `bots/<id>/memory` notes, `groups/<id>/NOTES.md` shared notes, `rooms/<id>/NOTES.md` room memory). Delete it to start fresh.

## Layout

`src/main.rs` starts the app. The `Eggbot` view is split in two folders: `app/` holds its state and behaviour, `ui/` draws it. Each other module owns one domain; the ones marked (tested) are pure logic with unit tests.

- `src/main.rs` — startup and window setup: PATH for Finder launches, extra icons, vibrancy, Dock icon.
- `src/app/` — the `Eggbot` view's state and behaviour:
  - `mod.rs` — the struct, its constructor (inputs, subscriptions, tray), show and quit, lookups by id.
  - `state.rs` — `state.json`: saved fields, load, migration of older files, save.
  - `bot.rs` — the bot model (presets, `Msg`, `Bot`), hatch, delete, reorder, folders, model and effort picks.
  - `turns.rs` — one turn: send, start with its role text, stream events, finish (notes, quiet runs, alerts).
  - `handoffs.rs` — `@Name` routing, the persisted queue, Continue chain, room transcript lines.
  - `schedules.rs` — the background tick, due schedules, and the usage guard (`may_start`, pause, throttle).
  - `rooms.rs` — rooms and groups: open, edit, start, delete.
  - `chat.rs` — the open chat: selection, drafts, list sync, search, unread and alerts.
  - `panels.rs` — panel forms: settings, bot editor, skills, schedules.
  - `setup.rs` — first-run checklist, sign-in watch, Codex account query.
  - `actions.rs` — actions, shortcuts, menus, and what each action does.
- `src/ui/` — everything drawn:
  - `mod.rs` — the root view and shared widgets (`button`, `primary`, `link`, `field`, `label`, `heading`, `divider`, `bar`, `panel`).
  - `theme.rs` — palette and light/dark appearance.
  - `sidebar.rs` — bot list (drag to reorder), rooms, groups, usage meters, hatch menu.
  - `topbar.rs` — the chat top bar: editor toggle, folders popover, search, context meter, Fresh start.
  - `chat.rs` — the main pane and one row per message.
  - `composer.rs` — message box, model and effort dropdowns, usage banner.
  - `panels.rs` — editor, settings, skills, schedules, and the setup checklist.
  - `room_view.rs`, `group_view.rs` — the room and group views.
- `src/claude.rs` — shared agent types (`Provider`, `Ev`, `Turn`, `Handle`, `Meter`) and the `claude -p` runner + stream-json parser (tested).
- `src/codex.rs` — Codex runner over `codex app-server` JSON-RPC, notification parser (tested), account/models query.
- `src/handoff.rs` — `@Name` mention parsing, roster, handoff prompt, queue resume (tested).
- `src/memory.rs` — durable notes: learn-block capture, dedupe, role injection (tested).
- `src/group.rs` — group membership and shared notes (tested). Not a room.
- `src/room.rs` — room roster, kickoff prompt, transcript, and room memory (tested).
- `src/schedule.rs` — schedule repeats and next-run times (tested).
- `src/usage.rs` — plan-usage guard: throttle and pause thresholds (tested).
- `src/skills.rs` — preset default skills, validation, and the role section (tested).
- `src/sandbox.rs` — Docker (any engine: OrbStack, Docker Desktop, Colima): bot image, one container per bot, sign-in terminal.
- `src/tray.rs` — menu bar egg icon (unread badge) and menu.
- `src/notify.rs` — macOS notifications (bundle only); a click opens the bot.
- `src/login.rs` — start at login (SMAppService, bundle only) and login-launch detection.
- `src/egg.rs` — small vector egg avatar (still, or wobbling while working).
- `docker/bot.Dockerfile` — the bot machine image.
- `scripts/icon.swift` — draws the app icon (used by `bundle.sh`).
- `docs/kb/` — knowledge base: `decisions.md`, `auth.md`, `gpui.md`, `claude-cli.md`, `sandbox.md`, `handoff.md`, `schedules.md`, `codex.md`, `context.md`, `notifications.md`.

## Hard rules

- Claude access only through the unmodified `claude` binary. Never read, store, or forward Claude OAuth tokens; never call Anthropic endpoints directly. See `docs/kb/auth.md`.
- Codex access only through the official `codex` CLI (`codex app-server` JSON-RPC, pinned version in the bot image). See `docs/kb/codex.md`.
- UI stack is `gpui-kit` 0.7 (bundles `gpui-pre` 0.3.7 + `gpui-component`). Import via `gpui_kit::*`, not `gpui::*`.

## File organization

- No god files. Each file has one job you can name in a few words (`composer`, `state.json load and save`). Aim for 200–600 lines.
- Before you add to a file over 600 lines, split out the part you touch by responsibility first, in its own `refactor:` commit with no behaviour change.
- New code goes in the file that owns that job. If no file owns it, add one; do not append it to `main.rs` or a view file because it is open.
- No tiny-file sprawl: no new file under ~80 lines unless it is a natural boundary.
- At most one folder level (for example `src/ui/`); no `mod.rs` that only re-exports.
- Tests stay in the same file as the code they test.

## Conventions

- Conventional Commits, one line, imperative: `feat(ui): add hatch animation`.
- No AI/tool attribution in commits, PRs, or files.
- Comments: one line, only for non-obvious reasons or traps.
- Mark deliberate shortcuts with `// ponytail: <ceiling>, <upgrade path>`.
