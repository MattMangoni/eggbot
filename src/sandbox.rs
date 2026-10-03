//! One long-lived Docker container per bot, on any Docker engine (OrbStack, Docker Desktop, Colima).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

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

/// True when `error` means the engine was down, so a waiting handoff must stay queued.
pub fn engine_down(error: &str) -> bool {
    error.contains("Could not start Docker")
        || error.contains("Docker did not start")
        || error.contains("Could not run docker")
        || error.contains("no Docker engine")
        || error.contains("Cannot connect to the Docker daemon")
        || error.contains("Is the docker daemon running")
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
        let mut child = Command::new("docker").args(["build", "-q", "-t", &image, "-"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        child.stdin.take().unwrap().write_all(DOCKERFILE.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("Image build failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
    }
    Ok(())
}

/// Per bot. Docker allows more; each bind is a macOS file share, so eggbot stops here.
pub const MAX_MOUNTS: usize = 16;

/// A user-picked folder, mounted at `/work/<name>`. `name` is chosen once and stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    pub path: PathBuf,
    pub name: String,
}

impl Mount {
    pub fn dest(&self) -> String {
        format!("/work/{}", self.name)
    }
}

/// Image, running, and every mount as `(destination, source)`.
struct Inspect {
    image: String,
    running: bool,
    binds: Vec<(String, String)>,
}

// tab between destination and source: host paths may contain spaces
const INSPECT: &str = "{{.Config.Image}}\n{{.State.Running}}\n{{range .Mounts}}{{.Destination}}\t{{.Source}}\n{{end}}";

/// The container is running with `folders` at `/work/<name>` (or `scratch` at `/work`) and `memory` at `/memory`.
pub fn ensure(bot: usize, folders: &[Mount], scratch: &Path, memory: &Path, status: &dyn Fn(&str)) -> Result<String, String> {
    check_folders(folders)?;
    ready(status)?;
    let (name, image) = (container(bot), image());
    let want = work_binds(folders, scratch, memory);
    if let Some((_, src)) = want.iter().find(|(_, src)| !mount_syntax_ok(src)) {
        return Err(format!("Docker cannot mount {src}."));
    }
    match docker(&["inspect", "-f", INSPECT, &name]) {
        Ok(text) => match parse_inspect(&text) {
            Some(got) if got.image == image && binds_match(&got.binds, &want) => {
                if got.running {
                    return Ok(name);
                }
                return docker(&["start", &name]).map(|_| name);
            }
            // folder or image changed: rebuild the container (the login volume survives)
            _ => drop(docker(&["rm", "-f", &name])),
        },
        Err(_) => {}
    }
    status("Preparing its machine…");
    let args = run_args(&name, &image, &want);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    docker(&refs)?;
    remove_old_images(&image);
    Ok(name)
}

/// Refuses a path Docker's `-v host:dest` form cannot carry, then canonicalizes it.
pub fn check_path(path: &Path) -> Result<PathBuf, String> {
    if !mount_syntax_ok(&path.to_string_lossy()) {
        return Err("Docker cannot mount that path.".into());
    }
    let canon = std::fs::canonicalize(path).map_err(|_| "That folder is not available.".to_string())?;
    if !canon.is_dir() {
        return Err("That folder is not available.".into());
    }
    if !mount_syntax_ok(&canon.to_string_lossy()) {
        return Err("Docker cannot mount that path.".into());
    }
    Ok(canon)
}

/// Appends picked folders. One message covers every path that was skipped.
pub fn add_mounts(folders: &mut Vec<Mount>, paths: &[PathBuf]) -> Result<(), String> {
    let mut errors = vec![];
    for path in paths {
        if folders.len() >= MAX_MOUNTS {
            errors.push(format!("A bot can mount at most {MAX_MOUNTS} folders."));
            break;
        }
        let path = match check_path(path) {
            Ok(path) => path,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if folders.iter().any(|f| f.path == path) {
            let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            errors.push(format!("{label} is already mounted."));
            continue;
        }
        let taken: Vec<String> = folders.iter().map(|f| f.name.clone()).collect();
        folders.push(Mount { name: mount_name(&path, &taken), path });
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join(" ")) }
}

/// Folds a pre-multi-mount `folder` into `folders`. A missing path is kept as stored.
pub fn adopt_legacy(folders: &mut Vec<Mount>, legacy: Option<PathBuf>) {
    let Some(path) = legacy else { return };
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    if folders.iter().any(|f| f.path == path) {
        return;
    }
    let taken: Vec<String> = folders.iter().map(|f| f.name.clone()).collect();
    folders.insert(0, Mount { name: mount_name(&path, &taken), path });
}

/// Directory name under `/work`: the last path component, made safe, then `-2`, `-3` on a clash.
pub fn mount_name(path: &Path, taken: &[String]) -> String {
    let raw = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let cleaned: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '-' }).collect();
    let mut base: String = cleaned.trim_matches(|c| c == '-' || c == '.').chars().take(48).collect();
    if base.is_empty() {
        base = "folder".into();
    }
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t == &name) && n < 10_000 {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// Where the turn starts: the only project folder, or `/work` when there are several or none.
pub fn cwd(folders: &[Mount]) -> String {
    match folders {
        [one] => one.dest(),
        _ => "/work".into(),
    }
}

/// Appended to the role so the bot hears the container paths. Empty when nothing is mounted.
pub fn folders_note(folders: &[Mount]) -> String {
    if folders.is_empty() {
        return String::new();
    }
    let list = folders.iter().map(|f| f.dest()).collect::<Vec<_>>().join(", ");
    let mut note = format!(" The user's folders are mounted at {list}. Only those directories are their files.");
    let mut nested = vec![];
    for child in folders {
        let parent = folders.iter().filter(|p| child.path != p.path && child.path.starts_with(&p.path)).max_by_key(|p| p.path.components().count());
        if let Some(parent) = parent {
            nested.push(format!("{} is inside {}", child.dest(), parent.dest()));
        }
    }
    if !nested.is_empty() {
        note.push_str(" Note: ");
        note.push_str(&nested.join("; "));
        note.push_str("; those files show up in both places.");
    }
    note
}

fn mount_syntax_ok(path: &str) -> bool {
    !path.chars().any(|c| matches!(c, ':' | ',' | '\n' | '\t' | '\0'))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with(['-', '.']) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn check_folders(folders: &[Mount]) -> Result<(), String> {
    if folders.len() > MAX_MOUNTS {
        return Err(format!("A bot can mount at most {MAX_MOUNTS} folders."));
    }
    let mut seen = vec![];
    for f in folders {
        if !valid_name(&f.name) {
            return Err(format!("Cannot mount a folder as /work/{}.", f.name));
        }
        if seen.contains(&f.name.as_str()) {
            return Err(format!("Two folders would both mount at /work/{}.", f.name));
        }
        seen.push(f.name.as_str());
    }
    Ok(())
}

/// `(destination, source)` including `/memory`. No folders: scratch at `/work`.
fn work_binds(folders: &[Mount], scratch: &Path, memory: &Path) -> Vec<(String, String)> {
    let mut binds = Vec::new();
    if folders.is_empty() {
        binds.push(("/work".into(), scratch.to_string_lossy().into_owned()));
    } else {
        for f in folders {
            binds.push((f.dest(), f.path.to_string_lossy().into_owned()));
        }
    }
    binds.push(("/memory".into(), memory.to_string_lossy().into_owned()));
    binds
}

fn run_args(name: &str, image: &str, binds: &[(String, String)]) -> Vec<String> {
    let mut args = vec!["run".into(), "-d".into(), "--name".into(), name.into(), "--label".into(), "eggbot=1".into(), "-v".into(), CLAUDE_VOLUME.into(), "-v".into(), CODEX_VOLUME.into()];
    for (dest, src) in binds {
        args.push("-v".into());
        args.push(format!("{src}:{dest}"));
    }
    args.push(image.into());
    args
}

fn parse_inspect(text: &str) -> Option<Inspect> {
    let mut lines = text.lines();
    let image = lines.next()?.trim().to_string();
    if image.is_empty() {
        return None;
    }
    let running = match lines.next()?.trim() {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    let mut binds = vec![];
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (dest, src) = line.split_once('\t')?;
        if dest.is_empty() || src.is_empty() {
            return None;
        }
        binds.push((dest.to_string(), src.to_string()));
    }
    Some(Inspect { image, running, binds })
}

fn norm_src(src: &str) -> String {
    let t = src.trim_end_matches('/');
    if t.is_empty() { "/".into() } else { t.into() }
}

fn managed(dest: &str) -> bool {
    dest == "/memory" || dest == "/work" || dest.starts_with("/work/")
}

fn binds_match(have: &[(String, String)], want: &[(String, String)]) -> bool {
    let mut have: Vec<(String, String)> = have.iter().filter(|(d, _)| managed(d)).map(|(d, s)| (d.clone(), norm_src(s))).collect();
    let mut want: Vec<(String, String)> = want.iter().map(|(d, s)| (d.clone(), norm_src(s))).collect();
    have.sort();
    want.sort();
    have == want
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_down_is_the_startup_failure() {
        assert!(engine_down("Could not start Docker: open failed. Start it yourself and send again."));
        assert!(engine_down("Docker did not start within 90 seconds"));
        assert!(engine_down("Could not run docker: No such file or directory (os error 2)"));
        assert!(engine_down("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"));
        assert!(!engine_down("Not logged in · Please run /login"));
        assert!(!engine_down("claude stopped (exit status: 1). "));
    }

    fn mount(path: &str, name: &str) -> Mount {
        Mount { path: PathBuf::from(path), name: name.into() }
    }

    #[test]
    fn names_are_safe_and_stable_across_clashes() {
        let samples = ["docs", "My Project", ".git", "***", "foo:bar", "..", "docs"];
        let mut taken = vec![];
        for raw in samples {
            let path = if raw.is_empty() { PathBuf::from("/") } else { PathBuf::from("/tmp").join(raw) };
            let name = mount_name(&path, &taken);
            assert!(valid_name(&name), "{raw} -> {name}");
            taken.push(name);
        }
        assert_eq!(taken[0], "docs");
        assert_eq!(taken[1], "My-Project");
        assert_eq!(taken[2], "git");
        assert_eq!(taken[3], "folder");
        assert_eq!(taken[4], "foo-bar");
        assert_eq!(taken[5], "folder-2");
        assert_eq!(taken[6], "docs-2");
        assert_eq!(mount_name(Path::new("/"), &[]), "folder");
    }

    #[test]
    fn add_mounts_dedups_canonical_paths_and_stops_at_the_cap() {
        let root = std::env::temp_dir().join(format!("eggbot-mounts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("My Project")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let docs = root.join("docs");
        let project = root.join("My Project");
        let mut folders = vec![];
        let err = add_mounts(&mut folders, &[docs.clone(), docs.clone(), project.clone()]).unwrap_err();
        assert!(err.contains("already mounted"));
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].name, "docs");
        assert_eq!(folders[1].name, "My-Project");
        assert_eq!(folders[0].path, std::fs::canonicalize(&docs).unwrap());
        assert!(add_mounts(&mut folders, &[PathBuf::from("/tmp/foo:bar")]).unwrap_err().contains("cannot mount"));
        assert_eq!(folders.len(), 2);
        while folders.len() < MAX_MOUNTS {
            let path = root.join(format!("f{}", folders.len()));
            std::fs::create_dir_all(&path).unwrap();
            add_mounts(&mut folders, &[path]).unwrap();
        }
        let extra = root.join("overflow");
        std::fs::create_dir_all(&extra).unwrap();
        assert!(add_mounts(&mut folders, &[extra]).unwrap_err().contains(&MAX_MOUNTS.to_string()));
        assert_eq!(folders.len(), MAX_MOUNTS);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_folder_is_kept_once() {
        let dir = std::env::temp_dir().join(format!("eggbot-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut folders = vec![];
        adopt_legacy(&mut folders, Some(dir.clone()));
        adopt_legacy(&mut folders, Some(dir.clone()));
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, std::fs::canonicalize(&dir).unwrap());
        let gone = PathBuf::from("/no/such/eggbot-path");
        adopt_legacy(&mut folders, Some(gone.clone()));
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].path, gone);
        assert_eq!(folders[0].name, "eggbot-path");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn binds_skip_scratch_when_folders_exist_and_ignore_login_volumes() {
        let scratch = Path::new("/scratch");
        let memory = Path::new("/mem");
        let empty = work_binds(&[], scratch, memory);
        assert_eq!(empty, vec![("/work".into(), "/scratch".into()), ("/memory".into(), "/mem".into())]);
        let folders = vec![mount("/Users/me/My Project", "My-Project"), mount("/Users/me/docs", "docs")];
        let binds = work_binds(&folders, scratch, memory);
        let args = run_args("eggbot-1", "eggbot-bot:abc", &binds);
        let has = |spec: &str| args.windows(2).any(|w| w[0] == "-v" && w[1] == spec);
        assert!(has("/Users/me/My Project:/work/My-Project"));
        assert!(has("/Users/me/docs:/work/docs"));
        assert!(has("/mem:/memory"));
        assert!(!args.iter().any(|a| a.ends_with(":/work")));
        let have = vec![
            ("/claude".into(), "/var/lib/docker/volumes/eggbot-claude/_data".into()),
            ("/work/docs".into(), "/Users/me/docs/".into()),
            ("/memory".into(), "/mem".into()),
            ("/codex".into(), "/var/lib/docker/volumes/eggbot-codex/_data".into()),
            ("/work/My-Project".into(), "/Users/me/My Project".into()),
        ];
        assert!(binds_match(&have, &binds));
        let mut wrong = have.clone();
        wrong[1].1 = "/Users/me/other".into();
        assert!(!binds_match(&wrong, &binds));
    }

    #[test]
    fn inspect_text_parses() {
        let text = "eggbot-bot:abc\ntrue\n/claude\t/vol/claude\n/work/docs\t/Users/me/docs\n/memory\t/mem\n";
        let got = parse_inspect(text).unwrap();
        assert_eq!(got.image, "eggbot-bot:abc");
        assert!(got.running);
        assert_eq!(got.binds.len(), 3);
        assert!(parse_inspect("eggbot-bot:abc\nmaybe\n").is_none());
        assert!(parse_inspect("nope").is_none());
    }

    #[test]
    fn cwd_and_note_follow_the_mount_list() {
        assert_eq!(cwd(&[]), "/work");
        let one = mount("/repos/proj", "proj");
        assert_eq!(cwd(&[one.clone()]), "/work/proj");
        let child = mount("/repos/proj/crates", "crates");
        assert_eq!(cwd(&[one.clone(), child.clone()]), "/work");
        assert!(folders_note(&[]).is_empty());
        let note = folders_note(&[one, child]);
        assert!(note.contains("/work/proj"));
        assert!(note.contains("/work/crates is inside /work/proj"));
        assert!(check_folders(&[mount("/a", "../x")]).is_err());
        assert!(check_folders(&[mount("/a", "docs"), mount("/b", "docs")]).is_err());
    }
}
