# Decisions

Made by Matteo on 2026-09-30 unless noted. Newest last.

| Topic | Decision | Notes |
|---|---|---|
| Platform | macOS first, keep code portable | GPUI has no iOS/Android |
| Model access | Official `claude -p --output-format stream-json` and `codex exec --json` | No API keys, no token reuse |
| Sandbox | One Docker container per bot, on any engine (OrbStack, Docker Desktop, Colima) | Matteo: Colima is not required; his Mac mini runs OrbStack |
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
| Busy receiver | Handoff queued, starts when the current turn ends | Persisted in `state.json` since 2026-10-01; an interrupted hop resumes when the bot is idle and Docker is up |
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
| App bundle | `scripts/bundle.sh` → `~/Applications/eggbot.app`, id `com.digitalmaze.eggbot`, ad-hoc signed, icon drawn by `scripts/icon.swift` | Needed for native notifications; no cargo-bundle |
| Proactive messages | Notify on reply / scheduled run / chain end or pause / error; only when eggbot is in the background; unread dot + menu bar badge; scheduled runs may reply QUIET | Matteo chose all four triggers, bundle first, quiet runs, unread dot |
| Settings | ⌘, panel in the window: Start at login switch + "Instructions for all bots" | Matteo chose one Settings panel |
| Start at login | SMAppService.mainApp; a login launch starts hidden (menu bar egg only), detected by the `oapp` Apple event with `lgit` | Matteo chose menu bar only |
| Shared instructions | One text for all bots on both providers, default = the old style line; NOTES rule stays fixed; Codex BASE plumbing not editable | Editing BASE could break role delivery |
| Sign-in clash | Retry the turn once after 5 s on "process is refreshing it" | Matteo chose retry over one login per bot |
| Long chats | GPUI virtual `list` (`ListState`, bottom-aligned, follows the tail): only rows on screen plus 800px overdraw are drawn; keep all on disk | 3,000 messages: 50–95% CPU before, ~2% after, smooth scrolling |
| Bot order | Drag a sidebar row; a blue line shows where it lands (just above the row under the pointer, or the space below the list for the end); no line where nothing would change; order saved; selection follows its bot | Matteo asked for a clear landing indicator |
| First-run setup | Centered checklist in place of the chat on first launch + eggbot → Setup…; suggests Colima (free, open source, cross-platform) via brew in Terminal; needs at least one sign-in | Matteo chose these options and Colima as the default |
| Search | ⌘F (or the top-bar icon) searches the open chat: user messages, replies, handoffs, scheduled prompts (not tool output), any case; starts at the newest match, Enter = older, ⇧Enter = newer, Esc closes; the current match gets a soft highlight | Matteo chose current chat, jump between matches, messages + replies |
| Sidebar visibility | ⌘B and View → Toggle Sidebar (checked while open) hide or show the bot list; open/closed is saved in `state.json` next to the width; a file without the field stays open | DM-68 recommended default: remember the last state across launches |
| Multiple folders | Each picked folder mounts at `/work/<name>` (16 per bot); one top-bar button ("Add folder" / the folder name / "N folders") opens a popover list: name + parent path, remove on hover, "Add folder…" (replaced the header chips and the composer link, 2026-10-02); handoff names partial overlap | DM-69. Supersedes the single `/work` mount. See `sandbox.md`, `handoff.md` |
| Usage guardrails | Throttle at 90% (one bot of that provider at a time), pause at 95% (no new turns; the one already running finishes). Either window counts (5h, 7d, week), the worse one wins. A window past its reset counts as empty, so held work can start without a new usage event. Due schedules keep their anchor and run once the meter allows. A typed message stays in the box while paused, so a reset overnight does not send a draft. Accepted handoffs stay on the persisted queue (DM-65); the guard only delays `pump_queues` until `may_start` allows, and quitting does not drop them. Thresholds are whole percents in Settings. Amber bars stay at 80%. | DM-66. Recommendation: pause at 95%, not at the existing 80% amber, so the warning stays a warning. In-flight turns are not killed. |
| Rooms | A room is a saved title, kickoff, and roster of existing bots. Start delivers the kickoff only to the facilitator (first member, or one you pick) through the handoff queue at hop 0, so a quit resumes it. Pause and throttle hold that kickoff the same way they hold any other hop. Other bots stay peers and join when the facilitator writes `@Name`. No shared transcript in this slice | DM-67, 2026-10-01. Facilitator-first is the recommended MVP; the alternative was sending the kickoff to every member at once. The transcript line is superseded by DM-70 |
| Skills | Each preset copies a small default list onto the bot at hatch (`skills: [{name, body}]` on the bot, matched by preset name). Custom starts empty. After hatch the bot owns the list: add, edit, and remove on every bot, including presets. Saved in `state.json`. Empty on older bots (no backfill). Injected into the role string after the role and before the roster, so a change reaches the next turn the same way a role edit does. An empty list adds no text. Not mounted as files: the bot would have to remember to read them | DM-72, 2026-10-01. Owned list, not a shared library, so one Reviewer's edits do not change another's. Cap 12 skills, name ≤48, body ≤4000 |
| Room transcript | Opening a room shows one transcript: the kickoff, each member's reply, and `@Name` handoffs whose target is still in that room. Events are appended when that hop is delivered and saved on the room in `state.json`. The room id rides on the persisted handoff (`Pending.room`), so a queued or interrupted hop still lands in the same transcript after relaunch. A handoff to a bot outside the room stays in the bot chats. Each bot's own chat is unchanged. The room dot tracks the transcript. Tool logs stay on the bot. Rounds from before this field are not reconstructed. Skills stay on the bot and still go out through `role_text` | DM-70, 2026-10-01. Recommendation: append room events on delivery, rather than a second messenger or scanning every member's chat (a bot can belong to two rooms, and a private turn must stay out) |
| Lasting learning | `/memory/NOTES.md` stays the private store. `skills::role_text` still builds the turn: skills, then roster, then this notes block, then folders. The notes block is a capped copy of the file, so it steers the next turn without a tool call. A reply may end with one `<eggbot-learn>` block of short bullets (fact, preference, lesson, forget); eggbot merges them deduped (16 per section), hides the block, and does not hand it off. The room transcript and the next bot see that same visible reply. The model may also edit the file. No extra model call. A group's own file is a second store (DM-73); unprefixed bullets still go only to the private file | DM-71, 2026-10-01. Does not replace `skills[]` or move the skills section |
| Group notes | Bots in a group share `groups/<id>/NOTES.md` (title and member ids in `state.json`). Not a room: no transcript, no kickoff, no facilitator. Private `/memory/NOTES.md` and `skills[]` stay per bot. The notes argument is the private block plus a capped section for each group the bot is in; other bots never see that section. A member writes group bullets in the same `<eggbot-learn>` block with a `group` or `shared` prefix (`- group preference: …`, and `- group Title preference: …` when they are in several). The block is still stripped before chat, handoff, and the room transcript. `@Name` is unchanged. No lead | DM-73, 2026-10-02. One file per group and a prefix on the existing learn block, rather than a second block or a lead bot. Room-scoped memory stays DM-74 |
| Room memory | Each room has `rooms/<id>/NOTES.md`, same Facts / Preferences / Lessons file as private and group notes. Not the transcript, and not a field on the room in `state.json`. Anyone who opens the room can read it; the room view shows the file and does not edit it. Only a member bot writes, with a `room` prefix on the same `<eggbot-learn>` block (`- room fact: …`, or `- room Title fact: …` when that bot is in several rooms). A bullet for a room they are not in is dropped, not saved privately. Injected only on a turn already in that room (`Pending.room`), after private notes and any group sections. A private turn does not get it. No lead | DM-74, 2026-10-02. A file next to the group file, not a merge into group notes and not a replacement of the transcript |
| Handoff judgment | The roster argument of `role_text` names each other bot's specialty and says to `@Name` only when that specialty fits the next step better than doing it yourself — never to narrate. It also lists up to four recent successful handoffs (`Bot.recent`; a paused chain is not stored) and, when the bot is in a room, that room's peers. The kickoff says the same. Still no lead bot | DM-71, 2026-10-01 |
| Esc and stop | Esc closes the topmost thing: hatch menu, then search, then the open panel; it stops the selected bot's turn only when nothing is open and a bot chat is on screen (with a room or group open, Esc does nothing). ⌘. always stops. Panels (Settings, bot editor, Schedules, Skills) are one `Panel` value, so only one is ever open. ⌘N opens a hidden sidebar to show the hatch menu. Settings (⌘,) shows in every main view, including setup and the no-bots screen, which also says "Or press ⌘N." | Matteo chose close-first Esc plus ⌘., 2026-10-02 (UI audit items 1, 2, 5) |
| Drafts and follow-ups | Each bot keeps its own composer draft while you switch bots. Drafts are in memory only, not saved in `state.json`. A message sent while the bot works, or held by the throttle, waits on that bot's persisted queue (`Pending::typed`, after anything already queued). It shows as a "Queued" bubble at the end of the chat, with an × to remove it, and becomes a normal user bubble when its turn starts. Stop leaves the queue alone, so the next queued message starts. Queued messages survive a restart; drafts do not. Stop is its own round button beside Send while the bot works; Send always sends or queues | Matteo chose memory-only drafts and queue-then-send, with Stop as a separate control, 2026-10-02 (UI audit items 3, 4). Follow-up answers the same day: × on queued bubbles, throttle-held sends queue too, Stop keeps the queue |
| Sidebar rows | Bot, room, and group rows share one height (`ROW_H`, 48px), `px_2`, a 16px icon slot, and `gap_3`, so their text lines up. Room and group lists show two rows before they scroll; the vertical budget at the 721px default window is tight (5 bots, 2 rooms, 2 groups fit). Padding below a capped list sits on the section, not the scrolling list, or `max_h` eats it. Groups use the Lucide `Users` icon and rooms `MessagesSquare`, both added to `ExtraIcons` in `main.rs` | Sidebar polish, 2026-10-03 |
| Collapsible sidebar sections | Clicking the Rooms or Groups title folds that list; a chevron shows the state and the + button does not toggle. The state persists as `rooms_collapsed` and `groups_collapsed` in `state.json` (missing = open) | Matteo, 2026-10-03, so a long list does not eat the 721px window |
| Disabled buttons | A button that cannot act is built with `button_off` (`src/ui/mod.rs`): dimmed, default cursor, no hover. Never `button(..).opacity(..)`, which keeps the hover background | Matteo, 2026-10-03 |
| Layout and CI | `main.rs` only starts the app. The view's state and behaviour live in `src/app/`, drawing in `src/ui/`, one job per file (aim 200–600 lines, one folder level). Branching logic sits in plain functions with tests in the same file. CI runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`, and warns (never fails) on a `src` file over 600 lines. `rustfmt.toml` uses `max_width = 200` and `use_small_heuristics = "Max"` | Matteo, 2026-10-03. The 100-column default rewrapped ~5,500 lines; this setting changed ~1,500 and keeps one-line builder chains |
| Unreadable `state.json` | A `state.json` that exists but does not read or parse moves to `state.json.bad` before anything saves. If that name is taken it uses `state.json.bad-<unix secs>`, then `-1`, `-2`, … so no older backup is replaced. Startup then hatches the starter bots without the setup checklist, and the first bot's chat shows an error row with the backup path. If the move fails, eggbot logs to stderr, shows the row, and does not save that session. A missing file is still a normal first launch. A file with no bots, rooms, or groups keeps every other setting and hatches the starter bots. On load, `next_id`, `next_room_id`, and `next_group_id` move past the highest saved id. `save` and the key test share one `saved_json!` key list, so a new `Saved` field that `save` misses fails the test | Matteo, 2026-10-03, after PR #19 found that a bad file was saved over and every bot was lost (`state::load_from`, tested). Empty-file settings, `next_id` repair, unique backup names, and the key test added the same day |

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
