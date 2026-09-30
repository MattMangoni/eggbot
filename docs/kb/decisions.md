# Decisions

Made by Matteo on 2026-09-30 unless noted. Newest last.

| Topic | Decision | Notes |
|---|---|---|
| Platform | macOS first, keep code portable | GPUI has no iOS/Android |
| Model access | Official `claude -p --output-format stream-json` and `codex exec --json` | No API keys, no token reuse |
| Sandbox | One Docker container per bot (Colima) | |
| Always-on | Runs when window closes (menu bar), scheduled tasks, persistent memory | Proactive messages deferred |
| Teamwork | Independent bots + explicit handoff (e.g. Implementer → Reviewer) | No lead/dispatcher bot |
| Workspace | Host project folder mounted into the bot container | |
| Look | "Warm egg": cream bg, charcoal ink, amber accent, animated egg avatars | Light theme only so far |
| Activity view | Chat + compact, expandable tool log | |
| Phase 2 order | CLIs on host first, then move into containers | Only the command prefix changes |
| Claude auth in containers | `claude /login` once inside a container; credentials live in a shared Docker volume; eggbot never touches the token | See `auth.md` |
| Presets | Reviewer, Implementer, Designer + blank Custom | All default to claude; codex chosen per bot |
| UI base | `gpui-kit` 0.7 (gpui-component) | Gives input, markdown, scroll |
| Bot setup | Clean bots: no user hooks, plugins or claude.ai connectors | Same behavior on host and in containers |
| Phase 2 work folder | `~/Library/Application Support/eggbot/bots/<id>/` per bot | Real project folder comes with containers |
| Host permissions | Read + edit only, no shell | Shell opens inside containers (Phase 3) |
| Usage meter | 5h / 7d bars from `rate_limit_event`, amber at 80% | |
| Memory | Each bot resumes its own Claude session; history in `state.json` | |
| KB location | `docs/kb/` in the repo | Readable by bots working on this repo |

## Phases

1. Shell and feel — done
2. Real CLI on host, streaming, persistence — built, awaiting review
3. Containers
4. Handoff
5. Menu bar + schedules
