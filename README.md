# eggbot

A native macOS app that hosts always-on AI bots. Each bot has a role (Reviewer, Implementer, Designer or your own), runs on your own Claude or ChatGPT subscription through the official CLIs, and works inside its own Docker container.

Built in Rust with [GPUI](https://www.gpui.rs).

## What it does

- **Bots with roles.** Start from a preset or write your own role. Pick the model and effort per bot.
- **Skills.** Each preset starts with a few procedures. Add, edit, or remove them on any bot; they stay with that bot and go out on the next turn.
- **Claude and Codex.** Each bot uses `claude` (Claude Code) or `codex` (Codex CLI) with your own login. No API keys.
- **One container per bot.** Bots get full power inside their own machine, and only the folders you mount.
- **Handoffs.** A bot that writes `@Name` hands its reply to that bot. Chains pause after 3 hops.
- **Schedules.** Daily, weekdays, or every N hours or minutes. A run with nothing to report stays quiet.
- **Memory.** Each bot keeps short notes (facts, preferences, lessons) that survive a restart and a fresh session, and reads them on later turns.
- **Proactive messages.** Notifications when a bot finishes, needs you, or a chain pauses, plus an unread dot.
- **Menu bar egg.** Close the window and the bots keep working. Start at login is optional.

## Requirements

- macOS 13 or later
- A Docker engine with the `docker` CLI: [OrbStack](https://orbstack.dev), [Docker Desktop](https://www.docker.com/products/docker-desktop/) or [Colima](https://github.com/abiosoft/colima). eggbot starts it when needed.
- A Claude Pro/Max plan and/or a ChatGPT plan with Codex
- Rust (stable) and Xcode Command Line Tools, to build

## Build and run

```sh
scripts/bundle.sh          # release build → ~/Applications/eggbot.app
open ~/Applications/eggbot.app
```

`cargo run` also works for development, but notifications and start at login need the app bundle.

On first launch a setup checklist walks you through it: install a Docker engine (Colima by default, through Homebrew in Terminal), start it, build the bot image (about a minute), and sign in to Claude and/or Codex with each provider's own login flow in Terminal. Reopen it any time with eggbot → Setup….

## How your login is handled

- Claude runs only through the unmodified `claude` binary, and Codex only through the official `codex` CLI.
- You sign in with each provider's own flow in Terminal. The login lives in a Docker volume that the bot containers share.
- eggbot never reads, stores or forwards your tokens, and never calls Anthropic or OpenAI endpoints itself.

Subscription plans assume individual use, and many busy bots use your plan limits quickly. The sidebar shows your usage. At 90% eggbot runs one bot per provider; at 95% it pauses new turns and holds schedules until the meter drops. Change the percents in Settings.

## Data

State lives in `~/Library/Application Support/eggbot/` (`state.json`, per bot `bots/<id>/work` and `bots/<id>/memory`, per group `groups/<id>/NOTES.md`, and per room `rooms/<id>/NOTES.md`). Delete that folder to start fresh.

## Development

See [`AGENTS.md`](AGENTS.md) for the layout and rules, and [`docs/kb/`](docs/kb/) for the decisions and findings behind the code.

## License

[MIT](LICENSE)
