//! Bots hand work to each other by writing `@Name` in a reply.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Chains pause after this many automatic handoffs and wait for the user.
pub const MAX_HOPS: u32 = 3;

/// A turn waiting on a bot, or the turn that was running when eggbot quit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub prompt: String,
    pub hops: u32,
    /// Throwaway session (schedules). Handoffs stay on the main session.
    #[serde(default)]
    pub fresh: bool,
    /// `@Name` handoff, including Continue chain (hops reset to 0).
    #[serde(default)]
    pub handoff: bool,
    /// Room this hop belongs to. Kept on the queue so a quit resumes the same transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<usize>,
    /// Typed while the bot was working. Its chat bubble is added when the turn starts, not mid-reply.
    #[serde(default)]
    pub typed: bool,
}

impl Pending {
    pub fn user(prompt: String) -> Self {
        Self { prompt, hops: 0, fresh: false, handoff: false, room: None, typed: false }
    }

    pub fn typed(prompt: String) -> Self {
        Self { typed: true, ..Self::user(prompt) }
    }

    pub fn handoff(prompt: String, hops: u32) -> Self {
        Self { prompt, hops, fresh: false, handoff: true, room: None, typed: false }
    }

    pub fn schedule(prompt: String) -> Self {
        Self { prompt, hops: 0, fresh: true, handoff: false, room: None, typed: false }
    }

    /// This hop must be restarted after a quit or crash. User turns and schedules stay stopped.
    pub fn inflight(&self) -> bool {
        self.handoff && !self.fresh
    }
}

/// Puts an interrupted handoff back at the front of `queue`. Other running turns were stopped.
pub fn restore(running: Option<Pending>, mut queue: Vec<Pending>) -> Vec<Pending> {
    if let Some(turn) = running.filter(Pending::inflight) {
        queue.insert(0, turn);
    }
    queue
}

/// The next turn to start. A busy bot, or one whose machine is offline, keeps the whole queue.
pub fn dequeue(mut queue: Vec<Pending>, busy: bool, offline: bool) -> (Option<Pending>, Vec<Pending>) {
    if busy || offline || queue.is_empty() {
        return (None, queue);
    }
    let next = queue.remove(0);
    (Some(next), queue)
}

/// Ids of the bots mentioned as `@Name` (case-insensitive, longest name wins: "@Reviewer 2" is not "@Reviewer").
pub fn mentions(text: &str, bots: &[(usize, &str)], sender: usize) -> Vec<usize> {
    let lower = text.to_lowercase();
    let mut names: Vec<(usize, String)> = bots.iter().map(|(id, n)| (*id, n.to_lowercase())).collect();
    names.sort_by_key(|(_, n)| std::cmp::Reverse(n.len()));
    let mut found = vec![];
    for (at, _) in lower.match_indices('@') {
        let rest = &lower[at + 1..];
        let hit = names.iter().find(|(_, n)| {
            rest.starts_with(n.as_str()) && !rest[n.len()..].starts_with(|c: char| c.is_alphanumeric())
        });
        if let Some((id, _)) = hit
            && *id != sender
            && !found.contains(id)
        {
            found.push(*id);
        }
    }
    found
}

/// Puts `name` at the front and drops duplicates past `limit`.
pub fn remember(mut recent: Vec<String>, name: &str, limit: usize) -> Vec<String> {
    recent.retain(|n| !n.eq_ignore_ascii_case(name));
    recent.insert(0, name.to_string());
    recent.truncate(limit);
    recent
}

/// `names` is newest-first. Drops anyone no longer on the roster.
pub fn recent<'a>(names: &'a [String], alive: &[&str], limit: usize) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = vec![];
    for name in names {
        let known = alive.iter().any(|a| a.eq_ignore_ascii_case(name));
        let seen = out.iter().any(|n| n.eq_ignore_ascii_case(name));
        if !known || seen {
            continue;
        }
        out.push(name.as_str());
        if out.len() == limit {
            break;
        }
    }
    out
}

/// Added to every bot's role: who the peers are, when to call them, recent deliveries, and room peers.
/// `recent` is newest-first. `rooms` is `(title, peer names)` for rooms this bot is in.
pub fn roster(bots: &[(usize, &str, &str)], me: usize, recent: &[&str], rooms: &[(&str, &[&str])]) -> String {
    let others: Vec<String> = bots.iter().filter(|(id, ..)| *id != me).map(|(_, name, blurb)| format!("@{name} ({blurb})")).collect();
    let mut parts = vec![];
    if !others.is_empty() {
        parts.push(format!(
            "Other bots you can hand work to: {}. Hand off when the next step matches their specialty better than doing it yourself. Write @Name only to make that bot act next. Do not @Name to narrate, acknowledge, or think out loud.",
            others.join(", ")
        ));
    }
    if !recent.is_empty() && !others.is_empty() {
        let names: Vec<String> = recent.iter().map(|n| format!("@{n}")).collect();
        parts.push(format!("Recent handoffs you delivered: {}.", join_and(&names)));
    }
    for (title, peers) in rooms {
        if peers.is_empty() {
            continue;
        }
        let names: Vec<String> = peers.iter().map(|n| format!("@{n}")).collect();
        parts.push(format!("In room \"{title}\" your peers are {}. Prefer them for this work.", join_and(&names)));
    }
    if parts.is_empty() { String::new() } else { format!("\n\n{}", parts.join("\n")) }
}

/// What the receiving bot is told.
/// Each mount is `(host path, container path)`. Partial overlap is spelled out; see `docs/kb/handoff.md`.
pub fn prompt(from: &str, text: &str, mine: &[(&Path, &str)], theirs: &[(&Path, &str)]) -> String {
    format!("Handoff from {from}, another bot in eggbot. {}\n\n{text}", folder_context(mine, theirs))
}

const DIFFERENT: &str = "They work in a different folder; you cannot see their files.";

/// How the receiver's mounts relate to the sender's. Host paths are compared; container paths are named.
fn folder_context(mine: &[(&Path, &str)], theirs: &[(&Path, &str)]) -> String {
    if mine.is_empty() || theirs.is_empty() {
        return DIFFERENT.into();
    }
    let mut shared = vec![];
    let mut shared_paths = vec![];
    for &(mp, md) in mine {
        for &(tp, td) in theirs {
            if mp == tp {
                shared_paths.push(mp);
                shared.push(if md == td { md.to_string() } else { format!("{md} (theirs is {td})") });
            }
        }
    }
    let under_shared = |path: &Path| shared_paths.iter().any(|s| path.starts_with(s));
    let mut extra = vec![];
    for &(mp, md) in mine {
        if under_shared(mp) {
            continue;
        }
        let closest = theirs.iter().copied().filter(|&(tp, _)| mp.starts_with(tp) && mp != tp).max_by_key(|&(tp, _)| tp.components().count());
        if let Some((_, td)) = closest {
            extra.push(format!("Your {md} is inside their {td}, so they can see your files and you cannot see the rest of that folder."));
        }
    }
    for &(tp, td) in theirs {
        if under_shared(tp) {
            continue;
        }
        let closest = mine.iter().copied().filter(|&(mp, _)| tp.starts_with(mp) && tp != mp).max_by_key(|&(mp, _)| mp.components().count());
        if let Some((_, md)) = closest {
            extra.push(format!("Their {td} is inside your {md}, so you can see their files there and they cannot see the rest of your folder."));
        }
    }
    match (shared.is_empty(), extra.is_empty()) {
        (true, true) => DIFFERENT.into(),
        (false, true) => format!("You share {} with them, so you can open the files they changed there.", join_and(&shared)),
        (true, false) => extra.join(" "),
        (false, false) => format!("You share {} with them, so you can open the files they changed there. {}", join_and(&shared), extra.join(" ")),
    }
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        rest => {
            let (last, head) = rest.split_last().unwrap();
            format!("{}, and {last}", head.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_mentions() {
        let bots = [(0, "Reviewer"), (1, "Implementer"), (2, "Reviewer 2")];
        assert_eq!(mentions("Done. @reviewer please check, cc @Reviewer 2.", &bots, 1), vec![0, 2]);
        assert_eq!(mentions("@Reviewer2 and @Reviewers and email a@b", &bots, 1), Vec::<usize>::new());
        assert_eq!(mentions("@Implementer thanks, @Reviewer again @Reviewer", &bots, 1), vec![0]);
    }

    #[test]
    fn restores_an_interrupted_handoff_ahead_of_the_queue() {
        let running = Pending::handoff("ship it".into(), 2);
        let waiting = Pending::schedule("nightly".into());
        assert_eq!(restore(Some(running.clone()), vec![waiting.clone()]), vec![running, waiting]);
        // Continue chain resets hops to 0 and is still a handoff.
        let continued = Pending::handoff("once more".into(), 0);
        assert!(continued.inflight());
        assert_eq!(restore(Some(continued.clone()), vec![]), vec![continued]);
    }

    #[test]
    fn stopped_user_and_schedule_turns_stay_stopped() {
        let waiting = Pending::handoff("later".into(), 1);
        assert_eq!(restore(Some(Pending::user("hello".into())), vec![waiting.clone()]), vec![waiting.clone()]);
        assert_eq!(restore(Some(Pending::schedule("check".into())), vec![waiting.clone()]), vec![waiting]);
        assert_eq!(restore(None, vec![]), vec![]);
    }

    #[test]
    fn dequeue_waits_while_busy_or_offline() {
        let queued = vec![Pending::handoff("go".into(), 1), Pending::handoff("then".into(), 2)];
        assert_eq!(dequeue(queued.clone(), true, false), (None, queued.clone()));
        assert_eq!(dequeue(queued.clone(), false, true), (None, queued.clone()));
        assert_eq!(dequeue(vec![], false, false), (None, vec![]));
        let (next, rest) = dequeue(queued.clone(), false, false);
        assert_eq!(next, Some(queued[0].clone()));
        assert_eq!(rest, vec![queued[1].clone()]);
    }

    #[test]
    fn pending_roundtrip_and_old_json() {
        let turn = Pending::handoff("ship it".into(), 2);
        let json = serde_json::to_string(&turn).unwrap();
        assert_eq!(serde_json::from_str::<Pending>(&json).unwrap(), turn);
        // fields added later still load
        let old = r#"{"prompt":"ship it","hops":2}"#;
        let loaded = serde_json::from_str::<Pending>(old).unwrap();
        assert!(!loaded.fresh && !loaded.handoff && !loaded.typed);
        assert!(loaded.room.is_none());
        let typed = Pending::typed("and then this".into());
        let again = serde_json::from_str::<Pending>(&serde_json::to_string(&typed).unwrap()).unwrap();
        assert!(again.typed && !again.inflight());
        assert_eq!(restore(Some(typed), vec![]), vec![]);
        let mut room_turn = Pending::handoff("go".into(), 0);
        room_turn.room = Some(3);
        let kept = serde_json::from_str::<Pending>(&serde_json::to_string(&room_turn).unwrap()).unwrap();
        assert_eq!(kept.room, Some(3));
        assert!(kept.inflight());
        assert_eq!(restore(Some(room_turn), vec![])[0].room, Some(3));
    }

    #[test]
    fn folder_overlap_is_named() {
        let proj = Path::new("/repos/proj");
        let other = Path::new("/repos/other");
        let crates = Path::new("/repos/proj/crates");
        let deep = Path::new("/repos/proj/crates/api");
        let share = |mine: &[(&Path, &str)], theirs: &[(&Path, &str)]| prompt("Implementer", "look", mine, theirs);
        assert_eq!(
            share(&[(proj, "/work/proj")], &[(proj, "/work/proj")]),
            "Handoff from Implementer, another bot in eggbot. You share /work/proj with them, so you can open the files they changed there.\n\nlook"
        );
        assert_eq!(
            share(&[(proj, "/work/proj")], &[(proj, "/work/proj-2")]),
            "Handoff from Implementer, another bot in eggbot. You share /work/proj (theirs is /work/proj-2) with them, so you can open the files they changed there.\n\nlook"
        );
        assert_eq!(
            share(&[(proj, "/work/proj"), (other, "/work/other")], &[(proj, "/work/proj"), (other, "/work/other")]),
            "Handoff from Implementer, another bot in eggbot. You share /work/proj and /work/other with them, so you can open the files they changed there.\n\nlook"
        );
        assert!(share(&[], &[(proj, "/work/proj")]).contains("different folder"));
        assert!(share(&[(proj, "/work/proj")], &[(other, "/work/other")]).contains("different folder"));
        assert!(share(&[(crates, "/work/crates")], &[(proj, "/work/proj")]).contains("Your /work/crates is inside their /work/proj"));
        assert!(share(&[(proj, "/work/proj")], &[(crates, "/work/crates")]).contains("Their /work/crates is inside your /work/proj"));
        // the closer parent wins, and a child of a shared folder is not repeated
        let nested = share(&[(deep, "/work/api")], &[(proj, "/work/proj"), (crates, "/work/crates")]);
        assert!(nested.contains("inside their /work/crates"));
        assert!(!nested.contains("inside their /work/proj"));
        let both = share(&[(proj, "/work/proj"), (Path::new("/elsewhere/sub"), "/work/sub")], &[(proj, "/work/proj"), (Path::new("/elsewhere"), "/work/elsewhere")]);
        assert!(both.contains("You share /work/proj"));
        assert!(both.contains("Your /work/sub is inside their /work/elsewhere"));
    }

    #[test]
    fn roster_says_when_to_call_whom() {
        let bots = [(0, "Reviewer", "Reads diffs, finds bugs, weighs risk"), (1, "Implementer", "Writes and changes code"), (2, "Designer", "UI and UX critique and polish")];
        let text = roster(&bots, 0, &["Implementer", "Designer"], &[("Daily standup", &["Implementer", "Designer"])]);
        assert!(text.contains("@Implementer (Writes and changes code)"));
        assert!(text.contains("@Designer (UI and UX critique and polish)"));
        assert!(!text.contains("@Reviewer"));
        assert!(text.contains("matches their specialty"));
        assert!(text.contains("Do not @Name to narrate"));
        assert!(text.contains("Recent handoffs you delivered: @Implementer and @Designer."));
        assert!(text.contains("In room \"Daily standup\" your peers are @Implementer and @Designer."));
        assert!(roster(&bots, 0, &[], &[]).contains("Hand off when"));
        assert_eq!(roster(&[(0, "Reviewer", "Reads diffs")], 0, &["Implementer"], &[]), "");
    }

    #[test]
    fn recent_handoffs_are_newest_unique_and_still_alive() {
        let mut names = vec![];
        names = remember(names, "Implementer", 4);
        names = remember(names, "Designer", 4);
        names = remember(names, "Implementer", 4);
        names = remember(names, "Ghost", 4);
        assert_eq!(names, vec!["Ghost".to_string(), "Implementer".into(), "Designer".into()]);
        assert_eq!(recent(&names, &["Implementer", "Designer"], 4), vec!["Implementer", "Designer"]);
        assert_eq!(recent(&names, &["Designer"], 4), vec!["Designer"]);
        assert_eq!(recent(&names, &["Implementer", "Designer"], 1), vec!["Implementer"]);
    }
}
