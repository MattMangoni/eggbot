mod claude;
mod egg;
mod handoff;
mod sandbox;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egg::{Mood, egg};
use gpui_kit::assets::{Assets, IconName};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

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
    msgs: Vec<Msg>,
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
    queue: Vec<(String, u32)>,
}

fn hatched_long_ago() -> Instant {
    Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now)
}

impl Bot {
    fn preset(&self) -> &'static Preset {
        &PRESETS[self.preset.min(PRESETS.len() - 1)]
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
}

struct Eggbot {
    p: Palette,
    bots: Vec<Bot>,
    selected: usize,
    next_id: usize,
    menu_open: bool,
    /// Bot id whose trash icon was clicked once; a second click deletes.
    confirm_delete: Option<usize>,
    usage: Option<(f32, f32)>,
    input: Entity<InputState>,
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
        let p = Palette::apply(window, cx);
        let saved: Option<Saved> = std::fs::read(data_dir().join("state.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let mut this = Self { p, bots: vec![], selected: 0, next_id: 0, menu_open: false, confirm_delete: None, usage: None, input, scroll: ScrollHandle::new() };
        match saved {
            Some(s) if !s.bots.is_empty() => (this.bots, this.next_id) = (s.bots, s.next_id),
            _ => {
                for i in 0..3 {
                    this.hatch(i, Duration::from_millis(300 + 350 * i as u64), cx);
                }
                this.selected = 0;
            }
        }
        this
    }

    fn save(&self) {
        let dir = data_dir();
        let state = serde_json::json!({ "next_id": self.next_id, "bots": self.bots });
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
            msgs: vec![],
            born: Instant::now() + delay,
            poked: None,
            pokes: 0,
            run: None,
            status: None,
            stopped: false,
            hops: 0,
            queue: vec![],
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
        self.start_turn(id, text, 0, cx);
    }

    fn start_turn(&mut self, id: usize, prompt: String, hops: u32, cx: &mut Context<Self>) {
        let roster: Vec<(usize, &str, &str)> = self.bots.iter().map(|b| (b.id, b.name.as_str(), b.preset().blurb)).collect();
        let others = handoff::roster(&roster, id);
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        bot.stopped = false;
        bot.hops = hops;
        let scratch = data_dir().join("bots").join(id.to_string());
        if let Err(e) = std::fs::create_dir_all(&scratch) {
            eprintln!("eggbot: could not create {}: {e}", scratch.display());
        }
        let turn = claude::Turn {
            bot: id,
            mount: bot.folder.clone().unwrap_or(scratch),
            prompt,
            role: format!("{}{others}{STYLE}", bot.preset().role),
            session: bot.session.clone(),
        };
        let (handle, events) = claude::run(turn);
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
    fn deliver(&mut self, id: usize, prompt: String, hops: u32, cx: &mut Context<Self>) {
        match self.bots.iter_mut().find(|b| b.id == id) {
            Some(b) if b.busy() => b.queue.push((prompt, hops)),
            Some(_) => self.start_turn(id, prompt, hops, cx),
            None => {}
        }
    }

    /// Sends the finished reply to every bot it mentions as @Name.
    fn hand_off(&mut self, from: usize, reply: String, hops: u32, cx: &mut Context<Self>) {
        let names: Vec<(usize, &str)> = self.bots.iter().map(|b| (b.id, b.name.as_str())).collect();
        let targets = handoff::mentions(&reply, &names, from);
        let Some(sender) = self.bots.iter().find(|b| b.id == from) else { return };
        let (from_name, color, folder) = (sender.name.clone(), sender.preset().color, sender.folder.clone());
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
                self.deliver(to, prompt, next, cx);
            }
        }
    }

    fn continue_chain(&mut self, id: usize, i: usize, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        if let Some(Msg::Handoff { prompt, paused, .. }) = bot.msgs.get_mut(i)
            && *paused
        {
            *paused = false;
            let prompt = prompt.clone();
            self.deliver(id, prompt, 0, cx);
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
        if let Ev::Usage { five_hour, seven_day } = ev {
            self.usage = Some((five_hour, seven_day));
            cx.notify();
            return;
        }
        let Some(bot) = self.bots.iter_mut().find(|b| b.id == id) else { return };
        bot.status = match ev {
            Ev::Status(ref s) => Some(s.clone()),
            _ => None,
        };
        match ev {
            Ev::Session(s) if !s.is_empty() => bot.session = Some(s),
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
                let start = bot.msgs.iter().rposition(|m| matches!(m, Msg::User(_) | Msg::Handoff { .. })).map_or(0, |i| i + 1);
                let reply: Vec<&str> = bot.msgs[start..].iter().filter_map(|m| if let Msg::Bot(t) = m { Some(t.as_str()) } else { None }).collect();
                let (reply, hops) = (reply.join("\n\n"), bot.hops);
                let next = (!bot.queue.is_empty()).then(|| bot.queue.remove(0));
                if ok {
                    self.hand_off(id, reply, hops, cx);
                }
                if let Some((prompt, hops)) = next {
                    self.start_turn(id, prompt, hops, cx);
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

    fn sign_in(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.bots.get(self.selected).map(|b| b.id) else { return };
        let opening = cx.background_executor().spawn(async { sandbox::ready(&|_| {}).and_then(|_| sandbox::sign_in()) });
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
                .child(egg(format!("side-{}", b.id), hex(b.preset().color), 30., b.mood()))
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
                            _ => b.preset().blurb.to_string(),
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
            .when_some(self.usage, |d, (h5, d7)| d.child(self.usage_meter(h5, d7)))
    }

    fn usage_meter(&self, five_hour: f32, seven_day: f32) -> impl IntoElement {
        let p = self.p;
        let bar = |label: &'static str, v: f32| {
            let v = v.clamp(0., 1.);
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(p.muted)
                .child(div().w(px(20.)).child(label))
                .child(
                    div().flex_1().h(px(4.)).rounded_full().bg(p.line).child(
                        div().h_full().rounded_full().w(relative(v)).bg(if v >= 0.8 { p.amber } else { p.muted.opacity(0.6) }),
                    ),
                )
                .child(div().w(px(30.)).text_right().child(format!("{:.0}%", v * 100.)))
        };
        div().mt_3().px_1().flex().flex_col().gap_1().child(bar("5h", five_hour)).child(bar("7d", seven_day))
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
                        this.input.update(cx, |s, cx| s.focus(window, cx));
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
        let color = hex(bot.preset().color);

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
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(bot.name.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(p.muted)
                            .child("claude")
                            .child("·")
                            .child(div().size(px(6.)).rounded_full().bg(if bot.busy() { p.amber } else { p.ok }))
                            .child(if bot.busy() { "working" } else { "idle" }),
                    ),
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
                .child(div().text_sm().text_color(p.muted).child(bot.preset().blurb))
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
        let content = div().flex_1().flex().flex_col().min_h_0().child(header).child(body).with_animation(
            ElementId::Name(format!("chat-{}", bot.id).into()),
            Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
            |d, t| d.opacity(t).mt(px(6. * (1. - t))),
        );

        div().flex_1().flex().flex_col().h_full().child(content).child(composer).into_any_element()
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
            .child(egg(format!("typing-{}", bot.id), hex(bot.preset().color), 24., Mood::Thinking))
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
                    .child(div().mt_1().child(egg(format!("msg-{}-{i}", bot.id), hex(bot.preset().color), 24., if streaming { Mood::Thinking } else { Mood::Still })))
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
                .when(t.contains("/login"), |d| {
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
                            .on_click(cx.listener(|this, _, _, cx| this.sign_in(cx)))
                            .child("Sign in to Claude"),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .bg(self.p.bg)
            .text_color(self.p.ink)
            .child(self.sidebar(cx))
            .child(self.chat(cx))
    }
}

fn main() {
    gpui_kit::application().with_assets(Assets).run(|cx| {
        gpui_kit::init(cx);
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
