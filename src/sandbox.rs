//! One long-lived Docker container per bot, on any Docker engine (OrbStack, Docker Desktop, Colima).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const DOCKERFILE: &str = include_str!("../docker/bot.Dockerfile");
/// Shared by all bot containers: the logins and session transcripts live here.
const CLAUDE_VOLUME: &str = "eggbot-claude:/claude";
const CODEX_VOLUME: &str = "eggbot-codex:/codex";

/// Tag changes whenever the Dockerfile changes, so edits rebuild the image and recreate containers.
fn image() -> String {
    let mut h = DefaultHasher::new();
    DOCKERFILE.hash(&mut h);
    format!("eggbot-bot:{:012x}", h.finish() & 0xffff_ffff_ffff)
}

pub fn container(bot: usize) -> String {
    format!("eggbot-{bot}")
}

/// `TZ=<zone>` for `docker exec`: the Mac's time zone, so bots see local dates and times.
pub fn tz() -> String {
    let zone = std::fs::read_link("/etc/localtime").ok().and_then(|p| p.to_str().and_then(|p| p.split("zoneinfo/").nth(1)).map(String::from));
    format!("TZ={}", zone.unwrap_or_else(|| "UTC".into()))
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{e}"))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() { Ok(text(&out.stdout)) } else { Err(text(&out.stderr)) }
}

fn docker(args: &[&str]) -> Result<String, String> {
    run(Command::new("docker").args(args))
}

/// Starts the engine behind the active Docker context, then waits until it answers.
pub fn wake() -> Result<(), String> {
    let context = docker(&["context", "show"]).unwrap_or_default();
    let app = |name: &str| ["/Applications", &format!("{}/Applications", std::env::var("HOME").unwrap_or_default())].iter().any(|d| Path::new(&format!("{d}/{name}.app")).exists());
    let open = |name: &str| run(Command::new("open").args(["-ga", name]));
    let colima = |profile: &str| run(Command::new("colima").args(["start", profile]));
    match context.as_str() {
        "orbstack" => open("OrbStack"),
        "desktop-linux" => open("Docker"),
        c if c.starts_with("colima") => colima(c.strip_prefix("colima-").unwrap_or("default")),
        // `colima stop` resets the context to "default", so look for an installed engine
        _ if run(Command::new("colima").arg("version")).is_ok() => colima("default"),
        _ if app("OrbStack") => open("OrbStack"),
        _ if app("Docker") => open("Docker"),
        _ => Err("no Docker engine found; install OrbStack, Docker Desktop or Colima".into()),
    }
    .map_err(|e| format!("Could not start Docker: {e}. Start it yourself and send again."))?;
    for _ in 0..45 {
        if docker(&["info"]).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    Err("Docker did not start within 90 seconds".into())
}

/// Docker is up and the bot image exists.
pub fn ready(status: &dyn Fn(&str)) -> Result<(), String> {
    if docker(&["info"]).is_err() {
        status("Starting Docker…");
        wake()?;
    }
    let image = image();
    if docker(&["image", "inspect", &image]).is_err() {
        status("Building the bot machine (first time, about a minute)…");
        let mut child = Command::new("docker")
            .args(["build", "-q", "-t", &image, "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child.stdin.take().unwrap().write_all(DOCKERFILE.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("Image build failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
    }
    Ok(())
}

/// The bot's container is running with `mount` at /work and `memory` at /memory; returns its name.
pub fn ensure(bot: usize, mount: &Path, memory: &Path, status: &dyn Fn(&str)) -> Result<String, String> {
    ready(status)?;
    let (name, image) = (container(bot), image());
    let (mount, memory) = (mount.to_string_lossy().to_string(), memory.to_string_lossy().to_string());
    let want = format!("{image}|{mount}|{memory}");
    let fmt = r#"{{.Config.Image}}|{{range .Mounts}}{{if eq .Destination "/work"}}{{.Source}}{{end}}{{end}}|{{range .Mounts}}{{if eq .Destination "/memory"}}{{.Source}}{{end}}{{end}}|{{.State.Running}}"#;
    match docker(&["inspect", "-f", fmt, &name]) {
        Ok(s) if s == format!("{want}|true") => return Ok(name),
        Ok(s) if s == format!("{want}|false") => return docker(&["start", &name]).map(|_| name),
        // folder or image changed: rebuild the container (the login volume survives)
        Ok(_) => drop(docker(&["rm", "-f", &name])),
        Err(_) => {}
    }
    status("Preparing its machine…");
    let (work, notes) = (format!("{mount}:/work"), format!("{memory}:/memory"));
    docker(&["run", "-d", "--name", &name, "--label", "eggbot=1", "-v", CLAUDE_VOLUME, "-v", CODEX_VOLUME, "-v", &work, "-v", &notes, &image])?;
    remove_old_images(&image);
    Ok(name)
}

/// Older bot images pile up after Dockerfile changes; docker refuses to remove the ones still in use.
fn remove_old_images(current: &str) {
    let tags = docker(&["images", "eggbot-bot", "--format", "{{.Repository}}:{{.Tag}}"]).unwrap_or_default();
    for old in tags.lines().filter(|t| *t != current) {
        let _ = docker(&["rmi", old]);
    }
}

/// Runs `cmd` in a new Terminal window, where the user can watch and answer it.
fn terminal(cmd: &str) -> Result<(), String> {
    run(Command::new("osascript").args(["-e", "tell application \"Terminal\" to activate", "-e", &format!("tell application \"Terminal\" to do script \"{cmd}\"")])).map(|_| ())
}

/// Opens Terminal with the provider's own login flow in a throwaway container; eggbot never sees the token.
pub fn sign_in(codex: bool) -> Result<(), String> {
    let login = if codex { "codex login --device-auth" } else { "claude" };
    terminal(&format!("docker run -it --rm -v {CLAUDE_VOLUME} -v {CODEX_VOLUME} {} {login}", image()))
}

/// Setup checks; each is a quick `docker` call.
pub fn installed() -> bool {
    docker(&["--version"]).is_ok()
}

pub fn running() -> bool {
    docker(&["info"]).is_ok()
}

pub fn image_ready() -> bool {
    docker(&["image", "inspect", &image()]).is_ok()
}

/// Installs Colima (free, open source) with the Docker CLI in Terminal, or opens its page without Homebrew.
pub fn install_engine() -> Result<(), String> {
    if run(Command::new("brew").arg("--version")).is_ok() {
        terminal("brew install colima docker && colima start --cpu 4 --memory 8")
    } else {
        run(Command::new("open").arg("https://github.com/abiosoft/colima#installation")).map(|_| ())
    }
}

/// True once the shared Claude volume holds a working login (`claude auth status` exits 0).
pub fn claude_signed_in() -> bool {
    docker(&["run", "--rm", "-v", CLAUDE_VOLUME, &image(), "claude", "auth", "status"]).is_ok()
}

/// Stops whatever agent turn is running inside the bot's container.
pub fn interrupt(bot: usize) {
    let _ = docker(&["exec", &container(bot), "pkill", "-f", "claude -p|codex app-server"]);
}

/// `codex app-server` in a throwaway container, for account questions when no bot container is needed.
pub fn codex_oneshot() -> Command {
    let mut cmd = Command::new("docker");
    cmd.args(["run", "--rm", "-i", "-v", CODEX_VOLUME, &image(), "codex", "app-server"]);
    cmd
}

/// Removes the bot's container; the shared login volume stays.
pub fn remove(bot: usize) {
    let _ = docker(&["rm", "-f", &container(bot)]);
}
