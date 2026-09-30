# Driving the `claude` CLI

Observed 2026-09-30 with Claude Code 2.1.286. Raw sample: run
`claude -p "<prompt>" --output-format stream-json --verbose --include-partial-messages`.

## Useful flags

- `-p` + `--output-format stream-json --verbose` — one JSON object per line. `--verbose` is required for stream-json.
- `--include-partial-messages` — adds `stream_event` lines with token deltas (for live typing).
- `--resume <session_id>` / `--session-id <uuid>` — continue or pin a conversation (bot memory).
- `--append-system-prompt <text>` — role prompt on top of Claude Code's own.
- `--permission-mode acceptEdits|auto|bypassPermissions|manual|dontAsk|plan`.
- `--allowedTools` / `--disallowedTools`, `--add-dir`.
- `--setting-sources user,project,local` — which settings load. Without it the bot inherits the user's global hooks, skills, plugins and CLAUDE.md.
- `--safe-mode` — skip all customizations.
- `--permission-prompts host|none` — who answers permission prompts in `-p` mode.

## What eggbot passes (see `src/claude.rs`)

Inside the container: `docker exec eggbot-<id> claude -p … --setting-sources project,local --strict-mcp-config --permission-mode bypassPermissions --disallowedTools RemoteTrigger,CronCreate,CronDelete,ScheduleWakeup,PushNotification`, plus `--append-system-prompt <role>` and `--resume <session>`.

(Phase 2 on the host used `--permission-mode acceptEdits --tools Read,Edit,Write,Glob,Grep,WebFetch,WebSearch`.)

Traps found:
- `--setting-sources` alone does NOT drop claude.ai connectors (Google Drive, Slack…) — they leak in as MCP tools. `--strict-mcp-config` removes them.
- `--disallowedTools Bash` is not enough for "no shell": `Monitor` runs commands, and `RemoteTrigger`/`CronCreate` create cloud agents/jobs. Use the `--tools` allowlist (comma-separated, one argument).
- With those flags only two builtin plugins and the built-in skills remain; no user hooks run.

- Claude Code moves long shell commands (e.g. `sleep 60`) to the background and can END the turn while they run; background tasks die when the `claude -p` process exits. A "busy" bot is only one whose turn is streaming. To test busy-state features, use a long text reply, not a long command.

## Event lines (by `type`)

| type | what matters |
|---|---|
| `system` / `init` | `session_id`, `cwd`, `tools`, `model` |
| `system` / `hook_started`, `hook_response` | user hooks running (noise) |
| `system` / `status`, `thinking_tokens`, `commands_changed` | noise for us |
| `stream_event` | `event.type`: `message_start`, `content_block_start` (has `content_block.type` = `text`/`tool_use`/`thinking`, tool `name`), `content_block_delta` (`delta.type` = `text_delta` with `text`, `input_json_delta`, `thinking_delta`), `content_block_stop`, `message_delta`, `message_stop` |
| `assistant` | full message; `message.content[]` blocks: `text`, `tool_use` (`name`, `input`), `thinking` |
| `user` | `tool_result` blocks (`tool_use_id`, `content`) |
| `rate_limit_event` | `rate_limit_info.status`, `unifiedWindows.five_hour.utilization` / `seven_day.utilization` (0..1), per-window `resetsAt` (unix). Sent only during a turn, so eggbot saves the last values. |
| `result` | end of turn: `session_id`, `stop_reason`, `is_error`, `result` text, `usage` |

`total_cost_usd` in `result` is an API-price estimate; on a subscription nothing is billed.

## Rendering plan

- Text: append `text_delta` to the current bot message.
- Tool row: `assistant` `tool_use` block → verb = tool name, target = `input.file_path` / `input.command` / `input.pattern`; detail = matching `tool_result` content.
- Done: `result` line → bot idle; store `session_id` for `--resume`.

# Codex CLI

- OpenAI help: ChatGPT plans can run Codex from the terminal, including scripted `codex exec` workflows. https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan
- Event format of `codex exec --json` not yet probed.
