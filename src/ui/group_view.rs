//! The group view: title, members, and the shared notes (read-only).

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::Input;
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{TRAFFIC_INSET, button};
use crate::egg::egg;
use crate::{Eggbot, Panel, hex};

impl Eggbot {
    /// Title, members, and their shared notes (read-only). Nothing here is a transcript.
    pub(crate) fn group_view(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let notes = std::fs::read_to_string(crate::group::notes_file(&crate::data_dir(), group_id)).unwrap_or_default();
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
}
