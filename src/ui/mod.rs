//! Everything eggbot draws. State and behaviour live in `main.rs`.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::claude::Provider;
use crate::egg::{Mood, egg};
use crate::{
    Appearance, Bot, CloseWindow, CycleAppearance, Dismiss, Eggbot, Find, FocusInput, Msg, NewBot, NextBot, OpenSettings, OpenSetup, Palette, Panel, PrevBot, Quit, SelectBot, StopTurn, ToggleSidebar,
    handoff, hex, set_dock_icon,
};

mod composer;
mod panels;
mod sidebar;
mod topbar;

const READ_W: f32 = 720.;
const SIDEBAR_MIN: f32 = 200.;
const SIDEBAR_MAX: f32 = 420.;
/// Room for the traffic lights. The chat top bar uses it when the sidebar is closed.
const TRAFFIC_INSET: f32 = 84.;

/// A small outlined button (top bar, panels, notices).
fn button(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div().id(id).flex().items_center().gap_1().h(px(28.)).px_3().rounded(px(8.)).border_1().border_color(p.line).text_xs().text_color(p.ink).cursor_pointer().hover(|d| d.bg(p.hover))
}

/// The filled variant for the main action of a panel. GPUI panics if `.hover` is set twice, so it is not built on `button`.
fn primary(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div().id(id).flex().items_center().h(px(28.)).px_3().rounded(px(8.)).bg(p.ink).text_xs().text_color(p.bg).cursor_pointer().hover(|d| d.opacity(0.85))
}

/// A soft, layered shadow (a hairline plus a wide faint blur); none in dark mode, where borders carry depth.
fn soft_shadow(p: Palette) -> Vec<BoxShadow> {
    if p.bg.l < 0.5 {
        return vec![];
    }
    let shadow =
        |alpha: f32, y: f32, blur: f32, spread: f32| BoxShadow { color: hsla(0., 0., 0., alpha), offset: point(px(0.), px(y)), blur_radius: px(blur), spread_radius: px(spread), inset: false };
    vec![shadow(0.03, 1., 2., 0.), shadow(0.05, 8., 28., -6.)]
}

/// A quiet text link with an icon (the row under the composer).
fn link(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div().id(id).flex().items_center().gap_1().px_2().py_1().rounded(px(6.)).text_xs().text_color(p.muted).cursor_pointer().hover(|d| d.bg(p.hover).text_color(p.ink))
}

fn bar(used: f32, width: f32, p: Palette, fill: Hsla) -> Div {
    let used = used.clamp(0., 1.);
    div().w(px(width)).h(px(4.)).rounded_full().bg(p.tint).child(div().h_full().rounded_full().w(relative(used)).bg(fill))
}

/// The bordered box around a text field in the panels.
fn field(p: Palette) -> Div {
    div().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line)
}

/// A panel section title.
fn heading(t: impl Into<SharedString>, p: Palette) -> Div {
    div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.ink).child(t.into())
}

impl Eggbot {
    fn chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn panel(&self) -> Div {
        let p = self.p;
        div().mx_auto().mt_3().w_full().max_w(px(READ_W)).p_4().flex().flex_col().gap_3().rounded(px(12.)).bg(p.card).border_1().border_color(p.line).shadow(soft_shadow(p))
    }

    /// The room's transcript, then the roster. Start still talks to the facilitator only.
    fn room_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let Some(id) = self.open_room else { return div().flex_1().into_any_element() };
        let Some((title, started, has_log)) = self.rooms.iter().find(|r| r.id == id).map(|r| (r.title.clone(), r.started, !r.transcript.is_empty())) else {
            return div().flex_1().into_any_element();
        };
        let reading = has_log || !self.room_live(id).is_empty();
        let setup = self.room_setup(cx);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .when(!self.sidebar_open, |d| d.pl(px(TRAFFIC_INSET)))
                    .border_b_1()
                    .border_color(p.line)
                    .child(div().text_sm().text_color(p.muted).child("Rooms"))
                    .child(div().text_sm().text_color(p.muted).child("/"))
                    .child(div().flex_1().min_w_0().text_sm().text_color(p.ink).truncate().child(title)),
            )
            .when(self.panel == Panel::Settings, |d| d.child(self.settings(cx)))
            .child(self.room_memory(id))
            .when(reading, |d| {
                d.child(div().flex_1().min_h_0().flex().flex_col().child(list(self.room_list.clone(), cx.processor(|this: &mut Self, ix: usize, _, cx| this.room_row(ix, cx))).flex_1()))
            })
            .when(!reading && started, |d| {
                d.child(div().px_6().pt_8().text_sm().text_color(p.muted).child("This room already started. New replies show up here; earlier ones stay in each bot's chat."))
            })
            .child(div().id("room-body").overflow_y_scroll().when(reading, |d| d.flex_none().max_h(px(320.))).when(!reading, |d| d.flex_1()).child(setup))
            .into_any_element()
    }

    /// Room memory, read-only. Anyone who opens the room can read it. Only a member bot can write.
    fn room_memory(&self, room_id: usize) -> AnyElement {
        let p = self.p;
        let notes = std::fs::read_to_string(crate::room::notes_file(&super::data_dir(), room_id)).unwrap_or_default();
        let notes = notes.trim().to_string();
        div()
            .id("room-memory")
            .flex()
            .flex_col()
            .gap_2()
            .flex_none()
            .max_h(px(180.))
            .overflow_y_scroll()
            .px_6()
            .py_4()
            .border_b_1()
            .border_color(p.line)
            .child(div().text_xs().text_color(p.muted).child("Memory"))
            .child(div().text_xs().text_color(p.muted).child("Anyone who opens this room can read this. Only a member bot can add to it. This is not the transcript."))
            .child(if notes.is_empty() {
                div().text_sm().text_color(p.muted).child("No memory yet.").into_any_element()
            } else {
                div().text_sm().text_color(p.ink).child(TextView::markdown(("room-memory", room_id), notes).selectable(true)).into_any_element()
            })
            .into_any_element()
    }

    /// Title, kickoff, and roster. Start talks to the facilitator; peers join through @Name.
    fn room_setup(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let Some(room_id) = self.open_room else { return div().into_any_element() };
        let Some((title, kickoff, members, facilitator, started)) =
            self.rooms.iter().find(|r| r.id == room_id).map(|r| (r.title.clone(), r.kickoff.clone(), r.members.clone(), r.facilitator, r.started))
        else {
            return div().into_any_element();
        };
        let why = crate::room::block(&title, &kickoff, &members, facilitator);
        let label = |t: &'static str| div().text_xs().text_color(p.muted).child(t);
        let field = || div().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line);
        let roster: Vec<_> = self
            .bots
            .iter()
            .map(|b| {
                let (bot_id, on, fac) = (b.id, members.contains(&b.id), facilitator == Some(b.id));
                let (unread, busy) = (b.unread, b.busy());
                div()
                    .id(("member", bot_id))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .py_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_member(room_id, bot_id, cx)))
                    .child(div().size(px(16.)).flex_none().rounded(px(4.)).border_1().border_color(if on { p.ink } else { p.line }).when(on, |d| d.bg(p.ink)))
                    .child(egg(format!("room-{bot_id}"), hex(b.color()), 14., b.mood()))
                    .child(div().flex_1().min_w_0().text_sm().text_color(p.ink).truncate().child(b.name.clone()))
                    .when(busy, |d| d.child(Spinner::new().color(p.muted).xsmall()))
                    .when(on && fac, |d| d.child(div().text_xs().text_color(p.muted).child("Facilitator")))
                    .when(on && !fac, |d| {
                        d.child(
                            div()
                                .id(("fac", bot_id))
                                .text_xs()
                                .text_color(p.muted)
                                .cursor_pointer()
                                .hover(|s| s.text_color(p.ink))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.set_facilitator(room_id, bot_id, cx);
                                }))
                                .child("Make facilitator"),
                        )
                    })
                    .child(
                        div()
                            .id(("open-bot", bot_id))
                            .text_xs()
                            .text_color(p.muted)
                            .cursor_pointer()
                            .hover(|s| s.text_color(p.ink))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_bot(bot_id, window, cx);
                            }))
                            .child(if unread { "Open · new" } else { "Open" }),
                    )
            })
            .collect();
        let confirming = self.confirm_delete_room == Some(room_id);
        div()
            .px_6()
            .pb_8()
            .child(
                self.panel()
                    .child(div().flex().flex_col().gap_1().child(label("Title")).child(field().child(Input::new(&self.room_title).appearance(false))))
                    .child(div().flex().flex_col().gap_1().child(label("Kickoff")).child(field().child(Textarea::new(&self.room_kickoff).appearance(false))))
                    .child(div().text_xs().text_color(p.muted).child("Start sends this to the facilitator only. Their reply, and any @Name that stays in this room, shows here. There is no lead."))
                    .child(div().h(px(1.)).bg(p.line))
                    .child(label("Bots"))
                    .when(self.bots.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("Hatch a bot first, then add it here.")))
                    .child(div().flex().flex_col().gap_1().children(roster))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pt_1()
                            .child(div().flex_1())
                            .when_some(self.room_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e)))
                            .when(self.room_error.is_none(), |d| d.when_some(self.room_status.clone(), |d, s| d.child(div().flex_1().text_xs().text_color(p.muted).child(s))))
                            .child(button("delete-room", p).on_click(cx.listener(move |this, _, _, cx| this.delete_room(room_id, cx))).child(if confirming { "Delete room?" } else { "Delete" }))
                            .child(primary("start-room", p).when(why.is_some(), |d| d.opacity(0.4)).on_click(cx.listener(move |this, _, _, cx| this.start_room(cx))).child(if started {
                                "Start again"
                            } else {
                                "Start"
                            })),
                    ),
            )
            .into_any_element()
    }

    /// One row of the room transcript, or a member still writing this room's turn.
    fn room_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(id) = self.open_room else { return div().into_any_element() };
        let live = self.room_live(id);
        let Some((ev, n)) = self.rooms.iter().find(|r| r.id == id).map(|r| (r.transcript.get(ix).cloned(), r.transcript.len() + live.len())) else {
            return div().into_any_element();
        };
        let transcript_len = n - live.len();
        let el = if let Some(ev) = ev {
            self.room_event(id, ix, &ev, cx)
        } else if ix >= transcript_len
            && let Some(&bot_id) = live.get(ix - transcript_len)
        {
            self.room_live_row(bot_id)
        } else {
            div().into_any_element()
        };
        let last = n > 0 && ix + 1 == n;
        div()
            .w_full()
            .flex()
            .justify_center()
            .px_4()
            .pt(px(if ix == 0 { 20. } else { 8. }))
            .when(last, |d| d.pb(px(20.)))
            .child(div().w_full().max_w(px(READ_W + 16.)).px_2().py_1().rounded(px(10.)).child(el))
            .into_any_element()
    }

    fn room_event(&self, room_id: usize, ix: usize, ev: &crate::room::Event, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let el = match ev {
            crate::room::Event::Kickoff { to, name, text } => {
                let to = *to;
                div().flex().justify_end().child(
                    div()
                        .max_w(relative(0.75))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .px_4()
                        .py_2()
                        .rounded(px(18.))
                        .bg(p.bubble)
                        .child(
                            div()
                                .id(("room-kick", ix))
                                .text_xs()
                                .text_color(p.muted)
                                .cursor_pointer()
                                .hover(|d| d.text_color(p.ink))
                                .on_click(cx.listener(move |this, _, window, cx| this.open_bot(to, window, cx)))
                                .child(format!("Kickoff · {name}")),
                        )
                        .child(div().text_size(px(15.)).line_height(relative(1.5)).text_color(p.ink).child(text.clone())),
                )
            }
            crate::room::Event::Reply { bot, name, color, text } => {
                let bot_id = *bot;
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div().flex().items_center().gap_2().text_xs().text_color(p.muted).child(egg(format!("room-egg-{room_id}-{ix}"), hex(*color), 14., Mood::Still)).child(
                            div()
                                .id(("room-who", ix))
                                .cursor_pointer()
                                .hover(|d| d.text_color(p.ink))
                                .on_click(cx.listener(move |this, _, window, cx| this.open_bot(bot_id, window, cx)))
                                .child(name.clone()),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(relative(1.6))
                            .text_color(p.ink)
                            .child(TextView::markdown(("room-md", room_id.saturating_mul(100_000).saturating_add(ix)), text.clone()).selectable(true)),
                    )
            }
            crate::room::Event::Handoff { from_name, to, to_name, paused, .. } => {
                let (to, paused) = (*to, *paused);
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(Icon::new(IconName::ArrowRight).size_3())
                    .child(format!("{from_name} handed off to"))
                    .child(
                        div()
                            .id(("room-to", ix))
                            .cursor_pointer()
                            .hover(|d| d.text_color(p.ink))
                            .on_click(cx.listener(move |this, _, window, cx| this.open_bot(to, window, cx)))
                            .child(to_name.clone()),
                    )
                    .when(paused, |d| {
                        d.child(format!("Paused after {} handoffs", handoff::MAX_HOPS))
                            .child(button(("room-cont", ix), p).on_click(cx.listener(move |this, _, _, cx| this.continue_room(room_id, to, cx))).child("Continue chain"))
                    })
            }
            crate::room::Event::Trouble { name, text, .. } => div().text_sm().text_color(p.err).child(format!("{name} · {text}")),
        };
        el.into_any_element()
    }

    /// The reply streaming into a member's chat for this room. It is not saved on the room until the turn ends.
    fn room_live_row(&self, bot_id: usize) -> AnyElement {
        let p = self.p;
        let Some(bot) = self.bots.iter().find(|b| b.id == bot_id) else { return div().into_any_element() };
        let text = super::reply_text(&bot.msgs, bot.reply_from);
        let body = if text.trim().is_empty() {
            div().text_sm().text_color(p.muted).child(bot.status.clone().unwrap_or_else(|| "Thinking…".into())).into_any_element()
        } else {
            div().text_size(px(15.)).line_height(relative(1.6)).text_color(p.ink).child(TextView::markdown(("room-live", bot_id), text).selectable(true)).into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(egg(format!("room-live-egg-{bot_id}"), hex(bot.color()), 14., bot.mood()))
                    .child(bot.name.clone())
                    .child(Spinner::new().color(p.muted).xsmall()),
            )
            .child(body)
            .into_any_element()
    }

    /// Title, members, and their shared notes (read-only). Nothing here is a transcript.
    fn group_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let Some(id) = self.open_group else { return div().flex_1().into_any_element() };
        let Some(title) = self.groups.iter().find(|g| g.id == id).map(|g| g.title.clone()) else {
            return div().flex_1().into_any_element();
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .when(!self.sidebar_open, |d| d.pl(px(TRAFFIC_INSET)))
                    .border_b_1()
                    .border_color(p.line)
                    .child(div().text_sm().text_color(p.muted).child("Groups"))
                    .child(div().text_sm().text_color(p.muted).child("/"))
                    .child(div().flex_1().min_w_0().text_sm().text_color(p.ink).truncate().child(title)),
            )
            .when(self.panel == Panel::Settings, |d| d.child(self.settings(cx)))
            .child(div().id("group-body").flex_1().overflow_y_scroll().child(self.group_setup(cx)))
            .into_any_element()
    }

    fn group_setup(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let Some(group_id) = self.open_group else { return div().into_any_element() };
        let Some(members) = self.groups.iter().find(|g| g.id == group_id).map(|g| g.members.clone()) else {
            return div().into_any_element();
        };
        let heading = |t: &'static str| div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.ink).child(t);
        let hint = |t: &'static str| div().text_xs().text_color(p.muted).child(t);
        let divider = || div().h(px(1.)).bg(p.line);
        // A filled box alone reads as a plain white square in dark mode, so the tick carries the state.
        let check = |on: bool| {
            div()
                .size(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .border_1()
                .border_color(if on { p.ink } else { p.muted.opacity(0.6) })
                .when(on, |d| d.bg(p.ink).child(Icon::new(IconName::Check).size_3().text_color(p.bg)))
        };
        let roster: Vec<_> = self
            .bots
            .iter()
            .map(|b| {
                let (bot_id, on) = (b.id, members.contains(&b.id));
                div()
                    .id(("group-member", bot_id))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .px_2()
                    .py_1p5()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_group_member(group_id, bot_id, cx)))
                    .child(check(on))
                    .child(egg(format!("group-{bot_id}"), hex(b.color()), 16., b.mood()))
                    .child(div().flex_1().min_w_0().text_sm().text_color(if on { p.ink } else { p.muted }).truncate().child(b.name.clone()))
                    .child(
                        div()
                            .id(("group-open", bot_id))
                            .px_1p5()
                            .rounded(px(6.))
                            .text_xs()
                            .text_color(p.muted)
                            .cursor_pointer()
                            .hover(|s| s.text_color(p.ink))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_bot(bot_id, window, cx);
                            }))
                            .child("Open chat"),
                    )
            })
            .collect();
        let count = members.iter().filter(|id| self.bots.iter().any(|b| b.id == **id)).count();
        let confirming = self.confirm_delete_group == Some(group_id);
        div()
            .px_6()
            .pb_8()
            .child(
                self.panel()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line).child(Input::new(&self.group_title).appearance(false)))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted)
                                    .child("Every bot in this group reads the same shared notes on each turn. A member adds to them with a group bullet at the end of a reply:"),
                            )
                            .child(div().px_3().py_2().rounded(px(8.)).bg(p.bubble).text_xs().font_family("Menlo").text_color(p.ink).child("- group fact: we deploy on Fridays"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted)
                                    .child("Tell one member \"remember for the group: we deploy on Fridays\" and the others know it next turn. Private notes and skills stay on each bot."),
                            ),
                    )
                    .child(divider())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().flex().items_baseline().gap_2().child(heading("Members")).child(div().text_xs().text_color(p.muted).child(if count == 1 {
                                "1 bot".to_string()
                            } else {
                                format!("{count} bots")
                            })))
                            .child(hint("Tick a bot to let it read and add to the shared notes.")),
                    )
                    .child(
                        div().flex().flex_col().gap_0p5().when(self.bots.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("Hatch a bot first, then add it here."))).children(roster),
                    )
                    .child(divider())
                    .child(self.group_notes(group_id))
                    .child(
                        div().flex().items_center().gap_2().child(div().flex_1()).child(
                            button("delete-group", p)
                                .on_click(cx.listener(move |this, _, _, cx| this.delete_group(group_id, cx)))
                                .when(confirming, |d| d.border_color(p.err).text_color(p.err))
                                .child(if confirming { "Delete group and notes?" } else { "Delete group" }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// Shared notes, read-only, like room memory. Only member bots write them.
    fn group_notes(&self, group_id: usize) -> AnyElement {
        let p = self.p;
        let notes = std::fs::read_to_string(crate::group::notes_file(&super::data_dir(), group_id)).unwrap_or_default();
        // Markdown headings render far larger than the panel text, so section titles show as bold labels.
        let notes = notes
            .lines()
            .filter(|line| !line.starts_with("# "))
            .map(|line| line.strip_prefix("## ").map_or(line.to_string(), |h| format!("**{}**", h.trim())))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.ink).child("Shared notes"))
                    .child(div().text_xs().text_color(p.muted).child("Read-only. Member bots write these; you cannot edit them here.")),
            )
            .child(div().id("group-notes").max_h(px(320.)).overflow_y_scroll().px_3().py_2().rounded(px(8.)).border_1().border_color(p.line).child(if notes.is_empty() {
                div().text_sm().text_color(p.muted).child("No shared notes yet.").into_any_element()
            } else {
                div().text_sm().text_color(p.ink).child(TextView::markdown(("group-notes", group_id), notes).selectable(true)).into_any_element()
            }))
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
                                    if let Some(Msg::Handoff { open, .. }) = this.bots.iter_mut().find(|b| b.id == id).and_then(|b| b.msgs.get_mut(i)) {
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
                    .child(format!("Signed in to {}", if *provider == Provider::Codex { "Codex" } else { "Claude" }))
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
                .when(t.contains("/login") || t.contains("codex login"), |d| {
                    let codex = t.contains("codex login");
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
                                if let Some(Msg::Tool { open, .. }) = this.bots.iter_mut().find(|b| b.id == id).and_then(|b| b.msgs.get_mut(i)) {
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

impl Render for Eggbot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.selects_stale {
            self.sync_selects(window, cx);
        }
        self.sync_draft(window, cx);
        self.sync_list();
        self.sync_room_list();
        // no background here: the window is blurred behind the translucent sidebar
        div()
            .size_full()
            .flex()
            .text_color(self.p.ink)
            .when(self.resizing, |d| d.cursor_col_resize())
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.resizing && this.sidebar_open {
                    this.sidebar_w = e.position.x.as_f32().clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resizing {
                        this.resizing = false;
                        this.save();
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.request_quit(window, cx)))
            .on_action(cx.listener(|this, _: &NewBot, _, cx| {
                // the hatch menu lives in the sidebar
                if !this.sidebar_open {
                    this.toggle_sidebar(cx);
                    this.menu_open = true;
                } else {
                    this.menu_open = !this.menu_open;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FocusInput, window, cx| this.focus_main(window, cx)))
            .on_action(cx.listener(|this, _: &PrevBot, window, cx| {
                if this.selected > 0 {
                    this.select(this.selected - 1, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextBot, window, cx| {
                if this.selected + 1 < this.bots.len() {
                    this.select(this.selected + 1, window, cx);
                }
            }))
            .on_action(cx.listener(|this, SelectBot(n): &SelectBot, window, cx| {
                if *n < this.bots.len() {
                    this.select(*n, window, cx);
                }
            }))
            .on_action(cx.listener(|this, a: &Appearance, window, cx| this.set_appearance(*a, window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSetup, _, cx| this.open_setup(cx)))
            .on_action(cx.listener(|this, _: &CycleAppearance, window, cx| this.set_appearance(this.appearance.next(), window, cx)))
            .on_action(cx.listener(|this, _: &Find, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &StopTurn, _, cx| this.stop(cx)))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                if this.menu_open {
                    this.menu_open = false;
                    cx.notify();
                } else if this.find_open {
                    this.close_find(window, cx);
                } else if this.panel != Panel::None {
                    this.close_panel(window, cx);
                } else if this.open_room.is_none() && this.open_group.is_none() {
                    this.stop(cx);
                }
            }))
            .on_action(cx.listener(|_, _: &CloseWindow, _, cx| {
                cx.hide();
                set_dock_icon(false);
            }))
            .when(self.sidebar_open, |d| d.child(self.sidebar(cx)))
            .child(self.chat(cx))
    }
}
