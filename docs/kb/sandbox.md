# Bot sandbox (Docker on Colima)

Code: `src/sandbox.rs`, image: `docker/bot.Dockerfile` (embedded with `include_str!`, tag = hash of the file).

## Layout

- One long-lived container per bot: `eggbot-<id>`, label `eggbot=1`, `CMD sleep infinity`; each turn is `docker exec eggbot-<id> claude -p …`.
- `/work` = the bot's project folder (or its scratch folder `~/Library/Application Support/eggbot/bots/<id>/`).
- `/claude` = named volume `eggbot-claude` (`CLAUDE_CONFIG_DIR`), `/codex` = named volume `eggbot-codex` (`CODEX_HOME`); both shared by all bots, hold logins and session transcripts.
- After creating a container, eggbot removes older `eggbot-bot` images that no container uses.
- `ensure()` recreates the container when the image tag or the /work mount changes; the volume survives.

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

- Token refresh race when several containers share one credentials file.
- Host-created Claude sessions cannot resume in containers (different config dir): the saved field was renamed to `sandbox_session` so old ids are dropped.

## Launched from Finder

Apps opened from Finder/Dock get `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so `docker`/`colima` (Homebrew) are not found. `main()` prepends `/opt/homebrew/bin:/usr/local/bin` before GPUI starts. The docker context in `~/.docker` still works (HOME is set).
