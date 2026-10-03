//! One turn of a bot: sending, starting it with its role text, streaming its events, and finishing it (notes, quiet runs, alerts).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::*;

use super::Eggbot;
use super::bot::{Bot, Msg, reply_text};
use super::state::{SHARED, data_dir};
use crate::claude::Provider;
use crate::memory::Update;
use crate::{claude, codex, group, handoff, memory, room, sandbox, skills, usage};

const FRESH_START: &str = "We are about to start a fresh session. Update /memory/NOTES.md with short bullets worth keeping (Facts, Preferences, Lessons — no chat logs), or end with one <eggbot-learn> block. Then reply with one short line.";

impl Eggbot {
    pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_draft(window, cx);
        let text = self.input.read(cx).value().trim().to_string();
        let Some(bot) = self.bots.get(self.selected) else { return };
        if text.is_empty() {
            return;
        }
        let (id, provider, busy) = (bot.id, bot.provider, bot.busy());
        // a paused send stays in the box, so a window that resets overnight does not fire a draft
        if self.breach_of(provider).is_some_and(|b| b.level == usage::Level::Pause) {
            cx.notify();
            return;
        }
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        // busy, or throttled behind another bot: the message waits on the persisted queue
        if busy || !self.may_start(provider) {
            self.deliver(id, handoff::Pending::typed(text), cx);
            return;
        }
        if let Some(bot) = self.bot_mut(id) {
            bot.msgs.push(Msg::User(text.clone()));
        }
        self.start_turn(id, handoff::Pending::user(text), cx);
    }

    /// `pending.fresh` runs the turn in a throwaway session (schedules), leaving the main session untouched.
    pub(crate) fn start_turn(&mut self, id: usize, mut pending: handoff::Pending, cx: &mut Context<Self>) {
        let Some((provider, busy)) = self.bot(id).map(|b| (b.provider, b.busy())) else { return };
        // never drop a hop: if the guard closed, put it back at the front of the saved queue
        if busy || !self.may_start(provider) {
            if let Some(bot) = self.bot_mut(id) {
                bot.queue.insert(0, pending);
            }
            self.save();
            cx.notify();
            return;
        }
        let roster_bots: Vec<(usize, String, String)> = self.bots.iter().map(|b| (b.id, b.name.clone(), b.blurb())).collect();
        let stored = self.bot(id).map(|b| b.recent.clone()).unwrap_or_default();
        let alive: Vec<&str> = roster_bots.iter().filter(|(i, ..)| *i != id).map(|(_, name, _)| name.as_str()).collect();
        let recent = handoff::recent(&stored, &alive, 4);
        let names: Vec<(usize, &str)> = roster_bots.iter().map(|(i, name, _)| (*i, name.as_str())).collect();
        let room_peers = room::peers(&self.rooms, &names, id);
        let room_refs: Vec<(&str, &[&str])> = room_peers.iter().map(|(title, peers)| (*title, peers.as_slice())).collect();
        let roster_refs: Vec<(usize, &str, &str)> = roster_bots.iter().map(|(i, name, blurb)| (*i, name.as_str(), blurb.as_str())).collect();
        let others = handoff::roster(&roster_refs, id, &recent, &room_refs);
        let shared = self.shared.clone().unwrap_or_else(|| SHARED.into());
        let membership: Vec<(String, String)> = group::of_bot(&self.groups, id)
            .into_iter()
            .map(|g| {
                let notes = std::fs::read_to_string(group::notes_file(&data_dir(), g.id)).unwrap_or_default();
                (g.title.clone(), notes)
            })
            .collect();
        let group_refs: Vec<(&str, &str)> = membership.iter().map(|(title, notes)| (title.as_str(), notes.as_str())).collect();
        // room memory rides only on a turn already in that room; a private turn stays private
        let acting_room = pending.room.and_then(|rid| self.rooms.iter().find(|r| r.id == rid && r.members.contains(&id)).map(|r| (r.id, r.title.clone())));
        let sole_room = self.rooms.iter().filter(|r| r.members.contains(&id)).count() == 1;
        let room_body = acting_room.as_ref().map(|(rid, _)| std::fs::read_to_string(room::notes_file(&data_dir(), *rid)).unwrap_or_default()).unwrap_or_default();
        let Some(bot) = self.bot_mut(id) else { return };
        let (hops, fresh) = (pending.hops, pending.fresh);
        let prompt = pending.prompt.clone();
        bot.stopped = false;
        bot.hops = hops;
        bot.fresh_turn = fresh;
        // cleared so a sign-in retry of this turn does not add the bubble twice
        if std::mem::take(&mut pending.typed) {
            bot.msgs.push(Msg::User(pending.prompt.clone()));
        }
        bot.reply_from = bot.msgs.len();
        bot.current = Some(pending);
        // notes stay in /memory; scratch is /work only when the user has mounted nothing
        let home = data_dir().join("bots").join(id.to_string());
        let (scratch, memory) = (home.join("work"), home.join("memory"));
        for dir in [&scratch, &memory] {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("eggbot: could not create {}: {e}", dir.display());
            }
        }
        let notes = std::fs::read_to_string(memory.join("NOTES.md")).unwrap_or_default();
        let room_for_turn = acting_room.as_ref().map(|(_, title)| (title.as_str(), room_body.as_str()));
        // skills stay between the role and the roster; notes are private, then group, then this room
        let notes_arg = room::notes_for_turn(&notes, &group_refs, room_for_turn, sole_room);
        let role = skills::role_text(bot.role(), &bot.skills, &others, &notes_arg, &sandbox::folders_note(&bot.folders), &shared);
        let send_role = bot.provider == Provider::Codex && (fresh || bot.thread.is_none() || bot.codex_role.as_ref() != Some(&role));
        bot.pending_role = (send_role && !fresh).then(|| role.clone());
        let turn = claude::Turn {
            send_role,
            bot: id,
            folders: bot.folders.clone(),
            scratch,
            memory,
            prompt,
            role: role.clone(),
            session: match (fresh, bot.provider) {
                (true, _) => None,
                (false, Provider::Codex) => bot.thread.clone(),
                (false, Provider::Claude) => bot.session.clone(),
            },
            model: bot.model.clone(),
            effort: bot.effort.clone(),
        };
        let (handle, events) = match bot.provider {
            Provider::Claude => claude::run(turn),
            Provider::Codex => codex::run(turn),
        };
        bot.run = Some(handle);
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |this, cx| this.apply(id, ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.save();
        // a new turn on screen jumps to the end; replies then follow the tail while you stay at the bottom
        if self.bots.get(self.selected).is_some_and(|b| b.id == id) {
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(bot) = self.bots.get_mut(self.selected)
            && let Some(run) = &bot.run
        {
            bot.stopped = true;
            run.stop();
            cx.notify();
        }
    }

    fn apply(&mut self, id: usize, ev: claude::Ev, cx: &mut Context<Self>) {
        use claude::Ev;
        if let Ev::Usage(meter) = ev {
            self.set_meter(meter, cx);
            cx.notify();
            return;
        }
        // collected before the bot borrow; a group or room bullet is written only where this bot is a member now
        let groups: Vec<(usize, String)> = group::of_bot(&self.groups, id).into_iter().map(|g| (g.id, g.title.clone())).collect();
        let rooms: Vec<(usize, String)> = self.rooms.iter().filter(|r| r.members.contains(&id)).map(|r| (r.id, r.title.clone())).collect();
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        bot.status = match ev {
            Ev::Status(ref s) => Some(s.clone()),
            _ => None,
        };
        match ev {
            Ev::Session(_) | Ev::Context { .. } if bot.fresh_turn => {}
            Ev::Session(s) if !s.is_empty() && bot.provider == Provider::Codex => bot.thread = Some(s),
            Ev::Session(s) if !s.is_empty() => bot.session = Some(s),
            Ev::Context { used, window } => {
                bot.context.0 = used.unwrap_or(bot.context.0);
                bot.context.1 = window.unwrap_or(bot.context.1);
            }
            Ev::TextStart => bot.msgs.push(Msg::Bot(String::new())),
            Ev::Text(t) => match bot.msgs.last_mut() {
                Some(Msg::Bot(s)) => s.push_str(&t),
                _ => bot.msgs.push(Msg::Bot(t)),
            },
            Ev::Tool { id, name, target } => bot.msgs.push(Msg::Tool { id, verb: name, target, detail: String::new(), open: false }),
            Ev::ToolResult { id: tool, content } => {
                if let Some(Msg::Tool { detail, .. }) = bot.msgs.iter_mut().rev().find(|m| matches!(m, Msg::Tool { id, .. } if *id == tool)) {
                    *detail = content;
                }
            }
            Ev::Done { error } => {
                bot.run = None;
                // quit_now already saved `current`; clearing it here would drop the hop
                if self.quitting {
                    return;
                }
                bot.msgs.retain(|m| !matches!(m, Msg::Bot(s) if s.is_empty()));
                // documented transient error when bots renew the shared Claude login at the same moment
                let clash = !bot.stopped && error.as_deref().is_some_and(|e| e.contains("process is refreshing it"));
                if clash
                    && !std::mem::take(&mut bot.retried)
                    && let Some(pending) = bot.current.clone()
                {
                    bot.retried = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(5)).await;
                        this.update(cx, |this, cx| this.deliver(id, pending, cx)).ok();
                    })
                    .detach();
                    cx.notify();
                    return;
                }
                bot.retried = false;
                // turn finished: a later save must not look like a crash mid-handoff
                let finished = bot.current.take();
                let room_id = finished.as_ref().and_then(|p| p.room);
                let stopped = bot.stopped;
                let ok = !stopped && error.is_none();
                let failed = error.clone().filter(|_| !stopped);
                match (stopped, error) {
                    (true, _) => bot.msgs.push(Msg::Error("Stopped.".into())),
                    (false, Some(e)) => bot.msgs.push(Msg::Error(e)),
                    _ => {}
                }
                // this turn's reply = bot text written after it started, not an earlier marker still sitting above
                let start = bot.reply_from.min(bot.msgs.len());
                let reply = reply_text(&bot.msgs, start);
                let hops = bot.hops;
                let fresh_turn = bot.fresh_turn;
                // hide the learn block before the transcript, handoff, quiet-check, or alert
                let reply = if ok { keep_notes(bot, &groups, &rooms, &reply, start) } else { reply };
                // a scheduled run with nothing to say stays out of the way
                let quiet = ok && fresh_turn && is_quiet(&reply);
                if quiet {
                    drop_quiet_reply(&mut bot.msgs, start);
                }
                let (name, color) = (bot.name.clone(), bot.color());
                // engine down: keep the hop queued instead of starting it into the same failure
                let engine_down = failed.as_deref().is_some_and(sandbox::engine_down);
                if engine_down {
                    bot.queue = handoff::restore(finished, std::mem::take(&mut bot.queue));
                }
                let provider = bot.provider;
                let refreshed = std::mem::take(&mut bot.refreshing);
                if let Some(role) = bot.pending_role.take().filter(|_| ok) {
                    bot.codex_role = Some(role);
                }
                if refreshed && ok {
                    // notes are saved: drop the session so the next turn starts clean
                    match bot.provider {
                        Provider::Claude => bot.session = None,
                        Provider::Codex => (bot.thread, bot.codex_role) = (None, None),
                    }
                    bot.context.0 = 0;
                    bot.msgs.push(Msg::Divider("New session · notes kept".into()));
                }
                if ok
                    && !refreshed
                    && !quiet
                    && let Some(rid) = room_id
                {
                    self.log_room(rid, |r| r.record_reply(id, &name, color, &reply));
                }
                let handed = ok && !refreshed && !quiet && self.hand_off(id, reply.clone(), hops, room_id, cx);
                match (stopped, failed) {
                    (false, Some(e)) => {
                        if let Some(rid) = room_id {
                            self.log_room(rid, |r| r.record_trouble(id, &name, &e));
                        }
                        self.alert(id, &format!("{name} needs you"), &e);
                    }
                    (true, _) => {
                        if let Some(rid) = room_id {
                            self.log_room(rid, |r| r.record_trouble(id, &name, "Stopped."));
                        }
                    }
                    (false, None) if ok && !quiet && !handed => self.alert(id, &name, &reply),
                    _ => {}
                }
                // over the limit: leave the persisted queue alone
                let next = if !engine_down && self.may_start(provider) { self.bot_mut(id).and_then(|b| (!b.queue.is_empty()).then(|| b.queue.remove(0))) } else { None };
                if let Some(pending) = next {
                    self.start_turn(id, pending, cx);
                }
                if !engine_down {
                    self.pump_queues(true, Some(id), cx);
                }
                self.release_schedules(cx);
                self.save();
            }
            _ => {}
        }
        cx.notify();
    }

    /// The bot saves its notes, then its next turn starts a new session.
    pub(crate) fn fresh_start(&mut self, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get(self.selected) else { return };
        if bot.busy() || !self.may_start(bot.provider) {
            cx.notify();
            return;
        }
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        bot.refreshing = true;
        bot.msgs.push(Msg::Scheduled { prompt: "Save your notes before a fresh session.".into(), label: "Fresh start".into() });
        let id = bot.id;
        self.start_turn(id, handoff::Pending::user(FRESH_START.into()), cx);
    }

    pub(crate) fn send_again(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bot(id) else { return };
        if bot.busy() || !self.may_start(bot.provider) {
            cx.notify();
            return;
        }
        let Some(bot) = self.bot_mut(id) else { return };
        if let Some(Msg::SignedIn { prompt, .. }) = bot.msgs.get_mut(i)
            && let Some(text) = prompt.take()
        {
            bot.msgs.push(Msg::User(text.clone()));
            self.start_turn(id, handoff::Pending::user(text), cx);
        }
    }
}

/// Merges an `<eggbot-learn>` block into the notes files and returns the reply without that block.
/// `start` is `reply_from`, so only this turn's bubbles change.
fn keep_notes(bot: &mut Bot, groups: &[(usize, String)], rooms: &[(usize, String)], reply: &str, start: usize) -> String {
    let (visible, updates) = memory::extract(reply);
    if visible != reply {
        show_visible(&mut bot.msgs, start, &visible);
    }
    for (path, updates) in note_files(&data_dir(), bot.id, groups, rooms, &updates) {
        if let Err(e) = memory::save(&path, &updates) {
            eprintln!("eggbot: could not save {}: {e}", path.display());
        }
    }
    visible
}

/// Replaces this turn's bot bubbles (from `start`) with one `visible` bubble; the learn block can span streamed chunks.
fn show_visible(msgs: &mut Vec<Msg>, start: usize, visible: &str) {
    let mut kept = false;
    let mut i = start.min(msgs.len());
    while i < msgs.len() {
        if matches!(msgs[i], Msg::Bot(_)) {
            if !kept && !visible.is_empty() {
                msgs[i] = Msg::Bot(visible.to_string());
                kept = true;
                i += 1;
            } else {
                msgs.remove(i);
            }
        } else {
            i += 1;
        }
    }
}

/// Which notes file each learned bullet goes to: private first, then each group, then each room.
/// `groups` and `rooms` are `(id, title)` for the ones this bot is in now; a bullet for any other is dropped.
fn note_files(root: &Path, bot: usize, groups: &[(usize, String)], rooms: &[(usize, String)], updates: &[Update]) -> Vec<(PathBuf, Vec<Update>)> {
    let room_titles: Vec<&str> = rooms.iter().map(|(_, title)| title.as_str()).collect();
    let routed_rooms = room::route(updates, &room_titles);
    let group_titles: Vec<&str> = groups.iter().map(|(_, title)| title.as_str()).collect();
    let routed = group::route(&routed_rooms.rest, &group_titles);
    let mut files = vec![];
    if !routed.private.is_empty() {
        files.push((root.join("bots").join(bot.to_string()).join("memory").join("NOTES.md"), routed.private));
    }
    files.extend(routed.shared.into_iter().map(|(i, u)| (group::notes_file(root, groups[i].0), u)));
    files.extend(routed_rooms.memory.into_iter().map(|(i, u)| (room::notes_file(root, rooms[i].0), u)));
    files
}

/// A scheduled run may answer QUIET when nothing needs the user.
fn is_quiet(reply: &str) -> bool {
    reply.trim().trim_end_matches('.') == "QUIET"
}

/// Removes this turn's reply bubbles and marks the quiet run with a divider; tool lines stay.
fn drop_quiet_reply(msgs: &mut Vec<Msg>, start: usize) {
    let tail = msgs.split_off(start.min(msgs.len()));
    msgs.extend(tail.into_iter().filter(|m| !matches!(m, Msg::Bot(_))));
    msgs.push(Msg::Divider("Nothing to report".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
    use core::prelude::v1::test;

    #[test]
    fn group_notes_sit_with_private_notes_after_the_roster() {
        let notes = group::notes_for("## Facts\n- private fact\n", &[("Reviewers", "## Preferences\n- reply in Italian\n")]);
        let skills = skills::defaults("Reviewer");
        let got = skills::role_text("ROLE", &skills, " ROSTER", &notes, " FOLDERS", "SHARED");
        let roster = got.find("ROSTER").unwrap();
        let private = got.find("private fact").unwrap();
        let shared = got.find("reply in Italian").unwrap();
        let folders = got.find("FOLDERS").unwrap();
        assert!(roster < private && private < shared && shared < folders);
        assert!(got.contains("Group notes for \"Reviewers\""));
        assert!(got.ends_with("SHARED"));
        let outsider = skills::role_text("ROLE", &[], " ROSTER", &group::notes_for("## Facts\n- private fact\n", &[]), "", "SHARED");
        assert!(!outsider.contains("Italian"));
        assert!(!outsider.contains("Group notes"));
    }

    #[test]
    fn notes_and_roster_follow_skills() {
        let skills = skills::defaults("Implementer");
        let notes = memory::context("## Facts\n- likes short replies\n");
        let others = handoff::roster(&[(0, "Implementer", "Writes and changes code"), (1, "Reviewer", "Reads diffs, finds bugs, weighs risk")], 0, &["Reviewer"], &[("Standup", &["Reviewer"])]);
        let got = skills::role_text("ROLE", &skills, &others, &notes, " The user's folders are mounted at /work/proj.", "SHARED");
        let skill_at = got.find("## Smallest change").unwrap();
        let roster_at = got.find("matches their specialty").unwrap();
        let notes_at = got.find("likes short replies").unwrap();
        let folders_at = got.find("/work/proj").unwrap();
        assert!(skill_at < roster_at && roster_at < notes_at && notes_at < folders_at);
        assert!(got.contains("In room \"Standup\""));
        assert!(got.ends_with("SHARED"));
    }

    fn kinds(msgs: &[Msg]) -> Vec<String> {
        msgs.iter()
            .map(|m| match m {
                Msg::Bot(t) => format!("bot:{t}"),
                Msg::User(t) => format!("user:{t}"),
                Msg::Divider(t) => format!("divider:{t}"),
                Msg::Tool { .. } => "tool".into(),
                _ => "other".into(),
            })
            .collect()
    }

    fn tool() -> Msg {
        Msg::Tool { id: "t1".into(), verb: "Read".into(), target: "a.rs".into(), detail: String::new(), open: false }
    }

    #[test]
    fn only_this_turns_bubbles_become_the_visible_reply() {
        let mut msgs = vec![Msg::Bot("earlier".into()), Msg::User("hi".into()), Msg::Bot("Done.\n<eggbot-".into()), tool(), Msg::Bot("learn>- fact: x</eggbot-learn>".into())];
        show_visible(&mut msgs, 2, "Done.");
        assert_eq!(kinds(&msgs), ["bot:earlier", "user:hi", "bot:Done.", "tool"]);
        // a reply that was only the block leaves no empty bubble
        show_visible(&mut msgs, 2, "");
        assert_eq!(kinds(&msgs), ["bot:earlier", "user:hi", "tool"]);
    }

    #[test]
    fn learned_bullets_go_to_the_private_group_or_room_file() {
        let (_, updates) = memory::extract("ok\n<eggbot-learn>\n- fact: mine\n- group fact: ours\n- room fact: at nine\n- room Other fact: dropped\n</eggbot-learn>");
        let files = note_files(Path::new("/support"), 3, &[(5, "Reviewers".into())], &[(8, "Standup".into())], &updates);
        let got: Vec<(String, Vec<&str>)> = files.iter().map(|(p, u)| (p.display().to_string(), u.iter().map(|u| u.text.as_str()).collect())).collect();
        let want = [("/support/bots/3/memory/NOTES.md", "mine"), ("/support/groups/5/NOTES.md", "ours"), ("/support/rooms/8/NOTES.md", "at nine")];
        assert_eq!(got, want.map(|(p, t)| (p.to_string(), vec![t])));
        // no group or room membership: their bullets are dropped, not saved privately
        let alone = note_files(Path::new("/support"), 3, &[], &[], &updates);
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].1.len(), 1);
        assert!(note_files(Path::new("/support"), 3, &[], &[], &[]).is_empty());
    }

    #[test]
    fn a_quiet_run_keeps_its_tool_lines_and_says_nothing_to_report() {
        assert!(is_quiet(" QUIET.\n") && is_quiet("QUIET"));
        assert!(!is_quiet("Quiet day, but the build broke."));
        let mut msgs = vec![Msg::Bot("earlier".into()), tool(), Msg::Bot("QUIET".into())];
        drop_quiet_reply(&mut msgs, 1);
        assert_eq!(kinds(&msgs), ["bot:earlier", "tool", "divider:Nothing to report"]);
    }
}
