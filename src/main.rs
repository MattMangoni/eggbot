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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app::{Eggbot, Panel};
use claude::{Meter, Provider};
use egg::Mood;
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

struct Preset {
    name: &'static str,
    blurb: &'static str,
    color: u32,
    role: &'static str,
}

const PRESETS: [Preset; 4] = [
    Preset {
        name: "Reviewer",
        blurb: "Reads diffs, finds bugs, weighs risk",
        color: 0xF5C6A5,
        role: "You are Reviewer, a code reviewer living in eggbot. Read code and diffs carefully. Find bugs, security issues and risky changes. For each finding give the file, the line, why it matters and a concrete fix, ranked by severity. Do not edit files unless asked.",
    },
    Preset {
        name: "Implementer",
        blurb: "Writes and changes code",
        color: 0xC6DDB8,
        role: "You are Implementer, a software engineer living in eggbot. Write and change code with the smallest correct diff. Match the existing style, reuse what exists, and explain briefly what you changed.",
    },
    Preset {
        name: "Designer",
        blurb: "UI and UX critique and polish",
        color: 0xD6CAF0,
        role: "You are Designer, a UI and UX specialist living in eggbot. Critique and improve hierarchy, spacing, typography, color, motion, accessibility and interaction states. Give concrete, actionable suggestions.",
    },
    Preset { name: "Custom", blurb: "A blank bot you shape yourself", color: 0xF3DF9C, role: "You are a helpful bot living in eggbot." },
];
/// One row of the setup checklist.
#[derive(Clone, PartialEq)]
enum Check {
    Unknown,
    Ok,
    Missing,
    /// An action is running (install, start, build, sign-in); the label says what.
    Busy(&'static str),
    Failed(String),
}

/// The first-run checklist: Docker engine, Docker running, bot image, Claude and Codex sign-in.
#[derive(Clone)]
struct Setup {
    engine: Check,
    running: Check,
    image: Check,
    claude: Check,
    codex: Check,
}

impl Setup {
    /// Docker works, the image exists, and at least one provider is signed in.
    fn done(&self) -> bool {
        [&self.engine, &self.running, &self.image].iter().all(|c| **c == Check::Ok) && (self.claude == Check::Ok || self.codex == Check::Ok)
    }
}

/// "Instructions for all bots" until the user edits them in Settings.
const SHARED: &str = "Reply in concise GitHub-flavored markdown.";
const QUIET: &str = "\n\n(This is a scheduled run. If nothing here needs the user's attention, reply with exactly QUIET and nothing else.)";
const FRESH_START: &str = "We are about to start a fresh session. Update /memory/NOTES.md with short bullets worth keeping (Facts, Preferences, Lessons — no chat logs), or end with one <eggbot-learn> block. Then reply with one short line.";

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

const SHELLS: [u32; 8] = [0xF5C6A5, 0xC6DDB8, 0xD6CAF0, 0xF3DF9C, 0xB9D8EA, 0xF2B8C6, 0xCFE3D8, 0xE3D2B9];
const MODELS: [(Option<&str>, &str); 5] = [(None, "Default"), (Some("fable"), "Fable"), (Some("opus"), "Opus"), (Some("sonnet"), "Sonnet"), (Some("haiku"), "Haiku")];

#[derive(Serialize, Deserialize)]
enum Msg {
    User(String),
    Bot(String),
    Tool {
        id: String,
        verb: String,
        target: String,
        detail: String,
        open: bool,
    },
    Error(String),
    /// Work handed over by another bot; `paused` when the chain hit the hop limit.
    Handoff {
        from: String,
        color: u32,
        prompt: String,
        text: String,
        paused: bool,
        open: bool,
        /// Set when this hop stays inside a room. Continue chain keeps it on the next turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        room: Option<usize>,
    },
    Sent {
        to: String,
    },
    /// Marks where a fresh session began.
    Divider(String),
    /// A turn started by a schedule, shown where a user message would be.
    Scheduled {
        prompt: String,
        label: String,
    },
    /// Replaces a "not signed in" error once the login works; `prompt` is what failed, for "Send again".
    SignedIn {
        provider: Provider,
        prompt: Option<String>,
    },
    /// A room kickoff, shown in the facilitator's chat. `prompt` is what the bot was told.
    Kickoff {
        room_id: usize,
        room: String,
        text: String,
        prompt: String,
    },
}

/// Bot text written at and after `from`. Earlier lines belong to a previous turn.
fn reply_text(msgs: &[Msg], from: usize) -> String {
    let from = from.min(msgs.len());
    msgs[from..].iter().filter_map(|m| if let Msg::Bot(t) = m { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n\n")
}

impl Msg {
    /// The text search looks in: what people and bots wrote, not tool output.
    fn searchable(&self) -> Option<&str> {
        match self {
            Msg::User(t) | Msg::Bot(t) | Msg::Handoff { text: t, .. } | Msg::Scheduled { prompt: t, .. } | Msg::Kickoff { text: t, .. } => Some(t),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Bot {
    id: usize,
    name: String,
    preset: usize,
    /// User-picked folders, each mounted at `/work/<name>`.
    #[serde(default)]
    folders: Vec<sandbox::Mount>,
    /// Single folder from before multi-mount. Folded into `folders` on load.
    #[serde(default, skip_serializing)]
    folder: Option<PathBuf>,
    // renamed when bots moved into containers: host sessions cannot resume there
    #[serde(rename = "sandbox_session")]
    session: Option<String>,
    #[serde(default)]
    provider: Provider,
    /// Codex thread id, kept apart from the Claude session so switching provider loses neither.
    #[serde(default)]
    thread: Option<String>,
    msgs: Vec<Msg>,
    #[serde(default)]
    schedules: Vec<schedule::Schedule>,
    /// Edits made in the bot editor; None = the preset's value.
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    color: Option<u32>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    /// Copied from the preset at hatch, then owned by this bot.
    #[serde(default)]
    skills: Vec<skills::Skill>,
    #[serde(skip)]
    run: Option<Arc<claude::Handle>>,
    #[serde(skip)]
    status: Option<String>,
    #[serde(skip)]
    stopped: bool,
    /// Handoff hops that led to the current turn (0 = started by the user).
    #[serde(skip)]
    hops: u32,
    /// `msgs` index where the running turn's output starts. A queued kickoff is not always the last marker.
    #[serde(skip)]
    reply_from: usize,
    /// Turns waiting for this one to finish (handoffs, and schedules that arrived mid-turn).
    #[serde(default)]
    queue: Vec<handoff::Pending>,
    /// The running turn uses a throwaway session (schedules): its session id and context are not kept.
    #[serde(skip)]
    fresh_turn: bool,
    /// The running turn saves notes before a fresh start; on success the session is dropped.
    #[serde(skip)]
    refreshing: bool,
    /// Codex: the role last delivered to the current thread; a different role is sent again once.
    #[serde(default)]
    codex_role: Option<String>,
    /// Role sent with the running turn; becomes `codex_role` when the turn succeeds.
    #[serde(skip)]
    pending_role: Option<String>,
    /// Tokens in the main session's context and the model's window (0 = unknown).
    #[serde(default)]
    context: (u64, u64),
    /// Something arrived that the user has not seen yet.
    #[serde(default)]
    unread: bool,
    /// The running turn. Retried after a sign-in clash, and restored when it is a handoff.
    #[serde(default)]
    current: Option<handoff::Pending>,
    /// The running turn is already that one retry.
    #[serde(skip)]
    retried: bool,
    /// Newest-first names this bot successfully handed work to. A paused chain is not recorded.
    #[serde(default)]
    recent: Vec<String>,
    /// Composer text kept while another bot is selected. Not saved across restarts.
    #[serde(skip)]
    draft: String,
}

impl Bot {
    fn preset(&self) -> &'static Preset {
        &PRESETS[self.preset.min(PRESETS.len() - 1)]
    }

    fn color(&self) -> u32 {
        self.color.unwrap_or(self.preset().color)
    }

    fn role(&self) -> &str {
        self.role.as_deref().unwrap_or(self.preset().role)
    }

    /// Sidebar subtitle: the preset blurb, or the start of an edited role.
    fn blurb(&self) -> String {
        match &self.role {
            Some(r) => r.lines().next().unwrap_or_default().chars().take(60).collect(),
            None => self.preset().blurb.to_string(),
        }
    }

    fn busy(&self) -> bool {
        self.run.is_some()
    }

    fn mood(&self) -> Mood {
        if self.busy() { Mood::Thinking } else { Mood::Still }
    }

    /// True while the bot works but is not writing text (thinking or running a tool).
    fn waiting(&self) -> bool {
        self.busy() && !matches!(self.msgs.last(), Some(Msg::Bot(_)))
    }
}

fn data_dir() -> PathBuf {
    // ponytail: macOS path only; use the `dirs` crate when Linux/Windows builds start
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/eggbot")
}

/// Merges an `<eggbot-learn>` block into NOTES.md and returns the reply without that block.
/// `start` is `reply_from`, so only this turn's bubbles change.
/// `groups` are `(id, title)` for groups this bot is in. A group bullet is saved on that group, not privately.
/// `rooms` are `(id, title)` for rooms this bot is in. A room bullet is saved on that room, not privately.
fn keep_notes(bot: &mut Bot, groups: &[(usize, String)], rooms: &[(usize, String)], reply: &str, start: usize) -> String {
    let (visible, updates) = memory::extract(reply);
    if visible != reply {
        // the block can span streamed chunks, so one visible reply replaces them
        let mut kept = false;
        let mut i = start.min(bot.msgs.len());
        while i < bot.msgs.len() {
            if matches!(bot.msgs[i], Msg::Bot(_)) {
                if !kept && !visible.is_empty() {
                    bot.msgs[i] = Msg::Bot(visible.clone());
                    kept = true;
                    i += 1;
                } else {
                    bot.msgs.remove(i);
                }
            } else {
                i += 1;
            }
        }
    }
    if updates.is_empty() {
        return visible;
    }
    let room_titles: Vec<&str> = rooms.iter().map(|(_, title)| title.as_str()).collect();
    let routed_rooms = room::route(&updates, &room_titles);
    let titles: Vec<&str> = groups.iter().map(|(_, title)| title.as_str()).collect();
    let routed = group::route(&routed_rooms.rest, &titles);
    let private_path = data_dir().join("bots").join(bot.id.to_string()).join("memory").join("NOTES.md");
    if let Err(e) = memory::save(&private_path, &routed.private) {
        eprintln!("eggbot: could not save notes: {e}");
    }
    for (index, updates) in &routed.shared {
        let path = group::notes_file(&data_dir(), groups[*index].0);
        if let Err(e) = memory::save(&path, updates) {
            eprintln!("eggbot: could not save group notes: {e}");
        }
    }
    for (index, updates) in &routed_rooms.memory {
        let path = room::notes_file(&data_dir(), rooms[*index].0);
        if let Err(e) = memory::save(&path, updates) {
            eprintln!("eggbot: could not save room memory: {e}");
        }
    }
    visible
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    next_id: usize,
    bots: Vec<Bot>,
    /// Last plan usage per provider; saved because it only arrives with a turn or an account query.
    #[serde(default)]
    meters: Vec<Meter>,
    #[serde(default = "default_sidebar")]
    sidebar_w: f32,
    // missing in older state.json stays open; bool's Default is false
    #[serde(default = "default_sidebar_open")]
    sidebar_open: bool,
    #[serde(default)]
    appearance: Appearance,
    /// Instructions for all bots; None = `SHARED`.
    #[serde(default)]
    shared: Option<String>,
    /// Fraction of a plan window where extra bots wait their turn. Missing in older state.json = 90%.
    #[serde(default = "default_throttle")]
    throttle: f32,
    /// Fraction where new turns wait. Missing in older state.json = 95%.
    #[serde(default = "default_pause")]
    pause: f32,
    #[serde(default)]
    next_room_id: usize,
    #[serde(default)]
    rooms: Vec<room::Room>,
    #[serde(default)]
    next_group_id: usize,
    #[serde(default)]
    groups: Vec<group::Group>,
}

fn default_sidebar() -> f32 {
    260.
}

fn default_sidebar_open() -> bool {
    true
}

fn default_throttle() -> f32 {
    usage::DEFAULT_THROTTLE
}

fn default_pause() -> f32 {
    usage::DEFAULT_PAUSE
}

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

    fn save(&self) {
        let dir = data_dir();
        let state = serde_json::json!({ "next_id": self.next_id, "bots": self.bots, "meters": self.meters, "sidebar_w": self.sidebar_w, "sidebar_open": self.sidebar_open, "appearance": self.appearance, "shared": self.shared, "throttle": self.throttle, "pause": self.pause, "next_room_id": self.next_room_id, "rooms": self.rooms, "next_group_id": self.next_group_id, "groups": self.groups });
        // write then rename, so a crash mid-write never loses the history
        let tmp = dir.join("state.json.tmp");
        let ok = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&tmp, state.to_string())).and_then(|_| std::fs::rename(&tmp, dir.join("state.json")));
        if let Err(e) = ok {
            eprintln!("eggbot: could not save state: {e}");
        }
    }

    fn hatch(&mut self, preset: usize) {
        self.leave_room();
        self.leave_group();
        self.panel = Panel::None;
        self.skill_at = None;
        let base = PRESETS[preset].name;
        let taken = |n: &str| self.bots.iter().any(|b| b.name == n);
        let name = (1..).map(|i| if i == 1 { base.to_string() } else { format!("{base} {i}") }).find(|n| !taken(n)).unwrap();
        self.bots.push(Bot {
            id: self.next_id,
            name,
            preset,
            folders: vec![],
            folder: None,
            session: None,
            provider: Provider::Claude,
            thread: None,
            msgs: vec![],
            schedules: vec![],
            role: None,
            color: None,
            model: None,
            effort: None,
            // copied by preset name, so this bot keeps its own list after hatch
            skills: skills::defaults(PRESETS[preset].name),
            run: None,
            status: None,
            stopped: false,
            hops: 0,
            reply_from: 0,
            queue: vec![],
            fresh_turn: false,
            refreshing: false,
            codex_role: None,
            pending_role: None,
            context: (0, 0),
            unread: false,
            current: None,
            retried: false,
            recent: vec![],
            draft: String::new(),
        });
        self.next_id += 1;
        self.selected = self.bots.len() - 1;
        self.selects_stale = true;
        self.save();
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

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone().unwrap_or_else(|| SHARED.into());
        self.edit_shared.update(cx, |s, cx| {
            s.set_value(shared, window, cx);
            s.focus(window, cx);
        });
        let (throttle, pause) = (usage::percent(self.throttle).to_string(), usage::percent(self.pause).to_string());
        self.limit_throttle.update(cx, |s, cx| s.set_value(throttle, window, cx));
        self.limit_pause.update(cx, |s, cx| s.set_value(pause, window, cx));
        (self.panel, self.login_error, self.settings_error) = (Panel::Settings, None, None);
        cx.notify();
    }

    /// Saves the instructions for all bots; each bot gets them from its next turn (both providers).
    fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.edit_shared.read(cx).value().trim().to_string();
        let limits = usage::parse_limits(&self.limit_throttle.read(cx).value(), &self.limit_pause.read(cx).value());
        let Ok((throttle, pause)) = limits else {
            self.settings_error = limits.err().map(str::to_string);
            cx.notify();
            return;
        };
        self.shared = (text != SHARED).then_some(text);
        (self.throttle, self.pause) = (throttle, pause);
        (self.panel, self.settings_error) = (Panel::None, None);
        self.save();
        // a looser limit can let held schedules and queued turns go
        self.release_schedules(cx);
        self.resume_queues(cx);
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn set_login(&mut self, on: bool, cx: &mut Context<Self>) {
        self.login_error = login::set(on).err();
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

    /// Moves bot `from` to just above row `before` (`before` = len: the end); the selection stays on the same bot.
    fn move_bot(&mut self, from: usize, before: usize, cx: &mut Context<Self>) {
        self.dragging = None;
        cx.notify();
        if from >= self.bots.len() || before > self.bots.len() || before == from || before == from + 1 {
            return;
        }
        let selected = self.bots[self.selected].id;
        let bot = self.bots.remove(from);
        self.bots.insert(if before > from { before - 1 } else { before }, bot);
        self.selected = self.bots.iter().position(|b| b.id == selected).unwrap_or(0);
        self.save();
        cx.notify();
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

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        if let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) {
            bot.msgs.push(Msg::User(text.clone()));
        }
        self.start_turn(id, handoff::Pending::user(text), cx);
    }

    /// Drops one message the user queued. `at` indexes the bot's whole queue; other kinds of queued work stay.
    fn unqueue(&mut self, id: usize, at: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        if bot.queue.get(at).is_some_and(|q| q.typed) {
            bot.queue.remove(at);
            self.save();
            cx.notify();
        }
    }

    /// `pending.fresh` runs the turn in a throwaway session (schedules), leaving the main session untouched.
    fn start_turn(&mut self, id: usize, mut pending: handoff::Pending, cx: &mut Context<Self>) {
        let Some(provider) = self.bots.iter().find(|b| b.id == id).map(|b| b.provider) else { return };
        let busy = self.bots.iter().find(|b| b.id == id).is_some_and(|b| b.busy());
        // never drop a hop: if the guard closed, put it back at the front of the saved queue
        if busy || !self.may_start(provider) {
            if let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) {
                bot.queue.insert(0, pending);
            }
            self.save();
            cx.notify();
            return;
        }
        let roster_bots: Vec<(usize, String, String)> = self.bots.iter().map(|b| (b.id, b.name.clone(), b.blurb())).collect();
        let stored = self.bots.iter().find(|b| b.id == id).map(|b| b.recent.clone()).unwrap_or_default();
        let alive: Vec<&str> = roster_bots.iter().filter(|(i, ..)| *i != id).map(|(_, name, _)| name.as_str()).collect();
        let recent = handoff::recent(&stored, &alive, 4);
        let room_peers: Vec<(String, Vec<String>)> = self
            .rooms
            .iter()
            .filter(|r| r.members.contains(&id))
            .map(|r| {
                let peers: Vec<String> = roster_bots.iter().filter(|(bid, _, _)| *bid != id && r.members.contains(bid)).map(|(_, name, _)| name.clone()).collect();
                (r.title.clone(), peers)
            })
            .filter(|(_, peers)| !peers.is_empty())
            .collect();
        let room_names: Vec<Vec<&str>> = room_peers.iter().map(|(_, peers)| peers.iter().map(String::as_str).collect()).collect();
        let room_refs: Vec<(&str, &[&str])> = room_peers.iter().zip(&room_names).map(|((title, _), peers)| (title.as_str(), peers.as_slice())).collect();
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
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
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

    /// Starts the turn now, or appends it to the persisted queue (busy, already queued, or over the usage limit).
    fn deliver(&mut self, id: usize, pending: handoff::Pending, cx: &mut Context<Self>) {
        let Some(provider) = self.bots.iter().find(|b| b.id == id).map(|b| b.provider) else { return };
        let wait = self.bots.iter().find(|b| b.id == id).is_some_and(|b| b.busy() || !b.queue.is_empty()) || !self.may_start(provider);
        if wait {
            if let Some(b) = self.bots.iter_mut().find(|b| b.id == id) {
                b.queue.push(pending);
            }
            self.save();
            cx.notify();
            return;
        }
        self.start_turn(id, pending, cx);
    }

    /// An in-flight @Name hop goes back on the queue. User turns and schedules stay stopped.
    fn restore_handoffs(&mut self) {
        let mut changed = false;
        for b in &mut self.bots {
            if b.current.is_none() {
                continue;
            }
            changed = true;
            let running = b.current.take();
            b.queue = handoff::restore(running, std::mem::take(&mut b.queue));
        }
        if changed {
            self.save();
        }
    }

    /// Starts the head of each idle bot's queue once Docker is up and the usage guard allows it.
    /// Offline, paused, or throttled-behind-another-bot: the queue is not touched.
    fn pump_queues(&mut self, online: bool, prefer: Option<usize>, cx: &mut Context<Self>) {
        let mut ids: Vec<usize> = self.bots.iter().map(|b| b.id).collect();
        if let Some(id) = prefer {
            ids.retain(|i| *i != id);
            ids.insert(0, id);
        }
        let mut starts = vec![];
        for id in ids {
            let Some(provider) = self.bots.iter().find(|b| b.id == id).map(|b| b.provider) else { continue };
            let busy = self.bots.iter().find(|b| b.id == id).is_some_and(|b| b.busy());
            if busy || !online || !self.may_start(provider) {
                continue;
            }
            let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { continue };
            let (next, queue) = handoff::dequeue(std::mem::take(&mut bot.queue), false, false);
            bot.queue = queue;
            if let Some(pending) = next {
                starts.push((id, pending));
            }
        }
        for (id, pending) in starts {
            let Some(provider) = self.bots.iter().find(|b| b.id == id).map(|b| b.provider) else { continue };
            // a sibling may have started in this loop; put the hop back rather than dropping it
            if !self.may_start(provider) {
                if let Some(b) = self.bots.iter_mut().find(|b| b.id == id) {
                    b.queue.insert(0, pending);
                }
                self.save();
                continue;
            }
            self.start_turn(id, pending, cx);
        }
    }

    /// Docker check off the UI thread, then start whatever the guard now allows.
    fn resume_queues(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let online = cx.background_executor().spawn(async { sandbox::running() }).await;
            this.update(cx, |this, cx| this.pump_queues(online, None, cx)).ok();
        })
        .detach();
    }

    /// Sends the finished reply to every bot it mentions as @Name; false when it mentions nobody.
    /// `room` is kept only for targets still on that room's roster (`room::carry`).
    fn hand_off(&mut self, from: usize, reply: String, hops: u32, room: Option<usize>, cx: &mut Context<Self>) -> bool {
        let names: Vec<(usize, &str)> = self.bots.iter().map(|b| (b.id, b.name.as_str())).collect();
        let targets = handoff::mentions(&reply, &names, from);
        let handed = !targets.is_empty();
        let Some(sender) = self.bots.iter().find(|b| b.id == from) else { return false };
        let (from_name, color) = (sender.name.clone(), sender.color());
        let from_mounts: Vec<(PathBuf, String)> = sender.folders.iter().map(|f| (f.path.clone(), f.dest())).collect();
        let next = hops + 1;
        for to in targets {
            let stays = self.rooms.iter().find(|r| Some(r.id) == room).and_then(|r| room::carry(room, &r.members, to));
            let Some(target) = self.bots.iter_mut().find(|b| b.id == to) else { continue };
            let to_mounts: Vec<(PathBuf, String)> = target.folders.iter().map(|f| (f.path.clone(), f.dest())).collect();
            let mine: Vec<(&std::path::Path, &str)> = from_mounts.iter().map(|(p, d)| (p.as_path(), d.as_str())).collect();
            let theirs: Vec<(&std::path::Path, &str)> = to_mounts.iter().map(|(p, d)| (p.as_path(), d.as_str())).collect();
            let prompt = handoff::prompt(&from_name, &reply, &mine, &theirs);
            let paused = next > handoff::MAX_HOPS;
            let to_name = target.name.clone();
            target.msgs.push(Msg::Handoff { from: from_name.clone(), color, prompt: prompt.clone(), text: reply.clone(), paused, open: false, room: stays });
            if let Some(s) = self.bots.iter_mut().find(|b| b.id == from) {
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
    fn log_room(&mut self, room_id: usize, record: impl FnOnce(&mut room::Room) -> bool) {
        let open = self.open_room == Some(room_id);
        if let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id)
            && record(room)
            && !open
        {
            room.unread = true;
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

    fn add_schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.sched_prompt.read(cx).value().trim().to_string();
        let value = self.sched_value.read(cx).value().to_string();
        let repeat = match (prompt.is_empty(), schedule::Repeat::parse(self.sched_kind, &value)) {
            (true, _) => Err("Write what the bot should do"),
            (false, r) => r,
        };
        match (repeat, self.bots.get_mut(self.selected)) {
            (Ok(repeat), Some(bot)) => {
                let id = bot.schedules.iter().map(|s| s.id + 1).max().unwrap_or(0);
                bot.schedules.push(schedule::Schedule { id, prompt, repeat, anchor: chrono::Local::now().timestamp() });
                self.sched_error = None;
                self.sched_prompt.update(cx, |s, cx| s.set_value("", window, cx));
                self.save();
            }
            (Err(e), _) => self.sched_error = Some(e.into()),
            _ => {}
        }
        cx.notify();
    }

    fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get(self.selected) else { return };
        let (name, role) = (bot.name.clone(), bot.role().to_string());
        self.edit_name.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
        });
        self.edit_role.update(cx, |s, cx| s.set_value(role, window, cx));
        self.panel = Panel::Editor;
        self.edit_error = None;
        self.selects_stale = true;
        if self.bots[self.selected].provider == Provider::Codex && self.codex_models.is_empty() && self.codex_query != Some(None) {
            self.refresh_codex(1, cx);
        }
        cx.notify();
    }

    fn save_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.edit_name.read(cx).value().trim().to_string();
        let role = self.edit_role.read(cx).value().trim().to_string();
        let Some(id) = self.bots.get(self.selected).map(|b| b.id) else { return };
        let taken = self.bots.iter().any(|b| b.id != id && b.name.eq_ignore_ascii_case(&name));
        self.edit_error = match () {
            _ if name.is_empty() || name.chars().count() > 24 => Some("Use a name of 1 to 24 characters".into()),
            _ if taken => Some("Another bot already has this name (@mentions need unique names)".into()),
            _ if role.is_empty() => Some("Write a role, even a short one".into()),
            _ => None,
        };
        if self.edit_error.is_none()
            && let Some(bot) = self.bots.get_mut(self.selected)
        {
            bot.name = name;
            bot.role = (role != bot.preset().role).then_some(role);
            self.panel = Panel::None;
            self.save();
            self.input.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    fn remove_schedule(&mut self, id: usize, cx: &mut Context<Self>) {
        if let Some(bot) = self.bots.get_mut(self.selected) {
            bot.schedules.retain(|s| s.id != id);
            self.save();
            cx.notify();
        }
    }

    fn toggle_skills(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = if self.panel == Panel::Skills { Panel::None } else { Panel::Skills };
        if self.panel == Panel::Skills {
            self.clear_skill_form(window, cx);
            self.skill_name.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    fn toggle_schedules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = if self.panel == Panel::Schedules { Panel::None } else { Panel::Schedules };
        self.sched_error = None;
        if self.panel == Panel::Schedules {
            self.sched_prompt.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Panel::None;
        self.focus_main(window, cx);
        cx.notify();
    }

    fn clear_skill_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.skill_at = None;
        self.skill_error = None;
        self.skill_name.update(cx, |s, cx| s.set_value("", window, cx));
        self.skill_body.update(cx, |s, cx| s.set_value("", window, cx));
    }

    fn edit_skill(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(skill) = self.bots.get(self.selected).and_then(|b| b.skills.get(index)).cloned() else { return };
        self.skill_at = Some(index);
        self.skill_error = None;
        self.skill_name.update(cx, |s, cx| {
            s.set_value(skill.name, window, cx);
            s.focus(window, cx);
        });
        self.skill_body.update(cx, |s, cx| s.set_value(skill.body, window, cx));
        cx.notify();
    }

    /// Writes the form onto this bot's own list. The next turn sends it with the role.
    fn save_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.skill_name.read(cx).value().to_string();
        let body = self.skill_body.read(cx).value().to_string();
        let at = self.skill_at;
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        match skills::upsert(&mut bot.skills, at, &name, &body) {
            Ok(()) => {
                self.save();
                self.clear_skill_form(window, cx);
            }
            Err(e) => self.skill_error = Some(e.into()),
        }
        cx.notify();
    }

    fn remove_skill(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        if !skills::remove(&mut bot.skills, index) {
            return;
        }
        if self.skill_at == Some(index) {
            self.clear_skill_form(window, cx);
        } else if let Some(at) = self.skill_at.filter(|at| *at > index) {
            self.skill_at = Some(at - 1);
        }
        self.save();
        cx.notify();
    }

    fn continue_chain(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.iter().find(|b| b.id == id) else { return };
        // the hop button does not override a full plan window; the banner says why
        if self.breach_of(bot.provider).is_some_and(|b| b.level == usage::Level::Pause) {
            cx.notify();
            return;
        }
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        let resumed = match bot.msgs.get_mut(i) {
            Some(Msg::Handoff { prompt, paused, room, .. }) if *paused => {
                *paused = false;
                Some((prompt.clone(), *room))
            }
            _ => None,
        };
        let Some((prompt, room_id)) = resumed else { return };
        if let Some(rid) = room_id
            && let Some(room) = self.rooms.iter_mut().find(|r| r.id == rid)
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
    fn continue_room(&mut self, room_id: usize, bot_id: usize, cx: &mut Context<Self>) {
        let Some(i) = self.bots.iter().find(|b| b.id == bot_id).and_then(|b| b.msgs.iter().rposition(|m| matches!(m, Msg::Handoff { paused: true, room: Some(rid), .. } if *rid == room_id))) else {
            return;
        };
        self.continue_chain(bot_id, i, cx);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
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
                let quiet = ok && fresh_turn && reply.trim().trim_end_matches('.') == "QUIET";
                if quiet {
                    let tail = bot.msgs.split_off(start);
                    bot.msgs.extend(tail.into_iter().filter(|m| !matches!(m, Msg::Bot(_))));
                    bot.msgs.push(Msg::Divider("Nothing to report".into()));
                }
                let (name, color) = (bot.name.clone(), bot.color());
                // engine down: keep the hop queued instead of starting it into the same failure
                let engine_down = failed.as_deref().is_some_and(sandbox::engine_down);
                if engine_down && let Some(turn) = finished.filter(handoff::Pending::inflight) {
                    bot.queue.insert(0, turn);
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
                let next = if !engine_down && self.may_start(provider) { self.bots.iter_mut().find(|b| b.id == id).and_then(|b| (!b.queue.is_empty()).then(|| b.queue.remove(0))) } else { None };
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

    /// Deletes the bot, its container and its scratch folder; never a mounted folder.
    fn delete(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(i) = self.bots.iter().position(|b| b.id == id) else { return };
        if let Some(run) = &self.bots[i].run {
            run.stop();
        }
        self.bots.remove(i);
        for room in &mut self.rooms {
            (room.members, room.facilitator) = room::forget(std::mem::take(&mut room.members), room.facilitator, id);
        }
        for group in &mut self.groups {
            group.members = group::forget(std::mem::take(&mut group.members), id);
        }
        self.selected = self.selected.min(self.bots.len().saturating_sub(1));
        self.confirm_delete = None;
        self.save();
        let scratch = data_dir().join("bots").join(id.to_string());
        cx.background_executor()
            .spawn(async move {
                sandbox::remove(id);
                let _ = std::fs::remove_dir_all(scratch);
            })
            .detach();
        cx.notify();
    }

    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get(self.selected) else { return };
        if bot.folders.len() >= sandbox::MAX_MOUNTS {
            self.folder_error = Some(format!("A bot can mount at most {} folders.", sandbox::MAX_MOUNTS));
            cx.notify();
            return;
        }
        let id = bot.id;
        let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Mount folders".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            if paths.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                let show = this.bots.get(this.selected).is_some_and(|b| b.id == id);
                if let Some(b) = this.bots.iter_mut().find(|b| b.id == id) {
                    let err = sandbox::add_mounts(&mut b.folders, &paths).err();
                    if show {
                        this.folder_error = err;
                    }
                }
                this.save();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remove_folder(&mut self, id: usize, index: usize, cx: &mut Context<Self>) {
        if let Some(bot) = self.bots.iter_mut().find(|b| b.id == id)
            && index < bot.folders.len()
        {
            bot.folders.remove(index);
        }
        self.folder_error = None;
        self.save();
        cx.notify();
    }

    /// The login finishes in Terminal; check every 5 s for 5 minutes and tell the user when it works.
    fn watch_sign_in(&mut self, provider: Provider, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..60 {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let ok = cx
                    .background_executor()
                    .spawn(async move {
                        match provider {
                            Provider::Claude => sandbox::claude_signed_in(),
                            Provider::Codex => codex::account().is_ok(),
                        }
                    })
                    .await;
                if ok {
                    this.update(cx, |this, cx| this.signed_in(provider, cx)).ok();
                    return;
                }
            }
        })
        .detach();
    }

    /// Turns each bot's latest "not signed in" error for `provider` into a green notice with "Send again".
    fn signed_in(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let marker = if provider == Provider::Codex { "codex login" } else { "/login" };
        for bot in &mut self.bots {
            let Some(k) = bot.msgs.iter().rposition(|m| matches!(m, Msg::Error(t) if t.contains(marker))) else { continue };
            let prompt = bot.msgs[..k].iter().rev().find_map(|m| match m {
                Msg::User(t) => Some(t.clone()),
                Msg::Handoff { prompt, .. } | Msg::Scheduled { prompt, .. } | Msg::Kickoff { prompt, .. } => Some(prompt.clone()),
                _ => None,
            });
            bot.msgs[k] = Msg::SignedIn { provider, prompt };
        }
        if provider == Provider::Codex {
            self.codex_query = None;
            self.refresh_codex(1, cx);
        }
        self.save();
        cx.notify();
    }

    /// The bot saves its notes, then its next turn starts a new session.
    fn fresh_start(&mut self, cx: &mut Context<Self>) {
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

    fn send_again(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.iter().find(|b| b.id == id) else { return };
        if bot.busy() || !self.may_start(bot.provider) {
            cx.notify();
            return;
        }
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        if let Some(Msg::SignedIn { prompt, .. }) = bot.msgs.get_mut(i)
            && let Some(text) = prompt.take()
        {
            bot.msgs.push(Msg::User(text.clone()));
            self.start_turn(id, handoff::Pending::user(text), cx);
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

    /// Codex usage and model list, from a throwaway container (no turn needed).
    /// With `tries` > 1 it keeps asking every 5 s, e.g. while the user finishes signing in.
    fn refresh_codex(&mut self, tries: u32, cx: &mut Context<Self>) {
        if self.codex_query == Some(None) {
            return;
        }
        self.codex_query = Some(None);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut answer = Err(String::new());
            for attempt in 0..tries {
                if attempt > 0 {
                    cx.background_executor().timer(Duration::from_secs(5)).await;
                }
                answer = cx.background_executor().spawn(async { codex::account() }).await;
                if answer.is_ok() {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                match answer {
                    Ok((meter, models)) => {
                        this.set_meter(meter, cx);
                        this.codex_models = models;
                        this.codex_query = None;
                        this.selects_stale = true;
                        this.save();
                    }
                    Err(e) => this.codex_query = Some(Some(e)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_setup(&mut self, cx: &mut Context<Self>) {
        if self.setup.is_some() {
            return;
        }
        let unknown = Check::Unknown;
        self.setup = Some(Setup { engine: unknown.clone(), running: unknown.clone(), image: unknown.clone(), claude: unknown.clone(), codex: unknown });
        self.panel = Panel::None;
        cx.notify();
        // re-check every few seconds while the checklist is open; installs and sign-ins finish outside eggbot
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(before)) = this.update(cx, |this, _| this.setup.clone()) else { return };
                let after = cx
                    .background_executor()
                    .spawn(async move {
                        let ok = |c: &Check, now: bool| match (c, now) {
                            (_, true) => Check::Ok,
                            (Check::Busy(_) | Check::Failed(_), false) => c.clone(),
                            _ => Check::Missing,
                        };
                        let engine = ok(&before.engine, sandbox::installed());
                        let running = ok(&before.running, engine == Check::Ok && sandbox::running());
                        let image = ok(&before.image, running == Check::Ok && sandbox::image_ready());
                        // sign-in checks start a container, so they stop once they pass
                        let signed = |c: &Check, check: &dyn Fn() -> bool| if *c == Check::Ok { Check::Ok } else { ok(c, image == Check::Ok && check()) };
                        let claude = signed(&before.claude, &sandbox::claude_signed_in);
                        let codex = signed(&before.codex, &|| codex::account().is_ok());
                        Setup { engine, running, image, claude, codex }
                    })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.setup.is_some() {
                            this.setup = Some(after);
                            cx.notify()
                        }
                    })
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(4)).await;
            }
        })
        .detach();
    }

    /// Runs a setup action off the main thread; its row shows `label` until the next check, or the error.
    fn setup_action(&mut self, row: fn(&mut Setup) -> &mut Check, label: &'static str, action: fn() -> Result<(), String>, cx: &mut Context<Self>) {
        let Some(setup) = &mut self.setup else { return };
        *row(setup) = Check::Busy(label);
        cx.notify();
        let task = cx.background_executor().spawn(async move { action() });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                this.update(cx, |this, cx| {
                    if let Some(setup) = &mut this.setup {
                        *row(setup) = Check::Failed(e);
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn sign_in(&mut self, codex: bool, cx: &mut Context<Self>) {
        self.watch_sign_in(if codex { Provider::Codex } else { Provider::Claude }, cx);
        let Some(id) = self.bots.get(self.selected).map(|b| b.id) else { return };
        let opening = cx.background_executor().spawn(async move { sandbox::ready(&|_| {}).and_then(|_| sandbox::sign_in(codex)) });
        cx.spawn(async move |this, cx| {
            if let Err(e) = opening.await {
                this.update(cx, |this, cx| {
                    if let Some(b) = this.bots.iter_mut().find(|b| b.id == id) {
                        b.msgs.push(Msg::Error(format!("Could not open the sign-in: {e}")));
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
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
    use super::*;
    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
    use core::prelude::v1::test;

    #[test]
    fn handoff_queue_loads_from_state_json() {
        let saved = r#"{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"queue":[{"prompt":"ship it","hops":2,"fresh":false,"handoff":true}],"current":{"prompt":"look","hops":1,"fresh":false,"handoff":true}}"#;
        let bot: Bot = serde_json::from_str(saved).unwrap();
        assert_eq!(bot.queue.len(), 1);
        assert!(bot.queue[0].inflight());
        assert!(bot.queue[0].room.is_none());
        assert_eq!(bot.current.as_ref().map(|p| p.hops), Some(1));
        assert!(bot.current.as_ref().unwrap().room.is_none());
        let restored = handoff::restore(bot.current, bot.queue);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].prompt, "look");

        let old = r#"{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[]}"#;
        let bot: Bot = serde_json::from_str(old).unwrap();
        assert!(bot.queue.is_empty());
        assert!(bot.current.is_none());
        assert!(bot.recent.is_empty());
        // a bot saved before skills stays empty; preset defaults are applied only at hatch
        assert!(bot.skills.is_empty());
        assert!(!skills::defaults("Reviewer").is_empty());
    }

    #[test]
    fn skills_round_trip_beside_queue_and_folders() {
        let raw = r#"{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"folders":[{"path":"/tmp/proj","name":"proj"}],"queue":[{"prompt":"ship it","hops":1,"fresh":false,"handoff":true}],"skills":[{"name":"Check","body":"Run tests."}]}"#;
        let bot: Bot = serde_json::from_str(raw).unwrap();
        assert_eq!(bot.skills.len(), 1);
        assert_eq!(bot.skills[0].body, "Run tests.");
        assert_eq!(bot.queue[0].prompt, "ship it");
        assert!(bot.queue[0].room.is_none());
        assert_eq!(bot.folders[0].name, "proj");
        let again: Bot = serde_json::from_str(&serde_json::to_string(&bot).unwrap()).unwrap();
        assert_eq!(again.skills, bot.skills);
        assert_eq!(again.queue[0].prompt, "ship it");
        assert!(again.queue[0].room.is_none());
        assert_eq!(again.folders[0].name, "proj");
        assert!(again.current.is_none());
        assert!(again.recent.is_empty());
    }

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

    #[test]
    fn state_json_keeps_queue_current_and_limits() {
        let raw = r#"{"next_id":2,"bots":[{"id":1,"name":"Reviewer","preset":0,"sandbox_session":null,"msgs":[],"folders":[{"path":"/tmp/proj","name":"proj"}],"queue":[{"prompt":"ship it","hops":2,"fresh":false,"handoff":true}],"current":{"prompt":"look","hops":1,"fresh":false,"handoff":true}}],"meters":[],"throttle":0.9,"pause":0.95}"#;
        let saved: Saved = serde_json::from_str(raw).unwrap();
        assert_eq!(usage::percent(saved.throttle), 90);
        assert_eq!(usage::percent(saved.pause), 95);
        assert!(saved.bots[0].skills.is_empty());
        assert!(saved.bots[0].queue[0].inflight());
        assert_eq!(saved.bots[0].current.as_ref().map(|p| p.prompt.as_str()), Some("look"));
        assert_eq!(saved.bots[0].folders[0].name, "proj");
        // same keys save() writes; queue, current, and folders ride inside bots
        let state = serde_json::json!({ "next_id": saved.next_id, "bots": saved.bots, "meters": saved.meters, "sidebar_w": saved.sidebar_w, "sidebar_open": saved.sidebar_open, "appearance": saved.appearance, "shared": saved.shared, "throttle": saved.throttle, "pause": saved.pause, "next_room_id": saved.next_room_id, "rooms": saved.rooms, "next_group_id": saved.next_group_id, "groups": saved.groups });
        let again: Saved = serde_json::from_value(state).unwrap();
        assert_eq!(again.bots[0].queue[0].prompt, "ship it");
        assert_eq!(again.bots[0].current.as_ref().unwrap().hops, 1);
        assert_eq!(again.bots[0].folders[0].name, "proj");
        assert!(again.bots[0].skills.is_empty());
        assert!(again.bots[0].folder.is_none());
        assert_eq!(usage::percent(again.pause), 95);
        assert!(saved.rooms.is_empty());
        assert!(saved.groups.is_empty());
        assert_eq!(again.next_room_id, 0);
        assert_eq!(again.next_group_id, 0);

        let legacy = r#"{"next_id":1,"bots":[]}"#;
        let old: Saved = serde_json::from_str(legacy).unwrap();
        assert_eq!(old.throttle, usage::DEFAULT_THROTTLE);
        assert_eq!(old.pause, usage::DEFAULT_PAUSE);
        assert!(old.rooms.is_empty());
        assert!(old.groups.is_empty());
        assert_eq!(old.next_group_id, 0);
    }
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
mod tests {
    use super::{Bot, Saved};

    #[test]
    fn sidebar_stays_open_when_state_has_no_flag() {
        let saved: Saved = serde_json::from_str(r#"{"next_id":0,"bots":[]}"#).unwrap();
        assert!(saved.sidebar_open);
        assert_eq!(saved.sidebar_w, 260.);
    }

    #[test]
    fn sidebar_closed_round_trips() {
        let saved: Saved = serde_json::from_str(r#"{"next_id":1,"bots":[],"sidebar_open":false,"sidebar_w":300.0}"#).unwrap();
        assert!(!saved.sidebar_open);
        assert_eq!(saved.sidebar_w, 300.);
    }

    #[test]
    fn legacy_folder_field_becomes_a_mount() {
        let dir = std::env::temp_dir().join(format!("eggbot-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = format!(r#"{{"id":1,"name":"Reviewer","preset":0,"folder":{},"msgs":[]}}"#, serde_json::to_string(&dir).unwrap());
        let mut bot: Bot = serde_json::from_str(&json).unwrap();
        assert!(bot.folders.is_empty());
        crate::sandbox::adopt_legacy(&mut bot.folders, bot.folder.take());
        let saved = serde_json::to_value(&bot).unwrap();
        assert!(saved.get("folder").is_none());
        let path = bot.folders[0].path.to_string_lossy();
        assert_eq!(saved["folders"][0]["path"].as_str(), Some(path.as_ref()));
        assert_eq!(saved["folders"][0]["name"], bot.folders[0].name);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rooms_round_trip_and_old_state_has_none() {
        let old: Saved = serde_json::from_str(r#"{"next_id":0,"bots":[]}"#).unwrap();
        assert!(old.rooms.is_empty());
        assert!(old.groups.is_empty());
        assert_eq!(old.next_room_id, 0);
        assert_eq!(old.next_group_id, 0);
        let saved: Saved = serde_json::from_str(
            r#"{"next_id":2,"bots":[],"next_room_id":4,"rooms":[{"id":3,"title":"Standup","kickoff":"What shipped?","members":[1,2],"facilitator":1,"unread":true,"started":true}]}"#,
        )
        .unwrap();
        assert_eq!(saved.next_room_id, 4);
        assert_eq!(saved.rooms[0].title, "Standup");
        assert_eq!(saved.rooms[0].members, vec![1, 2]);
        assert_eq!(saved.rooms[0].facilitator, Some(1));
        assert!(saved.rooms[0].unread && saved.rooms[0].started);
        assert!(saved.rooms[0].transcript.is_empty());
        assert_eq!(saved.throttle, super::usage::DEFAULT_THROTTLE);
        assert_eq!(saved.pause, super::usage::DEFAULT_PAUSE);
        let with_log: Saved = serde_json::from_str(r#"{"next_id":1,"bots":[],"rooms":[{"id":1,"title":"S","kickoff":"Hi","members":[1],"transcript":[{"Kickoff":{"to":1,"name":"Reviewer","text":"Hi"}},{"Reply":{"bot":1,"name":"Reviewer","color":1,"text":"Done"}},{"Handoff":{"from":1,"from_name":"Reviewer","color":1,"to":2,"to_name":"Implementer","paused":false}}]}]}"#).unwrap();
        assert_eq!(with_log.rooms[0].transcript.len(), 3);
        let again: Saved = serde_json::from_str(&serde_json::to_string(&with_log).unwrap()).unwrap();
        assert_eq!(again.rooms[0].transcript, with_log.rooms[0].transcript);
    }

    #[test]
    fn groups_round_trip_and_old_state_has_none() {
        let old: Saved = serde_json::from_str(r#"{"next_id":0,"bots":[]}"#).unwrap();
        assert!(old.groups.is_empty());
        assert_eq!(old.next_group_id, 0);
        let saved: Saved = serde_json::from_str(r#"{"next_id":2,"bots":[],"next_group_id":4,"groups":[{"id":3,"title":"Reviewers","members":[1,2]}]}"#).unwrap();
        assert_eq!(saved.next_group_id, 4);
        assert_eq!(saved.groups[0].title, "Reviewers");
        assert_eq!(saved.groups[0].members, vec![1, 2]);
        assert!(saved.rooms.is_empty());
        let again: Saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(again.groups, saved.groups);
        assert_eq!(again.next_group_id, 4);
    }

    #[test]
    fn room_kickoff_resumes_like_a_handoff_without_spending_a_hop() {
        let prompt = super::room::prompt("Standup", "What shipped?", &[super::room::Peer { name: "Implementer", blurb: "Writes code" }]);
        let mut turn = super::handoff::Pending::handoff(prompt, 0);
        turn.room = Some(4);
        assert!(turn.inflight());
        assert_eq!(turn.hops, 0);
        assert!(!turn.fresh);
        let restored = super::handoff::restore(Some(turn), vec![]);
        assert_eq!(restored[0].room, Some(4));
        assert_eq!(restored[0].hops, 0);
    }

    #[test]
    fn reply_text_ignores_lines_from_before_the_turn() {
        let msgs = vec![
            super::Msg::Bot("earlier".into()),
            super::Msg::Kickoff { room_id: 1, room: "Standup".into(), text: "go".into(), prompt: "prompt".into() },
            super::Msg::Bot("partial".into()),
            super::Msg::User("meanwhile".into()),
            super::Msg::Bot("other".into()),
        ];
        assert_eq!(super::reply_text(&msgs, 4), "other");
        assert_eq!(super::reply_text(&msgs, msgs.len()), "");
    }
}
