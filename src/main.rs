mod claude;
mod codex;
mod egg;
mod handoff;
mod sandbox;
mod schedule;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use claude::{Meter, Provider};
use egg::{Mood, egg};
use gpui_kit::assets::{Assets, IconName};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

actions!(eggbot, [Quit, CloseWindow]);

// the default bundle has only the component icons; add the extra ones we use
gpui_kit::assets::icon_assets!(ExtraIcons, [Clock, Trash, Pencil]);

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
    side: Hsla,
    card: Hsla,
    ink: Hsla,
    muted: Hsla,
    line: Hsla,
    amber: Hsla,
    ok: Hsla,
}

impl Palette {
    fn light() -> Self {
        Self { bg: hex(0xFBF7F0), side: hex(0xF3ECDF), card: hex(0xFFFDF9), ink: hex(0x2B2621), muted: hex(0x8C8276), line: hex(0xE9E0D2), amber: hex(0xF0A43A), ok: hex(0x6DB36A) }
    }

    fn dark() -> Self {
        Self { bg: hex(0x1F1B18), side: hex(0x181512), card: hex(0x2A2521), ink: hex(0xF2EADC), muted: hex(0x9A8F82), line: hex(0x3A332D), amber: hex(0xF0A43A), ok: hex(0x7CC279) }
    }

    /// Follows the macOS appearance and pushes our colors into gpui-component.
    fn apply(window: &mut Window, cx: &mut App) -> Self {
        Theme::sync_system_appearance(Some(window), cx);
        let p = if Theme::global(cx).is_dark() { Self::dark() } else { Self::light() };
        // separate update: the mode switch above reloads the stock colors
        Theme::update(cx, |t| {
            t.background = p.bg;
            t.foreground = p.ink;
            t.border = p.line;
            t.input = p.line;
            t.primary = p.amber;
            t.ring = p.amber;
            t.caret = p.ink;
            t.selection = p.amber.opacity(0.3);
            t.muted = p.side;
            t.muted_foreground = p.muted;
            t.accent = p.side;
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
const STYLE: &str = " Reply in concise GitHub-flavored markdown.";
const NOTES: &str = " You keep your own notes in /memory/NOTES.md. When a session starts, read it if it exists. Keep it short and current: durable facts about the user, the project, decisions and open work, never chat logs.";
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

const ROW_H: f32 = 60.;
const ROW_GAP: f32 = 4.;

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
    #[serde(skip, default = "hatched_long_ago")]
    born: Instant,
    #[serde(skip)]
    poked: Option<Instant>,
    #[serde(skip)]
    pokes: u32,
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
}

fn hatched_long_ago() -> Instant {
    Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now)
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
        let now = Instant::now();
        match now.checked_duration_since(self.born) {
            None => Mood::Unborn,
            Some(a) if a < egg::HATCH => Mood::Hatching,
            _ if self.busy() => Mood::Thinking,
            _ if self.poked.is_some_and(|t| now - t < egg::BOING) => Mood::Boing(self.pokes),
            _ => Mood::Idle,
        }
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
    input: Entity<InputState>,
    sched_open: bool,
    /// 0 daily, 1 weekdays, 2 every N hours, 3 every N minutes
    sched_kind: usize,
    sched_prompt: Entity<InputState>,
    sched_value: Entity<InputState>,
    sched_error: Option<String>,
    edit_open: bool,
    edit_name: Entity<InputState>,
    edit_role: Entity<TextareaState>,
    edit_error: Option<String>,
    model_select: Entity<SelectState<Vec<Choice>>>,
    effort_select: Entity<SelectState<Vec<Choice>>>,
    /// Dropdown options need refilling (bot, provider or Codex model list changed); done in render.
    selects_stale: bool,
    scroll: ScrollHandle,
}

impl Eggbot {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Message…"));
        cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = ev {
                this.send(window, cx);
            }
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
        let edit_role = cx.new(|cx| {
            let mut role = TextareaState::new(window, cx).placeholder("What is this bot for, and how should it work?");
            role.set_auto_grow(4, 12, cx);
            role
        });
        let model_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let effort_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        cx.subscribe_in(&model_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
            let SelectEvent::Confirm(Some(model)) = ev else { return };
            if let Some(b) = this.bots.get_mut(this.selected) {
                // effort levels differ per model; fall back to the default level
                b.model = model.clone();
                b.effort = None;
                this.selects_stale = true;
                this.save();
                cx.notify();
            }
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
        let mut this = Self { p, bots: vec![], selected: 0, next_id: 0, menu_open: false, confirm_delete: None, meters: vec![], codex_models: vec![], codex_query: None, tray: None, sched_open: false, sched_kind: 0, sched_prompt, sched_value, sched_error: None, edit_open: false, edit_name, edit_role, edit_error: None, model_select, effort_select, selects_stale: true, input, scroll: ScrollHandle::new() };
        match saved {
            Some(s) if !s.bots.is_empty() => {
                (this.bots, this.next_id, this.meters) = (s.bots, s.next_id, s.meters);
                this.scroll.scroll_to_bottom();
            }
            _ => {
                for i in 0..3 {
                    this.hatch(i, Duration::from_millis(300 + 350 * i as u64), cx);
                }
                this.selected = 0;
            }
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
        if let Some((tray, actions)) = tray::Tray::new() {
            this.tray = Some(tray);
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
                        let bots = this.bots.iter().map(|b| (b.id, b.name.clone(), b.busy())).collect();
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

    fn save(&self) {
        let dir = data_dir();
        let state = serde_json::json!({ "next_id": self.next_id, "bots": self.bots, "meters": self.meters });
        // write then rename, so a crash mid-write never loses the history
        let tmp = dir.join("state.json.tmp");
        let ok = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&tmp, state.to_string())).and_then(|_| std::fs::rename(&tmp, dir.join("state.json")));
        if let Err(e) = ok {
            eprintln!("eggbot: could not save state: {e}");
        }
    }

    /// Repaint after `delay`, so time-based moods (hatch, boing) can end.
    fn refresh_after(delay: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    fn hatch(&mut self, preset: usize, delay: Duration, cx: &mut Context<Self>) {
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
            born: Instant::now() + delay,
            poked: None,
            pokes: 0,
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
        });
        self.next_id += 1;
        self.selected = self.bots.len() - 1;
        self.save();
        Self::refresh_after(delay, cx);
        Self::refresh_after(delay + egg::HATCH, cx);
    }

    fn poke(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(b) = self.bots.get_mut(i) {
            b.pokes += 1;
            b.poked = Some(Instant::now());
            Self::refresh_after(egg::BOING, cx);
        }
    }

    fn select(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i != self.selected {
            self.poke(i, cx);
        }
        self.selected = i;
        self.menu_open = false;
        self.confirm_delete = None;
        self.edit_open = false;
        self.sched_open = false;
        self.scroll.scroll_to_bottom();
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
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
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        bot.stopped = false;
        bot.hops = hops;
        bot.fresh_turn = fresh;
        // separate folders, so a bot without a project never sees its notes inside /work
        let home = data_dir().join("bots").join(id.to_string());
        let (scratch, memory) = (home.join("work"), home.join("memory"));
        for dir in [&scratch, &memory] {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("eggbot: could not create {}: {e}", dir.display());
            }
        }
        let role = format!("{}{others}{NOTES}{STYLE}", bot.role());
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
        self.scroll.scroll_to_bottom();
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

    /// Sends the finished reply to every bot it mentions as @Name.
    fn hand_off(&mut self, from: usize, reply: String, hops: u32, cx: &mut Context<Self>) {
        let names: Vec<(usize, &str)> = self.bots.iter().map(|b| (b.id, b.name.as_str())).collect();
        let targets = handoff::mentions(&reply, &names, from);
        let Some(sender) = self.bots.iter().find(|b| b.id == from) else { return };
        let (from_name, color, folder) = (sender.name.clone(), sender.color(), sender.folder.clone());
        let next = hops + 1;
        for to in targets {
            let Some(target) = self.bots.iter_mut().find(|b| b.id == to) else { continue };
            let prompt = handoff::prompt(&from_name, &reply, folder.is_some() && folder == target.folder);
            let paused = next > handoff::MAX_HOPS;
            target.msgs.push(Msg::Handoff { from: from_name.clone(), color, prompt: prompt.clone(), text: reply.clone(), paused, open: false });
            let to_name = target.name.clone();
            if let Some(s) = self.bots.iter_mut().find(|b| b.id == from) {
                s.msgs.push(Msg::Sent { to: to_name });
            }
            if !paused {
                self.deliver(to, prompt, next, false, cx);
            }
        }
    }

    /// Starts (or queues) every schedule that is due.
    fn run_due(&mut self, cx: &mut Context<Self>) {
        let now = chrono::Local::now();
        let mut due = vec![];
        for b in &mut self.bots {
            for s in b.schedules.iter_mut().filter(|s| s.due(now)) {
                s.anchor = now.timestamp();
                b.msgs.push(Msg::Scheduled { prompt: s.prompt.clone(), label: s.repeat.label() });
                due.push((b.id, s.prompt.clone()));
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
                let ok = !bot.stopped && error.is_none();
                match (bot.stopped, error) {
                    (true, _) => bot.msgs.push(Msg::Error("Stopped.".into())),
                    (false, Some(e)) => bot.msgs.push(Msg::Error(e)),
                    _ => {}
                }
                // this turn's reply = bot text since the message that started it
                let start = bot.msgs.iter().rposition(|m| matches!(m, Msg::User(_) | Msg::Handoff { .. } | Msg::Scheduled { .. })).map_or(0, |i| i + 1);
                let reply: Vec<&str> = bot.msgs[start..].iter().filter_map(|m| if let Msg::Bot(t) = m { Some(t.as_str()) } else { None }).collect();
                let (reply, hops) = (reply.join("\n\n"), bot.hops);
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
                } else if ok {
                    self.hand_off(id, reply, hops, cx);
                }
                if let Some((prompt, hops, fresh)) = next {
                    self.start_turn(id, prompt, hops, fresh, cx);
                }
                self.save();
            }
            _ => {}
        }
        self.scroll.scroll_to_bottom();
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

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let rows = self.bots.iter().enumerate().map(|(i, b)| {
            let active = i == self.selected;
            let (id, confirming) = (b.id, self.confirm_delete == Some(b.id));
            let trash = div()
                .id(("trash", id))
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .py_1()
                .rounded(px(8.))
                .text_xs()
                .cursor_pointer()
                .when(confirming, |d| d.bg(p.amber).text_color(hex(0x2B2621)).px_2().child("Delete?"))
                .when(!confirming, |d| d.text_color(p.muted).opacity(0.).group_hover("row", |s| s.opacity(1.)).hover(|d| d.text_color(p.ink)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if this.confirm_delete == Some(id) {
                        this.delete(id, cx);
                    } else {
                        this.confirm_delete = Some(id);
                        cx.notify();
                    }
                }))
                .when(!confirming, |d| d.child(Icon::new(IconName::Trash).size_4()));
            div()
                .id(("bot", b.id))
                .group("row")
                .h(px(ROW_H))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .rounded(px(14.))
                .cursor_pointer()
                .when(!active, |d| d.hover(|d| d.bg(p.card.opacity(0.5))))
                .on_click(cx.listener(move |this, _, window, cx| this.select(i, window, cx)))
                .child(egg(format!("side-{}", b.id), hex(b.color()), 30., b.mood()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .overflow_hidden()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(b.name.clone()))
                        .child(div().text_xs().text_color(p.muted).truncate().child(match (b.busy(), b.queue.len()) {
                            (true, 0) => "thinking…".to_string(),
                            (true, n) => format!("thinking… · {n} queued"),
                            _ => b.blurb(),
                        })),
                )
                .child(trash)
        });

        // one highlight card that springs to the selected row
        let highlight = div()
            .absolute()
            .left_0()
            .right_0()
            .h(px(ROW_H))
            .rounded(px(14.))
            .bg(p.card)
            .shadow_sm()
            .with_spring(
                "selection",
                SpringAnimation::new(SpringConfig::new(320., 26., 1.)).to(px(self.selected as f32 * (ROW_H + ROW_GAP))).with_epsilon(0.25),
                |d, top| d.top(top),
            );

        div()
            .w(px(248.))
            .h_full()
            .flex()
            .flex_col()
            .bg(p.side)
            .border_r_1()
            .border_color(p.line)
            .pt(px(44.))
            .px_3()
            .pb_3()
            .child(
                div()
                    .px_3()
                    .pb_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(egg("logo", hex(0xF6D28B), 16., Mood::Still))
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child("eggbot")),
            )
            .child(
                div()
                    .id("bots")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(div().relative().flex().flex_col().gap(px(ROW_GAP)).when(!self.bots.is_empty(), |d| d.child(highlight)).children(rows)),
            )
            .when(self.menu_open, |d| d.child(self.hatch_menu(cx)))
            .child(
                div()
                    .id("hatch")
                    .mt_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .py_2()
                    .rounded(px(14.))
                    .border_1()
                    .border_dashed()
                    .border_color(p.muted.opacity(0.5))
                    .text_sm()
                    .text_color(p.muted)
                    .cursor_pointer()
                    .hover(|d| d.bg(p.card.opacity(0.6)).text_color(p.ink))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_open = !this.menu_open;
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::Plus).size_4())
                    .child("Hatch a bot"),
            )
            .children(self.meters.iter().map(|m| self.usage_meter(m)))
    }

    fn usage_meter(&self, meter: &Meter) -> impl IntoElement {
        let p = self.p;
        let now = chrono::Local::now().timestamp();
        let at = chrono::DateTime::from_timestamp(meter.at, 0).map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
        let bars = meter.windows.iter().map(|w| {
            // a window whose reset time has passed is back to zero
            let v = if w.reset > 0 && now >= w.reset { 0. } else { w.used.clamp(0., 1.) };
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(p.muted)
                .child(div().w(px(34.)).child(w.label.clone()))
                .child(
                    div().flex_1().h(px(4.)).rounded_full().bg(p.line).child(
                        div().h_full().rounded_full().w(relative(v)).bg(if v >= 0.8 { p.amber } else { p.muted.opacity(0.6) }),
                    ),
                )
                .child(div().w(px(30.)).text_right().child(format!("{:.0}%", v * 100.)))
        });
        div()
            .mt_3()
            .px_1()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().text_color(p.muted.opacity(0.7)).child(format!("{} · updated {at}", if meter.provider == Provider::Codex { "Codex" } else { "Claude" })))
            .children(bars)
    }

    fn hatch_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .mt_2()
            .rounded(px(16.))
            .bg(p.card)
            .shadow_lg()
            .border_1()
            .border_color(p.line)
            .children(PRESETS.iter().enumerate().map(|(i, preset)| {
                div()
                    .id(("preset", i))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_2()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.side))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.menu_open = false;
                        this.hatch(i, Duration::ZERO, cx);
                        if PRESETS[i].name == "Custom" {
                            this.open_editor(window, cx);
                        } else {
                            this.edit_open = false;
                            this.input.update(cx, |s, cx| s.focus(window, cx));
                        }
                        cx.notify();
                    }))
                    .child(egg(format!("preset-{i}"), hex(preset.color), 20., Mood::Unborn))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().child(preset.name))
                            .child(div().text_xs().text_color(p.muted).child(preset.blurb)),
                    )
            }))
            .with_animation("menu-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| {
                d.opacity(t).mt(px(8. + 6. * (1. - t)))
            })
    }

    fn chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let Some(bot) = self.bots.get(self.selected) else {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(egg("empty", hex(0xF6D28B), 88., Mood::Unborn))
                .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child("The nest is empty"))
                .child(div().text_sm().text_color(p.muted).child("Hatch a bot to begin."))
                .into_any_element();
        };
        let sel = self.selected;
        let color = hex(bot.color());

        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .pt(px(30.))
            .pb_3()
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .id("head-egg")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.poke(sel, cx);
                        cx.notify();
                    }))
                    .child(egg(format!("head-{}", bot.id), color, 34., bot.mood())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .child(
                        div()
                            .id("bot-name")
                            .group("name")
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.edit_open {
                                    this.edit_open = false;
                                    cx.notify();
                                } else {
                                    this.open_editor(window, cx);
                                }
                            }))
                            .child(bot.name.clone())
                            .child(div().text_color(p.muted).opacity(if self.edit_open { 1. } else { 0. }).group_hover("name", |s| s.opacity(1.)).child(Icon::new(IconName::Pencil).size_3())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(p.muted)
                            .child([Some(bot.provider.label().to_string()), bot.model.clone(), bot.effort.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · "))
                            .child("·")
                            .child(div().size(px(6.)).rounded_full().bg(if bot.busy() { p.amber } else { p.ok }))
                            .child(if bot.busy() { "working" } else { "idle" }),
                    ),
            )
            .when(bot.context.1 > 0, |d| {
                let used = (bot.context.0 as f32 / bot.context.1 as f32).clamp(0., 1.);
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(p.muted)
                        .child("context")
                        .child(div().w(px(48.)).h(px(4.)).rounded_full().bg(p.line).child(div().h_full().rounded_full().w(relative(used)).bg(if used >= 0.7 { p.amber } else { p.muted.opacity(0.6) })))
                        .child(format!("{:.0}%", used * 100.)),
                )
            })
            .child(
                div()
                    .id("fresh")
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .border_1()
                    .border_color(p.line)
                    .text_xs()
                    .text_color(p.muted)
                    .when(!bot.busy(), |d| d.cursor_pointer().hover(|d| d.border_color(p.muted).text_color(p.ink)))
                    .when(bot.busy(), |d| d.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.fresh_start(cx)))
                    .child(Icon::new(IconName::RefreshCw).size_3())
                    .child("Fresh start"),
            )
            .child(
                div()
                    .id("clock")
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .border_1()
                    .border_color(if self.sched_open { p.amber } else { p.line })
                    .text_xs()
                    .text_color(p.muted)
                    .cursor_pointer()
                    .hover(|d| d.border_color(p.muted).text_color(p.ink))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.sched_open = !this.sched_open;
                        this.sched_error = None;
                        this.edit_open = false;
                        if this.sched_open {
                            this.sched_prompt.update(cx, |s, cx| s.focus(window, cx));
                        }
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::Clock).size_3())
                    .when(!bot.schedules.is_empty(), |d| d.child(bot.schedules.len().to_string())),
            )
            .child(
                div()
                    .id("folder")
                    .cursor_pointer()
                    .hover(|d| d.border_color(p.muted).text_color(p.ink))
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx)))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .border_1()
                    .border_color(p.line)
                    .text_xs()
                    .text_color(p.muted)
                    .child(Icon::new(IconName::Folder).size_3())
                    .child(match &bot.folder {
                        Some(f) => f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.display().to_string()),
                        None => "Choose project folder".into(),
                    }),
            );

        let body = if bot.msgs.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .id("hero-egg")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.poke(sel, cx);
                            cx.notify();
                        }))
                        .child(egg(format!("hero-{}", bot.id), color, 88., bot.mood())),
                )
                .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(format!("Say hi to {}", bot.name)))
                .child(div().text_sm().text_color(p.muted).child(bot.blurb()))
                .into_any_element()
        } else {
            let msgs = bot.msgs.iter().enumerate().map(|(i, m)| self.message(bot, i, m, cx));
            div()
                .id(("msgs", bot.id))
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .px_6()
                        .py_5()
                        .max_w(px(760.))
                        .mx_auto()
                        .w_full()
                        .children(msgs)
                        .when(bot.waiting(), |d| d.child(self.typing(bot))),
                )
                .into_any_element()
        };

        let busy = bot.busy();
        let composer = div().px_6().pb_5().child(
            div()
                .max_w(px(760.))
                .mx_auto()
                .flex()
                .items_center()
                .gap_2()
                .pl_4()
                .pr_2()
                .py_2()
                .rounded(px(20.))
                .bg(p.card)
                .border_1()
                .border_color(p.line)
                .shadow_md()
                .child(div().flex_1().child(Input::new(&self.input).appearance(false)))
                .child(
                    div()
                        .id("send")
                        .size(px(34.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(if busy { p.ink } else { p.amber })
                        .text_color(if busy { p.bg } else { hex(0x2B2621) })
                        .cursor_pointer()
                        .hover(|d| d.opacity(0.85))
                        .on_click(cx.listener(move |this, _, window, cx| if busy { this.stop(cx) } else { this.send(window, cx) }))
                        .child(if busy { div().size(px(10.)).rounded(px(2.)).bg(p.bg).into_any_element() } else { Icon::new(IconName::ArrowUp).size_4().into_any_element() }),
                ),
        );

        // new id per bot, so switching bots replays the fade
        let content = div().flex_1().flex().flex_col().min_h_0().child(header).when(self.sched_open, |d| d.child(self.schedules(bot, cx))).when(self.edit_open, |d| d.child(self.editor(bot, cx))).child(body).with_animation(
            ElementId::Name(format!("chat-{}", bot.id).into()),
            Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
            |d, t| d.opacity(t).mt(px(6. * (1. - t))),
        );

        div().flex_1().flex().flex_col().h_full().child(content).child(composer).into_any_element()
    }

    /// Fills the model and effort dropdowns for the selected bot (options depend on provider and model).
    fn sync_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selects_stale = false;
        let Some(bot) = self.bots.get(self.selected) else { return };
        let choice = |value: Option<String>, label: String| Choice { value, label: label.into() };
        let models: Vec<Choice> = match bot.provider {
            Provider::Claude => MODELS.iter().map(|(a, l)| choice(a.map(str::to_string), l.to_string())).collect(),
            Provider::Codex => std::iter::once(choice(None, "Default".into())).chain(self.codex_models.iter().map(|m| choice(Some(m.id.clone()), m.name.clone()))).collect(),
        };
        let levels: Vec<String> = match bot.provider {
            Provider::Claude => ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec(),
            Provider::Codex => self.codex_models.iter().find(|m| bot.model.as_ref().map_or(m.default, |id| *id == m.id)).map(|m| m.efforts.clone()).unwrap_or_default(),
        };
        let efforts: Vec<Choice> = std::iter::once(choice(None, "Default".into())).chain(levels.into_iter().map(|l| choice(Some(l.clone()), l))).collect();
        let (model, effort) = (bot.model.clone(), bot.effort.clone());
        self.model_select.update(cx, |s, cx| {
            s.set_items(models, window, cx);
            s.set_selected_value(&model, window, cx);
        });
        self.effort_select.update(cx, |s, cx| {
            s.set_items(efforts, window, cx);
            s.set_selected_value(&effort, window, cx);
        });
    }

    /// The bot editor: name, role, egg color, model. Color and model apply at once; name and role on Save.
    fn editor(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let current = bot.color();
        let swatches = SHELLS.iter().map(|&c| {
            div()
                .id(("shell", c as usize))
                .size(px(22.))
                .rounded_full()
                .bg(hex(c))
                .cursor_pointer()
                .border_2()
                .border_color(if c == current { p.ink } else { p.card })
                .hover(|d| d.border_color(p.muted))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(b) = this.bots.get_mut(this.selected) {
                        b.color = (c != b.preset().color).then_some(c);
                        this.save();
                        cx.notify();
                    }
                }))
        });
        let codex_note = (bot.provider == Provider::Codex && self.codex_models.is_empty()).then(|| self.codex_query.clone()).flatten();
        let providers = [Provider::Claude, Provider::Codex].into_iter().map(|pr| {
            let on = bot.provider == pr;
            div()
                .id(pr.label())
                .px_3()
                .py_1()
                .rounded_full()
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.bg(p.ink).text_color(p.bg))
                .when(!on, |d| d.border_1().border_color(p.line).text_color(p.muted).hover(|d| d.text_color(p.ink)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let Some(b) = this.bots.get_mut(this.selected) else { return };
                    if b.provider != pr {
                        b.provider = pr;
                        b.model = None;
                        b.effort = None;
                        // the meter tracks the provider's session; it refills on the next turn
                        b.context = (0, 0);
                        this.selects_stale = true;
                        if pr == Provider::Codex && this.codex_models.is_empty() {
                            this.refresh_codex(1, cx);
                        }
                        this.save();
                        cx.notify();
                    }
                }))
                .child(if pr == Provider::Codex { "Codex" } else { "Claude" })
        });
        let label = |t: &'static str| div().text_xs().text_color(p.muted).child(t);
        let field = |d: Div| d.px_3().py_1().rounded(px(10.)).bg(p.bg).border_1().border_color(p.line);
        div()
            .mx_6()
            .mt_3()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(16.))
            .bg(p.card)
            .border_1()
            .border_color(p.line)
            .shadow_sm()
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().flex_1().flex().flex_col().gap_1().child(label("Name")).child(field(div()).child(Input::new(&self.edit_name).appearance(false))))
                    .child(div().flex().flex_col().gap_1().child(label("Brain")).child(div().flex().items_center().gap_1().children(providers))),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_3()
                    .child(div().w(px(240.)).flex().flex_col().gap_1().child(label("Model")).child(Select::new(&self.model_select).menu_max_h(px(320.))))
                    .child(div().w(px(160.)).flex().flex_col().gap_1().child(label("Effort")).child(Select::new(&self.effort_select)))
                    .when_some(codex_note, |d, failed| {
                        let pill = |id: &'static str, text: &'static str| div().id(id).mb_1().px_3().py_1().rounded_full().text_xs().bg(p.amber).text_color(hex(0x2B2621)).cursor_pointer().hover(|d| d.opacity(0.85)).child(text);
                        match failed {
                            None => d.child(div().mb_2().text_xs().text_color(p.muted).child("asking Codex for your models…")),
                            Some(e) if e.contains("codex login") => d
                                .child(div().mb_2().text_xs().text_color(p.muted).child("Codex is not signed in."))
                                .child(pill("codex-sign-in", "Sign in to Codex").on_click(cx.listener(|this, _, _, cx| this.sign_in(true, cx)))),
                            Some(e) => d
                                .child(div().mb_2().text_xs().text_color(p.amber).max_w(px(260.)).truncate().child(e))
                                .child(pill("codex-retry", "Retry").on_click(cx.listener(|this, _, _, cx| this.refresh_codex(1, cx)))),
                        }
                    }),
            )
            .child(div().flex().flex_col().gap_1().child(label("Role")).child(field(div()).child(Textarea::new(&self.edit_role).appearance(false))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("Egg"))
                    .children(swatches)
                    .child(div().flex_1())
                    .when_some(self.edit_error.clone(), |d, e| d.child(div().text_xs().text_color(p.amber).child(e)))
                    .child(
                        div()
                            .id("save-edit")
                            .px_4()
                            .py_1()
                            .rounded_full()
                            .bg(p.amber)
                            .text_sm()
                            .text_color(hex(0x2B2621))
                            .cursor_pointer()
                            .hover(|d| d.opacity(0.85))
                            .on_click(cx.listener(|this, _, window, cx| this.save_edit(window, cx)))
                            .child("Save"),
                    ),
            )
            .with_animation("edit-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
    }

    /// The clock panel: this bot's schedules and a form to add one.
    fn schedules(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let rows = bot.schedules.iter().map(|s| {
            let id = s.id;
            div()
                .flex()
                .items_center()
                .gap_3()
                .py_1()
                .text_sm()
                .child(Icon::new(IconName::Clock).size_4().text_color(p.muted))
                .child(div().flex_1().truncate().child(s.prompt.clone()))
                .child(div().text_xs().text_color(p.muted).child(format!("{} · next {}", s.repeat.label(), s.next_run().format("%a %H:%M"))))
                .child(
                    div()
                        .id(("unschedule", id))
                        .text_color(p.muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.ink))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_schedule(id, cx)))
                        .child(Icon::new(IconName::Trash).size_4()),
                )
        });
        let kinds = ["Daily", "Weekdays", "Every N hours", "Every N minutes"].iter().enumerate().map(|(k, label)| {
            let on = k == self.sched_kind;
            div()
                .id(("kind", k))
                .px_3()
                .py_1()
                .rounded_full()
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.bg(p.ink).text_color(p.bg))
                .when(!on, |d| d.border_1().border_color(p.line).text_color(p.muted).hover(|d| d.text_color(p.ink)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.sched_kind = k;
                    let hint = if k < 2 { "09:00" } else { "3" };
                    this.sched_value.update(cx, |s, cx| s.set_placeholder(hint, window, cx));
                    cx.notify();
                }))
                .child(*label)
        });
        let field = |state: &Entity<InputState>| div().px_3().py_1().rounded(px(10.)).bg(p.bg).border_1().border_color(p.line).child(Input::new(state).appearance(false));
        div()
            .mx_6()
            .mt_3()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(16.))
            .bg(p.card)
            .border_1()
            .border_color(p.line)
            .shadow_sm()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(format!("{}'s schedules", bot.name)))
            .when(bot.schedules.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("Nothing scheduled yet.")))
            .children(rows)
            .child(div().h(px(1.)).bg(p.line))
            .child(field(&self.sched_prompt))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(kinds)
                    .child(div().w(px(90.)).child(field(&self.sched_value)))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("add-schedule")
                            .px_4()
                            .py_1()
                            .rounded_full()
                            .bg(p.amber)
                            .text_sm()
                            .text_color(hex(0x2B2621))
                            .cursor_pointer()
                            .hover(|d| d.opacity(0.85))
                            .on_click(cx.listener(|this, _, window, cx| this.add_schedule(window, cx)))
                            .child("Add"),
                    ),
            )
            .when(self.sched_kind == 3, |d| d.child(div().text_xs().text_color(p.muted).child("Short intervals use your plan limit quickly.")))
            .when_some(self.sched_error.clone(), |d, e| d.child(div().text_xs().text_color(p.amber).child(e)))
            .with_animation("sched-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
    }

    /// Small thinking egg and three bouncing dots, shown while the bot works without writing.
    fn typing(&self, bot: &Bot) -> impl IntoElement {
        let muted = self.p.muted;
        let dot = |i: usize| {
            div().size(px(6.)).rounded_full().bg(muted).with_animation(
                ElementId::Name(format!("dot-{i}").into()),
                Animation::new(Duration::from_millis(1100)).repeat(),
                move |d, t| {
                    let phase = ((t - i as f32 * 0.14) * std::f32::consts::TAU).sin().max(0.);
                    d.mb(px(5. * phase)).opacity(0.4 + 0.6 * phase)
                },
            )
        };
        div()
            .flex()
            .items_end()
            .gap_3()
            .child(egg(format!("typing-{}", bot.id), hex(bot.color()), 24., Mood::Thinking))
            .child(div().h(px(24.)).flex().items_end().gap(px(5.)).pb_1().children((0..3).map(dot)))
            .when_some(bot.status.clone(), |d, s| d.child(div().pb_1().text_xs().text_color(muted).child(s)))
            .with_animation("typing-in", Animation::new(Duration::from_millis(200)), |d, t| d.opacity(t))
    }

    fn message(&self, bot: &Bot, i: usize, m: &Msg, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let el = match m {
            Msg::User(t) => div().flex().justify_end().child(
                div().max_w(px(520.)).px_4().py_2().rounded(px(18.)).bg(p.ink).text_color(p.bg).child(t.clone()),
            ),
            Msg::Bot(t) => {
                let streaming = bot.busy() && i + 1 == bot.msgs.len();
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(div().mt_1().child(egg(format!("msg-{}-{i}", bot.id), hex(bot.color()), 24., if streaming { Mood::Thinking } else { Mood::Still })))
                    .child(
                        div().max_w(px(640.)).px_4().py_3().rounded(px(18.)).bg(p.card).border_1().border_color(p.line).child(
                            TextView::markdown(("md", bot.id * 100_000 + i), t.clone()).selectable(true),
                        ),
                    )
            }
            Msg::Handoff { from, color, text, paused, open, .. } => {
                let (id, open, paused, from_name) = (bot.id, *open, *paused, from.clone());
                let shown: String = if open || text.chars().count() <= 320 { text.clone() } else { format!("{}…", text.chars().take(320).collect::<String>()) };
                div().child(
                    div()
                        .max_w(px(640.))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .px_4()
                        .py_3()
                        .rounded(px(18.))
                        .border_1()
                        .border_dashed()
                        .border_color(hex(*color))
                        .bg(hex(*color).opacity(0.12))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .child(egg(format!("from-{id}-{i}"), hex(*color), 18., Mood::Still))
                                .child(
                                    div()
                                        .id(("from", i))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .hover(|d| d.underline())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if let Some(j) = this.bots.iter().position(|b| b.name == from_name) {
                                                this.select(j, window, cx);
                                            }
                                        }))
                                        .child(format!("From {from}")),
                                )
                                .child(div().flex_1())
                                .when(paused, |d| {
                                    d.child(div().text_xs().text_color(p.muted).child(format!("chain paused after {} handoffs", handoff::MAX_HOPS))).child(
                                        div()
                                            .id(("continue", i))
                                            .px_3()
                                            .py_1()
                                            .rounded_full()
                                            .bg(p.amber)
                                            .text_xs()
                                            .text_color(hex(0x2B2621))
                                            .cursor_pointer()
                                            .hover(|d| d.opacity(0.85))
                                            .on_click(cx.listener(move |this, _, _, cx| this.continue_chain(id, i, cx)))
                                            .child("Continue chain"),
                                    )
                                }),
                        )
                        .child(TextView::markdown(("handoff", bot.id * 100_000 + i), shown).selectable(true))
                        .when(text.chars().count() > 320, |d| {
                            d.child(
                                div()
                                    .id(("more", i))
                                    .text_xs()
                                    .text_color(p.muted)
                                    .cursor_pointer()
                                    .hover(|d| d.text_color(p.ink))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(Msg::Handoff { open, .. }) = this.bots.iter_mut().find(|b| b.id == id).and_then(|b| b.msgs.get_mut(i)) {
                                            *open = !*open;
                                        }
                                        cx.notify();
                                    }))
                                    .child(if open { "Show less" } else { "Show more" }),
                            )
                        }),
                )
            }
            Msg::Scheduled { prompt, label } => div().flex().justify_end().child(
                div()
                    .max_w(px(520.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_4()
                    .py_2()
                    .rounded(px(18.))
                    .border_1()
                    .border_color(p.amber.opacity(0.6))
                    .bg(p.amber.opacity(0.12))
                    .child(div().flex().items_center().gap_1().text_xs().text_color(p.muted).child(Icon::new(IconName::Clock).size_3()).child(label.clone()))
                    .child(prompt.clone()),
            ),
            Msg::SignedIn { provider, prompt } => {
                let id = bot.id;
                div()
                    .ml(px(36.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(p.ok)
                    .child(Icon::new(IconName::Check).size_4())
                    .child(format!("Signed in to {}", if *provider == Provider::Codex { "Codex" } else { "Claude" }))
                    .when(prompt.is_some(), |d| {
                        d.child(
                            div()
                                .id(("again", i))
                                .ml_2()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .border_1()
                                .border_color(p.ok)
                                .text_color(p.ok)
                                .cursor_pointer()
                                .hover(|d| d.bg(p.ok.opacity(0.12)))
                                .on_click(cx.listener(move |this, _, _, cx| this.send_again(id, i, cx)))
                                .child("Send again"),
                        )
                    })
            }
            Msg::Divider(label) => div()
                .flex()
                .items_center()
                .gap_3()
                .my_2()
                .text_xs()
                .text_color(p.muted)
                .child(div().flex_1().h(px(1.)).bg(p.line))
                .child(label.clone())
                .child(div().flex_1().h(px(1.)).bg(p.line)),
            Msg::Sent { to } => {
                let to_name = to.clone();
                div().ml(px(36.)).child(
                    div()
                        .id(("sent", i))
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(p.muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.ink))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(j) = this.bots.iter().position(|b| b.name == to_name) {
                                this.select(j, window, cx);
                            }
                        }))
                        .child(Icon::new(IconName::ArrowRight).size_3())
                        .child(format!("sent to {to}")),
                )
            }
            Msg::Error(t) => div()
                .ml(px(36.))
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(p.amber)
                .child(Icon::new(IconName::CircleAlert).size_4())
                .child(t.clone())
                .when(t.contains("/login") || t.contains("codex login"), |d| {
                    let codex = t.contains("codex login");
                    d.child(
                        div()
                            .id(("sign-in", i))
                            .ml_2()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(p.amber)
                            .text_color(hex(0x2B2621))
                            .cursor_pointer()
                            .hover(|d| d.opacity(0.85))
                            .on_click(cx.listener(move |this, _, _, cx| this.sign_in(codex, cx)))
                            .child(if codex { "Sign in to Codex" } else { "Sign in to Claude" }),
                    )
                }),
            Msg::Tool { verb, target, detail, open, .. } => {
                let (id, open) = (bot.id, *open);
                let detail = if detail.is_empty() { "running…".to_string() } else { detail.clone() };
                div()
                    .ml(px(36.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .id(("tool", i))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .font_family("Menlo")
                            .text_color(p.muted)
                            .cursor_pointer()
                            .hover(|d| d.text_color(p.ink))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(Msg::Tool { open, .. }) = this.bots.iter_mut().find(|b| b.id == id).and_then(|b| b.msgs.get_mut(i)) {
                                    *open = !*open;
                                }
                                cx.notify();
                            }))
                            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).size_3())
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(verb.clone()))
                            .child(div().truncate().child(target.clone())),
                    )
                    .when(open, |d| {
                        d.child(
                            div()
                                .ml_5()
                                .p_3()
                                .rounded(px(10.))
                                .bg(p.side)
                                .text_xs()
                                .font_family("Menlo")
                                .text_color(p.muted)
                                .whitespace_normal()
                                .child(detail),
                        )
                    })
            }
        };
        el.with_animation(
            ElementId::Name(format!("in-{}-{i}", bot.id).into()),
            Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
            |d, t| d.opacity(t).mt(px(10. * (1. - t))),
        )
        .into_any_element()
    }
}

impl Render for Eggbot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.edit_open && self.selects_stale {
            self.sync_selects(window, cx);
        }
        div()
            .size_full()
            .flex()
            .bg(self.p.bg)
            .text_color(self.p.ink)
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.request_quit(window, cx)))
            .on_action(cx.listener(|_, _: &CloseWindow, _, cx| {
                cx.hide();
                set_dock_icon(false);
            }))
            .child(self.sidebar(cx))
            .child(self.chat(cx))
    }
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
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None), KeyBinding::new("cmd-w", CloseWindow, None)]);
        cx.set_menus([Menu { name: "eggbot".into(), items: vec![MenuItem::action("Close Window", CloseWindow), MenuItem::action("Quit eggbot", Quit)], disabled: false }]);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1080.), px(720.)), cx))),
            titlebar: Some(TitlebarOptions {
                title: Some("eggbot".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(16.), px(16.))),
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| Eggbot::new(window, cx))).unwrap();
        cx.activate(true);
    });
}
