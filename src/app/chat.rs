//! The open chat: which bot is selected, composer drafts, the message list, search, and unread marks and alerts.

use gpui_kit::*;

use super::{Eggbot, Panel};
use crate::notify;

impl Eggbot {
    pub(crate) fn select(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_room();
        self.leave_group();
        self.selected = i;
        self.mark_read(cx);
        self.selects_stale = true;
        self.menu_open = false;
        self.confirm_delete = None;
        self.panel = Panel::None;
        self.folder_error = None;
        (self.find_open, self.find_hits) = (false, vec![]);
        self.list.scroll_to_end();
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// Puts the composer text back on the bot it was typed for and loads the selected bot's draft.
    /// Runs in render and before a send, so every way of changing the selection is covered.
    pub(crate) fn sync_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.bots.get(self.selected).map(|b| b.id);
        if now == self.draft_bot {
            return;
        }
        let (old, text) = (self.draft_bot, self.input.read(cx).value().to_string());
        if let Some(b) = self.bots.iter_mut().find(|b| Some(b.id) == old) {
            b.draft = text;
        }
        let next = self.bots.get_mut(self.selected).map(|b| std::mem::take(&mut b.draft)).unwrap_or_default();
        self.input.update(cx, |s, cx| s.set_value(next, window, cx));
        self.draft_bot = now;
    }

    /// Tells the chat list about added or removed messages; visible rows re-measure themselves every frame.
    pub(crate) fn sync_list(&mut self) {
        let Some(bot) = self.bots.get(self.selected) else { return };
        let (count, old) = (bot.msgs.len() + 1, self.list.item_count());
        if self.list_bot != Some(bot.id) {
            self.list_bot = Some(bot.id);
            self.list.reset(count);
            self.list.scroll_to_end();
        } else if count > old {
            // new messages go before the typing row
            self.list.splice(old - 1..old - 1, count - old);
        } else if count < old {
            // ponytail: removals are assumed at the tail (empty replies, quiet runs); reset if middle removals appear
            self.list.splice(count - 1..old - 1, 0);
        }
    }

    pub(crate) fn open_bot(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.bots.iter().position(|b| b.id == id) {
            self.select(i, window, cx);
        }
    }

    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;
        self.find_input.update(cx, |s, cx| s.focus(window, cx));
        self.find_update(cx);
    }

    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        self.find_hits.clear();
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// Matches the query (any case) against the open chat and jumps to the newest match.
    pub(crate) fn find_update(&mut self, cx: &mut Context<Self>) {
        let query = self.find_input.read(cx).value().trim().to_lowercase();
        self.find_hits = match self.bots.get(self.selected) {
            Some(bot) if !query.is_empty() => bot.msgs.iter().enumerate().filter(|(_, m)| m.searchable().is_some_and(|t| t.to_lowercase().contains(&query))).map(|(i, _)| i).collect(),
            _ => vec![],
        };
        self.find_at = self.find_hits.len().saturating_sub(1);
        self.find_reveal(cx);
    }

    /// Moves to the previous (-1) or next (1) match, wrapping around.
    pub(crate) fn find_step(&mut self, dir: isize, cx: &mut Context<Self>) {
        if !self.find_hits.is_empty() {
            self.find_at = (self.find_at as isize + dir).rem_euclid(self.find_hits.len() as isize) as usize;
            self.find_reveal(cx);
        }
    }

    fn find_reveal(&mut self, cx: &mut Context<Self>) {
        if let Some(&ix) = self.find_hits.get(self.find_at) {
            // following the tail would snap back to the end; it resumes once you scroll to the bottom
            self.list.pause_following_tail();
            self.list.scroll_to_reveal_item(ix);
        }
        cx.notify();
    }

    /// The message search is pointing at, if any.
    pub(crate) fn find_current(&self) -> Option<usize> {
        self.find_open.then(|| self.find_hits.get(self.find_at).copied()).flatten()
    }

    /// The selected bot has been seen. A room dot clears once none of its bots are still unread.
    pub(crate) fn mark_read(&mut self, cx: &mut Context<Self>) {
        if self.open_room.is_some() || self.open_group.is_some() {
            return;
        }
        let Some(b) = self.bots.get_mut(self.selected).filter(|b| b.unread) else { return };
        b.unread = false;
        self.clear_idle_rooms();
        self.save();
        cx.notify();
    }

    /// Drops a room dot when the news was read in a member's chat.
    fn clear_idle_rooms(&mut self) {
        let unread: Vec<usize> = self.bots.iter().filter(|b| b.unread).map(|b| b.id).collect();
        for room in &mut self.rooms {
            if room.unread && !room.members.iter().any(|id| unread.contains(id)) {
                room.unread = false;
            }
        }
    }

    /// News from a bot: unread unless the user is looking at it, and a notification while eggbot is in the background.
    /// A room's own dot is its transcript, not every message a member receives.
    pub(crate) fn alert(&mut self, id: usize, title: &str, body: &str) {
        let seeing = self.open_room.is_none() && self.active && self.bots.get(self.selected).is_some_and(|b| b.id == id);
        if !seeing && let Some(b) = self.bot_mut(id) {
            b.unread = true;
        }
        if !self.active {
            let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
            notify::send(id, title, &body);
        }
    }

    pub(crate) fn focus_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_group.is_some() {
            self.group_title.update(cx, |s, cx| s.focus(window, cx));
        } else if self.open_room.is_some() {
            self.room_kickoff.update(cx, |s, cx| s.focus(window, cx));
        } else {
            self.input.update(cx, |s, cx| s.focus(window, cx));
        }
    }
}
