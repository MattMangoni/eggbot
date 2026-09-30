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
| Look | Serious, ChatGPT/T3 Code style: monochrome neutral grays, native vibrancy sidebar (resizable 200–420 px, width saved), top bar breadcrumb, replies without bubbles | Replaced "warm egg" on 2026-10-01 |
| Activity view | Chat + compact, expandable tool log | |
| Phase 2 order | CLIs on host first, then move into containers | Only the command prefix changes |
| Claude auth in containers | `claude /login` once inside a container; credentials live in a shared Docker volume; eggbot never touches the token | See `auth.md` |
| Presets | Reviewer, Implementer, Designer + blank Custom | All default to claude; codex chosen per bot |
| UI base | `gpui-kit` 0.7 (gpui-component) | Gives input, markdown, scroll |
| Bot setup | Clean bots: no user hooks, plugins or claude.ai connectors | Same behavior on host and in containers |
| Phase 2 work folder | `~/Library/Application Support/eggbot/bots/<id>/` per bot | Real project folder comes with containers |
| Host permissions | Read + edit only, no shell | Shell opens inside containers (Phase 3) |
| Usage meter | 5h / 7d bars from `rate_limit_event`, amber at 80%; last values + reset times saved in `state.json`, shown with "updated HH:MM" | Saved since 2026-09-30 (bars vanished after restart) |
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
| Bot editor | Click the name in the header; edit name (unique, ≤24), role, egg color (8 shells), model (Default/Fable/Opus/Sonnet/Haiku via `--model`) | Color + model apply at once; name + role on Save. "Custom" opens the editor on hatch |
| Codex interface | `codex app-server` (experimental JSON-RPC), Codex pinned in the image | Chosen for live typing, usage, clean stop |
| Codex login | `codex login --device-auth` in Terminal, `eggbot-codex` volume | eggbot never touches the token |
| Usage meters | One group per provider, window labels from duration (5h, 7d, week) | |
| Codex models | Live list from `model/list` | |
| Sign-in notice | After Sign in, check every 5 s for 5 min; the error turns into a green "Signed in to X" with "Send again" | Claude check: `claude auth status` exit code |
| Effort | Per bot, in the editor; Claude low…max, Codex the model's supported levels | |
| Model/effort UI | Two dropdowns (gpui-kit `Select`) instead of pills; Brain stays two pills | Matteo: pills took too much space |
| Context tools | Notes file per bot (/memory/NOTES.md), Fresh start button, context meter; no automatic fresh start | See `context.md` |
| Schedule sessions | Fresh session each run | |
| Handoff sessions | Main session | |
| Role updates | Must reach existing sessions for both providers (Codex: `<eggbot-context>` blocks; Claude: `--system-prompt-snapshot off`) | Matteo: "same behaviour should apply to Claude" |
| Eggs | Small and quiet: 16 px in sidebar, 14 px in the breadcrumb, 32 px in the empty state; wobble only while working; no hatch/poke/blush | |
| Model/effort UI (v2) | Combined provider+model dropdown and effort dropdown in the composer toolbar; editor keeps name, role, egg color | Supersedes the editor dropdowns |
| Shortcuts | ⌘N new bot, ⌘K focus input, ⌘1…9 select bot, ⌃Tab / ⌃⇧Tab next/previous, Esc stop (or close the new-bot menu), ⌘W hide, ⌘Q quit | |
| Composer | Multi-line (Enter sends, Shift+Enter newline, up to 8 lines) | |
| Working state | Spinner + "Thinking…" on the sidebar row | Matteo chose sidebar over top bar |
| KB location | `docs/kb/` in the repo | Readable by bots working on this repo |
| Appearance | View menu: Match System / Light / Dark, ⌘⇧D cycles, saved in state.json | Matteo chose the menu over a sidebar button |

## Phases

1. Shell and feel — done
2. Real CLI on host, streaming, persistence — done
3. Containers — done (folder picker, delete, stop confirmed by Matteo)
4. Handoff — done (first-run "unpaused" handoff was a manual Continue click)
5. Menu bar (5a) + schedules (5b) — done
6. Bot editor — done
7. Codex provider (parity) — done
8. Context management — done
9. Serious UI (ChatGPT / T3 Code style) — built
