//! The bot chat's top bar: name and editor toggle, folders popover, search field, context meter, Fresh start.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{TRAFFIC_INSET, bar, button};
use crate::egg::egg;
use crate::{Bot, Eggbot, Panel, hex};

impl Eggbot {
    /// The top-bar folders button: "Add folder" with none, else the folder name or count, opening a list to remove or add.
    fn folders_button(&self, bot: &Bot, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let label = match bot.folders.as_slice() {
            [] => "Add folder".to_string(),
            [f] => f.name.clone(),
            all => format!("{} folders", all.len()),
        };
        let trigger = Button::new("folders").ghost().xsmall().icon(IconName::Folder).label(label);
        if bot.folders.is_empty() {
            return trigger.on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))).into_any_element();
        }
        // the caret marks it as a dropdown; "Add folder" above opens the picker directly
        let trigger = trigger.dropdown_caret(true);
        let (me, id, folders) = (cx.entity().downgrade(), bot.id, bot.folders.clone());
        Popover::new("folders-list")
            .trigger(trigger)
            .content(move |_, _, cx| {
                let rows = folders.iter().enumerate().map(|(i, f)| {
                    let parent = f.path.parent().map(short_path).unwrap_or_default();
                    let me = me.clone();
                    div()
                        .id(("folder-row", i))
                        .group("folder-row")
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .hover(|d| d.bg(p.hover))
                        .child(Icon::new(IconName::Folder).size_3p5().text_color(p.muted))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().text_color(p.ink).truncate().child(f.name.clone()))
                                .child(div().text_xs().text_color(p.muted).truncate().child(parent)),
                        )
                        .child(
                            div()
                                .id(("unmount", i))
                                .p_1()
                                .rounded(px(6.))
                                .text_color(p.muted)
                                .cursor_pointer()
                                .invisible()
                                .group_hover("folder-row", |s| s.visible())
                                .hover(|d| d.text_color(p.ink))
                                .on_click(move |_, _, cx| {
                                    me.update(cx, |this, cx| this.remove_folder(id, i, cx)).ok();
                                })
                                .child(Icon::new(IconName::Close).size_3()),
                        )
                });
                let me = me.clone();
                div().w(px(300.)).flex().flex_col().children(rows).child(div().h(px(1.)).my_1().bg(p.line)).child(
                    div()
                        .id("folder-add")
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .text_sm()
                        .text_color(p.ink)
                        .cursor_pointer()
                        .hover(|d| d.bg(p.hover))
                        .on_click(cx.listener(move |state, _, window, cx| {
                            state.dismiss(window, cx);
                            me.update(cx, |this, cx| this.pick_folder(cx)).ok();
                        }))
                        .child(Icon::new(IconName::Plus).size_3p5().text_color(p.muted))
                        .child("Add folder…"),
                )
            })
            .into_any_element()
    }

    pub(crate) fn topbar(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            // traffic lights sit on this row when the sidebar is closed
            .when(!self.sidebar_open, |d| d.pl(px(TRAFFIC_INSET)))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .id("bot-name")
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .text_sm()
                    .text_color(p.ink)
                    .cursor_pointer()
                    .when(self.panel == Panel::Editor, |d| d.bg(p.hover))
                    .hover(|d| d.bg(p.hover))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.panel == Panel::Editor {
                            this.panel = Panel::None;
                            cx.notify();
                        } else {
                            this.open_editor(window, cx);
                        }
                    }))
                    .child(egg(format!("head-{}", bot.id), hex(bot.color()), 14., bot.mood()))
                    .child(bot.name.clone())
                    .child(Icon::new(IconName::ChevronDown).size_3().text_color(p.muted)),
            )
            .child(div().ml_1().child(self.folders_button(bot, cx)))
            .child(div().flex_1())
            .child(div().mr_2().child(self.find_bar(cx)))
            .when(bot.context.1 > 0, |d| {
                let used = bot.context.0 as f32 / bot.context.1 as f32;
                let fill = if used >= 0.8 { p.warn } else { p.muted };
                d.child(div().mr_2().flex().items_center().gap_2().text_xs().text_color(p.muted).child("Context").child(bar(used, 40., p, fill)).child(format!("{:.0}%", used.clamp(0., 1.) * 100.)))
            })
            .child(
                button("fresh", p)
                    .when(bot.busy() || !self.may_start(bot.provider), |d| d.opacity(0.4).cursor_default())
                    .on_click(cx.listener(|this, _, _, cx| this.fresh_start(cx)))
                    .child(Icon::new(IconName::RefreshCw).size_3())
                    .child("Fresh start"),
            )
    }

    /// The search field in the top bar (⌘F), or the icon that opens it.
    fn find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let icon_button =
            |id: &'static str, icon: IconName| div().id(id).p_1().rounded(px(6.)).text_color(p.muted).cursor_pointer().hover(|d| d.bg(p.hover).text_color(p.ink)).child(Icon::new(icon).size_3p5());
        if !self.find_open {
            return icon_button("find", IconName::Search).on_click(cx.listener(|this, _, window, cx| this.open_find(window, cx))).into_any_element();
        }
        let typed = !self.find_input.read(cx).value().trim().is_empty();
        let count = match (typed, self.find_hits.len()) {
            (false, _) => String::new(),
            (true, 0) => "No matches".into(),
            (true, n) => format!("{} of {n}", self.find_at + 1),
        };
        div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(28.))
            .pl_2()
            .pr_1()
            .rounded(px(8.))
            .border_1()
            .border_color(p.line)
            .child(Icon::new(IconName::Search).size_3p5().text_color(p.muted))
            .child(div().w(px(170.)).child(Input::new(&self.find_input).appearance(false).xsmall()))
            .child(div().text_xs().text_color(p.muted).whitespace_nowrap().child(count))
            .child(icon_button("find-older", IconName::ChevronUp).on_click(cx.listener(|this, _, _, cx| this.find_step(-1, cx))))
            .child(icon_button("find-newer", IconName::ChevronDown).on_click(cx.listener(|this, _, _, cx| this.find_step(1, cx))))
            .child(icon_button("find-close", IconName::Close).on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))))
            .into_any_element()
    }
}

fn short_path(path: &Path) -> String {
    let text = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => format!("~{}", &text[home.len()..]),
        _ => text,
    }
}
