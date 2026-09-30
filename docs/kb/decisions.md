# Decisions

Made by Matteo on 2026-09-30 unless noted. Newest last.

| Topic | Decision | Notes |
|---|---|---|
| Platform | macOS first, keep code portable | GPUI has no iOS/Android |
| Model access | Official `claude -p --output-format stream-json` and `codex exec --json` | No API keys, no token reuse |
| Sandbox | One Docker container per bot (Colima) | |
| Always-on | Runs when window closes (menu bar), scheduled tasks, persistent memory | Proactive messages deferred |
| Teamwork | Independent bots + automatic handoff: `@Name` anywhere in a reply sends the whole reply to that bot | Decided 2026-09-30; no lead/dispatcher bot |
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
| Colima VM | 4 CPU / 8 GB (`colima start --cpu 4 --memory 8`) | Was 2 CPU / 2 GB |
| Container powers | bypassPermissions, shell, network; no tools that reach outside (RemoteTrigger, Cron…) | Container is the sandbox |
| Container lifetime | One long-lived container per bot, removed on bot delete | |
| Project folder | Header chip opens folder picker; mounted at /work; bots may share a folder | |
| Image | Slim, no build-essential (852 MB); bots `sudo apt-get install` extras | |
| Delete bot | Hover trash, click twice; removes bot, container, scratch folder, never a project folder | |
| Handoff payload | Reply + context line (sender, shared folder or not) | |
| Hop limit | Chain pauses after 3 automatic handoffs; "Continue chain" resets the count | |
| Busy receiver | Handoff queued, starts when the current turn ends | Queue is not persisted |
| Handoff display | Card "From X" (sender color) in receiver, "→ sent to Y" in sender; both clickable | |
| Background | Menu bar egg (`tray-icon`); closing the window hides it and the Dock icon | Egg wobbles while a bot works |
| Quit | Ask when bots are working, then stop their turns; containers stay | |
| Login item | Not now; needs an app bundle | |
| Schedules | Clock button in bot header; daily, weekdays, every N hours, every N minutes; missed → run once on return; busy → queue | 5b |
| KB location | `docs/kb/` in the repo | Readable by bots working on this repo |

## Phases

1. Shell and feel — done
2. Real CLI on host, streaming, persistence — done
3. Containers — done (folder picker, delete, stop confirmed by Matteo)
4. Handoff — done (first-run "unpaused" handoff was a manual Continue click)
5. Menu bar (5a, done) + schedules (5b, built; scheduler verified, clock panel awaits manual test)
