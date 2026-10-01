mod claude;
mod codex;
mod egg;
mod handoff;
mod login;
mod notify;
mod sandbox;
mod schedule;
mod tray;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use claude::{Meter, Provider};
use egg::Mood;
use gpui_kit::assets::Assets;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::select::{SelectEvent, SelectItem, SelectState};
use gpui_kit::component::Theme;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

actions!(eggbot, [Quit, CloseWindow, NewBot, FocusInput, PrevBot, NextBot, StopTurn, CycleAppearance, OpenSettings, OpenSetup, Find, ToggleSidebar]);

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
const NOTES: &str = " You keep your own notes in /memory/NOTES.md. When a session starts, read it if it exists. Keep it short and current: durable facts about the user, the project, decisions and open work, never chat logs.";
const QUIET: &str = "\n\n(This is a scheduled run. If nothing here needs the user's attention, reply with exactly QUIET and nothing else.)";
const FRESH_START: &str = "We are about to start a fresh session. Update /memory/NOTES.md now with everything worth keeping from this session, then reply with one short line.";

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
    Tool { id: String, verb: String, target: String, detail: String, open: bool },
    Error(String),
    /// Work handed over by another bot; `paused` when the chain hit the hop limit.
    Handoff { from: String, color: u32, prompt: String, text: String, paused: bool, open: bool },
    Sent { to: String },
    /// Marks where a fresh session began.
    Divider(String),
    /// A turn started by a schedule, shown where a user message would be.
    Scheduled { prompt: String, label: String },
    /// Replaces a "not signed in" error once the login works; `prompt` is what failed, for "Send again".
    SignedIn { provider: Provider, prompt: Option<String> },
}

impl Msg {
    /// The text search looks in: what people and bots wrote, not tool output.
    fn searchable(&self) -> Option<&str> {
        match self {
            Msg::User(t) | Msg::Bot(t) | Msg::Handoff { text: t, .. } | Msg::Scheduled { prompt: t, .. } => Some(t),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Bot {
    id: usize,
    name: String,
    preset: usize,
    #[serde(default)]
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
    #[serde(skip)]
    run: Option<Arc<claude::Handle>>,
    #[serde(skip)]
    status: Option<String>,
    #[serde(skip)]
    stopped: bool,
    /// Handoff hops that led to the current turn (0 = started by the user).
    #[serde(skip)]
    hops: u32,
    /// Handoffs waiting for the current turn to end: (prompt, hops).
    #[serde(skip)]
    queue: Vec<(String, u32, bool)>,
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
    /// The running turn's (prompt, hops, fresh), to run it again after a sign-in clash.
    #[serde(skip)]
    current: Option<(String, u32, bool)>,
    /// The running turn is already that one retry.
    #[serde(skip)]
    retried: bool,
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
}

fn default_sidebar() -> f32 {
    260.
}

fn default_sidebar_open() -> bool {
    true
}

struct Eggbot {
    p: Palette,
    bots: Vec<Bot>,
    selected: usize,
    next_id: usize,
    menu_open: bool,
    /// Bot id whose trash icon was clicked once; a second click deletes.
    confirm_delete: Option<usize>,
    meters: Vec<Meter>,
    /// Codex models this account can use; fetched on demand.
    codex_models: Vec<codex::Model>,
    /// None = idle; Some(None) = asking now; Some(Some(e)) = the last question failed with `e`.
    codex_query: Option<Option<String>>,
    tray: Option<tray::Tray>,
    input: Entity<TextareaState>,
    sched_open: bool,
    /// 0 daily, 1 weekdays, 2 every N hours, 3 every N minutes
    sched_kind: usize,
    sched_prompt: Entity<InputState>,
    sched_value: Entity<InputState>,
    sched_error: Option<String>,
    edit_open: bool,
    edit_name: Entity<InputState>,
    edit_role: Entity<TextareaState>,
    settings_open: bool,
    edit_shared: Entity<TextareaState>,
    shared: Option<String>,
    login_error: Option<String>,
    edit_error: Option<String>,
    model_select: Entity<SelectState<Vec<Choice>>>,
    effort_select: Entity<SelectState<Vec<Choice>>>,
    /// Dropdown options need refilling (bot, provider or Codex model list changed); done in render.
    selects_stale: bool,
    sidebar_w: f32,
    /// Bot list visible. The width is kept while it is closed.
    sidebar_open: bool,
    appearance: Appearance,
    /// The window is in front; otherwise news goes out as notifications.
    active: bool,
    /// ⌘F search in the open chat: the query field, matching message indices (oldest first) and the current one.
    find_open: bool,
    find_input: Entity<InputState>,
    find_hits: Vec<usize>,
    find_at: usize,
    /// The first-run checklist, shown in place of the chat while open.
    setup: Option<Setup>,
    /// The sidebar row being dragged, to hide drop lines that would change nothing.
    dragging: Option<usize>,
    /// Dragging the sidebar's edge.
    resizing: bool,
    /// The chat's virtual list: one row per message of the selected bot, plus the typing row.
    list: ListState,
    /// The bot whose messages `list` holds.
    list_bot: Option<usize>,
}

impl Eggbot {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Enter sends, Shift+Enter adds a line; grows up to 8 lines
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx).placeholder("Message…").submit_on_enter(true);
            input.set_auto_grow(1, 8, cx);
            input
        });
        cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = ev {
                this.send(window, cx);
            }
        })
        .detach();
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search this chat"));
        cx.subscribe_in(&find_input, window, |this, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.find_update(cx),
            // Enter walks back in time, ⇧Enter forward
            InputEvent::PressEnter { shift, .. } => this.find_step(if *shift { 1 } else { -1 }, cx),
            _ => {}
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
        let model_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let effort_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        cx.subscribe_in(&model_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
            // values look like "claude", "claude:opus", "codex", "codex:<model id>"
            let SelectEvent::Confirm(Some(Some(value))) = ev else { return };
            let (provider, model) = match value.split_once(':') {
                Some((p, m)) => (p, Some(m.to_string())),
                None => (value.as_str(), None),
            };
            let provider = if provider == "codex" { Provider::Codex } else { Provider::Claude };
            let Some(b) = this.bots.get_mut(this.selected) else { return };
            if b.provider != provider {
                b.provider = provider;
                // the meter tracks the provider's session; it refills on the next turn
                b.context = (0, 0);
            }
            // effort levels differ per model; fall back to the default level
            b.model = model;
            b.effort = None;
            if provider == Provider::Codex && this.codex_models.is_empty() {
                this.refresh_codex(1, cx);
            }
            this.selects_stale = true;
            this.save();
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&effort_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
            let SelectEvent::Confirm(Some(effort)) = ev else { return };
            if let Some(b) = this.bots.get_mut(this.selected) {
                b.effort = effort.clone();
                this.save();
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(&edit_name, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.save_edit(window, cx);
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
        cx.spawn(async move |this, cx| {
            // first check soon after launch, so runs missed while eggbot was closed happen once
            let mut wait = Duration::from_secs(3);
            loop {
                cx.background_executor().timer(wait).await;
                if this.update(cx, |this, cx| this.run_due(cx)).is_err() {
                    break;
                }
                wait = Duration::from_secs(20);
            }
        })
        .detach();
        let p = Palette::apply(window, cx);
        let saved: Option<Saved> = std::fs::read(data_dir().join("state.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let appearance = saved.as_ref().map_or_else(Appearance::default, |s| s.appearance);
        let first_launch = saved.is_none();
        let mut this = Self { p, bots: vec![], selected: 0, next_id: 0, menu_open: false, confirm_delete: None, meters: vec![], codex_models: vec![], codex_query: None, tray: None, sched_open: false, sched_kind: 0, sched_prompt, sched_value, sched_error: None, edit_open: false, edit_name, edit_role, edit_error: None, settings_open: false, edit_shared, shared: None, login_error: None, model_select, effort_select, selects_stale: true, sidebar_w: default_sidebar(), sidebar_open: default_sidebar_open(), appearance, active: false, find_open: false, find_input, find_hits: vec![], find_at: 0, setup: None, dragging: None, resizing: false, input, list: ListState::new(0, ListAlignment::Bottom, px(800.)), list_bot: None };
        this.list.set_follow_mode(FollowMode::Tail);
        match saved {
            Some(s) if !s.bots.is_empty() => {
                (this.bots, this.next_id, this.meters, this.sidebar_w, this.sidebar_open, this.shared) = (s.bots, s.next_id, s.meters, s.sidebar_w, s.sidebar_open, s.shared);
            }
            _ => {
                for i in 0..3 {
                    this.hatch(i);
                }
                this.selected = 0;
            }
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
        let (clicks, actions) = async_channel::unbounded();
        notify::init(clicks.clone());
        this.tray = tray::Tray::new(clicks);
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(action) = actions.recv().await {
                let done = this.update_in(cx, |this, window, cx| match action {
                    tray::Action::Open => this.show(window, cx),
                    tray::Action::Bot(id) => {
                        this.show(window, cx);
                        if let Some(i) = this.bots.iter().position(|b| b.id == id) {
                            this.select(i, window, cx);
                        }
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
        this
    }

    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        set_dock_icon(true);
        cx.activate(true);
        window.activate_window();
    }

    /// Asks before stopping working bots; their containers stay for a fast next launch.
    fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        for b in self.bots.iter().filter(|b| b.busy()) {
            sandbox::interrupt(b.id);
        }
        self.save();
        cx.quit();
    }

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
        let state = serde_json::json!({ "next_id": self.next_id, "bots": self.bots, "meters": self.meters, "sidebar_w": self.sidebar_w, "sidebar_open": self.sidebar_open, "appearance": self.appearance, "shared": self.shared });
        // write then rename, so a crash mid-write never loses the history
        let tmp = dir.join("state.json.tmp");
        let ok = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&tmp, state.to_string())).and_then(|_| std::fs::rename(&tmp, dir.join("state.json")));
        if let Err(e) = ok {
            eprintln!("eggbot: could not save state: {e}");
        }
    }

    fn hatch(&mut self, preset: usize) {
        let base = PRESETS[preset].name;
        let taken = |n: &str| self.bots.iter().any(|b| b.name == n);
        let name = (1..).map(|i| if i == 1 { base.to_string() } else { format!("{base} {i}") }).find(|n| !taken(n)).unwrap();
        self.bots.push(Bot {
            id: self.next_id,
            name,
            preset,
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
            run: None,
            status: None,
            stopped: false,
            hops: 0,
            queue: vec![],
            fresh_turn: false,
            refreshing: false,
            codex_role: None,
            pending_role: None,
            context: (0, 0),
            unread: false,
            current: None,
            retried: false,
        });
        self.next_id += 1;
        self.selected = self.bots.len() - 1;
        self.selects_stale = true;
        self.save();
    }

    fn select(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = i;
        self.mark_read(cx);
        self.selects_stale = true;
        self.menu_open = false;
        self.confirm_delete = None;
        self.edit_open = false;
        self.sched_open = false;
        self.settings_open = false;
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
        (self.settings_open, self.edit_open, self.sched_open, self.login_error) = (true, false, false, None);
        cx.notify();
    }

    /// Saves the instructions for all bots; each bot gets them from its next turn (both providers).
    fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.edit_shared.read(cx).value().trim().to_string();
        self.shared = (text != SHARED).then_some(text);
        self.settings_open = false;
        self.save();
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn set_login(&mut self, on: bool, cx: &mut Context<Self>) {
        self.login_error = login::set(on).err();
        cx.notify();
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

    /// The selected bot has been seen.
    fn mark_read(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = self.bots.get_mut(self.selected).filter(|b| b.unread) {
            b.unread = false;
            self.save();
            cx.notify();
        }
    }

    /// News from a bot: unread unless the user is looking at it, and a notification while eggbot is in the background.
    fn alert(&mut self, id: usize, title: &str, body: &str) {
        if self.active && self.bots.get(self.selected).is_some_and(|b| b.id == id) {
            return;
        }
        if let Some(b) = self.bots.iter_mut().find(|b| b.id == id) {
            b.unread = true;
        }
        if !self.active {
            let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
            notify::send(id, title, &body);
        }
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_string();
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        if text.is_empty() || bot.busy() {
            return;
        }
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        bot.msgs.push(Msg::User(text.clone()));
        let id = bot.id;
        self.start_turn(id, text, 0, false, cx);
    }

    /// `fresh` runs the turn in a throwaway session (schedules), leaving the main session untouched.
    fn start_turn(&mut self, id: usize, prompt: String, hops: u32, fresh: bool, cx: &mut Context<Self>) {
        let blurbs: Vec<String> = self.bots.iter().map(|b| b.blurb()).collect();
        let roster: Vec<(usize, &str, &str)> = self.bots.iter().zip(&blurbs).map(|(b, blurb)| (b.id, b.name.as_str(), blurb.as_str())).collect();
        let others = handoff::roster(&roster, id);
        let shared = self.shared.clone().unwrap_or_else(|| SHARED.into());
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        bot.stopped = false;
        bot.hops = hops;
        bot.fresh_turn = fresh;
        bot.current = Some((prompt.clone(), hops, fresh));
        // separate folders, so a bot without a project never sees its notes inside /work
        let home = data_dir().join("bots").join(id.to_string());
        let (scratch, memory) = (home.join("work"), home.join("memory"));
        for dir in [&scratch, &memory] {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("eggbot: could not create {}: {e}", dir.display());
            }
        }
        let role = format!("{}{others}{NOTES}\n\n{shared}", bot.role());
        let send_role = bot.provider == Provider::Codex && (fresh || bot.thread.is_none() || bot.codex_role.as_ref() != Some(&role));
        bot.pending_role = (send_role && !fresh).then(|| role.clone());
        let turn = claude::Turn {
            send_role,
            bot: id,
            mount: bot.folder.clone().unwrap_or(scratch),
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

    /// Starts the turn now, or queues it behind the bot's current turn.
    fn deliver(&mut self, id: usize, prompt: String, hops: u32, fresh: bool, cx: &mut Context<Self>) {
        match self.bots.iter_mut().find(|b| b.id == id) {
            Some(b) if b.busy() => b.queue.push((prompt, hops, fresh)),
            Some(_) => self.start_turn(id, prompt, hops, fresh, cx),
            None => {}
        }
    }

    /// Sends the finished reply to every bot it mentions as @Name; false when it mentions nobody.
    fn hand_off(&mut self, from: usize, reply: String, hops: u32, cx: &mut Context<Self>) -> bool {
        let names: Vec<(usize, &str)> = self.bots.iter().map(|b| (b.id, b.name.as_str())).collect();
        let targets = handoff::mentions(&reply, &names, from);
        let handed = !targets.is_empty();
        let Some(sender) = self.bots.iter().find(|b| b.id == from) else { return false };
        let (from_name, color, folder) = (sender.name.clone(), sender.color(), sender.folder.clone());
        let next = hops + 1;
        for to in targets {
            let Some(target) = self.bots.iter_mut().find(|b| b.id == to) else { continue };
            let prompt = handoff::prompt(&from_name, &reply, folder.is_some() && folder == target.folder);
            let paused = next > handoff::MAX_HOPS;
            target.msgs.push(Msg::Handoff { from: from_name.clone(), color, prompt: prompt.clone(), text: reply.clone(), paused, open: false });
            let to_name = target.name.clone();
            if let Some(s) = self.bots.iter_mut().find(|b| b.id == from) {
                s.msgs.push(Msg::Sent { to: to_name.clone() });
            }
            if paused {
                self.alert(to, "Chain paused", &format!("{from_name} handed off to {to_name} after {} hops. Open eggbot to continue.", handoff::MAX_HOPS));
            } else {
                self.deliver(to, prompt, next, false, cx);
            }
        }
        handed
    }

    /// Starts (or queues) every schedule that is due.
    fn run_due(&mut self, cx: &mut Context<Self>) {
        let now = chrono::Local::now();
        let mut due = vec![];
        for b in &mut self.bots {
            for s in b.schedules.iter_mut().filter(|s| s.due(now)) {
                s.anchor = now.timestamp();
                b.msgs.push(Msg::Scheduled { prompt: s.prompt.clone(), label: s.repeat.label() });
                due.push((b.id, format!("{}{QUIET}", s.prompt)));
            }
        }
        if due.is_empty() {
            return;
        }
        for (id, prompt) in due {
            self.deliver(id, prompt, 0, true, cx);
        }
        self.save();
        cx.notify();
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
        self.edit_open = true;
        self.sched_open = false;
        self.settings_open = false;
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
            self.edit_open = false;
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

    fn continue_chain(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        if let Some(Msg::Handoff { prompt, paused, .. }) = bot.msgs.get_mut(i)
            && *paused
        {
            *paused = false;
            let prompt = prompt.clone();
            self.deliver(id, prompt, 0, false, cx);
            self.save();
            cx.notify();
        }
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
            self.set_meter(meter);
            cx.notify();
            return;
        }
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
                bot.msgs.retain(|m| !matches!(m, Msg::Bot(s) if s.is_empty()));
                // documented transient error when bots renew the shared Claude login at the same moment
                let clash = !bot.stopped && error.as_deref().is_some_and(|e| e.contains("process is refreshing it"));
                if clash && !std::mem::take(&mut bot.retried)
                    && let Some((prompt, hops, fresh)) = bot.current.clone()
                {
                    bot.retried = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(5)).await;
                        this.update(cx, |this, cx| this.deliver(id, prompt, hops, fresh, cx)).ok();
                    })
                    .detach();
                    cx.notify();
                    return;
                }
                bot.retried = false;
                let ok = !bot.stopped && error.is_none();
                let failed = error.clone().filter(|_| !bot.stopped);
                match (bot.stopped, error) {
                    (true, _) => bot.msgs.push(Msg::Error("Stopped.".into())),
                    (false, Some(e)) => bot.msgs.push(Msg::Error(e)),
                    _ => {}
                }
                // this turn's reply = bot text since the message that started it
                let start = bot.msgs.iter().rposition(|m| matches!(m, Msg::User(_) | Msg::Handoff { .. } | Msg::Scheduled { .. })).map_or(0, |i| i + 1);
                let reply: Vec<&str> = bot.msgs[start..].iter().filter_map(|m| if let Msg::Bot(t) = m { Some(t.as_str()) } else { None }).collect();
                let (reply, hops) = (reply.join("\n\n"), bot.hops);
                // a scheduled run with nothing to say stays out of the way
                let quiet = ok && bot.fresh_turn && reply.trim().trim_end_matches('.') == "QUIET";
                if quiet {
                    let tail = bot.msgs.split_off(start);
                    bot.msgs.extend(tail.into_iter().filter(|m| !matches!(m, Msg::Bot(_))));
                    bot.msgs.push(Msg::Divider("Nothing to report".into()));
                }
                let name = bot.name.clone();
                let next = (!bot.queue.is_empty()).then(|| bot.queue.remove(0));
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
                let handed = ok && !refreshed && !quiet && self.hand_off(id, reply.clone(), hops, cx);
                match failed {
                    Some(e) => self.alert(id, &format!("{name} needs you"), &e),
                    None if ok && !quiet && !handed => self.alert(id, &name, &reply),
                    None => {}
                }
                if let Some((prompt, hops, fresh)) = next {
                    self.start_turn(id, prompt, hops, fresh, cx);
                }
                self.save();
            }
            _ => {}
        }
        cx.notify();
    }

    /// Deletes the bot, its container and its scratch folder; never a mounted project folder.
    fn delete(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(i) = self.bots.iter().position(|b| b.id == id) else { return };
        if let Some(run) = &self.bots[i].run {
            run.stop();
        }
        self.bots.remove(i);
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
        let Some(id) = self.bots.get(self.selected).map(|b| b.id) else { return };
        let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Mount".into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = picked.await
                && let Some(path) = paths.pop()
            {
                this.update(cx, |this, cx| {
                    if let Some(b) = this.bots.iter_mut().find(|b| b.id == id) {
                        b.folder = Some(path);
                    }
                    this.save();
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
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
                Msg::Handoff { prompt, .. } | Msg::Scheduled { prompt, .. } => Some(prompt.clone()),
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
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        if bot.busy() {
            return;
        }
        bot.refreshing = true;
        bot.msgs.push(Msg::Scheduled { prompt: "Save your notes before a fresh session.".into(), label: "Fresh start".into() });
        let id = bot.id;
        self.start_turn(id, FRESH_START.into(), 0, false, cx);
    }

    fn send_again(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        if bot.busy() {
            return;
        }
        if let Some(Msg::SignedIn { prompt, .. }) = bot.msgs.get_mut(i)
            && let Some(text) = prompt.take()
        {
            bot.msgs.push(Msg::User(text.clone()));
            self.start_turn(id, text, 0, false, cx);
        }
    }

    fn set_meter(&mut self, meter: Meter) {
        self.meters.retain(|m| m.provider != meter.provider);
        self.meters.push(meter);
        self.meters.sort_by_key(|m| m.provider.label());
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
                        this.set_meter(meter);
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
        (self.settings_open, self.edit_open, self.sched_open) = (false, false, false);
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
                if this.update(cx, |this, cx| if this.setup.is_some() { this.setup = Some(after); cx.notify() }).is_err() {
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
        Menu { name: "eggbot".into(), items: vec![MenuItem::action("Settings…", OpenSettings), MenuItem::action("Setup…", OpenSetup), MenuItem::separator(), MenuItem::action("Close Window", CloseWindow), MenuItem::action("Quit eggbot", Quit)], disabled: false },
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
            KeyBinding::new("escape", StopTurn, None),
            KeyBinding::new("cmd-shift-d", CycleAppearance, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("cmd-f", Find, None),
        ]);
        cx.bind_keys((1..=9).map(|n| KeyBinding::new(&format!("cmd-{n}"), SelectBot(n - 1), None)));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1080.), px(720.)), cx))),
            // transparent, with a native vibrancy view behind (GPUI's own Blurred has no effect here)
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: Some(TitlebarOptions {
                title: Some("eggbot".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(16.), px(16.))),
            }),
            show: !at_login,
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            add_vibrancy(window);
            cx.new(|cx| Eggbot::new(window, cx))
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
    use super::Saved;

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
}
