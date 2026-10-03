//! The bot chat: the main pane, the virtual message list, and one row per message (tool lines, handoffs, queued bubbles).

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{READ_W, button};
use crate::app::bot::{Bot, Msg};
use crate::app::setup::needs_login;
use crate::app::{Eggbot, Panel};
use crate::claude::Provider;
use crate::egg::{Mood, egg};
use crate::ui::theme::hex;
use crate::{handoff, usage};

impl Eggbot {
    pub(crate) fn chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let main = div().flex_1().min_w_0().h_full().flex().flex_col().bg(p.bg);
        if let Some(setup) = &self.setup {
            return main.child(div().h(px(44.)).flex_none()).when(self.panel == Panel::Settings, |d| d.child(self.settings(cx))).child(self.setup_view(setup, cx)).into_any_element();
        }
        if self.open_room.is_some() {
            return self.room_view(cx);
        }
        if self.open_group.is_some() {
            return self.group_view(cx);
        }
        let settings = (self.panel == Panel::Settings).then(|| self.settings(cx).into_any_element());
        let Some(bot) = self.bots.get(self.selected) else {
            return main
                .child(div().h(px(44.)).flex_none())
                .children(settings)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .pb(px(60.))
                        .child(egg("empty", hex(0xE3D2B9), 32., Mood::Still))
                        .child(div().mt_4().text_2xl().text_color(p.ink).child("No bots yet"))
                        .child(div().mt_1().text_sm().text_color(p.muted).child("Create one with + in the sidebar, or press ⌘N.")),
                )
                .into_any_element();
        };

        let panels = match self.panel {
            Panel::None => None,
            Panel::Settings => settings,
            Panel::Editor => Some(self.editor(bot, cx).into_any_element()),
            Panel::Schedules => Some(self.schedules(bot, cx).into_any_element()),
            Panel::Skills => Some(self.skills(bot, cx).into_any_element()),
        };

        let body = if bot.msgs.is_empty() && !bot.queue.iter().any(|q| q.typed) {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .px_6()
                .pb(px(80.))
                .child(egg(format!("hero-{}", bot.id), hex(bot.color()), 32., bot.mood()))
                .child(div().mt_4().mb_6().text_2xl().text_color(p.ink).child(format!("What should {} work on?", bot.name)))
                .child(div().w_full().max_w(px(READ_W)).child(self.composer(bot, cx)))
                .into_any_element()
        } else {
            // a virtual list: only the messages on screen (plus some overdraw) are drawn
            let msgs = list(self.list.clone(), cx.processor(|this: &mut Self, ix: usize, _, cx| this.row(ix, cx))).flex_1();
            div().flex_1().min_h_0().flex().flex_col().child(msgs).child(div().px_6().pb_4().child(div().max_w(px(READ_W)).mx_auto().w_full().child(self.composer(bot, cx)))).into_any_element()
        };

        // new id per bot, so switching bots replays the fade
        main.child(self.topbar(bot, cx))
            .children(panels)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body).with_animation(
                ElementId::Name(format!("chat-{}", bot.id).into()),
                Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t),
            ))
            .into_any_element()
    }

    /// One row of the chat list: message `ix`, or the typing indicator after the last message.
    fn row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(bot) = self.bots.get(self.selected) else { return div().into_any_element() };
        let el = match bot.msgs.get(ix) {
            Some(m) => self.message(bot, ix, m, cx),
            None => {
                let queued: Vec<AnyElement> = bot.queue.iter().enumerate().filter(|(_, q)| q.typed).map(|(at, q)| self.queued(bot, at, &q.prompt, cx)).collect();
                div().flex().flex_col().gap_3().when(bot.waiting(), |d| d.child(self.typing(bot))).children(queued).into_any_element()
            }
        };
        let (last, hit) = (ix == bot.msgs.len(), self.find_current() == Some(ix));
        // more air before a new request, less between consecutive tool lines
        let top = match (ix.checked_sub(1).and_then(|j| bot.msgs.get(j)), bot.msgs.get(ix)) {
            (None, _) => 20.,
            (Some(Msg::Tool { .. }), Some(Msg::Tool { .. })) => 0.,
            (_, Some(Msg::User(_) | Msg::Kickoff { .. } | Msg::Scheduled { .. } | Msg::Handoff { .. })) => 16.,
            _ => 8.,
        };
        // the 8px inset leaves room for the search highlight without moving the text
        div()
            .w_full()
            .flex()
            .justify_center()
            .px_4()
            .pt(px(top))
            .when(last, |d| d.pb(px(20.)))
            .child(div().w_full().max_w(px(READ_W + 16.)).px_2().py_1().rounded(px(10.)).when(hit, |d| d.bg(self.p.tint)).child(el))
            .into_any_element()
    }

    /// Shown while the bot works without writing: a wobbling egg and a pulsing label.
    fn typing(&self, bot: &Bot) -> impl IntoElement {
        let label = bot.status.clone().unwrap_or_else(|| "Thinking…".into());
        div().flex().items_center().gap_2().text_sm().text_color(self.p.muted).child(egg(format!("typing-{}", bot.id), hex(bot.color()), 14., Mood::Thinking)).child(div().child(label).with_animation(
            "pulse",
            Animation::new(Duration::from_millis(1600)).repeat(),
            |d, t| d.opacity(0.45 + 0.55 * (t * std::f32::consts::TAU).cos().abs()),
        ))
    }

    /// A message waiting on the bot's queue (busy or throttled); × drops it. It becomes a normal bubble when its turn starts.
    fn queued(&self, bot: &Bot, at: usize, text: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let note = if bot.busy() { "Queued · sends after this turn" } else { "Queued" };
        let id = bot.id;
        let remove = div()
            .id(("unqueue", at))
            .p_1()
            .rounded(px(6.))
            .text_color(p.muted)
            .cursor_pointer()
            .hover(|d| d.bg(p.hover).text_color(p.ink))
            .on_click(cx.listener(move |this, _, _, cx| this.unqueue(id, at, cx)))
            .child(Icon::new(IconName::Close).size_3());
        div()
            .flex()
            .justify_end()
            .child(
                div()
                    .max_w(relative(0.75))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_4()
                    .py_2()
                    .rounded(px(18.))
                    .border_1()
                    .border_color(p.line)
                    .child(div().flex().items_center().gap_2().child(div().flex_1().text_xs().text_color(p.muted).child(note)).child(remove))
                    .child(div().text_size(px(15.)).line_height(relative(1.5)).text_color(p.muted).child(text.to_string())),
            )
            .into_any_element()
    }

    fn message(&self, bot: &Bot, i: usize, m: &Msg, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let el = match m {
            Msg::User(t) => div()
                .flex()
                .justify_end()
                .child(div().max_w(relative(0.75)).px_4().py_2().rounded(px(18.)).bg(p.bubble).text_size(px(15.)).line_height(relative(1.5)).text_color(p.ink).child(t.clone())),
            Msg::Kickoff { room_id, room, text, .. } => {
                let room_id = *room_id;
                let title = room.clone();
                div().flex().justify_end().child(
                    div()
                        .max_w(relative(0.75))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_4()
                        .py_2()
                        .rounded(px(18.))
                        .bg(p.bubble)
                        .child(
                            div()
                                .id(("kickoff", i))
                                .text_xs()
                                .text_color(p.muted)
                                .cursor_pointer()
                                .hover(|d| d.text_color(p.ink))
                                .on_click(cx.listener(move |this, _, window, cx| this.show_room(room_id, window, cx)))
                                .child(format!("Room · {title}")),
                        )
                        .child(div().text_size(px(15.)).line_height(relative(1.5)).text_color(p.ink).child(text.clone())),
                )
            }
            Msg::Bot(t) => div().text_size(px(15.)).line_height(relative(1.6)).text_color(p.ink).child(TextView::markdown(("md", bot.id * 100_000 + i), t.clone()).selectable(true)),
            Msg::Handoff { from, color, text, paused, open, .. } => {
                let (id, open, paused, from_name) = (bot.id, *open, *paused, from.clone());
                let long = text.chars().count() > 320;
                let shown: String = if open || !long { text.clone() } else { format!("{}…", text.chars().take(320).collect::<String>()) };
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(p.muted)
                            .child(egg(format!("from-{id}-{i}"), hex(*color), 12., Mood::Still))
                            .child(
                                div()
                                    .id(("from", i))
                                    .cursor_pointer()
                                    .hover(|d| d.text_color(p.ink))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(j) = this.bots.iter().position(|b| b.name == from_name) {
                                            this.select(j, window, cx);
                                        }
                                    }))
                                    .child(format!("Handoff from {from}")),
                            )
                            .child(div().flex_1())
                            .when(paused, |d| {
                                d.child(format!("Paused after {} handoffs", handoff::MAX_HOPS))
                                    .child(button(("continue", i), p).on_click(cx.listener(move |this, _, _, cx| this.continue_chain(id, i, cx))).child("Continue chain"))
                            }),
                    )
                    .child(div().text_size(px(15.)).line_height(relative(1.6)).text_color(p.ink).child(TextView::markdown(("handoff", bot.id * 100_000 + i), shown).selectable(true)))
                    .when(long, |d| {
                        d.child(
                            div()
                                .id(("more", i))
                                .text_xs()
                                .text_color(p.muted)
                                .cursor_pointer()
                                .hover(|d| d.text_color(p.ink))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(Msg::Handoff { open, .. }) = this.bot_mut(id).and_then(|b| b.msgs.get_mut(i)) {
                                        *open = !*open;
                                    }
                                    cx.notify();
                                }))
                                .child(if open { "Show less" } else { "Show more" }),
                        )
                    })
            }
            Msg::Scheduled { prompt, label } => div().flex().justify_end().child(
                div()
                    .max_w(relative(0.75))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_4()
                    .py_2()
                    .rounded(px(18.))
                    .bg(p.bubble)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(p.muted)
                            .child(Icon::new(if label == "Fresh start" { IconName::RefreshCw } else { IconName::Clock }).size_3())
                            .child(label.clone()),
                    )
                    .child(div().text_size(px(15.)).line_height(relative(1.5)).text_color(p.ink).child(prompt.clone())),
            ),
            Msg::SignedIn { provider, prompt } => {
                let id = bot.id;
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(p.ok)
                    .child(Icon::new(IconName::Check).size_4())
                    .child(format!("Signed in to {}", usage::provider_name(*provider)))
                    .when(prompt.is_some(), |d| d.child(button(("again", i), p).ml_2().on_click(cx.listener(move |this, _, _, cx| this.send_again(id, i, cx))).child("Send again")))
            }
            Msg::Divider(label) => {
                div().flex().items_center().gap_3().text_xs().text_color(p.muted).child(div().flex_1().h(px(1.)).bg(p.line)).child(label.clone()).child(div().flex_1().h(px(1.)).bg(p.line))
            }
            Msg::Sent { to } => {
                let to_name = to.clone();
                div().child(
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
                        .child(format!("Handed off to {to}")),
                )
            }
            Msg::Error(t) => div()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .text_color(p.err)
                .child(Icon::new(IconName::CircleAlert).size_4().flex_none())
                .child(div().min_w_0().child(t.clone()))
                .when(t.contains("Docker"), |d| d.child(button(("open-setup", i), p).ml_2().flex_none().on_click(cx.listener(|this, _, _, cx| this.open_setup(cx))).child("Open setup")))
                .when(needs_login(t, Provider::Claude) || needs_login(t, Provider::Codex), |d| {
                    let codex = needs_login(t, Provider::Codex);
                    d.child(button(("sign-in", i), p).ml_2().flex_none().on_click(cx.listener(move |this, _, _, cx| this.sign_in(codex, cx))).child(if codex {
                        "Sign in to Codex"
                    } else {
                        "Sign in to Claude"
                    }))
                }),
            Msg::Tool { verb, target, detail, open, .. } => {
                let (id, open) = (bot.id, *open);
                let detail = if detail.is_empty() { "running…".to_string() } else { detail.clone() };
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
                            .text_color(p.muted)
                            .cursor_pointer()
                            .hover(|d| d.text_color(p.ink))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(Msg::Tool { open, .. }) = this.bot_mut(id).and_then(|b| b.msgs.get_mut(i)) {
                                    *open = !*open;
                                }
                                cx.notify();
                            }))
                            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).size_3().flex_none())
                            .child(div().flex_none().child(verb.clone()))
                            .child(div().min_w_0().truncate().font_family("Menlo").child(target.clone())),
                    )
                    .when(open, |d| d.child(div().ml_5().p_3().rounded(px(8.)).bg(p.bubble).text_xs().font_family("Menlo").text_color(p.muted).whitespace_normal().child(detail)))
            }
        };
        el.with_animation(ElementId::Name(format!("in-{}-{i}", bot.id).into()), Animation::new(Duration::from_millis(200)).with_easing(ease_out_quint()), |d, t| d.opacity(t)).into_any_element()
    }
}
