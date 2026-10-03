//! `state.json`: what is saved, where it lives, and how an older file still loads.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Eggbot;
use super::bot::Bot;
use crate::claude::Meter;
use crate::ui::theme::Appearance;
use crate::{group, handoff, room, sandbox, usage};

/// "Instructions for all bots" until the user edits them in Settings.
pub(crate) const SHARED: &str = "Reply in concise GitHub-flavored markdown.";

pub(crate) fn data_dir() -> PathBuf {
    // ponytail: macOS path only; use the `dirs` crate when Linux/Windows builds start
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/eggbot")
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Saved {
    pub(crate) next_id: usize,
    pub(crate) bots: Vec<Bot>,
    /// Last plan usage per provider; saved because it only arrives with a turn or an account query.
    #[serde(default)]
    pub(crate) meters: Vec<Meter>,
    #[serde(default = "default_sidebar")]
    pub(crate) sidebar_w: f32,
    // missing in older state.json stays open; bool's Default is false
    #[serde(default = "default_sidebar_open")]
    pub(crate) sidebar_open: bool,
    #[serde(default)]
    pub(crate) appearance: Appearance,
    /// Instructions for all bots; None = `SHARED`.
    #[serde(default)]
    pub(crate) shared: Option<String>,
    /// Fraction of a plan window where extra bots wait their turn. Missing in older state.json = 90%.
    #[serde(default = "default_throttle")]
    pub(crate) throttle: f32,
    /// Fraction where new turns wait. Missing in older state.json = 95%.
    #[serde(default = "default_pause")]
    pub(crate) pause: f32,
    #[serde(default)]
    pub(crate) next_room_id: usize,
    #[serde(default)]
    pub(crate) rooms: Vec<room::Room>,
    #[serde(default)]
    pub(crate) next_group_id: usize,
    #[serde(default)]
    pub(crate) groups: Vec<group::Group>,
}

pub(crate) fn default_sidebar() -> f32 {
    260.
}

pub(crate) fn default_sidebar_open() -> bool {
    true
}

fn default_throttle() -> f32 {
    usage::DEFAULT_THROTTLE
}

fn default_pause() -> f32 {
    usage::DEFAULT_PAUSE
}

/// What startup found at `state.json`.
pub(crate) enum Loaded {
    /// No file: a first launch.
    Fresh,
    Saved(Saved),
    /// The file did not read or parse; it now lives at this path.
    BackedUp(PathBuf),
    /// The file did not read or parse and could not be moved; this session must not save over it.
    Stuck,
}

pub(crate) fn load() -> Loaded {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    load_from(&data_dir(), now)
}

/// Reads `dir/state.json`; a file that does not parse moves to `state.json.bad`, or the first free `state.json.bad-<now>[-n]`.
fn load_from(dir: &Path, now: u64) -> Loaded {
    let path = dir.join("state.json");
    let read = match std::fs::read(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Fresh,
        read => read,
    };
    if let Some(s) = read.ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        return Loaded::Saved(s);
    }
    let mut bad = dir.join("state.json.bad");
    let mut n = 0;
    // rename silently replaces an existing file, so never pick a taken name
    while bad.exists() {
        bad = dir.join(if n == 0 { format!("state.json.bad-{now}") } else { format!("state.json.bad-{now}-{n}") });
        n += 1;
    }
    match std::fs::rename(&path, &bad) {
        Ok(()) => Loaded::BackedUp(bad),
        Err(e) => {
            eprintln!("eggbot: could not read {} or move it aside ({e}); not saving state this session", path.display());
            Loaded::Stuck
        }
    }
}

impl Saved {
    /// A file without bots, rooms, or groups keeps its settings and hatches the starter bots.
    pub(crate) fn has_content(&self) -> bool {
        !self.bots.is_empty() || !self.rooms.is_empty() || !self.groups.is_empty()
    }

    /// Brings an older file up to date: a single `folder` becomes a mount, the next bot, room, and group ids
    /// clear every saved id, and an in-flight `@Name` hop goes back on its queue (user turns and schedules stay stopped).
    fn migrate(&mut self) {
        for b in &mut self.bots {
            sandbox::adopt_legacy(&mut b.folders, b.folder.take());
            b.queue = handoff::restore(b.current.take(), std::mem::take(&mut b.queue));
        }
        let used = self.bots.iter().map(|b| b.id.saturating_add(1)).max().unwrap_or(0);
        self.next_id = self.next_id.max(used);
        let used = self.rooms.iter().map(|r| r.id.saturating_add(1)).max().unwrap_or(0);
        self.next_room_id = self.next_room_id.max(used);
        let used = self.groups.iter().map(|g| g.id.saturating_add(1)).max().unwrap_or(0);
        self.next_group_id = self.next_group_id.max(used);
    }
}

/// The `state.json` object, read from any value with `Saved`'s field names (`Eggbot` or `Saved`).
macro_rules! saved_json {
    ($s:expr) => {
        serde_json::json!({ "next_id": $s.next_id, "bots": $s.bots, "meters": $s.meters, "sidebar_w": $s.sidebar_w, "sidebar_open": $s.sidebar_open, "appearance": $s.appearance, "shared": $s.shared, "throttle": $s.throttle, "pause": $s.pause, "next_room_id": $s.next_room_id, "rooms": $s.rooms, "next_group_id": $s.next_group_id, "groups": $s.groups })
    };
}

impl Eggbot {
    /// Takes over a loaded file. Appearance is applied by the caller, which also saves.
    pub(crate) fn restore(&mut self, mut s: Saved) {
        s.migrate();
        (self.bots, self.next_id, self.meters, self.sidebar_w, self.sidebar_open, self.shared, self.throttle, self.pause) =
            (s.bots, s.next_id, s.meters, s.sidebar_w, s.sidebar_open, s.shared, s.throttle, s.pause);
        (self.rooms, self.next_room_id, self.groups, self.next_group_id) = (s.rooms, s.next_room_id, s.groups, s.next_group_id);
    }

    pub(crate) fn save(&self) {
        if self.no_save {
            return;
        }
        let dir = data_dir();
        let state = saved_json!(self);
        // write then rename, so a crash mid-write never loses the history
        let tmp = dir.join("state.json.tmp");
        let ok = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&tmp, state.to_string())).and_then(|_| std::fs::rename(&tmp, dir.join("state.json")));
        if let Err(e) = ok {
            eprintln!("eggbot: could not save state: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{handoff, skills};

    #[test]
    fn handoff_queue_loads_from_state_json() {
        let saved = r#"{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"queue":[{"prompt":"ship it","hops":2,"fresh":false,"handoff":true}],"current":{"prompt":"look","hops":1,"fresh":false,"handoff":true}}"#;
        let bot: Bot = serde_json::from_str(saved).unwrap();
        assert_eq!(bot.queue.len(), 1);
        assert!(bot.queue[0].inflight());
        assert!(bot.queue[0].room.is_none());
        assert_eq!(bot.current.as_ref().map(|p| p.hops), Some(1));
        assert!(bot.current.as_ref().unwrap().room.is_none());
        let restored = handoff::restore(bot.current, bot.queue);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].prompt, "look");

        let old = r#"{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[]}"#;
        let bot: Bot = serde_json::from_str(old).unwrap();
        assert!(bot.queue.is_empty());
        assert!(bot.current.is_none());
        assert!(bot.recent.is_empty());
        // a bot saved before skills stays empty; preset defaults are applied only at hatch
        assert!(bot.skills.is_empty());
        assert!(!skills::defaults("Reviewer").is_empty());
    }

    #[test]
    fn skills_round_trip_beside_queue_and_folders() {
        let raw = r#"{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"folders":[{"path":"/tmp/proj","name":"proj"}],"queue":[{"prompt":"ship it","hops":1,"fresh":false,"handoff":true}],"skills":[{"name":"Check","body":"Run tests."}]}"#;
        let bot: Bot = serde_json::from_str(raw).unwrap();
        assert_eq!(bot.skills.len(), 1);
        assert_eq!(bot.skills[0].body, "Run tests.");
        assert_eq!(bot.queue[0].prompt, "ship it");
        assert!(bot.queue[0].room.is_none());
        assert_eq!(bot.folders[0].name, "proj");
        let again: Bot = serde_json::from_str(&serde_json::to_string(&bot).unwrap()).unwrap();
        assert_eq!(again.skills, bot.skills);
        assert_eq!(again.queue[0].prompt, "ship it");
        assert!(again.queue[0].room.is_none());
        assert_eq!(again.folders[0].name, "proj");
        assert!(again.current.is_none());
        assert!(again.recent.is_empty());
    }

    #[test]
    fn state_json_keeps_queue_current_and_limits() {
        let raw = r#"{"next_id":2,"bots":[{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"folders":[{"path":"/tmp/proj","name":"proj"}],"queue":[{"prompt":"ship it","hops":2,"fresh":false,"handoff":true}],"current":{"prompt":"look","hops":1,"fresh":false,"handoff":true}}],"meters":[],"throttle":0.9,"pause":0.95}"#;
        let saved: Saved = serde_json::from_str(raw).unwrap();
        assert_eq!(usage::percent(saved.throttle), 90);
        assert_eq!(usage::percent(saved.pause), 95);
        assert!(saved.bots[0].skills.is_empty());
        assert!(saved.bots[0].queue[0].inflight());
        assert_eq!(saved.bots[0].current.as_ref().map(|p| p.prompt.as_str()), Some("look"));
        assert_eq!(saved.bots[0].folders[0].name, "proj");
        // same keys save() writes; queue, current, and folders ride inside bots
        let state = saved_json!(saved);
        let again: Saved = serde_json::from_value(state).unwrap();
        assert_eq!(again.bots[0].queue[0].prompt, "ship it");
        assert_eq!(again.bots[0].current.as_ref().unwrap().hops, 1);
        assert_eq!(again.bots[0].folders[0].name, "proj");
        assert!(again.bots[0].skills.is_empty());
        assert!(again.bots[0].folder.is_none());
        assert_eq!(usage::percent(again.pause), 95);
        assert!(saved.rooms.is_empty());
        assert!(saved.groups.is_empty());
        assert_eq!(again.next_room_id, 0);
        assert_eq!(again.next_group_id, 0);

        let legacy = r#"{"next_id":1,"bots":[]}"#;
        let old: Saved = serde_json::from_str(legacy).unwrap();
        assert_eq!(old.throttle, usage::DEFAULT_THROTTLE);
        assert_eq!(old.pause, usage::DEFAULT_PAUSE);
        assert!(old.rooms.is_empty());
        assert!(old.groups.is_empty());
        assert_eq!(old.next_group_id, 0);
    }

    #[test]
    fn sidebar_stays_open_when_state_has_no_flag() {
        let saved: Saved = serde_json::from_str(r#"{"next_id":0,"bots":[]}"#).unwrap();
        assert!(saved.sidebar_open);
        assert_eq!(saved.sidebar_w, 260.);
    }

    #[test]
    fn sidebar_closed_round_trips() {
        let saved: Saved = serde_json::from_str(r#"{"next_id":1,"bots":[],"sidebar_open":false,"sidebar_w":300.0}"#).unwrap();
        assert!(!saved.sidebar_open);
        assert_eq!(saved.sidebar_w, 300.);
    }

    #[test]
    fn legacy_folder_field_becomes_a_mount() {
        let dir = std::env::temp_dir().join(format!("eggbot-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = format!(r#"{{"id":1,"name":"Reviewer","preset":0,"folder":{},"msgs":[]}}"#, serde_json::to_string(&dir).unwrap());
        let mut bot: Bot = serde_json::from_str(&json).unwrap();
        assert!(bot.folders.is_empty());
        crate::sandbox::adopt_legacy(&mut bot.folders, bot.folder.take());
        let saved = serde_json::to_value(&bot).unwrap();
        assert!(saved.get("folder").is_none());
        let path = bot.folders[0].path.to_string_lossy();
        assert_eq!(saved["folders"][0]["path"].as_str(), Some(path.as_ref()));
        assert_eq!(saved["folders"][0]["name"], bot.folders[0].name);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rooms_round_trip_and_old_state_has_none() {
        let old: Saved = serde_json::from_str(r#"{"next_id":0,"bots":[]}"#).unwrap();
        assert!(old.rooms.is_empty());
        assert!(old.groups.is_empty());
        assert_eq!(old.next_room_id, 0);
        assert_eq!(old.next_group_id, 0);
        let saved: Saved = serde_json::from_str(
            r#"{"next_id":2,"bots":[],"next_room_id":4,"rooms":[{"id":3,"title":"Standup","kickoff":"What shipped?","members":[1,2],"facilitator":1,"unread":true,"started":true}]}"#,
        )
        .unwrap();
        assert_eq!(saved.next_room_id, 4);
        assert_eq!(saved.rooms[0].title, "Standup");
        assert_eq!(saved.rooms[0].members, vec![1, 2]);
        assert_eq!(saved.rooms[0].facilitator, Some(1));
        assert!(saved.rooms[0].unread && saved.rooms[0].started);
        assert!(saved.rooms[0].transcript.is_empty());
        assert_eq!(saved.throttle, crate::usage::DEFAULT_THROTTLE);
        assert_eq!(saved.pause, crate::usage::DEFAULT_PAUSE);
        let with_log: Saved = serde_json::from_str(r#"{"next_id":1,"bots":[],"rooms":[{"id":1,"title":"S","kickoff":"Hi","members":[1],"transcript":[{"Kickoff":{"to":1,"name":"Reviewer","text":"Hi"}},{"Reply":{"bot":1,"name":"Reviewer","color":1,"text":"Done"}},{"Handoff":{"from":1,"from_name":"Reviewer","color":1,"to":2,"to_name":"Implementer","paused":false}}]}]}"#).unwrap();
        assert_eq!(with_log.rooms[0].transcript.len(), 3);
        let again: Saved = serde_json::from_str(&serde_json::to_string(&with_log).unwrap()).unwrap();
        assert_eq!(again.rooms[0].transcript, with_log.rooms[0].transcript);
    }

    #[test]
    fn groups_round_trip_and_old_state_has_none() {
        let old: Saved = serde_json::from_str(r#"{"next_id":0,"bots":[]}"#).unwrap();
        assert!(old.groups.is_empty());
        assert_eq!(old.next_group_id, 0);
        let saved: Saved = serde_json::from_str(r#"{"next_id":2,"bots":[],"next_group_id":4,"groups":[{"id":3,"title":"Reviewers","members":[1,2]}]}"#).unwrap();
        assert_eq!(saved.next_group_id, 4);
        assert_eq!(saved.groups[0].title, "Reviewers");
        assert_eq!(saved.groups[0].members, vec![1, 2]);
        assert!(saved.rooms.is_empty());
        let again: Saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(again.groups, saved.groups);
        assert_eq!(again.next_group_id, 4);
    }

    #[test]
    fn migrate_folds_old_fields_and_requeues_an_interrupted_hop() {
        let raw = r#"{"next_id":3,"next_group_id":1,"bots":[{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"folder":"/no/such/eggbot-proj","queue":[{"prompt":"later","hops":1,"handoff":true}],"current":{"prompt":"look","hops":2,"handoff":true}},{"id":2,"name":"Implementer","preset":1,"sandbox_session":null,"msgs":[],"current":{"prompt":"hi","hops":0}}],"rooms":[{"id":4,"title":"S","kickoff":"","members":[]}],"groups":[{"id":7,"title":"G","members":[]}]}"#;
        let mut saved: Saved = serde_json::from_str(raw).unwrap();
        assert!(saved.has_content());
        saved.migrate();
        let first = &saved.bots[0];
        assert!(first.folder.is_none());
        assert_eq!(first.folders[0].name, "eggbot-proj");
        assert_eq!(first.queue.iter().map(|p| p.prompt.as_str()).collect::<Vec<_>>(), ["look", "later"]);
        assert!(first.current.is_none());
        // a user turn that was running stays stopped
        assert!(saved.bots[1].queue.is_empty() && saved.bots[1].current.is_none());
        assert_eq!((saved.next_id, saved.next_room_id, saved.next_group_id), (3, 5, 8));
        assert!(!Saved::default().has_content());

        // a next_id at or below a saved bot id moves past it, so a new bot never reuses an id
        let mut low: Saved = serde_json::from_str(r#"{"next_id":1,"bots":[{"id":5,"name":"A","preset":0,"msgs":[]},{"id":2,"name":"B","preset":0,"msgs":[]}]}"#).unwrap();
        low.migrate();
        assert_eq!(low.next_id, 6);
    }

    #[test]
    fn save_writes_every_saved_key() {
        let keys = |v: serde_json::Value| v.as_object().unwrap().keys().cloned().collect::<std::collections::BTreeSet<_>>();
        let saved = Saved::default();
        assert_eq!(keys(saved_json!(saved)), keys(serde_json::to_value(&saved).unwrap()));
    }

    #[test]
    fn an_empty_state_json_keeps_its_settings() {
        let raw = r#"{"next_id":9,"bots":[],"meters":[{"provider":"Claude","windows":[{"label":"5h","used":0.5,"reset":100}],"at":50}],"sidebar_w":310.0,"sidebar_open":false,"appearance":"Dark","shared":"Be brief.","throttle":0.75,"pause":0.875,"next_room_id":4,"rooms":[],"next_group_id":6,"groups":[]}"#;
        let mut saved: Saved = serde_json::from_str(raw).unwrap();
        assert!(!saved.has_content());
        saved.migrate();
        // restore() takes every field, then startup hatches the starter bots from next_id
        assert_eq!(saved_json!(saved), serde_json::from_str::<serde_json::Value>(raw).unwrap());
    }

    #[test]
    fn an_unreadable_state_json_moves_aside_and_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("eggbot-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let state = dir.join("state.json");
        assert!(matches!(load_from(&dir, 7), Loaded::Fresh));

        std::fs::write(&state, r#"{"next_id":1,"bots":[]}"#).unwrap();
        assert!(matches!(load_from(&dir, 7), Loaded::Saved(s) if s.next_id == 1));

        std::fs::write(&state, "{broken").unwrap();
        assert!(matches!(load_from(&dir, 7), Loaded::BackedUp(p) if p == dir.join("state.json.bad")));
        assert!(!state.exists());
        assert_eq!(std::fs::read_to_string(dir.join("state.json.bad")).unwrap(), "{broken");

        // an older backup stays; the new one gets a timestamp
        std::fs::write(&state, "{again").unwrap();
        assert!(matches!(load_from(&dir, 7), Loaded::BackedUp(p) if p == dir.join("state.json.bad-7")));
        assert_eq!(std::fs::read_to_string(dir.join("state.json.bad")).unwrap(), "{broken");
        assert_eq!(std::fs::read_to_string(dir.join("state.json.bad-7")).unwrap(), "{again");

        // a taken timestamp gets a counter; no backup is replaced
        std::fs::write(&state, "{third").unwrap();
        assert!(matches!(load_from(&dir, 7), Loaded::BackedUp(p) if p == dir.join("state.json.bad-7-1")));
        std::fs::write(&state, "{fourth").unwrap();
        assert!(matches!(load_from(&dir, 7), Loaded::BackedUp(p) if p == dir.join("state.json.bad-7-2")));
        assert_eq!(std::fs::read_to_string(dir.join("state.json.bad-7")).unwrap(), "{again");
        assert_eq!(std::fs::read_to_string(dir.join("state.json.bad-7-1")).unwrap(), "{third");
        assert_eq!(std::fs::read_to_string(dir.join("state.json.bad-7-2")).unwrap(), "{fourth");

        // a read-only directory makes the rename fail; the file stays where it was
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&state, "{stuck").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let stuck = matches!(load_from(&dir, 8), Loaded::Stuck);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(stuck);
        assert_eq!(std::fs::read_to_string(&state).unwrap(), "{stuck");
        let _ = std::fs::remove_dir_all(dir);
    }
}
