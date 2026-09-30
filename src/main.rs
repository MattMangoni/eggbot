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

#[derive(Clone, Copy)]
enum Provider {
    Claude,
}

impl Provider {
    fn label(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
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

const ROW_H: f32 = 60.;
const ROW_GAP: f32 = 4.;

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
    poked: Option<Instant>,
    pokes: u32,
    busy: bool,
    msgs: Vec<Msg>,
}

impl Bot {
    fn mood(&self) -> Mood {
        let now = Instant::now();
        match now.checked_duration_since(self.born) {
            None => Mood::Unborn,
            Some(a) if a < egg::HATCH => Mood::Hatching,
            _ if self.busy => Mood::Thinking,
            _ if self.poked.is_some_and(|t| now - t < egg::BOING) => Mood::Boing(self.pokes),
            _ => Mood::Idle,
        }
    }

    /// True while the reply is being written, before its first word arrives.
    fn waiting(&self) -> bool {
        self.busy && !matches!(self.msgs.last(), Some(Msg::Bot(_)))
    }
}

struct Eggbot {
    p: Palette,
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
        cx.observe_window_appearance(window, |this, window, cx| {
            this.p = Palette::apply(window, cx);
            cx.notify();
        })
        .detach();
        input.update(cx, |s, cx| s.focus(window, cx));
        let p = Palette::apply(window, cx);
        let mut this = Self { p, bots: vec![], selected: 0, next_id: 0, menu_open: false, input, scroll: ScrollHandle::new() };
        for (i, preset) in PRESETS[..3].iter().enumerate() {
            this.hatch(preset, Duration::from_millis(300 + 350 * i as u64), cx);
        }
        this.selected = 0;
        this
    }

    /// Repaint after `delay`, so time-based moods (hatch, boing) can end.
    fn refresh_after(delay: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
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
            poked: None,
            pokes: 0,
            busy: false,
            msgs: vec![],
        });
        self.next_id += 1;
        self.selected = self.bots.len() - 1;
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
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
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
            wait(900).await;
            this.update(cx, |this, cx| {
                this.push(id, Msg::Tool { verb: "Read", target: "src/main.rs".into(), detail: "fn main() {\n    gpui_kit::application().run(…)\n}".into(), open: false });
                cx.notify();
            })
            .ok();
            wait(1200).await;
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
        let p = self.p;
        let rows = self.bots.iter().enumerate().map(|(i, b)| {
            let active = i == self.selected;
            div()
                .id(("bot", b.id))
                .h(px(ROW_H))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .rounded(px(14.))
                .cursor_pointer()
                .when(!active, |d| d.hover(|d| d.bg(p.card.opacity(0.5))))
                .on_click(cx.listener(move |this, _, window, cx| this.select(i, window, cx)))
                .child(egg(format!("side-{}", b.id), b.color, 30., b.mood()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .overflow_hidden()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(b.name.clone()))
                        .child(div().text_xs().text_color(p.muted).truncate().child(if b.busy { "thinking…" } else { b.blurb })),
                )
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
                        this.hatch(&PRESETS[i], Duration::ZERO, cx);
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
            return div().flex_1().into_any_element();
        };
        let sel = self.selected;

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
                    .child(egg(format!("head-{}", bot.id), bot.color, 34., bot.mood())),
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
                            .child(bot.provider.label())
                            .child("·")
                            .child(div().size(px(6.)).rounded_full().bg(if bot.busy { p.amber } else { p.ok }))
                            .child(if bot.busy { "working" } else { "idle" }),
                    ),
            )
            // ponytail: placeholder until Phase 3 mounts a real project folder
            .child(
                div()
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
                    .child("No project folder"),
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
                        .child(egg(format!("hero-{}", bot.id), bot.color, 88., bot.mood())),
                )
                .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(format!("Say hi to {}", bot.name)))
                .child(div().text_sm().text_color(p.muted).child(bot.blurb))
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
                        .bg(if bot.busy { p.line } else { p.amber })
                        .text_color(hex(0x2B2621))
                        .cursor_pointer()
                        .hover(|d| d.opacity(0.85))
                        .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
                        .child(Icon::new(IconName::ArrowUp).size_4()),
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

    /// Small thinking egg and three bouncing dots, shown until the first word arrives.
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
            .child(egg(format!("typing-{}", bot.id), bot.color, 24., Mood::Thinking))
            .child(div().h(px(24.)).flex().items_end().gap(px(5.)).pb_1().children((0..3).map(dot)))
            .with_animation("typing-in", Animation::new(Duration::from_millis(200)), |d, t| d.opacity(t))
    }

    fn message(&self, bot: &Bot, i: usize, m: &Msg, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let el = match m {
            Msg::User(t) => div().flex().justify_end().child(
                div().max_w(px(520.)).px_4().py_2().rounded(px(18.)).bg(p.ink).text_color(p.bg).child(t.clone()),
            ),
            Msg::Bot(t) => {
                let streaming = bot.busy && i + 1 == bot.msgs.len();
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(div().mt_1().child(egg(format!("msg-{}-{i}", bot.id), bot.color, 24., if streaming { Mood::Thinking } else { Mood::Still })))
                    .child(
                        div().max_w(px(640.)).px_4().py_3().rounded(px(18.)).bg(p.card).border_1().border_color(p.line).child(
                            TextView::markdown(("md", bot.id * 10_000 + i), t.clone()).selectable(true),
                        ),
                    )
            }
            Msg::Tool { verb, target, detail, open } => {
                let (id, open) = (bot.id, *open);
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
                                .bg(p.side)
                                .text_xs()
                                .font_family("Menlo")
                                .text_color(p.muted)
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
