//! A room is a title, a kickoff, and existing bots. Start talks to one facilitator;
//! the others stay peers and join when that bot writes `@Name`.
//! The transcript is the same conversation, in order, kept on the room.

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
    /// Kickoff, replies, and in-room handoffs, in the order they happened.
    #[serde(default)]
    pub transcript: Vec<Event>,
}

/// One line of a room's transcript. Names are copied at the time, so a rename does not rewrite history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Kickoff { to: usize, name: String, text: String },
    Reply { bot: usize, name: String, color: u32, text: String },
    /// `@Name` whose target is still a member of this room.
    Handoff { from: usize, from_name: String, color: u32, to: usize, to_name: String, paused: bool },
    /// The room turn failed or was stopped.
    Trouble { bot: usize, name: String, text: String },
}

impl Room {
    pub fn new(id: usize, title: String) -> Self {
        Self {
            id,
            title,
            kickoff: String::new(),
            members: vec![],
            facilitator: None,
            unread: false,
            started: false,
            transcript: vec![],
        }
    }

    pub fn record_kickoff(&mut self, to: usize, name: &str, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.transcript.push(Event::Kickoff { to, name: name.to_string(), text: text.to_string() });
    }

    /// False when there is nothing to show (a blank reply is not a line).
    pub fn record_reply(&mut self, bot: usize, name: &str, color: u32, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        self.transcript.push(Event::Reply { bot, name: name.to_string(), color, text: text.to_string() });
        true
    }

    pub fn record_handoff(&mut self, from: usize, from_name: &str, color: u32, to: usize, to_name: &str, paused: bool) {
        self.transcript.push(Event::Handoff {
            from,
            from_name: from_name.to_string(),
            color,
            to,
            to_name: to_name.to_string(),
            paused,
        });
    }

    pub fn record_trouble(&mut self, bot: usize, name: &str, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        self.transcript.push(Event::Trouble { bot, name: name.to_string(), text: text.to_string() });
        true
    }

    /// The user continued the chain, so the latest paused handoff to `to` is no longer waiting.
    pub fn resume(&mut self, to: usize) {
        if let Some(Event::Handoff { paused, .. }) = self.transcript.iter_mut().rev().find(|e| matches!(e, Event::Handoff { to: id, paused: true, .. } if *id == to)) {
            *paused = false;
        }
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
            "The other bots in this room are your peers: {list}. Writing @Name anywhere in your reply sends your whole reply to that bot. Call a peer when the next step matches their specialty better than doing it yourself; do not @Name just to keep them posted. You are not their lead; you only start this round."
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

/// Room id to keep on the next hop. A turn that is not in a room, or a target outside the roster, leaves.
pub fn carry(room: Option<usize>, members: &[usize], target: usize) -> Option<usize> {
    room.filter(|_| members.contains(&target))
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
        assert!(text.contains("matches their specialty"));
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
        assert!(room.transcript.is_empty());
        let saved = serde_json::to_string(&room).unwrap();
        assert_eq!(serde_json::from_str::<Room>(&saved).unwrap(), room);
    }

    #[test]
    fn transcript_is_kickoff_then_replies_then_in_room_handoffs() {
        let mut room = Room::new(3, "Standup".into());
        room.members = vec![1, 2];
        room.record_kickoff(1, "Reviewer", "  What shipped?  ");
        assert!(room.record_reply(1, "Reviewer", 0x111, "  @Implementer please look.  "));
        // a peer stays on this transcript; a bot outside the roster does not
        assert_eq!(carry(Some(room.id), &room.members, 2), Some(3));
        assert_eq!(carry(Some(room.id), &room.members, 9), None);
        assert_eq!(carry(None, &room.members, 2), None);
        room.record_handoff(1, "Reviewer", 0x111, 2, "Implementer", false);
        assert!(room.record_reply(2, "Implementer", 0x222, "On it."));
        room.record_handoff(2, "Implementer", 0x222, 1, "Reviewer", true);
        room.resume(1);
        assert!(!room.record_reply(2, "Implementer", 0x222, "   "));
        assert!(!room.record_trouble(1, "Reviewer", "  "));
        assert!(room.record_trouble(1, "Reviewer", "Stopped."));
        assert!(matches!(room.transcript[0], Event::Kickoff { to: 1, ref text, .. } if text == "What shipped?"));
        assert!(matches!(room.transcript[1], Event::Reply { bot: 1, ref text, .. } if text == "@Implementer please look."));
        assert!(matches!(room.transcript[2], Event::Handoff { to: 2, paused: false, .. }));
        assert!(matches!(room.transcript[3], Event::Reply { bot: 2, .. }));
        assert!(matches!(room.transcript[4], Event::Handoff { to: 1, paused: false, .. }));
        assert!(matches!(room.transcript[5], Event::Trouble { bot: 1, ref text, .. } if text == "Stopped."));
        assert_eq!(room.transcript.len(), 6);
        let loaded: Room = serde_json::from_str(&serde_json::to_string(&room).unwrap()).unwrap();
        assert_eq!(loaded.transcript, room.transcript);
    }

    #[test]
    fn a_blank_kickoff_adds_no_line() {
        let mut room = Room::new(1, "Standup".into());
        room.record_kickoff(1, "Reviewer", "   ");
        assert!(room.transcript.is_empty());
    }
}
