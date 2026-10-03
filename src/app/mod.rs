//! The Eggbot view state: every field, construction (inputs, subscriptions, loops, loading state), show and quit.

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::*;

use crate::claude::{Meter, Provider};
use crate::ui::composer::Choice;
use crate::ui::theme::{Appearance, Palette};
use crate::{codex, group, notify, room, sandbox, set_dock_icon, tray, usage};
use bot::{Bot, Msg};
use setup::Setup;
use state::{Loaded, default_sidebar, default_sidebar_open};

pub(crate) mod actions;
pub(crate) mod bot;
mod chat;
mod handoffs;
mod panels;
mod rooms;
mod schedules;
pub(crate) mod setup;
pub(crate) mod state;
mod turns;

/// The one panel open above the main pane. Settings shows in every view; the others need a bot chat.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Panel {
    None,
    Settings,
    Editor,
    Schedules,
    Skills,
}

pub(crate) struct Eggbot {
    pub(crate) p: Palette,
    pub(crate) bots: Vec<Bot>,
    pub(crate) selected: usize,
    next_id: usize,
    pub(crate) menu_open: bool,
    /// Quit is in progress: don't drop the running handoff out of `current`.
    quitting: bool,
    /// Bot id whose trash icon was clicked once; a second click deletes.
    pub(crate) confirm_delete: Option<usize>,
    pub(crate) meters: Vec<Meter>,
    /// One-at-a-time starts at this fraction of any plan window (5h, 7d, week).
    pub(crate) throttle: f32,
    /// New turns wait at this fraction. The turn already running finishes.
    pub(crate) pause: f32,
    /// Codex models this account can use; fetched on demand.
    pub(crate) codex_models: Vec<codex::Model>,
    /// None = idle; Some(None) = asking now; Some(Some(e)) = the last question failed with `e`.
    pub(crate) codex_query: Option<Option<String>>,
    tray: Option<tray::Tray>,
    pub(crate) input: Entity<TextareaState>,
    /// Bot id whose draft is in `input`; a different selected bot swaps drafts.
    draft_bot: Option<usize>,
    pub(crate) panel: Panel,
    /// 0 daily, 1 weekdays, 2 every N hours, 3 every N minutes
    pub(crate) sched_kind: usize,
    pub(crate) sched_prompt: Entity<InputState>,
    pub(crate) sched_value: Entity<InputState>,
    pub(crate) sched_error: Option<String>,
    pub(crate) skill_name: Entity<InputState>,
    pub(crate) skill_body: Entity<TextareaState>,
    /// Index of the skill the form is editing. None means the form adds a new one.
    pub(crate) skill_at: Option<usize>,
    pub(crate) skill_error: Option<String>,
    pub(crate) edit_name: Entity<InputState>,
    pub(crate) edit_role: Entity<TextareaState>,
    pub(crate) edit_shared: Entity<TextareaState>,
    pub(crate) shared: Option<String>,
    pub(crate) limit_throttle: Entity<InputState>,
    pub(crate) limit_pause: Entity<InputState>,
    pub(crate) settings_error: Option<String>,
    pub(crate) login_error: Option<String>,
    pub(crate) edit_error: Option<String>,
    pub(crate) model_select: Entity<SelectState<Vec<Choice>>>,
    pub(crate) effort_select: Entity<SelectState<Vec<Choice>>>,
    /// Dropdown options need refilling (bot, provider or Codex model list changed); done in render.
    pub(crate) selects_stale: bool,
    pub(crate) sidebar_w: f32,
    /// Bot list visible. The width is kept while it is closed.
    pub(crate) sidebar_open: bool,
    /// The Rooms and Groups lists are folded to their titles.
    pub(crate) rooms_collapsed: bool,
    pub(crate) groups_collapsed: bool,
    pub(crate) appearance: Appearance,
    /// Shown under the composer when adding a folder is refused.
    pub(crate) folder_error: Option<String>,
    /// The window is in front; otherwise news goes out as notifications.
    active: bool,
    /// ⌘F search in the open chat: the query field, matching message indices (oldest first) and the current one.
    pub(crate) find_open: bool,
    pub(crate) find_input: Entity<InputState>,
    pub(crate) find_hits: Vec<usize>,
    pub(crate) find_at: usize,
    /// The first-run checklist, shown in place of the chat while open.
    pub(crate) setup: Option<Setup>,
    /// The sidebar row being dragged, to hide drop lines that would change nothing.
    pub(crate) dragging: Option<usize>,
    pub(crate) rooms: Vec<room::Room>,
    next_room_id: usize,
    /// The room open in the main pane; None means the selected bot's chat.
    pub(crate) open_room: Option<usize>,
    pub(crate) room_title: Entity<InputState>,
    pub(crate) room_kickoff: Entity<TextareaState>,
    pub(crate) room_error: Option<String>,
    /// Set after Start: sent, or waiting because the facilitator is busy.
    pub(crate) room_status: Option<String>,
    /// Room id whose Delete was clicked once.
    pub(crate) confirm_delete_room: Option<usize>,
    pub(crate) groups: Vec<group::Group>,
    next_group_id: usize,
    /// The group open in the main pane. A group is notes, not a room.
    pub(crate) open_group: Option<usize>,
    pub(crate) group_title: Entity<InputState>,
    /// Group id whose Delete was clicked once.
    pub(crate) confirm_delete_group: Option<usize>,
    /// Dragging the sidebar's edge.
    pub(crate) resizing: bool,
    /// The chat's virtual list: one row per message of the selected bot, plus the typing row.
    pub(crate) list: ListState,
    /// The bot whose messages `list` holds.
    list_bot: Option<usize>,
    /// The open room's transcript. Separate from `list`, which is one bot's chat.
    pub(crate) room_list: ListState,
    /// Room id `room_list` was built for. None after the room closes, so the next open jumps to the end.
    room_list_for: Option<usize>,
    /// `state.json` could not be read or moved aside; saving would overwrite it.
    no_save: bool,
}

impl Eggbot {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Enter sends, Shift+Enter adds a line; grows up to 8 lines
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx).placeholder("Message…").submit_on_enter(true);
            input.set_auto_grow(1, 8, cx);
            input
        });
        cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::PressEnter { shift: false, .. } => this.send(window, cx),
                // the send button and the focus border follow the box
                InputEvent::Change | InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }
        })
        .detach();
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search this chat"));
        cx.subscribe_in(&find_input, window, |this, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.find_update(cx),
            // Enter walks back in time, ⇧Enter forward
            InputEvent::PressEnter { shift, .. } => this.find_step(if *shift { 1 } else { -1 }, cx),
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        })
        .detach();
        cx.observe_window_appearance(window, |this, window, cx| {
            this.p = Palette::apply(window, cx);
            cx.notify();
        })
        .detach();
        input.update(cx, |s, cx| s.focus(window, cx));
        let sched_prompt = cx.new(|cx| InputState::new(window, cx).placeholder("What should it do? e.g. Review yesterday's commits"));
        let sched_value = cx.new(|cx| InputState::new(window, cx).placeholder("09:00"));
        let skill_name = cx.new(|cx| InputState::new(window, cx).placeholder("Skill name, e.g. Review a diff"));
        let skill_body = cx.new(|cx| {
            let mut body = TextareaState::new(window, cx).placeholder("The steps this bot should follow.");
            body.set_auto_grow(3, 8, cx);
            body
        });
        let edit_name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let edit_shared = cx.new(|cx| {
            let mut shared = TextareaState::new(window, cx).placeholder("e.g. Reply in Italian. Never push to git.");
            shared.set_auto_grow(3, 10, cx);
            shared
        });
        let edit_role = cx.new(|cx| {
            let mut role = TextareaState::new(window, cx).placeholder("What is this bot for, and how should it work?");
            role.set_auto_grow(4, 12, cx);
            role
        });
        let limit_throttle = cx.new(|cx| InputState::new(window, cx).placeholder("90"));
        let limit_pause = cx.new(|cx| InputState::new(window, cx).placeholder("95"));
        let room_title = cx.new(|cx| InputState::new(window, cx).placeholder("Daily standup"));
        let room_kickoff = cx.new(|cx| {
            let mut kickoff = TextareaState::new(window, cx).placeholder("What should this room do? e.g. Plan the sprint: goals, scope, who does what.");
            kickoff.set_auto_grow(3, 8, cx);
            kickoff
        });
        let group_title = cx.new(|cx| InputState::new(window, cx).placeholder("Reviewers"));
        let model_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let effort_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        cx.subscribe_in(&model_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
            if let SelectEvent::Confirm(Some(Some(value))) = ev {
                this.pick_model(value, cx);
            }
        })
        .detach();
        cx.subscribe_in(&effort_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
            if let SelectEvent::Confirm(Some(effort)) = ev {
                this.pick_effort(effort.clone(), cx);
            }
        })
        .detach();
        cx.subscribe_in(&edit_name, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.save_edit(window, cx);
            }
        })
        .detach();
        cx.subscribe_in(&room_title, window, |this, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                this.room_title_changed(cx);
            }
        })
        .detach();
        cx.subscribe_in(&room_kickoff, window, |this, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                this.room_kickoff_changed(cx);
            }
        })
        .detach();
        cx.subscribe_in(&group_title, window, |this, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                this.group_title_changed(cx);
            }
        })
        .detach();
        for field in [&sched_prompt, &sched_value] {
            cx.subscribe_in(field, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.add_schedule(window, cx);
                }
            })
            .detach();
        }
        cx.subscribe_in(&skill_name, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.save_skill(window, cx);
            }
        })
        .detach();
        Self::start_ticker(cx);
        let p = Palette::apply(window, cx);
        let loaded = state::load();
        let first_launch = matches!(loaded, Loaded::Fresh);
        let no_save = matches!(loaded, Loaded::Stuck);
        let (saved, notice) = match loaded {
            Loaded::Saved(s) => (Some(s), None),
            Loaded::Fresh => (None, None),
            Loaded::BackedUp(bad) => (None, Some(format!("Could not read state.json, so eggbot started fresh; the old file is saved at {}.", bad.display()))),
            Loaded::Stuck => (None, Some("Could not read state.json, so eggbot will not save changes this session.".to_string())),
        };
        let appearance = saved.as_ref().map_or_else(Appearance::default, |s| s.appearance);
        let mut this = Self {
            p,
            bots: vec![],
            selected: 0,
            next_id: 0,
            menu_open: false,
            quitting: false,
            confirm_delete: None,
            meters: vec![],
            throttle: usage::DEFAULT_THROTTLE,
            pause: usage::DEFAULT_PAUSE,
            codex_models: vec![],
            codex_query: None,
            tray: None,
            panel: Panel::None,
            sched_kind: 0,
            sched_prompt,
            sched_value,
            sched_error: None,
            skill_name,
            skill_body,
            skill_at: None,
            skill_error: None,
            edit_name,
            edit_role,
            edit_error: None,
            edit_shared,
            shared: None,
            limit_throttle,
            limit_pause,
            settings_error: None,
            login_error: None,
            model_select,
            effort_select,
            selects_stale: true,
            sidebar_w: default_sidebar(),
            sidebar_open: default_sidebar_open(),
            rooms_collapsed: false,
            groups_collapsed: false,
            appearance,
            folder_error: None,
            active: false,
            find_open: false,
            find_input,
            find_hits: vec![],
            find_at: 0,
            setup: None,
            dragging: None,
            resizing: false,
            input,
            draft_bot: None,
            list: ListState::new(0, ListAlignment::Bottom, px(800.)),
            list_bot: None,
            room_list: ListState::new(0, ListAlignment::Bottom, px(800.)),
            room_list_for: None,
            rooms: vec![],
            next_room_id: 0,
            open_room: None,
            room_title,
            room_kickoff,
            room_error: None,
            room_status: None,
            confirm_delete_room: None,
            groups: vec![],
            next_group_id: 0,
            open_group: None,
            group_title,
            confirm_delete_group: None,
            no_save,
        };
        this.list.set_follow_mode(FollowMode::Tail);
        this.room_list.set_follow_mode(FollowMode::Tail);
        match saved {
            Some(s) if s.has_content() => this.restore(s),
            _ => {
                for i in 0..3 {
                    this.hatch(i);
                }
                this.selected = 0;
            }
        }
        if let (Some(t), Some(b)) = (notice, this.bots.first_mut()) {
            b.msgs.push(Msg::Error(t));
        }
        // after loading: it saves state
        this.set_appearance(appearance, window, cx);
        if first_launch {
            this.open_setup(cx);
        }
        if this.bots.iter().any(|b| b.provider == Provider::Codex) {
            this.refresh_codex(1, cx);
        }
        window.on_window_should_close(cx, |_, cx| {
            // closing only hides: the bots keep working and the menu bar egg brings the window back
            cx.hide();
            set_dock_icon(false);
            false
        });
        cx.observe_window_activation(window, |this, window, cx| {
            this.active = window.is_window_active();
            if this.active {
                this.mark_read(cx);
            }
        })
        .detach();
        this.start_tray(window, cx);
        this
    }

    /// The menu bar egg: its menu and notification clicks share one channel, and a timer keeps the icon and menu current.
    fn start_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (clicks, actions) = async_channel::unbounded();
        notify::init(clicks.clone());
        self.tray = tray::Tray::new(clicks);
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(action) = actions.recv().await {
                let done = this.update_in(cx, |this, window, cx| match action {
                    tray::Action::Open => this.show(window, cx),
                    tray::Action::Bot(id) => {
                        this.show(window, cx);
                        this.open_bot(id, window, cx);
                    }
                    tray::Action::Quit => this.request_quit(window, cx),
                });
                if done.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            for tick in 0.. {
                cx.background_executor().timer(Duration::from_millis(350)).await;
                let alive = this.update(cx, |this, _| {
                    let bots = this.bots.iter().map(|b| (b.id, b.name.clone(), b.busy(), b.unread)).collect();
                    if let Some(t) = &mut this.tray {
                        t.update(bots, tick);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// The bot with this id. Bots move in the list, so ids are the stable handle.
    pub(crate) fn bot(&self, id: usize) -> Option<&Bot> {
        self.bots.iter().find(|b| b.id == id)
    }

    pub(crate) fn bot_mut(&mut self, id: usize) -> Option<&mut Bot> {
        self.bots.iter_mut().find(|b| b.id == id)
    }

    pub(crate) fn room(&self, id: usize) -> Option<&room::Room> {
        self.rooms.iter().find(|r| r.id == id)
    }

    pub(crate) fn room_mut(&mut self, id: usize) -> Option<&mut room::Room> {
        self.rooms.iter_mut().find(|r| r.id == id)
    }

    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        set_dock_icon(true);
        cx.activate(true);
        window.activate_window();
    }

    /// Asks before stopping working bots; their containers stay for a fast next launch.
    pub(crate) fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let busy = self.bots.iter().filter(|b| b.busy()).count();
        if busy == 0 {
            return self.quit_now(cx);
        }
        self.show(window, cx);
        let what = if busy == 1 { "1 bot is working".to_string() } else { format!("{busy} bots are working") };
        let answer = window.prompt(PromptLevel::Warning, &what, Some("Quit anyway? Their current turns will stop."), &["Quit", "Cancel"], cx);
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                this.update(cx, |this, cx| this.quit_now(cx)).ok();
            }
        })
        .detach();
    }

    fn quit_now(&mut self, cx: &mut Context<Self>) {
        // Done arrives after the kill; it must not clear `current` or the in-flight handoff is lost
        self.quitting = true;
        for b in self.bots.iter().filter(|b| b.busy()) {
            sandbox::interrupt(b.id);
        }
        self.save();
        cx.quit();
    }
}
