//! The sidebar: bots (drag to reorder), rooms, groups, plan usage, and the hatch menu.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{TRAFFIC_INSET, bar, heading, soft_shadow};
use crate::app::Eggbot;
use crate::app::bot::{PRESETS, drop_index};
use crate::claude::Meter;
use crate::egg::{Mood, egg};
use crate::ui::theme::{Palette, hex};
use crate::usage;

const ROW_H: f32 = 48.;
const ROW_GAP: f32 = 2.;
/// The line that shows where a dragged bot will land.
const DROP_LINE: u32 = 0x0A84FF;

impl Eggbot {
    pub(crate) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let (me, len) = (cx.entity().downgrade(), self.bots.len());
        // a bot dropped on row `i` lands just above it; the line shows only where that changes the order
        let drop_line = move |group: SharedString, before: usize, from: Option<usize>| {
            let noop = from.is_some_and(|f| drop_index(len, f, before).is_none());
            div().absolute().left_1().right_1().top(px(-ROW_GAP / 2. - 1.)).h(px(2.)).rounded_full().when(!noop, |d| {
                // GPUI applies group drag styles only to elements with a hitbox; a no-op group hover adds one
                d.group_hover(group.clone(), |s| s).group_drag_over::<DraggedBot>(group, |s| s.bg(hex(DROP_LINE)))
            })
        };
        let rows = self
            .bots
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let (id, confirming) = (b.id, self.confirm_delete == Some(b.id));
                let group: SharedString = format!("row-{id}").into();
                let hold = self.breach_of(b.provider);
                let waiting = !b.queue.is_empty() || self.schedules_wait(b);
                let (subtitle, sub_color) = if b.busy() {
                    let text = match b.queue.len() {
                        0 => b.status.clone().unwrap_or_else(|| "Thinking…".into()),
                        n => format!("Thinking… · {n} queued"),
                    };
                    (text, p.muted)
                } else if let Some(br) = hold.as_ref().filter(|br| br.level == usage::Level::Pause || waiting) {
                    let color = if br.level == usage::Level::Pause { p.err } else { p.warn };
                    (usage::short(usage::provider_name(b.provider), br), color)
                } else if !b.queue.is_empty() {
                    (format!("{} queued", b.queue.len()), p.muted)
                } else {
                    (b.blurb(), p.muted)
                };
                let trash = div()
                    .id(("trash", id))
                    .flex()
                    .items_center()
                    .px_1()
                    .py(px(2.))
                    .rounded(px(6.))
                    .text_xs()
                    .cursor_pointer()
                    .when(confirming, |d| d.bg(p.err).text_color(hex(0xFFFFFF)).px_2().child("Delete?"))
                    .when(!confirming, |d| d.text_color(p.muted).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)).hover(|d| d.text_color(p.ink)).child(Icon::new(IconName::Trash).size_3p5()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if this.confirm_delete == Some(id) {
                            this.delete(id, cx);
                        } else {
                            this.confirm_delete = Some(id);
                            cx.notify();
                        }
                    }));
                let me = me.clone();
                div()
                    .id(("bot", id))
                    .group(group.clone())
                    .relative()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(self.open_room.is_some() || self.open_group.is_some() || i != self.selected, |d| d.hover(|d| d.bg(p.hover)))
                    .on_click(cx.listener(move |this, _, window, cx| this.select(i, window, cx)))
                    // drag a row onto another to reorder
                    .on_drag(DraggedBot { ix: i, name: b.name.clone().into(), color: b.color(), p }, move |d, _, _, cx| {
                        me.update(cx, |this, cx| {
                            this.dragging = Some(d.ix);
                            cx.notify();
                        })
                        .ok();
                        cx.new(|_| d.clone())
                    })
                    .on_drop(cx.listener(move |this, d: &DraggedBot, _, cx| this.move_bot(d.ix, i, cx)))
                    .child(drop_line(group.clone(), i, self.dragging))
                    .child(egg(format!("side-{id}"), hex(b.color()), 16., b.mood()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_hidden()
                            .child(div().text_sm().text_color(p.ink).truncate().child(b.name.clone()))
                            .child(div().text_xs().text_color(sub_color).truncate().child(subtitle)),
                    )
                    .when(b.busy(), |d| d.child(Spinner::new().color(p.muted).xsmall()))
                    // unread takes the trash's slot; selecting the bot clears it
                    .when(b.unread && !b.busy(), |d| d.child(div().w(px(22.)).flex_none().flex().justify_center().child(div().size(px(7.)).rounded_full().bg(p.ink))))
                    .when(!b.unread && !b.busy(), |d| d.child(trash))
            })
            .collect::<Vec<_>>();

        // one highlight that springs to the selected row
        let highlight = div().absolute().left_0().right_0().h(px(ROW_H)).rounded(px(8.)).bg(p.tint).with_spring(
            "selection",
            SpringAnimation::new(SpringConfig::new(320., 28., 1.)).to(px(self.selected as f32 * (ROW_H + ROW_GAP))).with_epsilon(0.25),
            |d, top| d.top(top),
        );

        let handle = div().id("resize").absolute().top_0().bottom_0().right(px(-3.)).w(px(6.)).cursor_col_resize().when(self.resizing, |d| d.bg(p.line)).hover(|d| d.bg(p.line)).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.resizing = true;
                cx.notify();
            }),
        );

        div()
            .relative()
            .w(px(self.sidebar_w))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(p.side)
            .border_r_1()
            .border_color(p.line)
            .child(
                // traffic lights sit on the left of this row
                div().h(px(44.)).flex_none().flex().items_center().pl(px(TRAFFIC_INSET)).pr_2().child(heading("eggbot", p).flex_1()).child(
                    div()
                        .id("new-bot")
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .text_color(p.muted)
                        .cursor_pointer()
                        .when(self.menu_open, |d| d.bg(p.hover).text_color(p.ink))
                        .hover(|d| d.bg(p.hover).text_color(p.ink))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu_open = !this.menu_open;
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::Plus).size_4()),
                ),
            )
            .when(self.menu_open, |d| d.child(self.hatch_menu(cx)))
            .child(
                div()
                    .id("bots")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .px_2()
                    .pt_1()
                    .child(
                        div().relative().flex().flex_col().gap(px(ROW_GAP)).when(self.open_room.is_none() && self.open_group.is_none() && !self.bots.is_empty(), |d| d.child(highlight)).children(rows),
                    )
                    // the space below the last bot: drop here to move a bot to the end
                    .child(
                        div()
                            .id("bots-end")
                            .group("bots-end")
                            .relative()
                            .flex_grow(1.)
                            .min_h(px(ROW_H))
                            .on_drop(cx.listener(|this, d: &DraggedBot, _, cx| this.move_bot(d.ix, this.bots.len(), cx)))
                            .child(drop_line("bots-end".into(), self.bots.len(), self.dragging)),
                    ),
            )
            .child(self.rooms_nav(cx))
            .child(self.groups_nav(cx))
            .when(!self.meters.is_empty(), |d| {
                d.child(div().flex_none().flex().flex_col().gap_3().px_4().py_3().border_t_1().border_color(p.line).children(self.meters.iter().map(|m| self.usage_meter(m))))
            })
            .child(handle)
    }

    /// A selectable row for rooms and groups: same height, padding and icon slot as a bot row.
    fn nav_row(&self, id: (&'static str, usize), on: bool, icon: impl IntoElement, title: String, subtitle: String) -> Stateful<Div> {
        let p = self.p;
        div()
            .id(id)
            .h(px(ROW_H))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_2()
            .rounded(px(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(p.tint))
            .when(!on, |d| d.hover(|s| s.bg(p.hover)))
            .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(icon))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_hidden()
                    .child(div().text_sm().text_color(p.ink).truncate().child(title))
                    .child(div().text_xs().text_color(p.muted).truncate().child(subtitle)),
            )
    }

    /// A section title with its + button; the button lines up with the one in the sidebar header.
    fn nav_header(&self, title: &'static str, plus: Stateful<Div>) -> Div {
        let p = self.p;
        div().h(px(32.)).flex_none().flex().items_center().pl_4().pr_2().child(div().flex_1().text_xs().font_weight(FontWeight::MEDIUM).text_color(p.muted).child(title)).child(
            plus.size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_color(p.muted)
                .cursor_pointer()
                .hover(|d| d.bg(p.hover).text_color(p.ink))
                .child(Icon::new(IconName::Plus).size_4()),
        )
    }

    /// Rooms sit under the bot list: a title, who is in, and a dot when a member has news.
    fn rooms_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let rows = self.rooms.iter().map(|r| {
            let id = r.id;
            let on = self.open_room == Some(id);
            let fac = r.facilitator.and_then(|fid| self.bot(fid));
            let subtitle = match (fac, r.members.len()) {
                (Some(b), n) if n > 1 => format!("{} + {}", b.name, n - 1),
                (Some(b), _) => b.name.clone(),
                _ => "No bots yet".into(),
            };
            let icon = div().size(px(9.)).rounded(px(2.)).border_1().border_color(if on { p.ink } else { p.muted });
            self.nav_row(("room", id), on, icon, r.title.clone(), subtitle)
                .on_click(cx.listener(move |this, _, window, cx| this.show_room(id, window, cx)))
                .when(r.unread && !on, |d| d.child(div().size(px(7.)).flex_none().rounded_full().bg(p.ink)))
        });
        let plus = div().id("new-room").on_click(cx.listener(|this, _, window, cx| this.new_room(window, cx)));
        div()
            .flex_none()
            .flex()
            .flex_col()
            .pb_2()
            .border_t_1()
            .border_color(p.line)
            .child(self.nav_header("Rooms", plus))
            .child(div().id("room-list").max_h(px(ROW_H * 2. + ROW_GAP)).overflow_y_scroll().flex().flex_col().gap(px(ROW_GAP)).px_2().children(rows))
    }

    /// Groups sit under rooms. A group shares notes; it has no transcript and no facilitator.
    fn groups_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let rows = self.groups.iter().map(|g| {
            let id = g.id;
            let on = self.open_group == Some(id);
            let names: Vec<String> = g.members.iter().filter_map(|bid| self.bot(*bid).map(|b| b.name.clone())).collect();
            let subtitle = match names.as_slice() {
                [] => "No bots yet".to_string(),
                [one] => one.clone(),
                [first, rest @ ..] => format!("{first} + {}", rest.len()),
            };
            let icon = Icon::new(IconName::Network).size_4().text_color(if on { p.ink } else { p.muted });
            self.nav_row(("group", id), on, icon, g.title.clone(), subtitle).on_click(cx.listener(move |this, _, window, cx| this.show_group(id, window, cx)))
        });
        let plus = div().id("new-group").on_click(cx.listener(|this, _, window, cx| this.new_group(window, cx)));
        div()
            .flex_none()
            .flex()
            .flex_col()
            .pb_2()
            .border_t_1()
            .border_color(p.line)
            .child(self.nav_header("Groups", plus))
            .child(div().id("group-list").max_h(px(ROW_H * 2. + ROW_GAP)).overflow_y_scroll().flex().flex_col().gap(px(ROW_GAP)).px_2().children(rows))
    }

    fn usage_meter(&self, meter: &Meter) -> impl IntoElement {
        let p = self.p;
        let now = chrono::Local::now().timestamp();
        let at = chrono::DateTime::from_timestamp(meter.at, 0).map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
        let name = usage::provider_name(meter.provider);
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_xs()
            .text_color(p.muted)
            .child(div().flex().items_center().child(div().flex_1().font_weight(FontWeight::MEDIUM).child(name)).child(div().opacity(0.8).child(format!("updated {at}"))))
            .children(meter.windows.iter().map(|w| {
                let v = usage::window_used(w, now);
                let fill = if v >= self.pause {
                    p.err
                } else if v >= usage::AMBER {
                    p.warn
                } else {
                    p.muted
                };
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(34.)).child(w.label.clone()))
                    .child(div().flex_1().child(bar(v, 0., p, fill).w_full()))
                    .child(div().w(px(30.)).text_right().text_color(fill).child(format!("{}%", usage::percent(v))))
            }))
            .when_some(self.breach_of(meter.provider), |d, br| {
                let color = if br.level == usage::Level::Pause { p.err } else { p.warn };
                d.child(div().text_color(color).child(usage::meter_line(&br)))
            })
    }

    fn hatch_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        div()
            .mx_2()
            .mb_2()
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .bg(p.card)
            .border_1()
            .border_color(p.line)
            .shadow(soft_shadow(p))
            .child(div().px_2().py_1().text_xs().text_color(p.muted).child("New bot"))
            .children(PRESETS.iter().enumerate().map(|(i, preset)| {
                div()
                    .id(("preset", i))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.menu_open = false;
                        this.hatch(i);
                        if PRESETS[i].name == "Custom" {
                            this.open_editor(window, cx);
                        } else {
                            this.input.update(cx, |s, cx| s.focus(window, cx));
                        }
                        cx.notify();
                    }))
                    .child(egg(format!("preset-{i}"), hex(preset.color), 14., Mood::Still))
                    .child(
                        div().flex().flex_col().overflow_hidden().child(div().text_sm().text_color(p.ink).child(preset.name)).child(div().text_xs().text_color(p.muted).truncate().child(preset.blurb)),
                    )
            }))
            .with_animation("menu-in", Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
    }
}

/// A sidebar row being dragged; it also draws the small card that follows the pointer.
#[derive(Clone)]
struct DraggedBot {
    ix: usize,
    name: SharedString,
    color: u32,
    p: Palette,
}

impl Render for DraggedBot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .rounded(px(8.))
            .bg(p.card)
            .border_1()
            .border_color(p.line)
            .shadow(soft_shadow(p))
            .text_sm()
            .text_color(p.ink)
            .child(egg("dragged", hex(self.color), 14., Mood::Still))
            .child(self.name.clone())
    }
}
