//! Handoffs and the persisted queue: `@Name` routing, the hop limit and Continue chain, room transcript lines, and starting queued work.

use std::path::PathBuf;

use gpui_kit::*;

use super::Eggbot;
use super::bot::Msg;
use crate::{handoff, room, sandbox, usage};

impl Eggbot {
    /// Drops one message the user queued. `at` indexes the bot's whole queue; other kinds of queued work stay.
    pub(crate) fn unqueue(&mut self, id: usize, at: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bot_mut(id) else { return };
        if bot.queue.get(at).is_some_and(|q| q.typed) {
            bot.queue.remove(at);
            self.save();
            cx.notify();
        }
    }

    /// Starts the turn now, or appends it to the persisted queue (busy, already queued, or over the usage limit).
    pub(crate) fn deliver(&mut self, id: usize, pending: handoff::Pending, cx: &mut Context<Self>) {
        let Some((provider, waiting)) = self.bot(id).map(|b| (b.provider, b.busy() || !b.queue.is_empty())) else { return };
        let wait = waiting || !self.may_start(provider);
        if wait {
            if let Some(b) = self.bot_mut(id) {
                b.queue.push(pending);
            }
            self.save();
            cx.notify();
            return;
        }
        self.start_turn(id, pending, cx);
    }

    /// Starts the head of each idle bot's queue once Docker is up and the usage guard allows it.
    /// Offline, paused, or throttled-behind-another-bot: the queue is not touched.
    pub(crate) fn pump_queues(&mut self, online: bool, prefer: Option<usize>, cx: &mut Context<Self>) {
        let mut ids: Vec<usize> = self.bots.iter().map(|b| b.id).collect();
        if let Some(id) = prefer {
            ids.retain(|i| *i != id);
            ids.insert(0, id);
        }
        let mut starts = vec![];
        for id in ids {
            let Some((provider, busy)) = self.bot(id).map(|b| (b.provider, b.busy())) else { continue };
            if !self.may_start(provider) {
                continue;
            }
            let Some(bot) = self.bot_mut(id) else { continue };
            let (next, queue) = handoff::dequeue(std::mem::take(&mut bot.queue), busy, !online);
            bot.queue = queue;
            if let Some(pending) = next {
                starts.push((id, pending));
            }
        }
        for (id, pending) in starts {
            let Some(provider) = self.bot(id).map(|b| b.provider) else { continue };
            // a sibling may have started in this loop; put the hop back rather than dropping it
            if !self.may_start(provider) {
                if let Some(b) = self.bot_mut(id) {
                    b.queue.insert(0, pending);
                }
                self.save();
                continue;
            }
            self.start_turn(id, pending, cx);
        }
    }

    /// Docker check off the UI thread, then start whatever the guard now allows.
    pub(crate) fn resume_queues(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let online = cx.background_executor().spawn(async { sandbox::running() }).await;
            this.update(cx, |this, cx| this.pump_queues(online, None, cx)).ok();
        })
        .detach();
    }

    /// Sends the finished reply to every bot it mentions as @Name; false when it mentions nobody.
    /// `room` is kept only for targets still on that room's roster (`room::carry`).
    pub(crate) fn hand_off(&mut self, from: usize, reply: String, hops: u32, room: Option<usize>, cx: &mut Context<Self>) -> bool {
        let names: Vec<(usize, &str)> = self.bots.iter().map(|b| (b.id, b.name.as_str())).collect();
        let targets = handoff::mentions(&reply, &names, from);
        let handed = !targets.is_empty();
        let Some(sender) = self.bot(from) else { return false };
        let (from_name, color) = (sender.name.clone(), sender.color());
        let from_mounts: Vec<(PathBuf, String)> = sender.folders.iter().map(|f| (f.path.clone(), f.dest())).collect();
        let next = hops + 1;
        for to in targets {
            let stays = self.rooms.iter().find(|r| Some(r.id) == room).and_then(|r| room::carry(room, &r.members, to));
            let Some(target) = self.bot_mut(to) else { continue };
            let to_mounts: Vec<(PathBuf, String)> = target.folders.iter().map(|f| (f.path.clone(), f.dest())).collect();
            let mine: Vec<(&std::path::Path, &str)> = from_mounts.iter().map(|(p, d)| (p.as_path(), d.as_str())).collect();
            let theirs: Vec<(&std::path::Path, &str)> = to_mounts.iter().map(|(p, d)| (p.as_path(), d.as_str())).collect();
            let prompt = handoff::prompt(&from_name, &reply, &mine, &theirs);
            let paused = next > handoff::MAX_HOPS;
            let to_name = target.name.clone();
            target.msgs.push(Msg::Handoff { from: from_name.clone(), color, prompt: prompt.clone(), text: reply.clone(), paused, open: false, room: stays });
            if let Some(s) = self.bot_mut(from) {
                s.msgs.push(Msg::Sent { to: to_name.clone() });
                if !paused {
                    s.recent = handoff::remember(std::mem::take(&mut s.recent), &to_name, 4);
                }
            }
            if let Some(rid) = stays {
                self.log_room(rid, |r| {
                    r.record_handoff(from, &from_name, color, to, &to_name, paused);
                    true
                });
            }
            if paused {
                self.alert(to, "Chain paused", &format!("{from_name} handed off to {to_name} after {} hops. Open eggbot to continue.", handoff::MAX_HOPS));
            } else {
                let mut pending = handoff::Pending::handoff(prompt, next);
                pending.room = stays;
                self.deliver(to, pending, cx);
            }
        }
        handed
    }

    /// Adds a transcript line through `record`; the room dot lights when a line was added while the room is closed.
    pub(crate) fn log_room(&mut self, room_id: usize, record: impl FnOnce(&mut room::Room) -> bool) {
        let open = self.open_room == Some(room_id);
        if let Some(room) = self.room_mut(room_id)
            && record(room)
            && !open
        {
            room.unread = true;
        }
    }

    pub(crate) fn continue_chain(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bot(id) else { return };
        // the hop button does not override a full plan window; the banner says why
        if self.breach_of(bot.provider).is_some_and(|b| b.level == usage::Level::Pause) {
            cx.notify();
            return;
        }
        let Some(bot) = self.bot_mut(id) else { return };
        let resumed = match bot.msgs.get_mut(i) {
            Some(Msg::Handoff { prompt, paused, room, .. }) if *paused => {
                *paused = false;
                Some((prompt.clone(), *room))
            }
            _ => None,
        };
        let Some((prompt, room_id)) = resumed else { return };
        if let Some(rid) = room_id
            && let Some(room) = self.room_mut(rid)
        {
            room.resume(id);
        }
        let mut pending = handoff::Pending::handoff(prompt, 0);
        pending.room = room_id;
        self.deliver(id, pending, cx);
        self.save();
        cx.notify();
    }

    /// Continues the paused in-room handoff that landed on `bot_id`.
    pub(crate) fn continue_room(&mut self, room_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(i) = self.bot(bot_id).and_then(|b| b.msgs.iter().rposition(|m| matches!(m, Msg::Handoff { paused: true, room: Some(rid), .. } if *rid == room_id))) else {
            return;
        };
        self.continue_chain(bot_id, i, cx);
    }
}
