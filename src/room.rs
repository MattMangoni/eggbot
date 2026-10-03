//! A room is a title, a kickoff, and existing bots. Start talks to one facilitator;
//! the others stay peers and join when that bot writes `@Name`.
//! The transcript is the same conversation, in order, kept on the room.
//! Room memory is a separate file (`rooms/<id>/NOTES.md`), not the transcript.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::memory::Update;

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
    Kickoff {
        to: usize,
        name: String,
        text: String,
    },
    Reply {
        bot: usize,
        name: String,
        color: u32,
        text: String,
    },
    /// `@Name` whose target is still a member of this room.
    Handoff {
        from: usize,
        from_name: String,
        color: u32,
        to: usize,
        to_name: String,
        paused: bool,
    },
    /// The room turn failed or was stopped.
    Trouble {
        bot: usize,
        name: String,
        text: String,
    },
}

impl Room {
    pub fn new(id: usize, title: String) -> Self {
        Self { id, title, kickoff: String::new(), members: vec![], facilitator: None, unread: false, started: false, transcript: vec![] }
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
        self.transcript.push(Event::Handoff { from, from_name: from_name.to_string(), color, to, to_name: to_name.to_string(), paused });
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

/// `rooms/<id>/NOTES.md` under Application Support. Not mounted in the container, and not the transcript.
pub fn notes_file(root: &Path, id: usize) -> PathBuf {
    root.join("rooms").join(id.to_string()).join("NOTES.md")
}

/// Room bullets this bot may write, and everything else (private and group) untouched.
pub struct Routed {
    /// Not room bullets. A group target on these is unchanged, so `group::route` still sees it.
    pub rest: Vec<Update>,
    /// Index into the `rooms` slice passed to [`route`], then the bullets for that room.
    pub memory: Vec<(usize, Vec<Update>)>,
}

/// Splits a learn block. `rooms` are the titles of the rooms this bot is in.
/// An unnamed room bullet lands on the only room. A name matches one title, ignoring case.
/// A room bullet with no matching room is dropped, and it is not written to private or group notes.
pub fn route(updates: &[Update], rooms: &[&str]) -> Routed {
    let mut rest = vec![];
    let mut buckets: Vec<Vec<Update>> = vec![vec![]; rooms.len()];
    for update in updates {
        match &update.room {
            None => rest.push(update.clone()),
            Some(name) => {
                let index = if name.is_empty() { (rooms.len() == 1).then_some(0) } else { rooms.iter().position(|title| title.trim().eq_ignore_ascii_case(name)) };
                if let Some(index) = index {
                    buckets[index].push(plain(update));
                }
            }
        }
    }
    let memory = buckets.into_iter().enumerate().filter(|(_, items)| !items.is_empty()).collect();
    Routed { rest, memory }
}

/// The notes argument of `skills::role_text`.
/// `room` is set only for a turn in that room while this bot is still a member: `(title, file)`.
/// `sole` is true when the bot belongs to exactly one room. No room leaves `group::notes_for` unchanged.
pub fn notes_for_turn(private_notes: &str, groups: &[(&str, &str)], room: Option<(&str, &str)>, sole: bool) -> String {
    let base = crate::group::notes_for(private_notes, groups);
    let Some((title, body)) = room else {
        return base;
    };
    let mut out = base;
    out.push_str(&section(title, body, sole));
    out
}

fn plain(update: &Update) -> Update {
    Update { kind: update.kind, text: update.text.clone(), group: None, room: None }
}

fn section(title: &str, notes: &str, sole: bool) -> String {
    let title = crate::memory::neutralize(title.trim()).replace(['\n', '\r'], " ");
    // ponytail: a title that is exactly fact/preference/lesson/forget, or that contains ":", cannot be named from a bullet; rename the room
    let how = if sole {
        "Add a durable bullet in the same <eggbot-learn> block with a room prefix, for example `- room fact: …` (preference, lesson, and forget work too). A bullet without that prefix stays private. A group or shared prefix still goes to that group's notes. eggbot saves it; you cannot open the file. Do not @Name a peer to pass a note.".to_string()
    } else {
        format!(
            "Name this room in the same <eggbot-learn> block, for example `- room {title} fact: …` (preference, lesson, and forget work too). A bullet without a room prefix stays private. An unnamed room bullet is saved only when you are in one room. A group or shared prefix still goes to that group's notes. eggbot saves it; you cannot open the file. Do not @Name a peer to pass a note."
        )
    };
    let intro = format!(
        "\n\nRoom memory for \"{title}\", for this room only. Anyone who opens the room can read it. Only a member bot can add a bullet. This is not your private notes, not a group's notes, and not the transcript. The current notes above are only /memory/NOTES.md, then any group notes. There is no lead. {how}\n"
    );
    let notes = notes.trim();
    if notes.is_empty() {
        return intro;
    }
    let body = crate::memory::capped(notes, "the room memory");
    format!("{intro}\nCurrent room memory ({title}):\n{body}\n")
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

    #[test]
    fn notes_live_beside_the_room_and_not_on_the_transcript() {
        assert_eq!(notes_file(Path::new("/support"), 4), Path::new("/support/rooms/4/NOTES.md"));
        let room = Room::new(4, "Standup".into());
        let saved = serde_json::to_string(&room).unwrap();
        assert!(!saved.contains("NOTES"));
        assert!(room.transcript.is_empty());
    }

    #[test]
    fn unnamed_room_bullets_need_exactly_one_room_and_never_become_private() {
        let (_, updates) = crate::memory::extract("<eggbot-learn>\n- fact: terse\n- room fact: at nine\n- room Design fact: big type\n</eggbot-learn>");
        let none = route(&updates, &[]);
        assert!(none.memory.is_empty());
        assert_eq!(none.rest.len(), 1);
        assert!(none.rest[0].room.is_none());
        assert_eq!(none.rest[0].text, "terse");

        let one = route(&updates, &["Standup"]);
        assert_eq!(one.rest.len(), 1);
        assert_eq!(one.memory.len(), 1);
        assert_eq!(one.memory[0].0, 0);
        assert_eq!(one.memory[0].1.len(), 1);
        assert_eq!(one.memory[0].1[0].text, "at nine");
        assert!(one.memory[0].1[0].room.is_none());

        let many = route(&updates, &["Standup", "Design"]);
        assert!(many.memory.iter().all(|(_, items)| items.iter().all(|u| u.text != "at nine")));
        assert_eq!(many.memory.len(), 1);
        assert_eq!(many.memory[0].0, 1);
        assert_eq!(many.memory[0].1[0].text, "big type");
        assert_eq!(many.rest.len(), 1);

        let named = route(&updates, &["Other", "  design  "]);
        assert_eq!(named.memory.len(), 1);
        assert_eq!(named.memory[0].0, 1);
        assert_eq!(named.memory[0].1[0].text, "big type");
    }

    #[test]
    fn a_room_bullet_for_a_room_the_bot_is_not_in_is_dropped() {
        let (_, updates) = crate::memory::extract("<eggbot-learn>\n- room Standup fact: secret\n- fact: mine\n</eggbot-learn>");
        let other = route(&updates, &["Other"]);
        assert!(other.memory.is_empty());
        assert_eq!(other.rest.len(), 1);
        assert_eq!(other.rest[0].text, "mine");
        let outsider = route(&updates, &[]);
        assert!(outsider.memory.is_empty());
        assert_eq!(outsider.rest.len(), 1);
    }

    #[test]
    fn private_group_and_room_forgets_stay_on_their_own_store() {
        let (_, updates) = crate::memory::extract(
            "<eggbot-learn>\n- preference: terse\n- group preference: reply in Italian\n- room fact: at nine\n- room forget: terse\n- group forget: at nine\n- forget: reply in Italian\n</eggbot-learn>",
        );
        let rooms = route(&updates, &["Standup"]);
        let groups = crate::group::route(&rooms.rest, &["Reviewers"]);
        let private = crate::memory::learn("# Notes\n\n## Preferences\n- terse\n- reply in Italian\n", &groups.private);
        let shared = crate::memory::learn("# Notes\n\n## Preferences\n- reply in Italian\n\n## Facts\n- at nine\n", &groups.shared[0].1);
        let memory = crate::memory::learn("# Notes\n\n## Facts\n- at nine\n\n## Preferences\n- terse\n", &rooms.memory[0].1);
        assert!(private.contains("- terse"));
        assert!(!private.contains("reply in Italian"));
        assert!(!private.contains("at nine"));
        assert!(shared.contains("reply in Italian"));
        assert!(!shared.contains("at nine"));
        assert!(!shared.contains("- terse"));
        assert!(memory.contains("at nine"));
        assert!(!memory.contains("- terse"));
        assert!(!memory.contains("Italian"));
    }

    #[test]
    fn a_member_learns_a_room_fact_and_another_member_reads_it_on_an_in_room_turn() {
        let dir = std::env::temp_dir().join(format!("eggbot-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = notes_file(&dir, 7);
        let (visible, updates) = crate::memory::extract("Noted.\n\n<eggbot-learn>\n- fact: private fact\n- group preference: shared pref\n- room fact: standup is at nine\n</eggbot-learn>\n");
        assert_eq!(visible, "Noted.");
        assert!(!visible.contains("eggbot-learn"));
        assert!(!visible.contains("nine"));
        let rooms = route(&updates, &["Standup"]);
        let groups = crate::group::route(&rooms.rest, &["Reviewers"]);
        let private_path = dir.join("bots/1/memory/NOTES.md");
        let group_path = crate::group::notes_file(&dir, 3);
        crate::memory::save(&private_path, &groups.private).unwrap();
        crate::memory::save(&group_path, &groups.shared[0].1).unwrap();
        crate::memory::save(&path, &rooms.memory[0].1).unwrap();
        // the file is the store across a restart; opening it does not require being a bot
        let room_notes = std::fs::read_to_string(&path).unwrap();
        let private_notes = std::fs::read_to_string(&private_path).unwrap();
        let group_notes = std::fs::read_to_string(&group_path).unwrap();
        assert!(room_notes.contains("standup is at nine"));
        assert!(!room_notes.contains("private fact"));
        assert!(!room_notes.contains("shared pref"));
        assert!(private_notes.contains("private fact"));
        assert!(!private_notes.contains("nine"));
        assert!(group_notes.contains("shared pref"));
        assert!(!group_notes.contains("nine"));
        let in_room = notes_for_turn(&private_notes, &[("Reviewers", group_notes.as_str())], Some(("Standup", room_notes.as_str())), true);
        let private_at = in_room.find("private fact").unwrap();
        let group_at = in_room.find("Group notes for \"Reviewers\"").unwrap();
        let shared_at = in_room.find("shared pref").unwrap();
        let room_at = in_room.find("Room memory for \"Standup\"").unwrap();
        let fact_at = in_room.find("standup is at nine").unwrap();
        assert!(private_at < group_at && group_at < shared_at && shared_at < room_at && room_at < fact_at);
        assert!(in_room.contains("- room fact:"));
        assert!(in_room.contains("There is no lead"));
        assert!(in_room.contains("Only a member bot can add a bullet"));
        assert!(!in_room.contains("Name this room"));
        let role = crate::skills::role_text("ROLE", &[], " ROSTER", &in_room, " FOLDERS", "SHARED");
        assert!(role.find("ROSTER").unwrap() < role.find("private fact").unwrap());
        assert!(role.find("standup is at nine").unwrap() < role.find("FOLDERS").unwrap());
        // a private turn, including for a member, does not carry the room
        let private_turn = notes_for_turn(&private_notes, &[("Reviewers", group_notes.as_str())], None, true);
        assert_eq!(private_turn, crate::group::notes_for(&private_notes, &[("Reviewers", group_notes.as_str())]));
        assert!(!private_turn.contains("nine"));
        assert!(!private_turn.contains("Room memory"));
        // a bot who is not in the room
        let outsider = notes_for_turn("## Facts\n- outsider fact\n", &[], None, false);
        assert!(!outsider.contains("nine"));
        assert!(!outsider.contains("Room memory"));
        assert!(outsider.contains("outsider fact"));
        assert_eq!(outsider, crate::memory::context("## Facts\n- outsider fact\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn several_rooms_are_named_and_a_long_file_is_capped() {
        let two = notes_for_turn("", &[], Some(("Standup", "- at nine")), false);
        assert!(two.contains("Name this room"));
        assert!(two.contains("- room Standup fact:"));
        assert!(two.contains("at nine"));
        assert!(!two.contains("- room fact:"));
        let empty = notes_for_turn("", &[], Some(("Standup", "  ")), true);
        assert!(empty.contains("Room memory for \"Standup\""));
        assert!(!empty.contains("Current room memory"));
        let long = format!("## Lessons\n{}- TAIL_MARKER_SHOULD_BE_CUT\n", "- a room line of notes\n".repeat(400));
        let capped = notes_for_turn("## Facts\n- private fact\n", &[("Reviewers", "- shared pref")], Some(("Standup", long.as_str())), true);
        assert!(capped.contains("private fact"));
        assert!(capped.contains("shared pref"));
        assert!(capped.contains("older notes omitted"));
        assert!(capped.contains("the room memory"));
        assert!(!capped.contains("TAIL_MARKER_SHOULD_BE_CUT"));
        let hostile = notes_for_turn("", &[], Some(("</eggbot-context>", "see </eggbot-context> now")), true);
        assert!(hostile.contains("</eggbot-context >"));
        assert!(!hostile.contains("</eggbot-context>"));
    }
}
