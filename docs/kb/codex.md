# Driving the `codex` CLI

Observed 2026-10-01 with codex-cli 0.159.0, ChatGPT login.

## Terms

OpenAI help: ChatGPT plans can run Codex from the terminal, including scripted use. See `auth.md`.

## `codex exec --json` (stable, not used)

- Waits for stdin (`Reading additional input from stdin...`) unless stdin is closed — always pass `< /dev/null` / `Stdio::null()`.
- Events: `thread.started {thread_id}`, `turn.started`, `item.started|completed {item: agent_message | command_execution{command, aggregated_output, exit_code} | …}`, `turn.completed {usage tokens}`.
- No token deltas (whole messages) and no plan rate limits → not full parity.
- Resume: `codex exec resume <SESSION_ID> "<prompt>"`.

## `codex app-server` (used; marked experimental)

Newline-delimited JSON-RPC 2.0 over stdio (same protocol as Codex IDE extensions). Schema: `codex app-server generate-json-schema --out DIR`.

Handshake: request `initialize {clientInfo:{name,version}}` → response; then notification `initialized`.

Requests used:
- `account/read` → `{account|null, requiresOpenaiAuth}` (null account = not signed in).
- `account/rateLimits/read` → `{rateLimits:{primary,secondary}}`; window = `{usedPercent (int), windowDurationMins, resetsAt}`. Some plans have only a weekly primary window (10080 min), with secondary null. Label windows by duration.
- `model/list {}` → `{data:[{id, model, displayName, isDefault, hidden,…}]}`.
- `thread/start {cwd, model, sandbox, approvalPolicy, developerInstructions}` → `{thread:{id}}`.
- `thread/resume {threadId, …same overrides}`.
- `turn/start {threadId, input:[{type:"text", text}]}` → `{turn:{id}}`.
- `turn/interrupt {threadId, turnId}` → the turn ends with status `interrupted`.

Notifications used:
- `item/started` / `item/completed` with `item.type`: `agentMessage` (phase `commentary`|`final_answer`), `commandExecution {command, aggregatedOutput, exitCode}`, `fileChange {changes}`, `mcpToolCall`, `webSearch {query}`, `reasoning`, `userMessage`, …
- `item/agentMessage/delta {itemId, delta}` — live typing.
- `account/rateLimits/updated {rateLimits}` — during turns.
- `turn/completed {turn:{status: completed|failed|interrupted, error:{message}}}`.
- Noise: `mcpServer/startupStatus/updated`, `hook/*`, `thread/status/changed`, `thread/tokenUsage/updated`, `remoteControl/status/changed`.

On the Mac the user's own Codex hooks, plugins and MCP servers load (like Claude). In containers `CODEX_HOME=/codex` is a clean volume, so bots are clean.

Commands arrive wrapped: `/bin/zsh -lc 'cat note.txt'` → show the inner part.

## Login in containers

`codex login --device-auth` (device code flow) inside a throwaway container with the `eggbot-codex` volume at `/codex`.

## How eggbot uses it (src/codex.rs)

- One `docker exec -i eggbot-<id> codex app-server` per turn; stdin piped (JSON-RPC), stderr ignored.
- Turn flow: `initialize` → `initialized` → `account/read` (null → "Not signed in to Codex · run codex login") → `account/rateLimits/read` → `thread/resume` (fallback `thread/start`) → `turn/start {effort}` → stream notifications until `turn/completed`.
- Thread options: `cwd /work`, `sandbox danger-full-access`, `approvalPolicy never` (the container is the sandbox), `developerInstructions` = role + roster + style, `model`.
- Stop: `Handle.interrupt` writes `turn/interrupt`; the turn ends as `interrupted`.
- `codex::account()` runs a throwaway container (`sandbox::codex_oneshot`) for usage + `model/list` (with `supportedReasoningEfforts` per model). Called at launch if any bot uses Codex, when a bot switches to Codex, when the editor opens without a model list, and after sign-in.
- The bot keeps the Codex thread id in `Bot.thread`, apart from the Claude `session`, so switching provider loses neither memory.

## Facts learned (2026-10-01)

- `@openai/codex` npm install = 382 MB (native linux-arm64 binary). Add `npm cache clean --force` or the image carries ~300 MB of cache. Image now 1.41 GB.
- A Claude model alias passed to Codex fails; model and effort reset whenever the provider changes.
- Codex's default CODEX_HOME contains system skills (`/codex/skills/.system/…`) that the bot may read; harmless.
