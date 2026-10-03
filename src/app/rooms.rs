//! Rooms and groups: open, edit, start, and delete them, and keep the room transcript list in step.

use gpui_kit::*;

use super::bot::Msg;
use super::state::data_dir;
use super::{Eggbot, Panel};
use crate::claude::Provider;
use crate::{group, handoff, room};

impl Eggbot {
    /// Keeps the room list in step with the transcript plus any member still writing this room's turn.
    pub(crate) fn sync_room_list(&mut self) {
        let Some(id) = self.open_room else {
            self.room_list_for = None;
            return;
        };
        let n = self.rooms.iter().find(|r| r.id == id).map(|r| r.transcript.len()).unwrap_or(0);
        let count = n + self.room_live(id).len();
        if self.room_list_for != Some(id) {
            self.room_list_for = Some(id);
            self.room_list.reset(count);
            if count > 0 {
                self.room_list.scroll_to_end();
            }
            return;
        }
        let old = self.room_list.item_count();
        if count > old {
            self.room_list.splice(old..old, count - old);
        } else if count < old {
            self.room_list.splice(count..old, 0);
        }
    }

    /// Members whose running turn belongs to this room, in a stable order.
    pub(crate) fn room_live(&self, room_id: usize) -> Vec<usize> {
        let mut ids: Vec<usize> = self.bots.iter().filter(|b| b.busy() && b.current.as_ref().is_some_and(|p| p.room == Some(room_id))).map(|b| b.id).collect();
        ids.sort_unstable();
        ids
    }

    /// Closing a room leaves its dot off. Opening it already counted as reading the transcript.
    pub(crate) fn leave_room(&mut self) {
        if self.open_room.take().is_some() {
            self.confirm_delete_room = None;
        }
    }

    pub(crate) fn show_room(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_room == Some(id) {
            return;
        }
        let Some(room) = self.rooms.iter().find(|r| r.id == id) else { return };
        let (title, kickoff) = (room.title.clone(), room.kickoff.clone());
        self.leave_group();
        self.leave_room();
        if let Some(room) = self.rooms.iter_mut().find(|r| r.id == id) {
            room.unread = false;
        }
        self.open_room = Some(id);
        (self.room_error, self.room_status, self.confirm_delete_room) = (None, None, None);
        (self.menu_open, self.panel) = (false, Panel::None);
        (self.find_open, self.find_hits) = (false, vec![]);
        self.room_title.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        self.room_kickoff.update(cx, |s, cx| s.set_value(kickoff, window, cx));
        self.save();
        cx.notify();
    }

    pub(crate) fn new_room(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let taken = |n: &str| self.rooms.iter().any(|r| r.title == n);
        let title = (1..).map(|i| if i == 1 { "Room".to_string() } else { format!("Room {i}") }).find(|n| !taken(n)).unwrap();
        let id = self.next_room_id;
        self.next_room_id += 1;
        self.rooms.push(room::Room::new(id, title));
        self.show_room(id, window, cx);
    }

    pub(crate) fn room_title_changed(&mut self, cx: &mut Context<Self>) {
        self.write_room(true, cx);
    }

    pub(crate) fn room_kickoff_changed(&mut self, cx: &mut Context<Self>) {
        self.write_room(false, cx);
    }

    fn write_room(&mut self, title: bool, cx: &mut Context<Self>) {
        let Some(id) = self.open_room else { return };
        let value = if title { self.room_title.read(cx).value().to_string() } else { self.room_kickoff.read(cx).value().to_string() };
        let Some(room) = self.rooms.iter_mut().find(|r| r.id == id) else { return };
        if title {
            room.title = value;
        } else {
            room.kickoff = value;
        }
        (self.room_error, self.room_status, self.confirm_delete_room) = (None, None, None);
        self.save();
        cx.notify();
    }

    pub(crate) fn toggle_member(&mut self, room_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id) else { return };
        (room.members, room.facilitator) = room::toggle(std::mem::take(&mut room.members), room.facilitator, bot_id);
        (self.room_error, self.room_status, self.confirm_delete_room) = (None, None, None);
        self.save();
        cx.notify();
    }

    pub(crate) fn set_facilitator(&mut self, room_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id) else { return };
        (room.members, room.facilitator) = room::facilitate(std::mem::take(&mut room.members), bot_id);
        (self.room_error, self.room_status) = (None, None);
        self.save();
        cx.notify();
    }

    pub(crate) fn delete_room(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.confirm_delete_room != Some(id) {
            self.confirm_delete_room = Some(id);
            cx.notify();
            return;
        }
        self.rooms.retain(|r| r.id != id);
        if self.open_room == Some(id) {
            self.open_room = None;
        }
        self.confirm_delete_room = None;
        self.save();
        let dir = data_dir().join("rooms").join(id.to_string());
        cx.background_executor()
            .spawn(async move {
                let _ = std::fs::remove_dir_all(dir);
            })
            .detach();
        cx.notify();
    }

    pub(crate) fn leave_group(&mut self) {
        if self.open_group.take().is_some() {
            self.confirm_delete_group = None;
        }
    }

    pub(crate) fn show_group(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_group == Some(id) {
            return;
        }
        let Some(title) = self.groups.iter().find(|g| g.id == id).map(|g| g.title.clone()) else { return };
        self.leave_room();
        self.open_group = Some(id);
        self.confirm_delete_group = None;
        (self.menu_open, self.panel) = (false, Panel::None);
        (self.find_open, self.find_hits) = (false, vec![]);
        self.group_title.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn new_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let taken = |n: &str| self.groups.iter().any(|g| g.title == n);
        let title = (1..).map(|i| if i == 1 { "Group".to_string() } else { format!("Group {i}") }).find(|n| !taken(n)).unwrap();
        let id = self.next_group_id;
        self.next_group_id += 1;
        self.groups.push(group::Group::new(id, title));
        self.save();
        self.show_group(id, window, cx);
    }

    pub(crate) fn group_title_changed(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.open_group else { return };
        let value = self.group_title.read(cx).value().to_string();
        let Some(group) = self.groups.iter_mut().find(|g| g.id == id) else { return };
        group.title = value;
        self.confirm_delete_group = None;
        self.save();
        cx.notify();
    }

    pub(crate) fn toggle_group_member(&mut self, group_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(group) = self.groups.iter_mut().find(|g| g.id == group_id) else { return };
        group.members = group::toggle(std::mem::take(&mut group.members), bot_id);
        self.confirm_delete_group = None;
        self.save();
        cx.notify();
    }

    pub(crate) fn delete_group(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.confirm_delete_group != Some(id) {
            self.confirm_delete_group = Some(id);
            cx.notify();
            return;
        }
        self.groups.retain(|g| g.id != id);
        if self.open_group == Some(id) {
            self.open_group = None;
        }
        self.confirm_delete_group = None;
        self.save();
        let dir = data_dir().join("groups").join(id.to_string());
        cx.background_executor()
            .spawn(async move {
                let _ = std::fs::remove_dir_all(dir);
            })
            .detach();
        cx.notify();
    }

    /// Sends the kickoff to the facilitator only. Peers join later through `@Name`.
    pub(crate) fn start_room(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.open_room else { return };
        let Some(room) = self.rooms.iter().find(|r| r.id == id) else { return };
        let title = room.title.trim().to_string();
        let kickoff = room.kickoff.trim().to_string();
        if let Some(why) = room::block(&title, &kickoff, &room.members, room.facilitator) {
            self.room_error = Some(why.into());
            self.room_status = None;
            cx.notify();
            return;
        }
        let facilitator = room.facilitator.unwrap();
        let members = room.members.clone();
        let peers: Vec<(String, String)> = self.bots.iter().filter(|b| b.id != facilitator && members.contains(&b.id)).map(|b| (b.name.clone(), b.blurb())).collect();
        let peer_refs: Vec<room::Peer<'_>> = peers.iter().map(|(name, blurb)| room::Peer { name, blurb }).collect();
        let prompt = room::prompt(&title, &kickoff, &peer_refs);
        let Some(name) = self.bots.iter().find(|b| b.id == facilitator).map(|b| b.name.clone()) else {
            self.room_error = Some("That facilitator was deleted".into());
            cx.notify();
            return;
        };
        let facilitator_bot = self.bots.iter().find(|b| b.id == facilitator);
        let (busy, queued, provider) = facilitator_bot.map(|b| (b.busy(), !b.queue.is_empty(), b.provider)).unwrap_or((false, false, Provider::Claude));
        // deliver() is the only door: pause, throttle, and a busy bot all stay on the persisted queue
        let held = !self.may_start(provider);
        if let Some(bot) = self.bots.iter_mut().find(|b| b.id == facilitator) {
            bot.msgs.push(Msg::Kickoff { room_id: id, room: title, text: kickoff.clone(), prompt: prompt.clone() });
        }
        if let Some(room) = self.rooms.iter_mut().find(|r| r.id == id) {
            room.record_kickoff(facilitator, &name, &kickoff);
            room.started = true;
            room.unread = false;
        }
        self.room_error = None;
        self.room_status = Some(if held {
            format!("{name}'s plan is at its limit, so the kickoff waits in their queue.")
        } else if busy || queued {
            format!("{name} is busy, so the kickoff waits in their queue.")
        } else {
            format!("Kickoff sent to {name}.")
        });
        // hop 0 so this round does not spend the chain limit; a quit resumes it like any other handoff
        let mut pending = handoff::Pending::handoff(prompt, 0);
        pending.room = Some(id);
        self.deliver(facilitator, pending, cx);
    }
}
