# eggbot

A native macOS app (Rust + GPUI) that hosts always-on AI bots. Each bot has a role (Reviewer, Implementer, Designer, custom), runs on the user's Claude or ChatGPT subscription through the official CLIs, and works inside its own Docker container.

## Working with Matteo

- Ask every open question; never decide on Matteo's behalf. Always give a recommendation with the options.
- Stop at the end of each phase (and on any surprise) and check in before continuing.
- Simplicity first: no speculative abstractions, fewest files, reuse before writing.
- Record what you learn in `docs/kb/` as you go (decisions, API findings, traps).

## Commands

- `cargo run` — build and open the app.
- `cargo build` — compile only. First build takes minutes (GPUI).
- `cargo test` — parser check.
- `scripts/bundle.sh` — release build → `eggbot.app` (id `com.digitalmaze.eggbot`, ad-hoc signed) in `~/Applications`. Notifications only work from the bundle.
- App data: `~/Library/Application Support/eggbot/` (`state.json`, `bots/<id>/work` scratch, `bots/<id>/memory` notes). Delete it to start fresh.

## Layout

- `src/main.rs` — app state and behaviour (turns, handoffs, schedules, sessions), window setup.
- `src/ui.rs` — everything drawn: sidebar, top bar, chat, composer, panels.
- `src/claude.rs` — shared agent types (`Provider`, `Ev`, `Turn`, `Handle`, `Meter`) and the `claude -p` runner + stream-json parser (tested).
- `src/codex.rs` — Codex runner over `codex app-server` JSON-RPC, notification parser (tested), account/models query.
- `src/handoff.rs` — `@Name` mention parsing, roster and handoff prompt (tested).
- `src/room.rs` — room roster and kickoff prompt (tested).
- `src/schedule.rs` — schedule repeats and next-run times (tested).
- `src/usage.rs` — plan-usage guard: throttle and pause thresholds (tested).
- `src/skills.rs` — preset default skills, validation, and the role section (tested).
- `src/tray.rs` — menu bar egg icon (unread badge) and menu.
- `src/notify.rs` — macOS notifications (bundle only); a click opens the bot.
- `src/login.rs` — start at login (SMAppService, bundle only) and login-launch detection.
- `src/sandbox.rs` — Docker (any engine: OrbStack, Docker Desktop, Colima): bot image, one container per bot, sign-in terminal.
- `docker/bot.Dockerfile` — the bot machine image.
- `scripts/icon.swift` — draws the app icon (used by `bundle.sh`).
- `src/egg.rs` — small vector egg avatar (still, or wobbling while working).
- `docs/kb/` — knowledge base: `decisions.md`, `auth.md`, `gpui.md`, `claude-cli.md`, `sandbox.md`, `handoff.md`, `schedules.md`, `codex.md`, `context.md`, `notifications.md`.

## Hard rules

- Claude access only through the unmodified `claude` binary. Never read, store, or forward Claude OAuth tokens; never call Anthropic endpoints directly. See `docs/kb/auth.md`.
- Codex access only through the official `codex` CLI (`codex app-server` JSON-RPC, pinned version in the bot image). See `docs/kb/codex.md`.
- UI stack is `gpui-kit` 0.7 (bundles `gpui-pre` 0.3.7 + `gpui-component`). Import via `gpui_kit::*`, not `gpui::*`.

## Conventions

- Conventional Commits, one line, imperative: `feat(ui): add hatch animation`.
- No AI/tool attribution in commits, PRs, or files.
- Comments: one line, only for non-obvious reasons or traps.
- Mark deliberate shortcuts with `// ponytail: <ceiling>, <upgrade path>`.
