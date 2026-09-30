mod egg;

use std::time::{Duration, Instant};

use egg::{Mood, egg};
use gpui_kit::assets::{Assets, IconName};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

const CREAM: u32 = 0xFBF7F0;
const SIDEBAR: u32 = 0xF3ECDF;
const CARD: u32 = 0xFFFDF9;
const INK: u32 = 0x2B2621;
const MUTED: u32 = 0x8C8276;
const LINE: u32 = 0xE9E0D2;
const AMBER: u32 = 0xF0A43A;

#[derive(Clone, Copy)]
enum Provider {
    Claude,
    Codex,
}

impl Provider {
    fn label(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }
}

struct Preset {
    name: &'static str,
    blurb: &'static str,
    provider: Provider,
    color: u32,
}

const PRESETS: [Preset; 4] = [
    Preset { name: "Reviewer", blurb: "Reads diffs, finds bugs, weighs risk", provider: Provider::Claude, color: 0xF5C6A5 },
    Preset { name: "Implementer", blurb: "Writes and changes code", provider: Provider::Claude, color: 0xC6DDB8 },
    Preset { name: "Designer", blurb: "UI and UX critique and polish", provider: Provider::Claude, color: 0xD6CAF0 },
    Preset { name: "Custom", blurb: "A blank bot you shape yourself", provider: Provider::Claude, color: 0xF3DF9C },
];

enum Msg {
    User(SharedString),
    Bot(String),
    Tool { verb: &'static str, target: SharedString, detail: SharedString, open: bool },
}

struct Bot {
    id: usize,
    name: SharedString,
    blurb: &'static str,
    provider: Provider,
    color: Hsla,
    born: Instant,
    busy: bool,
    msgs: Vec<Msg>,
}

impl Bot {
    fn mood(&self) -> Mood {
        let age = Instant::now().checked_duration_since(self.born);
        match age {
            None => Mood::Unborn,
            Some(a) if a < egg::HATCH => Mood::Hatching,
            _ if self.busy => Mood::Thinking,
            _ => Mood::Idle,
        }
    }
}

struct Eggbot {
    bots: Vec<Bot>,
    selected: usize,
    next_id: usize,
    menu_open: bool,
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
        input.update(cx, |s, cx| s.focus(window, cx));
        let mut this = Self { bots: vec![], selected: 0, next_id: 0, menu_open: false, input, scroll: ScrollHandle::new() };
        for (i, p) in PRESETS[..3].iter().enumerate() {
            this.hatch(p, Duration::from_millis(300 + 350 * i as u64), cx);
        }
        this.selected = 0;
        this
    }

    fn hatch(&mut self, p: &Preset, delay: Duration, cx: &mut Context<Self>) {
        let taken = |n: &str| self.bots.iter().any(|b| b.name == n);
        let name = (1..).map(|i| if i == 1 { p.name.to_string() } else { format!("{} {i}", p.name) }).find(|n| !taken(n)).unwrap();
        self.bots.push(Bot {
            id: self.next_id,
            name: name.into(),
            blurb: p.blurb,
            provider: p.provider,
            color: hex(p.color),
            born: Instant::now() + delay,
            busy: false,
            msgs: vec![],
        });
        self.next_id += 1;
        self.selected = self.bots.len() - 1;
        // repaint when the egg starts and when it finishes hatching, so the mood switches
        cx.spawn(async move |this, cx| {
            for wait in [delay, egg::HATCH] {
                cx.background_executor().timer(wait).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }
        })
        .detach();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_string();
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        if text.is_empty() || bot.busy {
            return;
        }
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        bot.msgs.push(Msg::User(text.clone().into()));
        bot.busy = true;
        let id = bot.id;
        let name = bot.name.clone();
        self.scroll.scroll_to_bottom();
        cx.notify();

        // ponytail: fake echo bot so we can tune the feel; Phase 2 replaces it with the real CLIs
        cx.spawn(async move |this, cx| {
            let ex = cx.background_executor().clone();
            let wait = move |ms| ex.timer(Duration::from_millis(ms));
            wait(700).await;
            this.update(cx, |this, cx| {
                this.push(id, Msg::Tool { verb: "Read", target: "src/main.rs".into(), detail: "fn main() {\n    gpui_kit::application().run(…)\n}".into(), open: false });
                cx.notify();
            })
            .ok();
            wait(500).await;
            let reply = format!("**{name}** heard you:\n\n> {text}\n\nThis is a *fake* reply so we can tune the feel. Real models arrive in Phase 2.\n\n```rust\nlet egg = hatch();\n```");
            this.update(cx, |this, _| this.push(id, Msg::Bot(String::new()))).ok();
            for word in reply.split_inclusive(' ') {
                wait(28).await;
                this.update(cx, |this, cx| {
                    if let Some(Msg::Bot(s)) = this.bot_mut(id).and_then(|b| b.msgs.last_mut()) {
                        s.push_str(word);
                    }
                    this.scroll.scroll_to_bottom();
                    cx.notify();
                })
                .ok();
            }
            this.update(cx, |this, cx| {
                if let Some(b) = this.bot_mut(id) {
                    b.busy = false;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn bot_mut(&mut self, id: usize) -> Option<&mut Bot> {
        self.bots.iter_mut().find(|b| b.id == id)
    }

    fn push(&mut self, id: usize, msg: Msg) {
        if let Some(b) = self.bot_mut(id) {
            b.msgs.push(msg);
        }
        self.scroll.scroll_to_bottom();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.bots.iter().enumerate().map(|(i, b)| {
            let active = i == self.selected;
            div()
                .id(("bot", b.id))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded(px(14.))
                .cursor_pointer()
                .when(active, |d| d.bg(hex(CARD)).shadow_sm())
                .when(!active, |d| d.hover(|d| d.bg(hex(CARD).opacity(0.5))))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.selected = i;
                    this.menu_open = false;
                    this.input.update(cx, |s, cx| s.focus(window, cx));
                    cx.notify();
                }))
                .child(egg(format!("side-{}", b.id), b.color, 30., b.mood()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .overflow_hidden()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(b.name.clone()))
                        .child(div().text_xs().text_color(hex(MUTED)).truncate().child(if b.busy { "thinking…" } else { b.blurb })),
                )
        });

        div()
            .w(px(248.))
            .h_full()
            .flex()
            .flex_col()
            .bg(hex(SIDEBAR))
            .border_r_1()
            .border_color(hex(LINE))
            .pt(px(44.))
            .px_3()
            .pb_3()
            .child(
                div()
                    .px_3()
                    .pb_4()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("eggbot"),
            )
            .child(div().id("bots").flex_1().flex().flex_col().gap_1().overflow_y_scroll().children(rows))
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
                    .border_color(hex(MUTED).opacity(0.5))
                    .text_sm()
                    .text_color(hex(MUTED))
                    .cursor_pointer()
                    .hover(|d| d.bg(hex(CARD).opacity(0.6)).text_color(hex(INK)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_open = !this.menu_open;
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::Plus).size_4())
                    .child("Hatch a bot"),
            )
    }

    fn hatch_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .mt_2()
            .rounded(px(16.))
            .bg(hex(CARD))
            .shadow_lg()
            .border_1()
            .border_color(hex(LINE))
            .children(PRESETS.iter().enumerate().map(|(i, p)| {
                div()
                    .id(("preset", i))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_2()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|d| d.bg(hex(SIDEBAR)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu_open = false;
                        this.hatch(&PRESETS[i], Duration::ZERO, cx);
                        cx.notify();
                    }))
                    .child(egg(format!("preset-{i}"), hex(p.color), 20., Mood::Unborn))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().child(p.name))
                            .child(div().text_xs().text_color(hex(MUTED)).child(p.blurb)),
                    )
            }))
            .with_animation("menu-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| {
                d.opacity(t).mt(px(8. + 6. * (1. - t)))
            })
    }

    fn chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(bot) = self.bots.get(self.selected) else {
            return div().flex_1().into_any_element();
        };

        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .pt(px(30.))
            .pb_3()
            .border_b_1()
            .border_color(hex(LINE))
            .child(egg(format!("head-{}", bot.id), bot.color, 34., bot.mood()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(bot.name.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(hex(MUTED))
                            .child(bot.provider.label())
                            .child("·")
                            .child(div().size(px(6.)).rounded_full().bg(if bot.busy { hex(AMBER) } else { hsla(0.33, 0.45, 0.55, 1.) }))
                            .child(if bot.busy { "working" } else { "idle" }),
                    ),
            );

        let body = if bot.msgs.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(egg(format!("hero-{}", bot.id), bot.color, 88., bot.mood()))
                .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(format!("Say hi to {}", bot.name)))
                .child(div().text_sm().text_color(hex(MUTED)).child(bot.blurb))
                .into_any_element()
        } else {
            let msgs = bot.msgs.iter().enumerate().map(|(i, m)| self.message(bot, i, m, cx));
            div()
                .id(("msgs", bot.id))
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(div().flex().flex_col().gap_3().px_6().py_5().max_w(px(760.)).mx_auto().w_full().children(msgs))
                .into_any_element()
        };

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
                .bg(hex(CARD))
                .border_1()
                .border_color(hex(LINE))
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
                        .bg(if bot.busy { hex(LINE) } else { hex(AMBER) })
                        .text_color(hex(INK))
                        .cursor_pointer()
                        .hover(|d| d.opacity(0.85))
                        .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
                        .child(Icon::new(IconName::ArrowUp).size_4()),
                ),
        );

        div().flex_1().flex().flex_col().h_full().child(header).child(body).child(composer).into_any_element()
    }

    fn message(&self, bot: &Bot, i: usize, m: &Msg, cx: &mut Context<Self>) -> AnyElement {
        let el = match m {
            Msg::User(t) => div().flex().justify_end().child(
                div().max_w(px(520.)).px_4().py_2().rounded(px(18.)).bg(hex(INK)).text_color(hex(CREAM)).child(t.clone()),
            ),
            Msg::Bot(t) => div().child(
                div().max_w(px(640.)).px_4().py_3().rounded(px(18.)).bg(hex(CARD)).border_1().border_color(hex(LINE)).child(
                    TextView::markdown(("md", bot.id * 10_000 + i), t.clone()).selectable(true),
                ),
            ),
            Msg::Tool { verb, target, detail, open } => {
                let (id, open) = (bot.id, *open);
                div()
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
                            .text_color(hex(MUTED))
                            .cursor_pointer()
                            .hover(|d| d.text_color(hex(INK)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(Msg::Tool { open, .. }) = this.bot_mut(id).and_then(|b| b.msgs.get_mut(i)) {
                                    *open = !*open;
                                }
                                cx.notify();
                            }))
                            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).size_3())
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(*verb))
                            .child(target.clone()),
                    )
                    .when(open, |d| {
                        d.child(
                            div()
                                .ml_5()
                                .p_3()
                                .rounded(px(10.))
                                .bg(hex(SIDEBAR))
                                .text_xs()
                                .font_family("Menlo")
                                .text_color(hex(MUTED))
                                .whitespace_normal()
                                .child(detail.clone()),
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
            .bg(hex(CREAM))
            .text_color(hex(INK))
            .child(self.sidebar(cx))
            .child(self.chat(cx))
    }
}

fn main() {
    gpui_kit::application().with_assets(Assets).run(|cx| {
        gpui_kit::init(cx);
        Theme::update(cx, |t| {
            t.background = hex(CREAM);
            t.foreground = hex(INK);
            t.border = hex(LINE);
            t.input = hex(LINE);
            t.primary = hex(AMBER);
            t.ring = hex(AMBER);
            t.caret = hex(INK);
            t.selection = hex(AMBER).opacity(0.3);
            t.muted = hex(SIDEBAR);
            t.muted_foreground = hex(MUTED);
            t.accent = hex(SIDEBAR);
        });
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
