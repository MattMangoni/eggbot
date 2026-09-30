# eggbot

A native macOS app that hosts always-on AI bots. Each bot has a role (Reviewer, Implementer, Designer or your own), runs on your own Claude or ChatGPT subscription through the official CLIs, and works inside its own Docker container.

Built in Rust with [GPUI](https://www.gpui.rs).

## What it does

- **Bots with roles.** Start from a preset or write your own role. Pick the model and effort per bot.
- **Claude and Codex.** Each bot uses `claude` (Claude Code) or `codex` (Codex CLI) with your own login. No API keys.
- **One container per bot.** Bots get full power inside their own machine, and only the project folder you mount.
- **Handoffs.** A bot that writes `@Name` hands its reply to that bot. Chains pause after 3 hops.
- **Schedules.** Daily, weekdays, or every N hours or minutes. A run with nothing to report stays quiet.
- **Memory.** Each bot keeps short notes that survive fresh sessions.
- **Proactive messages.** Notifications when a bot finishes, needs you, or a chain pauses, plus an unread dot.
- **Menu bar egg.** Close the window and the bots keep working. Start at login is optional.

## Requirements

- macOS 13 or later
- [Colima](https://github.com/abiosoft/colima) (or another Docker engine) and the `docker` CLI, from Homebrew
- A Claude Pro/Max plan and/or a ChatGPT plan with Codex
- Rust (stable) and Xcode Command Line Tools, to build

## Build and run

```sh
scripts/bundle.sh          # release build → ~/Applications/eggbot.app
open ~/Applications/eggbot.app
```

`cargo run` also works for development, but notifications and start at login need the app bundle.

The first turn builds the bot image (about a minute). Sign in when a bot asks: eggbot opens Terminal with the provider's own login flow.

## How your login is handled

- Claude runs only through the unmodified `claude` binary, and Codex only through the official `codex` CLI.
- You sign in with each provider's own flow in Terminal. The login lives in a Docker volume that the bot containers share.
- eggbot never reads, stores or forwards your tokens, and never calls Anthropic or OpenAI endpoints itself.

Subscription plans assume individual use, and many busy bots use your plan limits quickly. The sidebar shows your usage.

## Data

State lives in `~/Library/Application Support/eggbot/` (`state.json`, and per bot `bots/<id>/work` and `bots/<id>/memory`). Delete that folder to start fresh.

## Development

See [`AGENTS.md`](AGENTS.md) for the layout and rules, and [`docs/kb/`](docs/kb/) for the decisions and findings behind the code.

## License

[MIT](LICENSE)
