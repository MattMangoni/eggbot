//! One long-lived Docker container per bot, on Colima.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const DOCKERFILE: &str = include_str!("../docker/bot.Dockerfile");
/// Shared by all bot containers: the Claude login and session transcripts live here.
const VOLUME: &str = "eggbot-claude";

/// Tag changes whenever the Dockerfile changes, so edits rebuild the image and recreate containers.
fn image() -> String {
    let mut h = DefaultHasher::new();
    DOCKERFILE.hash(&mut h);
    format!("eggbot-bot:{:012x}", h.finish() & 0xffff_ffff_ffff)
}

pub fn container(bot: usize) -> String {
    format!("eggbot-{bot}")
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{e}"))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() { Ok(text(&out.stdout)) } else { Err(text(&out.stderr)) }
}

fn docker(args: &[&str]) -> Result<String, String> {
    run(Command::new("docker").args(args))
}

/// Docker is up and the bot image exists.
pub fn ready(status: &dyn Fn(&str)) -> Result<(), String> {
    if docker(&["info"]).is_err() {
        status("Waking the sandbox…");
        run(Command::new("colima").arg("start")).map_err(|e| format!("Could not start Colima: {e}"))?;
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

/// The bot's container is running with `mount` at /work; returns its name.
pub fn ensure(bot: usize, mount: &Path, status: &dyn Fn(&str)) -> Result<String, String> {
    ready(status)?;
    let (name, image, mount) = (container(bot), image(), mount.to_string_lossy().to_string());
    let want = format!("{image}|{mount}");
    let fmt = r#"{{.Config.Image}}|{{range .Mounts}}{{if eq .Destination "/work"}}{{.Source}}{{end}}{{end}}|{{.State.Running}}"#;
    match docker(&["inspect", "-f", fmt, &name]) {
        Ok(s) if s == format!("{want}|true") => return Ok(name),
        Ok(s) if s == format!("{want}|false") => return docker(&["start", &name]).map(|_| name),
        // folder or image changed: rebuild the container (the login volume survives)
        Ok(_) => drop(docker(&["rm", "-f", &name])),
        Err(_) => {}
    }
    status("Preparing its machine…");
    let (vol, work) = (format!("{VOLUME}:/claude"), format!("{mount}:/work"));
    docker(&["run", "-d", "--name", &name, "--label", "eggbot=1", "-v", &vol, "-v", &work, &image]).map(|_| name)
}

/// Opens Terminal with an interactive `claude` in a throwaway container, for Anthropic's own login flow.
pub fn sign_in() -> Result<(), String> {
    let cmd = format!("docker run -it --rm -v {VOLUME}:/claude {} claude", image());
    run(Command::new("osascript").args(["-e", "tell application \"Terminal\" to activate", "-e", &format!("tell application \"Terminal\" to do script \"{cmd}\"")])).map(|_| ())
}

/// Stops whatever `claude` turn is running inside the bot's container.
pub fn interrupt(bot: usize) {
    let _ = docker(&["exec", &container(bot), "pkill", "-f", "claude -p"]);
}

/// Removes the bot's container; the shared login volume stays.
pub fn remove(bot: usize) {
    let _ = docker(&["rm", "-f", &container(bot)]);
}
