//! A room is a title, a kickoff, and existing bots. Start talks to one facilitator;
//! the others stay peers and join when that bot writes `@Name`.

use serde::{Deserialize, Serialize};

/// Saved in `state.json`. `members` are bot ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: usize,
    pub title: String,
    pub kickoff: String,
    pub members: Vec<usize>,
    /// Bot that receives the kickoff. Empty until the first member is added.
    #[serde(default)]
    pub facilitator: Option<usize>,
    #[serde(default)]
    pub unread: bool,
    /// The kickoff has been delivered at least once.
    #[serde(default)]
    pub started: bool,
}

impl Room {
    pub fn new(id: usize, title: String) -> Self {
        Self { id, title, kickoff: String::new(), members: vec![], facilitator: None, unread: false, started: false }
    }
}

/// A bot the facilitator can hand work to.
pub struct Peer<'a> {
    pub name: &'a str,
    pub blurb: &'a str,
}

/// Why Start cannot run yet.
pub fn block(title: &str, kickoff: &str, members: &[usize], facilitator: Option<usize>) -> Option<&'static str> {
    if title.trim().is_empty() {
        Some("Name the room")
    } else if kickoff.trim().is_empty() {
        Some("Write a kickoff")
    } else if members.is_empty() {
        Some("Add at least one bot")
    } else if facilitator.is_none_or(|id| !members.contains(&id)) {
        Some("Choose a facilitator")
    } else {
        None
    }
}

/// What the facilitator is told. Peers are named so `@Name` reaches them; nobody is appointed lead.
pub fn prompt(title: &str, kickoff: &str, peers: &[Peer<'_>]) -> String {
    let roster = if peers.is_empty() {
        "No other bots are in this room.".to_string()
    } else {
        let list = peers.iter().map(|p| format!("@{} ({})", p.name, p.blurb)).collect::<Vec<_>>().join(", ");
        format!(
            "The other bots in this room are your peers: {list}. Writing @Name anywhere in your reply sends your whole reply to that bot, so only write @Name when you want them to act. You are not their lead; you only start this round."
        )
    };
    format!("Room \"{}\". You are the facilitator for this kickoff.\n{roster}\n\n{}", title.trim(), kickoff.trim())
}

/// Adds `id`, or removes it. The first member becomes the facilitator; removing the facilitator promotes the next.
pub fn toggle(mut members: Vec<usize>, facilitator: Option<usize>, id: usize) -> (Vec<usize>, Option<usize>) {
    if let Some(i) = members.iter().position(|m| *m == id) {
        members.remove(i);
        let facilitator = if facilitator == Some(id) { members.first().copied() } else { facilitator.filter(|f| members.contains(f)) };
        (members, facilitator)
    } else {
        members.push(id);
        (members, facilitator.or(Some(id)))
    }
}

/// `id` receives the next kickoff, and joins the roster if they were not in it.
pub fn facilitate(mut members: Vec<usize>, id: usize) -> (Vec<usize>, Option<usize>) {
    if !members.contains(&id) {
        members.push(id);
    }
    (members, Some(id))
}

/// Drops a deleted bot. If they were the facilitator, the first remaining member takes that seat.
pub fn forget(mut members: Vec<usize>, facilitator: Option<usize>, gone: usize) -> (Vec<usize>, Option<usize>) {
    members.retain(|id| *id != gone);
    let facilitator = match facilitator {
        Some(id) if id != gone && members.contains(&id) => Some(id),
        _ => members.first().copied(),
    };
    (members, facilitator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_peers_and_keeps_the_kickoff() {
        let peers = [Peer { name: "Implementer", blurb: "Writes code" }, Peer { name: "Designer", blurb: "UI critique" }];
        let text = prompt("  Daily standup  ", "  What shipped?\n", &peers);
        assert!(text.contains("Room \"Daily standup\""));
        assert!(text.contains("@Implementer (Writes code)"));
        assert!(text.contains("@Designer (UI critique)"));
        assert!(text.contains("You are not their lead"));
        assert!(text.ends_with("What shipped?"));
        assert!(!text.contains("@Facilitator"));
    }

    #[test]
    fn a_solo_room_names_no_peers() {
        let text = prompt("Standup", "Hello", &[]);
        assert!(text.contains("No other bots are in this room."));
        assert!(text.ends_with("Hello"));
    }

    #[test]
    fn toggle_and_forget_keep_a_facilitator() {
        let (members, fac) = toggle(vec![], None, 1);
        assert_eq!((members.as_slice(), fac), ([1].as_slice(), Some(1)));
        let (members, fac) = toggle(members, fac, 2);
        assert_eq!((members.as_slice(), fac), ([1, 2].as_slice(), Some(1)));
        let (members, fac) = toggle(members, fac, 1);
        assert_eq!((members.as_slice(), fac), ([2].as_slice(), Some(2)));
        let (members, fac) = facilitate(members, 3);
        assert_eq!((members.as_slice(), fac), ([2, 3].as_slice(), Some(3)));
        let (members, fac) = forget(members, fac, 3);
        assert_eq!((members.as_slice(), fac), ([2].as_slice(), Some(2)));
        assert_eq!(forget(vec![], None, 1), (vec![], None));
    }

    #[test]
    fn start_waits_for_a_title_a_kickoff_and_a_facilitator() {
        assert_eq!(block("  ", "hi", &[1], Some(1)), Some("Name the room"));
        assert_eq!(block("Standup", "  ", &[1], Some(1)), Some("Write a kickoff"));
        assert_eq!(block("Standup", "hi", &[], None), Some("Add at least one bot"));
        assert_eq!(block("Standup", "hi", &[1], None), Some("Choose a facilitator"));
        assert_eq!(block("Standup", "hi", &[1], Some(2)), Some("Choose a facilitator"));
        assert_eq!(block("Standup", "hi", &[1], Some(1)), None);
    }

    #[test]
    fn older_room_json_defaults_the_new_fields() {
        let room: Room = serde_json::from_str(r#"{"id":1,"title":"Standup","kickoff":"What shipped?","members":[1,2]}"#).unwrap();
        assert_eq!(room.facilitator, None);
        assert!(!room.unread && !room.started);
        let saved = serde_json::to_string(&room).unwrap();
        assert_eq!(serde_json::from_str::<Room>(&saved).unwrap(), room);
    }
}
