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
- App data: `~/Library/Application Support/eggbot/` (`state.json`, `bots/<id>/`). Delete it to start fresh.

## Layout

- `src/main.rs` — app state, window, sidebar, chat, composer.
- `src/claude.rs` — runs one `claude -p` turn, parses stream-json into events (`cargo test` covers the parser).
- `src/egg.rs` — vector egg avatar and its animations (hatch, idle, thinking).
- `docs/kb/` — knowledge base: `decisions.md`, `auth.md`, `gpui.md`, `claude-cli.md`.

## Hard rules

- Claude access only through the unmodified `claude` binary. Never read, store, or forward Claude OAuth tokens; never call Anthropic endpoints directly. See `docs/kb/auth.md`.
- Codex access only through the official `codex` CLI.
- UI stack is `gpui-kit` 0.7 (bundles `gpui-pre` 0.3.7 + `gpui-component`). Import via `gpui_kit::*`, not `gpui::*`.

## Conventions

- Conventional Commits, one line, imperative: `feat(ui): add hatch animation`.
- No AI/tool attribution in commits, PRs, or files.
- Comments: one line, only for non-obvious reasons or traps.
- Mark deliberate shortcuts with `// ponytail: <ceiling>, <upgrade path>`.
