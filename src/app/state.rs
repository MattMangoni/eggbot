//! `state.json`: what is saved, where it lives, and how an older file still loads.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Eggbot;
use super::bot::Bot;
use crate::claude::Meter;
use crate::{Appearance, group, room, usage};

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

pub(crate) fn default_throttle() -> f32 {
    usage::DEFAULT_THROTTLE
}

pub(crate) fn default_pause() -> f32 {
    usage::DEFAULT_PAUSE
}

impl Eggbot {
    pub(crate) fn save(&self) {
        let dir = data_dir();
        let state = serde_json::json!({ "next_id": self.next_id, "bots": self.bots, "meters": self.meters, "sidebar_w": self.sidebar_w, "sidebar_open": self.sidebar_open, "appearance": self.appearance, "shared": self.shared, "throttle": self.throttle, "pause": self.pause, "next_room_id": self.next_room_id, "rooms": self.rooms, "next_group_id": self.next_group_id, "groups": self.groups });
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
        let state = serde_json::json!({ "next_id": saved.next_id, "bots": saved.bots, "meters": saved.meters, "sidebar_w": saved.sidebar_w, "sidebar_open": saved.sidebar_open, "appearance": saved.appearance, "shared": saved.shared, "throttle": saved.throttle, "pause": saved.pause, "next_room_id": saved.next_room_id, "rooms": saved.rooms, "next_group_id": saved.next_group_id, "groups": saved.groups });
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
}
