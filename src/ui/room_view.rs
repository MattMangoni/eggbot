//! The room view: header, read-only room memory, the transcript list, and the room setup (title, kickoff, roster).

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{READ_W, TRAFFIC_INSET, button, primary};
use crate::app::{Eggbot, Panel};
use crate::egg::{Mood, egg};
use crate::handoff;
use crate::ui::theme::hex;

impl Eggbot {
    /// The room's transcript, then the roster. Start still talks to the facilitator only.
    pub(crate) fn room_view(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let notes = std::fs::read_to_string(crate::room::notes_file(&crate::app::state::data_dir(), room_id)).unwrap_or_default();
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
        let text = crate::app::bot::reply_text(&bot.msgs, bot.reply_from);
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
}
