mod app;
mod claude;
mod codex;
mod egg;
mod group;
mod handoff;
mod login;
mod memory;
mod notify;
mod room;
mod sandbox;
mod schedule;
mod skills;
mod tray;
mod ui;
mod usage;

use app::bot::{Bot, Msg};
use app::state::data_dir;
use app::{Eggbot, Panel};
use claude::{Meter, Provider};
use gpui_kit::assets::Assets;
use gpui_kit::component::Theme;
use gpui_kit::component::select::SelectItem;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

actions!(eggbot, [Quit, CloseWindow, NewBot, FocusInput, PrevBot, NextBot, StopTurn, Dismiss, CycleAppearance, OpenSettings, OpenSetup, Find, ToggleSidebar]);

/// ⌘1…⌘9 selects the bot at that position.
#[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = eggbot)]
struct SelectBot(usize);

/// Light or dark, chosen in the View menu.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = eggbot)]
enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }
}

// the default bundle has only the component icons; add the extra ones we use
gpui_kit::assets::icon_assets!(ExtraIcons, [Clock, Trash, Pencil, CircleCheck, CircleDashed]);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Hsla,
    /// Translucent: the window behind it is blurred (vibrancy).
    side: Hsla,
    card: Hsla,
    ink: Hsla,
    muted: Hsla,
    line: Hsla,
    hover: Hsla,
    /// Selected row and meter tracks: readable over the frosted sidebar.
    tint: Hsla,
    bubble: Hsla,
    ok: Hsla,
    warn: Hsla,
    err: Hsla,
}

impl Palette {
    fn light() -> Self {
        Self {
            bg: hex(0xFFFFFF),
            side: hsla(0., 0., 1., 0.45),
            card: hex(0xFFFFFF),
            ink: hex(0x0D0D0D),
            muted: hex(0x6B6B6B),
            line: hex(0xE5E5E5),
            hover: hsla(0., 0., 0., 0.05),
            tint: hsla(0., 0., 0., 0.09),
            bubble: hex(0xF4F4F4),
            ok: hex(0x16A34A),
            warn: hex(0xD97706),
            err: hex(0xDC2626),
        }
    }

    fn dark() -> Self {
        Self {
            bg: hex(0x0F0F0F),
            side: hsla(0., 0., 0.05, 0.25),
            card: hex(0x171717),
            ink: hex(0xECECEC),
            muted: hex(0xA3A3A3),
            line: hex(0x2A2A2A),
            hover: hsla(0., 0., 1., 0.06),
            tint: hsla(0., 0., 1., 0.11),
            bubble: hex(0x1F1F1F),
            ok: hex(0x4ADE80),
            warn: hex(0xFBBF24),
            err: hex(0xF87171),
        }
    }

    /// Follows the macOS appearance and pushes our colors into gpui-component.
    fn apply(window: &mut Window, cx: &mut App) -> Self {
        Theme::sync_system_appearance(Some(window), cx);
        let p = if Theme::global(cx).is_dark() { Self::dark() } else { Self::light() };
        // separate update: the mode switch above reloads the stock colors
        Theme::update(cx, |t| {
            // gpui-component's root paints this over the whole window; clear it so the sidebar blur shows
            t.background = transparent_black();
            t.foreground = p.ink;
            t.border = p.line;
            t.input = p.line;
            t.primary = p.ink;
            t.primary_foreground = p.bg;
            t.ring = p.muted;
            t.caret = p.ink;
            t.selection = p.muted.opacity(0.3);
            t.muted = p.bubble;
            t.muted_foreground = p.muted;
            t.accent = p.hover;
            t.popover = p.card;
            t.list_hover = p.hover;
            t.list_active = p.hover;
        });
        p
    }
}

const QUIET: &str = "\n\n(This is a scheduled run. If nothing here needs the user's attention, reply with exactly QUIET and nothing else.)";

/// A dropdown option: what is shown, and what is stored (None = the provider's default).
#[derive(Clone)]
struct Choice {
    value: Option<String>,
    label: SharedString,
}

impl SelectItem for Choice {
    type Value = Option<String>;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

const MODELS: [(Option<&str>, &str); 5] = [(None, "Default"), (Some("fable"), "Fable"), (Some("opus"), "Opus"), (Some("sonnet"), "Sonnet"), (Some("haiku"), "Haiku")];

impl Eggbot {
    /// Forces the whole app light or dark (vibrancy, menus and popovers follow) and refreshes the View menu.
    fn set_appearance(&mut self, appearance: Appearance, window: &mut Window, cx: &mut Context<Self>) {
        use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
        self.appearance = appearance;
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            let name = match appearance {
                Appearance::System => None,
                Appearance::Light => Some(unsafe { NSAppearanceNameAqua }),
                Appearance::Dark => Some(unsafe { NSAppearanceNameDarkAqua }),
            };
            let look = name.and_then(NSAppearance::appearanceNamed);
            NSApplication::sharedApplication(mtm).setAppearance(look.as_deref());
        }
        self.p = Palette::apply(window, cx);
        set_menus(appearance, self.sidebar_open, cx);
        self.save();
        cx.notify();
    }

    /// ⌘B hides or shows the bot list. The width stays, and the choice is saved.
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        self.resizing = false;
        set_menus(self.appearance, self.sidebar_open, cx);
        self.save();
        cx.notify();
    }

    fn select(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
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
    fn sync_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn sync_list(&mut self) {
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

    /// Keeps the room list in step with the transcript plus any member still writing this room's turn.
    fn sync_room_list(&mut self) {
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
    fn room_live(&self, room_id: usize) -> Vec<usize> {
        let mut ids: Vec<usize> = self.bots.iter().filter(|b| b.busy() && b.current.as_ref().is_some_and(|p| p.room == Some(room_id))).map(|b| b.id).collect();
        ids.sort_unstable();
        ids
    }

    fn open_bot(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.bots.iter().position(|b| b.id == id) {
            self.select(i, window, cx);
        }
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;
        self.find_input.update(cx, |s, cx| s.focus(window, cx));
        self.find_update(cx);
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        self.find_hits.clear();
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// Matches the query (any case) against the open chat and jumps to the newest match.
    fn find_update(&mut self, cx: &mut Context<Self>) {
        let query = self.find_input.read(cx).value().trim().to_lowercase();
        self.find_hits = match self.bots.get(self.selected) {
            Some(bot) if !query.is_empty() => bot.msgs.iter().enumerate().filter(|(_, m)| m.searchable().is_some_and(|t| t.to_lowercase().contains(&query))).map(|(i, _)| i).collect(),
            _ => vec![],
        };
        self.find_at = self.find_hits.len().saturating_sub(1);
        self.find_reveal(cx);
    }

    /// Moves to the previous (-1) or next (1) match, wrapping around.
    fn find_step(&mut self, dir: isize, cx: &mut Context<Self>) {
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
    fn find_current(&self) -> Option<usize> {
        self.find_open.then(|| self.find_hits.get(self.find_at).copied()).flatten()
    }

    /// The selected bot has been seen. A room dot clears once none of its bots are still unread.
    fn mark_read(&mut self, cx: &mut Context<Self>) {
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
    fn alert(&mut self, id: usize, title: &str, body: &str) {
        let seeing = self.open_room.is_none() && self.active && self.bots.get(self.selected).is_some_and(|b| b.id == id);
        if !seeing && let Some(b) = self.bots.iter_mut().find(|b| b.id == id) {
            b.unread = true;
        }
        if !self.active {
            let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
            notify::send(id, title, &body);
        }
    }

    /// Starts due schedules the usage guard is willing to run. Held ones keep their anchor.
    fn run_due(&mut self, cx: &mut Context<Self>) {
        let before = self.guard_levels();
        let started = self.release_schedules(cx);
        if started || before != self.guard_levels() {
            cx.notify();
        }
    }

    /// Moves due schedules onto a bot when the guard allows. Held ones keep their anchor, so they stay due.
    fn release_schedules(&mut self, cx: &mut Context<Self>) -> bool {
        let now = chrono::Local::now();
        let levels = [Provider::Claude, Provider::Codex].map(|p| (p, self.breach_of(p).map(|b| b.level).unwrap_or(usage::Level::Ok)));
        let mut taken: Vec<Provider> = self.bots.iter().filter(|b| b.busy()).map(|b| b.provider).collect();
        for b in &self.bots {
            if !b.queue.is_empty() && !taken.contains(&b.provider) {
                taken.push(b.provider);
            }
        }
        let mut due = vec![];
        for b in &mut self.bots {
            let level = levels.iter().find(|(p, _)| *p == b.provider).map(|(_, l)| *l).unwrap_or(usage::Level::Ok);
            let provider_taken = taken.contains(&b.provider);
            if !usage::schedule_action(level, b.busy(), !b.queue.is_empty(), provider_taken) {
                continue;
            }
            let mut fired = false;
            for s in b.schedules.iter_mut().filter(|s| s.due(now)) {
                s.anchor = now.timestamp();
                b.msgs.push(Msg::Scheduled { prompt: s.prompt.clone(), label: s.repeat.label() });
                due.push((b.id, format!("{}{QUIET}", s.prompt)));
                fired = true;
            }
            if fired && level == usage::Level::Throttle && !taken.contains(&b.provider) {
                taken.push(b.provider);
            }
        }
        if due.is_empty() {
            return false;
        }
        for (id, prompt) in due {
            self.deliver(id, handoff::Pending::schedule(prompt), cx);
        }
        self.save();
        true
    }

    /// Closing a room leaves its dot off. Opening it already counted as reading the transcript.
    fn leave_room(&mut self) {
        if self.open_room.take().is_some() {
            self.confirm_delete_room = None;
        }
    }

    fn show_room(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    fn new_room(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let taken = |n: &str| self.rooms.iter().any(|r| r.title == n);
        let title = (1..).map(|i| if i == 1 { "Room".to_string() } else { format!("Room {i}") }).find(|n| !taken(n)).unwrap();
        let id = self.next_room_id;
        self.next_room_id += 1;
        self.rooms.push(room::Room::new(id, title));
        self.show_room(id, window, cx);
    }

    fn room_title_changed(&mut self, cx: &mut Context<Self>) {
        self.write_room(true, cx);
    }

    fn room_kickoff_changed(&mut self, cx: &mut Context<Self>) {
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

    fn toggle_member(&mut self, room_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id) else { return };
        (room.members, room.facilitator) = room::toggle(std::mem::take(&mut room.members), room.facilitator, bot_id);
        (self.room_error, self.room_status, self.confirm_delete_room) = (None, None, None);
        self.save();
        cx.notify();
    }

    fn set_facilitator(&mut self, room_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id) else { return };
        (room.members, room.facilitator) = room::facilitate(std::mem::take(&mut room.members), bot_id);
        (self.room_error, self.room_status) = (None, None);
        self.save();
        cx.notify();
    }

    fn delete_room(&mut self, id: usize, cx: &mut Context<Self>) {
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

    fn leave_group(&mut self) {
        if self.open_group.take().is_some() {
            self.confirm_delete_group = None;
        }
    }

    fn show_group(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    fn new_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let taken = |n: &str| self.groups.iter().any(|g| g.title == n);
        let title = (1..).map(|i| if i == 1 { "Group".to_string() } else { format!("Group {i}") }).find(|n| !taken(n)).unwrap();
        let id = self.next_group_id;
        self.next_group_id += 1;
        self.groups.push(group::Group::new(id, title));
        self.save();
        self.show_group(id, window, cx);
    }

    fn group_title_changed(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.open_group else { return };
        let value = self.group_title.read(cx).value().to_string();
        let Some(group) = self.groups.iter_mut().find(|g| g.id == id) else { return };
        group.title = value;
        self.confirm_delete_group = None;
        self.save();
        cx.notify();
    }

    fn toggle_group_member(&mut self, group_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(group) = self.groups.iter_mut().find(|g| g.id == group_id) else { return };
        group.members = group::toggle(std::mem::take(&mut group.members), bot_id);
        self.confirm_delete_group = None;
        self.save();
        cx.notify();
    }

    fn delete_group(&mut self, id: usize, cx: &mut Context<Self>) {
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
    fn start_room(&mut self, cx: &mut Context<Self>) {
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

    fn focus_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_group.is_some() {
            self.group_title.update(cx, |s, cx| s.focus(window, cx));
        } else if self.open_room.is_some() {
            self.room_kickoff.update(cx, |s, cx| s.focus(window, cx));
        } else {
            self.input.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    fn set_meter(&mut self, meter: Meter, cx: &mut Context<Self>) {
        let provider = meter.provider;
        let before = self.breach_of(provider).map(|b| b.level);
        self.meters.retain(|m| m.provider != meter.provider);
        self.meters.push(meter);
        self.meters.sort_by_key(|m| m.provider.label());
        let after = self.breach_of(provider);
        if after.as_ref().is_some_and(|b| b.level == usage::Level::Pause) && before != Some(usage::Level::Pause) {
            self.announce_pause(provider, after.as_ref().unwrap());
        }
        self.release_schedules(cx);
        self.resume_queues(cx);
        self.save();
    }

    fn announce_pause(&mut self, provider: Provider, breach: &usage::Breach) {
        let Some(id) = self.bots.iter().find(|b| b.provider == provider).map(|b| b.id) else { return };
        let text = usage::explain(usage::provider_name(provider), breach, self.pause, self.throttle);
        self.alert(id, &format!("{} usage paused", usage::provider_name(provider)), &text);
    }

    fn breach_of(&self, provider: Provider) -> Option<usage::Breach> {
        let now = chrono::Local::now().timestamp();
        self.meters.iter().find(|m| m.provider == provider).and_then(|m| usage::breach(&m.windows, now, self.throttle, self.pause))
    }

    /// Pause blocks every new turn. Throttle blocks a second bot of the same provider.
    fn may_start(&self, provider: Provider) -> bool {
        match self.breach_of(provider).map(|b| b.level) {
            Some(usage::Level::Pause) => false,
            Some(usage::Level::Throttle) => !self.bots.iter().any(|b| b.provider == provider && b.busy()),
            _ => true,
        }
    }

    fn guard_levels(&self) -> [usage::Level; 2] {
        [Provider::Claude, Provider::Codex].map(|p| self.breach_of(p).map(|b| b.level).unwrap_or(usage::Level::Ok))
    }

    /// Due schedules of this bot are sitting out a throttle or a pause.
    fn schedules_wait(&self, bot: &Bot) -> bool {
        let Some(breach) = self.breach_of(bot.provider) else { return false };
        let provider_taken = self.bots.iter().any(|b| b.id != bot.id && b.provider == bot.provider && (b.busy() || !b.queue.is_empty()));
        !usage::schedule_action(breach.level, bot.busy(), !bot.queue.is_empty(), provider_taken)
    }
}

/// Puts macOS's "sidebar" material behind the whole window; eggbot's opaque main area paints over it.
fn add_vibrancy(window: &Window) {
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let (Some(mtm), Ok(handle)) = (objc2::MainThreadMarker::new(), HasWindowHandle::window_handle(window)) else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: GPUI's AppKit handle points at its live NSView, and we are on the main thread
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let Some(parent) = (unsafe { view.superview() }) else { return };
    let effect = NSVisualEffectView::initWithFrame(mtm.alloc(), parent.bounds());
    effect.setMaterial(NSVisualEffectMaterial::Sidebar);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::FollowsWindowActiveState);
    effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    parent.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, Some(view));
}

fn set_menus(appearance: Appearance, sidebar_open: bool, cx: &mut App) {
    let pick = |name: &str, a: Appearance| MenuItem::Action { name: name.to_string().into(), action: Box::new(a), os_action: None, checked: a == appearance, disabled: false };
    cx.set_menus([
        Menu {
            name: "eggbot".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::action("Setup…", OpenSetup),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindow),
                MenuItem::action("Quit eggbot", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::Action { name: "Toggle Sidebar".to_string().into(), action: Box::new(ToggleSidebar), os_action: None, checked: sidebar_open, disabled: false },
                MenuItem::separator(),
                pick("Match System", Appearance::System),
                pick("Light", Appearance::Light),
                pick("Dark", Appearance::Dark),
                MenuItem::separator(),
                MenuItem::action("Next Appearance", CycleAppearance),
            ],
            disabled: false,
        },
    ]);
}

/// The Dock icon shows only while the window is visible; the menu bar egg is always there.
fn set_dock_icon(visible: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let policy = if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
        NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
    }
}

#[cfg(test)]
mod persist_tests {

    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
}

fn main() {
    // apps opened from Finder get a bare PATH; docker lives in Homebrew, /usr/local/bin or OrbStack's own folder
    let (path, home) = (std::env::var("PATH").unwrap_or_default(), std::env::var("HOME").unwrap_or_default());
    // SAFETY: still single-threaded, before GPUI starts
    unsafe { std::env::set_var("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{home}/.orbstack/bin:{path}")) };
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        gpui_kit::init(cx);
        // opened by macOS at login: start quietly, with only the menu bar egg
        let at_login = login::launched_at_login();
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-n", NewBot, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("cmd-k", FocusInput, None),
            // ⌘[ / ⌘] are outdent/indent inside text fields, so switching uses ⌃Tab
            KeyBinding::new("ctrl-shift-tab", PrevBot, None),
            KeyBinding::new("ctrl-tab", NextBot, None),
            KeyBinding::new("escape", Dismiss, None),
            KeyBinding::new("cmd-.", StopTurn, None),
            KeyBinding::new("cmd-shift-d", CycleAppearance, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("cmd-f", Find, None),
        ]);
        cx.bind_keys((1..=9).map(|n| KeyBinding::new(&format!("cmd-{n}"), SelectBot(n - 1), None)));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1080.), px(720.)), cx))),
            // transparent, with a native vibrancy view behind (GPUI's own Blurred has no effect here)
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: Some(TitlebarOptions { title: Some("eggbot".into()), appears_transparent: true, traffic_light_position: Some(point(px(16.), px(16.))) }),
            show: !at_login,
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            add_vibrancy(window);
            cx.new(|cx| app::Eggbot::new(window, cx))
        })
        .unwrap();
        if at_login {
            set_dock_icon(false);
        } else {
            cx.activate(true);
        }
    });
}

#[cfg(test)]
mod tests {}
