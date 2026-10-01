# Subscription auth: what is allowed

Checked 2026-09-30 against https://code.claude.com/docs/en/legal-and-compliance and https://code.claude.com/docs/en/authentication.

## Claude

Allowed:
- An end user signing in to the **unmodified** Claude Code binary with their own subscription, including when a platform hosts it.
- `claude setup-token` → `CLAUDE_CODE_OAUTH_TOKEN` for scripts/CI (one-year token, Pro/Max/Team/Enterprise, model requests only).

Not allowed:
- Using subscription OAuth tokens to call the API directly from our own code.
- Offering Claude.ai login inside our app, or routing other users' requests through a subscription.
- Developers "collect, store, or intermediate Claude.ai credentials or session tokens".

Risk that is not a ban: Pro/Max limits "assume ordinary, individual usage". Many parallel always-on bots burn the weekly limit fast — surface limit errors in the UI, keep schedules small.

Our approach: user runs `claude /login` inside a container once; the CLI stores credentials in a Docker volume shared by bot containers. eggbot never reads the token.

Credential facts:
- macOS: credentials live in the Keychain, so mounting `~/.claude` into a container does not carry the login.
- Linux (containers): `~/.claude/.credentials.json`, mode 0600.
- `--bare` mode ignores `CLAUDE_CODE_OAUTH_TOKEN`.
- Shared volume + concurrent containers: Claude Code uses a refresh lock across processes; 2.1.285 fixed a race in recovering that lock (our image has 2.1.286). The docs still list a transient error, "another Claude Code process is refreshing it". eggbot retries such a turn once after 5 s (`Bot.retried`); a second failure shows as a normal error with the "needs you" notification. Codex: not documented, no retry.
- `claude setup-token` is the documented container path, but the token would pass through eggbot, which our rules forbid.

## Codex

- Login stored in `~/.codex/auth.json` (a file, mountable read-only).
- OpenAI help (checked 2026-09-30): ChatGPT plans can run Codex from the terminal, including scripted `codex exec`. ChatGPT Terms of Use apply. Limits vary by plan; do not rely on a fixed quota.
