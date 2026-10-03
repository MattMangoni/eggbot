# Bot sandbox (Docker: OrbStack, Docker Desktop or Colima)

Code: `src/sandbox.rs`, image: `docker/bot.Dockerfile` (embedded with `include_str!`, tag = hash of the file).

## Layout

- One long-lived container per bot: `eggbot-<id>`, label `eggbot=1`, `CMD sleep infinity`; each turn is `docker exec eggbot-<id> claude -p …`.
- `/work/<name>` = one user-picked folder. With none picked, scratch `~/Library/Application Support/eggbot/bots/<id>/work` is mounted at `/work` instead.
- `/claude` = named volume `eggbot-claude` (`CLAUDE_CONFIG_DIR`), `/codex` = named volume `eggbot-codex` (`CODEX_HOME`); both shared by all bots, hold logins and session transcripts.
- `/memory` = that bot's notes folder. It is not inside `/work`.
- After creating a container, eggbot removes older `eggbot-bot` images that no container uses.
- `ensure()` recreates the container when the image tag or the mount set changes; the login volumes survive. A stopped container with the same mounts is started. Mount edits apply on the next turn.

## Multiple folders

Each bot stores `folders: [{path, name}]` in `state.json`. The folder picker is the only way in: eggbot never mounts a path the user did not pick. New paths are canonicalized, so a symlink and its target (and macOS `/var` vs `/private/var`) are one mount. A saved `folder` string from before this is folded in on load.

- No folders: scratch at `/work`, as before.
- One or more: each is a bind at `/work/<name>`, not at `/work`. `/work` itself is only the image directory, so files written there disappear when the container is recreated. The turn starts in the single folder (`docker exec -w`, Codex `cwd`); with several it starts in `/work`. The role text lists the paths.
- A bot that used to have its project at `/work` moves to `/work/<name>` the next time the container is created. Sessions can still mention the old paths; the role text has the new ones.
- `name` comes from the last path component. Characters other than ASCII letters, digits, `.`, `_` and `-` become `-`. A clash on that bot gets `-2`, `-3` (`docs`, `docs-2`). The name is stored, so removing one folder does not rename the others.
- Paths that contain `:`, `,`, a tab or a newline are refused. `-v host:dest` cannot carry `:` or `,`, and the mount check splits inspect output on tabs.
- Cap is 16 folders per bot (`MAX_MOUNTS`). Docker Engine has no small mount limit, but each bind is a macOS file share (OrbStack, Docker Desktop or Colima) and `docker run` must stay under `ARG_MAX`. 16 plus the two login volumes and `/memory` is far under both. The header refuses another add; `ensure` refuses a longer list.
- A folder and a subdirectory of it can both be mounted. In the container they are siblings (`/work/proj` and `/work/crates`), so the child's files show up twice. The role text says so.
- Handoffs describe shared and nested mounts; see `handoff.md`.

## Facts learned (2026-09-30)

- Colima default VM here: 2 CPU, 2 GiB RAM. Claude Code wants 4 GB+; several bots need more (`colima stop && colima start --cpu 4 --memory 8`).
- Image is 852 MB without build-essential (1.16 GB with it). Build ~40 s.
- Native installer as non-root user → `~/.local/bin/claude`. `bypassPermissions` refuses root, hence user `bot` with passwordless sudo.
- A fresh named volume copies ownership from the image dir, so `/claude` is writable by `bot`.
- Colima bind mounts of `$HOME` paths: files written in the container are owned by the Mac user on the host. `/private/tmp` is not mounted by default.
- `docker build -` with the Dockerfile on stdin needs no build context.
- Legacy builder warns (no buildx); harmless.
- Killing `docker exec` does not kill the process inside: `sandbox::interrupt` runs `pkill -f "claude -p"` in the container (image includes `procps`).
- Not logged in → `result` has `is_error: true`, text `Not logged in · Please run /login`. UI shows a "Sign in to Claude" button for errors containing `/login`.
- Sign in: Terminal runs `docker run -it --rm -v eggbot-claude:/claude <image> claude` (Anthropic's own flow; eggbot never reads the token).

- Verified 2026-09-30: after sign-in the volume holds `.credentials.json` and `.claude.json` (both under `CLAUDE_CONFIG_DIR`); a turn ran `Bash` in the container as `bot` on Linux aarch64.

## Open

- Token refresh race when several containers share one credentials file: see `auth.md` (lock + one retry).
- Host-created Claude sessions cannot resume in containers (different config dir): the saved field was renamed to `sandbox_session` so old ids are dropped.

## Launched from Finder

Apps opened from Finder/Dock get `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so `docker`/`colima` are not found. `main()` prepends `/opt/homebrew/bin:/usr/local/bin:~/.orbstack/bin` before GPUI starts. The docker context in `~/.docker` still works (HOME is set).

## Time zone

Containers run in UTC. Each `docker exec` gets `-e TZ=<zone>` from the Mac's `/etc/localtime` link (`sandbox::tz()`), so bots see local dates and a move to another zone applies on the next turn. The image already has `/usr/share/zoneinfo`.

## Starting Docker

eggbot needs only the `docker` CLI (no compose). When `docker info` fails, `sandbox::wake` starts the engine behind `docker context show`: `orbstack` → `open -ga OrbStack`, `desktop-linux` → `open -ga Docker`, `colima[-profile]` → `colima start <profile>`. `colima stop` resets the context to `default`, so for `default` it tries an installed engine: Colima, then OrbStack, then Docker Desktop. Then it waits up to 90 s for `docker info`. Tested with Colima stopped; OrbStack and Docker Desktop paths not tested here.

## First-run setup (`Eggbot::open_setup` in `src/app/setup.rs`, `setup_view` in `src/ui/panels.rs`)

Shown on first launch (no `state.json`) and from eggbot → Setup…; a bot error that mentions Docker links to it. Rows: Docker engine (`docker --version`), Docker running (`docker info`), bot image (`docker image inspect`), Claude (`claude auth status` in a throwaway container), Codex (`codex::account`). A loop re-checks every 4 s while it is open, because installs and sign-ins finish in Terminal; sign-in checks stop once they pass. Actions: Install Colima (`brew install colima docker && colima start --cpu 4 --memory 8` in Terminal, or the Colima page without Homebrew), Start Docker (`sandbox::wake`), Build (`sandbox::ready`), Sign in (the usual Terminal flow). "Start using eggbot" needs Docker, the image and at least one sign-in; "Skip for now" closes it.
