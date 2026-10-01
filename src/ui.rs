//! Everything eggbot draws. State and behaviour live in `main.rs`.

use std::path::Path;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::select::Select;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::claude::{Meter, Provider};
use crate::egg::{Mood, egg};
use crate::usage;
use crate::{Appearance, Bot, Choice, CloseWindow, CycleAppearance, Check, Eggbot, Find, OpenSettings, OpenSetup, Setup, sandbox, FocusInput, MODELS, Msg, NewBot, NextBot, PRESETS, Palette, PrevBot, Quit, SHELLS, SelectBot, StopTurn, ToggleSidebar, handoff, hex, login, set_dock_icon};

const ROW_H: f32 = 52.;
const ROW_GAP: f32 = 2.;
/// The line that shows where a dragged bot will land.
const DROP_LINE: u32 = 0x0A84FF;
const READ_W: f32 = 720.;
const SIDEBAR_MIN: f32 = 200.;
const SIDEBAR_MAX: f32 = 420.;
/// Room for the traffic lights. The chat top bar uses it when the sidebar is closed.
const TRAFFIC_INSET: f32 = 84.;

/// A small outlined button (top bar, panels, notices).
fn button(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .h(px(28.))
        .px_3()
        .rounded(px(8.))
        .border_1()
        .border_color(p.line)
        .text_xs()
        .text_color(p.ink)
        .cursor_pointer()
        .hover(|d| d.bg(p.hover))
}

/// The filled variant for the main action of a panel. GPUI panics if `.hover` is set twice, so it is not built on `button`.
fn primary(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(28.))
        .px_3()
        .rounded(px(8.))
        .bg(p.ink)
        .text_xs()
        .text_color(p.bg)
        .cursor_pointer()
        .hover(|d| d.opacity(0.85))
}

/// A soft, layered shadow (a hairline plus a wide faint blur); none in dark mode, where borders carry depth.
fn soft_shadow(p: Palette) -> Vec<BoxShadow> {
    if p.bg.l < 0.5 {
        return vec![];
    }
    let shadow = |alpha: f32, y: f32, blur: f32, spread: f32| BoxShadow { color: hsla(0., 0., 0., alpha), offset: point(px(0.), px(y)), blur_radius: px(blur), spread_radius: px(spread), inset: false };
    vec![shadow(0.03, 1., 2., 0.), shadow(0.05, 8., 28., -6.)]
}

fn short_path(path: &Path) -> String {
    let text = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => format!("~{}", &text[home.len()..]),
        _ => text,
    }
}

/// A quiet text link with an icon (the row under the composer).
fn link(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .rounded(px(6.))
        .text_xs()
        .text_color(p.muted)
        .cursor_pointer()
        .hover(|d| d.bg(p.hover).text_color(p.ink))
}

fn bar(used: f32, width: f32, p: Palette, fill: Hsla) -> Div {
    let used = used.clamp(0., 1.);
    div()
        .w(px(width))
        .h(px(4.))
        .rounded_full()
        .bg(p.tint)
        .child(div().h_full().rounded_full().w(relative(used)).bg(fill))
}

impl Eggbot {
    /// Fills the model and effort dropdowns for the selected bot (options depend on provider and model).
    fn sync_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selects_stale = false;
        let Some(bot) = self.bots.get(self.selected) else { return };
        let choice = |value: &str, label: String| Choice { value: Some(value.to_string()), label: label.into() };
        let models: Vec<Choice> = MODELS
            .iter()
            .map(|(alias, label)| match alias {
                Some(a) => choice(&format!("claude:{a}"), format!("Claude · {label}")),
                None => choice("claude", "Claude".into()),
            })
            .chain(std::iter::once(choice("codex", "Codex".into())))
            .chain(self.codex_models.iter().map(|m| choice(&format!("codex:{}", m.id), format!("Codex · {}", m.name))))
            .collect();
        let current = match (&bot.provider, &bot.model) {
            (Provider::Claude, None) => "claude".to_string(),
            (Provider::Claude, Some(m)) => format!("claude:{m}"),
            (Provider::Codex, None) => "codex".to_string(),
            (Provider::Codex, Some(m)) => format!("codex:{m}"),
        };
        let levels: Vec<String> = match bot.provider {
            Provider::Claude => ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec(),
            Provider::Codex => self.codex_models.iter().find(|m| bot.model.as_ref().map_or(m.default, |id| *id == m.id)).map(|m| m.efforts.clone()).unwrap_or_default(),
        };
        let capital = |l: &str| l[..1].to_uppercase() + &l[1..];
        let efforts: Vec<Choice> = std::iter::once(Choice { value: None, label: "Default effort".into() })
            .chain(levels.iter().map(|l| Choice { value: Some(l.clone()), label: capital(l).into() }))
            .collect();
        let effort = bot.effort.clone();
        self.model_select.update(cx, |s, cx| {
            s.set_items(models, window, cx);
            s.set_selected_value(&Some(current), window, cx);
        });
        self.effort_select.update(cx, |s, cx| {
            s.set_items(efforts, window, cx);
            s.set_selected_value(&effort, window, cx);
        });
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let me = cx.entity().downgrade();
        // a bot dropped on row `i` lands just above it; the line shows only where that changes the order
        let drop_line = move |group: SharedString, before: usize, from: Option<usize>| {
            let noop = from.is_some_and(|f| before == f || before == f + 1);
            div().absolute().left_1().right_1().top(px(-ROW_GAP / 2. - 1.)).h(px(2.)).rounded_full().when(!noop, |d| {
                // GPUI applies group drag styles only to elements with a hitbox; a no-op group hover adds one
                d.group_hover(group.clone(), |s| s).group_drag_over::<DraggedBot>(group, |s| s.bg(hex(DROP_LINE)))
            })
        };
        let rows = self.bots.iter().enumerate().map(|(i, b)| {
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
                .when(self.open_room.is_some() || i != self.selected, |d| d.hover(|d| d.bg(p.hover)))
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
        }).collect::<Vec<_>>();

        // one highlight that springs to the selected row
        let highlight = div()
            .absolute()
            .left_0()
            .right_0()
            .h(px(ROW_H))
            .rounded(px(8.))
            .bg(p.tint)
            .with_spring("selection", SpringAnimation::new(SpringConfig::new(320., 28., 1.)).to(px(self.selected as f32 * (ROW_H + ROW_GAP))).with_epsilon(0.25), |d, top| d.top(top));

        let handle = div()
            .id("resize")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-3.))
            .w(px(6.))
            .cursor_col_resize()
            .when(self.resizing, |d| d.bg(p.line))
            .hover(|d| d.bg(p.line))
            .on_mouse_down(
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
                div()
                    .h(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(TRAFFIC_INSET))
                    .pr_2()
                    .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.ink).child("eggbot"))
                    .child(
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
                    .child(div().relative().flex().flex_col().gap(px(ROW_GAP)).when(self.open_room.is_none() && !self.bots.is_empty(), |d| d.child(highlight)).children(rows))
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
            .children(self.meters.iter().map(|m| self.usage_meter(m)))
            .child(div().h_3())
            .child(handle)
    }

    /// Rooms sit under the bot list: a title, who is in, and a dot when a member has news.
    fn rooms_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let rows = self.rooms.iter().map(|r| {
            let id = r.id;
            let on = self.open_room == Some(id);
            let fac = r.facilitator.and_then(|fid| self.bots.iter().find(|b| b.id == fid));
            let subtitle = match (fac, r.members.len()) {
                (Some(b), n) if n > 1 => format!("{} + {}", b.name, n - 1),
                (Some(b), _) => b.name.clone(),
                _ => "No bots yet".into(),
            };
            div()
                .id(("room", id))
                .h(px(36.))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded(px(8.))
                .cursor_pointer()
                .when(on, |d| d.bg(p.tint))
                .when(!on, |d| d.hover(|s| s.bg(p.hover)))
                .on_click(cx.listener(move |this, _, window, cx| this.show_room(id, window, cx)))
                .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(div().size(px(8.)).rounded(px(2.)).border_1().border_color(if on { p.ink } else { p.muted })))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .overflow_hidden()
                        .child(div().text_sm().text_color(p.ink).truncate().child(r.title.clone()))
                        .child(div().text_xs().text_color(p.muted).truncate().child(subtitle)),
                )
                .when(r.unread && !on, |d| d.child(div().size(px(7.)).flex_none().rounded_full().bg(p.ink)))
        });
        div()
            .flex_none()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.line)
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .pl_4()
                    .pr_2()
                    .child(div().flex_1().text_xs().text_color(p.muted).child("Rooms"))
                    .child(
                        div()
                            .id("new-room")
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_color(p.muted)
                            .cursor_pointer()
                            .hover(|d| d.bg(p.hover).text_color(p.ink))
                            .on_click(cx.listener(|this, _, window, cx| this.new_room(window, cx)))
                            .child(Icon::new(IconName::Plus).size_4()),
                    ),
            )
            .child(div().id("room-list").max_h(px(160.)).overflow_y_scroll().flex().flex_col().px_2().pb_1().children(rows))
    }

    fn usage_meter(&self, meter: &Meter) -> impl IntoElement {
        let p = self.p;
        let now = chrono::Local::now().timestamp();
        let at = chrono::DateTime::from_timestamp(meter.at, 0).map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
        let name = if meter.provider == Provider::Codex { "Codex" } else { "Claude" };
        div()
            .px_4()
            .pt_2()
            .flex()
            .flex_col()
            .gap_1()
            .text_xs()
            .text_color(p.muted)
            .child(div().flex().child(div().flex_1().child(name)).child(format!("updated {at}")))
            .children(meter.windows.iter().map(|w| {
                let v = usage::window_used(w, now);
                let fill = if v >= self.pause { p.err } else if v >= usage::AMBER { p.warn } else { p.muted };
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
                            this.edit_open = false;
                            this.input.update(cx, |s, cx| s.focus(window, cx));
                        }
                        cx.notify();
                    }))
                    .child(egg(format!("preset-{i}"), hex(preset.color), 14., Mood::Still))
                    .child(div().flex().flex_col().overflow_hidden().child(div().text_sm().text_color(p.ink).child(preset.name)).child(div().text_xs().text_color(p.muted).truncate().child(preset.blurb)))
            }))
            .with_animation("menu-in", Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
    }

    /// Header chips: each mounted folder, with remove, and a button that adds more.
    fn folder_chips(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let id = bot.id;
        let chips = bot.folders.iter().enumerate().map(|(i, f)| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .h(px(24.))
                .pl_2()
                .pr(px(2.))
                .rounded(px(6.))
                .border_1()
                .border_color(p.line)
                .text_xs()
                .text_color(p.ink)
                .child(Icon::new(IconName::Folder).size_3().text_color(p.muted))
                .child(div().max_w(px(220.)).truncate().child(short_path(&f.path)))
                .child(
                    div()
                        .id(("unmount", id * 32 + i))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(18.))
                        .rounded(px(4.))
                        .text_color(p.muted)
                        .cursor_pointer()
                        .hover(|d| d.bg(p.hover).text_color(p.ink))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.remove_folder(id, i, cx);
                        }))
                        .child(Icon::new(IconName::Close).size_3()),
                )
        });
        div()
            .id("folder-chips")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll()
            .flex()
            .items_center()
            .gap_1()
            .children(chips)
            .child(
                div()
                    .id("add-folder")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(24.))
                    .px_2()
                    .rounded(px(6.))
                    .text_xs()
                    .text_color(p.muted)
                    .cursor_pointer()
                    .hover(|d| d.bg(p.hover).text_color(p.ink))
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx)))
                    .child(Icon::new(IconName::Plus).size_3())
                    .child(if bot.folders.is_empty() { "Add folder" } else { "Add" }),
            )
    }

    fn topbar(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .when(self.edit_open, |d| d.bg(p.hover))
                    .hover(|d| d.bg(p.hover))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.edit_open {
                            this.edit_open = false;
                            cx.notify();
                        } else {
                            this.open_editor(window, cx);
                        }
                    }))
                    .child(egg(format!("head-{}", bot.id), hex(bot.color()), 14., bot.mood()))
                    .child(bot.name.clone())
                    .child(Icon::new(IconName::ChevronDown).size_3().text_color(p.muted)),
            )
            .child(self.folder_chips(bot, cx))
            .child(div().mr_2().child(self.find_bar(cx)))
            .when(bot.context.1 > 0, |d| {
                let used = bot.context.0 as f32 / bot.context.1 as f32;
                let fill = if used >= 0.8 { p.warn } else { p.muted };
                d.child(div().mr_2().flex().items_center().gap_2().text_xs().text_color(p.muted).child("Context").child(bar(used, 40., p, fill)).child(format!("{:.0}%", used.clamp(0., 1.) * 100.)))
            })
            .child(
                button("fresh", p)
                    .when(bot.busy() || !self.may_start(bot.provider), |d| d.opacity(0.4))
                    .on_click(cx.listener(|this, _, _, cx| this.fresh_start(cx)))
                    .child(Icon::new(IconName::RefreshCw).size_3())
                    .child("Fresh start"),
            )
    }

    fn chat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let main = div().flex_1().min_w_0().h_full().flex().flex_col().bg(p.bg);
        if let Some(setup) = &self.setup {
            return main.child(div().h(px(44.)).flex_none()).child(self.setup_view(setup, cx)).into_any_element();
        }
        if self.open_room.is_some() {
            return self.room_view(cx);
        }
        let Some(bot) = self.bots.get(self.selected) else {
            return main
                .child(div().h(px(44.)).flex_none())
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(egg("empty", hex(0xE3D2B9), 32., Mood::Still))
                        .child(div().text_2xl().text_color(p.ink).child("No bots yet"))
                        .child(div().text_sm().text_color(p.muted).child("Create one with + in the sidebar.")),
                )
                .into_any_element();
        };

        let panels = div()
            .when(self.settings_open, |d| d.child(self.settings(cx)))
            .when(self.edit_open, |d| d.child(self.editor(bot, cx)))
            .when(self.sched_open, |d| d.child(self.schedules(bot, cx)));

        let body = if bot.msgs.is_empty() {
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
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(msgs)
                .child(div().px_6().pb_4().child(div().max_w(px(READ_W)).mx_auto().w_full().child(self.composer(bot, cx))))
                .into_any_element()
        };

        // new id per bot, so switching bots replays the fade
        main.child(self.topbar(bot, cx))
            .child(panels)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body).with_animation(
                ElementId::Name(format!("chat-{}", bot.id).into()),
                Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t),
            ))
            .into_any_element()
    }

    /// The input card with the model and effort dropdowns, and the folder/schedules row under it.
    fn composer(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let busy = bot.busy();
        let paused = self.breach_of(bot.provider).is_some_and(|b| b.level == usage::Level::Pause);
        let send = div()
            .id("send")
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(p.ink)
            .text_color(p.bg)
            .cursor_pointer()
            .when(paused && !busy, |d| d.opacity(0.4))
            .hover(|d| d.opacity(0.8))
            .on_click(cx.listener(move |this, _, window, cx| if busy { this.stop(cx) } else { this.send(window, cx) }))
            .child(if busy { div().size(px(10.)).rounded(px(2.)).bg(p.bg).into_any_element() } else { Icon::new(IconName::ArrowUp).size_4().into_any_element() });

        // dropdowns hug their label, like the references
        let model_label = self.model_select.read(cx).selected_value().cloned().flatten().and_then(|v| {
            let (provider, model) = v.split_once(':').unwrap_or((v.as_str(), ""));
            let name = MODELS.iter().find(|(a, _)| *a == Some(model)).map(|(_, l)| l.to_string()).or_else(|| self.codex_models.iter().find(|m| m.id == model).map(|m| m.name.clone()));
            Some(match (provider, name) {
                ("codex", Some(n)) => format!("Codex · {n}"),
                ("codex", None) => "Codex".into(),
                (_, Some(n)) => format!("Claude · {n}"),
                _ => "Claude".into(),
            })
        }).unwrap_or_else(|| "Claude".into());
        let effort_label = bot.effort.clone().unwrap_or_else(|| "Default effort".into());
        let fit = |label: &str| (label.chars().count() as f32 * 6.2 + 28.).clamp(48., 260.);
        let codex_note = (bot.provider == Provider::Codex && self.codex_models.is_empty()).then(|| self.codex_query.clone()).flatten();
        let note = codex_note.map(|failed| match failed {
            None => div().text_xs().text_color(p.muted).child("Loading Codex models…").into_any_element(),
            Some(e) if e.contains("codex login") => link("codex-sign-in", p).child("Codex is not signed in · Sign in").on_click(cx.listener(|this, _, _, cx| this.sign_in(true, cx))).into_any_element(),
            Some(e) => link("codex-retry", p).child(format!("Codex: {e} · Retry")).on_click(cx.listener(|this, _, _, cx| this.refresh_codex(1, cx))).into_any_element(),
        });

        let waiting = self.schedules_wait(bot) && bot.schedules.iter().any(|s| s.due(chrono::Local::now()));
        let schedules = match (bot.schedules.len(), waiting) {
            (0, _) => "Schedules".to_string(),
            (1, false) => "1 schedule".to_string(),
            (1, true) => "1 schedule · waiting".to_string(),
            (n, false) => format!("{n} schedules"),
            (n, true) => format!("{n} schedules · waiting"),
        };
        // the meter already says "one at a time"; the banner appears when this bot cannot start
        let guard = self.breach_of(bot.provider).filter(|br| br.level == usage::Level::Pause || !self.may_start(bot.provider) || !bot.queue.is_empty()).map(|br| {
            let color = if br.level == usage::Level::Pause { p.err } else { p.warn };
            let mut text = usage::explain(usage::provider_name(bot.provider), &br, self.pause, self.throttle);
            if !bot.queue.is_empty() {
                text.push_str(" Waiting work starts once the limit allows it.");
            }
            div()
                .mb_2()
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(12.))
                .bg(p.card)
                .border_1()
                .border_color(p.line)
                .text_xs()
                .text_color(color)
                .child(Icon::new(IconName::CircleAlert).size_3p5())
                .child(text)
        });

        div()
            .flex()
            .flex_col()
            .children(guard)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(20.))
                    .bg(p.card)
                    .border_1()
                    .border_color(p.line)
                    .shadow(soft_shadow(p))
                    // the multi-line textarea adds its own 10px inset, so text lines up with the dropdown labels
                    .child(div().px_2().pt_2().child(Textarea::new(&self.input).appearance(false)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_3()
                            .pb_3()
                            // Select fills its parent, so a fixed-width box sets its size
                            .child(div().flex_none().w(px(fit(&model_label))).child(Select::new(&self.model_select).appearance(false).xsmall().menu_width(px(240.)).menu_max_h(px(320.))))
                            .child(div().w(px(1.)).h(px(14.)).bg(p.line))
                            .child(div().flex_none().w(px(fit(&effort_label))).child(Select::new(&self.effort_select).appearance(false).xsmall().menu_width(px(160.))))
                            .child(div().flex_1())
                            .child(send),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2p5()
                    .pt_1()
                    .child(link("folder", p).on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))).child(Icon::new(IconName::Folder).size_3()).child("Add folder"))
                    .child(
                        link("clock", p)
                            .when(self.sched_open, |d| d.bg(p.hover).text_color(p.ink))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.sched_open = !this.sched_open;
                                this.settings_open = false;
                                this.sched_error = None;
                                this.edit_open = false;
                                if this.sched_open {
                                    this.sched_prompt.update(cx, |s, cx| s.focus(window, cx));
                                }
                                cx.notify();
                            }))
                            .child(Icon::new(IconName::Clock).size_3())
                            .child(schedules),
                    )
                    .child(div().flex_1())
                    .when_some(self.folder_error.clone(), |d, e| d.child(div().min_w_0().text_xs().text_color(p.err).truncate().child(e)))
                    .children(note),
            )
    }

    /// The search field in the top bar (⌘F), or the icon that opens it.
    fn find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let icon_button = |id: &'static str, icon: IconName| div().id(id).p_1().rounded(px(6.)).text_color(p.muted).cursor_pointer().hover(|d| d.bg(p.hover).text_color(p.ink)).child(Icon::new(icon).size_3p5());
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

    /// The first-run checklist: each row turns green on its own as the checks pass.
    fn setup_view(&self, s: &Setup, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        type Act = Box<dyn Fn(&mut Eggbot, &mut Context<Eggbot>)>;
        let row = |key: &'static str, title: &'static str, check: &Check, ok: &'static str, todo: &'static str, action: Option<(&'static str, Act)>| {
            let (icon, line) = match check {
                Check::Ok => (Icon::new(IconName::CircleCheck).size_4().text_color(p.ok).into_any_element(), ok.to_string()),
                Check::Unknown => (Spinner::new().color(p.muted).xsmall().into_any_element(), "Checking…".to_string()),
                Check::Busy(label) => (Spinner::new().color(p.muted).xsmall().into_any_element(), label.to_string()),
                Check::Failed(e) => (Icon::new(IconName::CircleAlert).size_4().text_color(p.err).into_any_element(), e.clone()),
                Check::Missing => (Icon::new(IconName::CircleDashed).size_4().text_color(p.muted).into_any_element(), todo.to_string()),
            };
            let failed = matches!(check, Check::Failed(_));
            div()
                .flex()
                .items_center()
                .gap_3()
                .py_2()
                .child(div().w(px(18.)).flex_none().flex().justify_center().child(icon))
                .child(div().flex().flex_col().flex_1().min_w_0().child(div().text_sm().text_color(p.ink).child(title)).child(div().text_xs().text_color(if failed { p.err } else { p.muted }).child(line)))
                .when_some(action.filter(|_| !matches!(check, Check::Ok | Check::Unknown)), |d, (label, act)| {
                    d.child(button(key, p).flex_none().on_click(cx.listener(move |this, _, _, cx| act(this, cx))).child(if matches!(check, Check::Busy(_)) { "Again" } else { label }))
                })
        };
        let ready = |c: &Check| *c == Check::Ok;
        let sign_in = |codex: bool| -> Act {
            Box::new(move |this: &mut Eggbot, cx: &mut Context<Eggbot>| {
                if let Some(s) = &mut this.setup {
                    *(if codex { &mut s.codex } else { &mut s.claude }) = Check::Busy("Finish signing in in Terminal…");
                }
                this.sign_in(codex, cx);
            })
        };
        let rows = div()
            .flex()
            .flex_col()
            .child(row("setup-engine", "Docker engine", &s.engine, "Docker is installed.", "Each bot runs in its own container. Colima is free and open source.", Some(("Install Colima", Box::new(|this: &mut Eggbot, cx: &mut Context<Eggbot>| this.setup_action(|s| &mut s.engine, "Installing in Terminal…", sandbox::install_engine, cx))))))
            .child(row("setup-running", "Docker running", &s.running, "Docker is running.", "Start your Docker engine.", ready(&s.engine).then(|| ("Start Docker", Box::new(|this: &mut Eggbot, cx: &mut Context<Eggbot>| this.setup_action(|s| &mut s.running, "Starting Docker…", sandbox::wake, cx)) as Act))))
            .child(row("setup-image", "Bot machine", &s.image, "The bot machine is ready.", "The image every bot runs in. Built once, in about a minute.", ready(&s.running).then(|| ("Build", Box::new(|this: &mut Eggbot, cx: &mut Context<Eggbot>| this.setup_action(|s| &mut s.image, "Building, about a minute…", || sandbox::ready(&|_| {}), cx)) as Act))))
            .child(div().h(px(1.)).my_1().bg(p.line))
            .child(row("setup-claude", "Claude", &s.claude, "Signed in to Claude.", "Sign in with your Claude plan, in Terminal.", ready(&s.image).then(|| ("Sign in", sign_in(false)))))
            .child(row("setup-codex", "Codex", &s.codex, "Signed in to Codex.", "Sign in with your ChatGPT plan, in Terminal.", ready(&s.image).then(|| ("Sign in", sign_in(true)))));
        let done = s.done();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px_6()
            .pb(px(60.))
            .child(egg("setup", hex(0xE3D2B9), 28., Mood::Still))
            .child(div().mt_4().text_2xl().text_color(p.ink).child("Set up eggbot"))
            .child(div().mt_1().mb_6().text_sm().text_color(p.muted).child("Each bot works in its own container, on your own Claude or ChatGPT plan."))
            .child(self.panel().max_w(px(520.)).mt_0().child(rows))
            .child(
                div()
                    .mt_6()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(link("setup-skip", p).child("Skip for now").on_click(cx.listener(|this, _, _, cx| {
                        this.setup = None;
                        cx.notify();
                    })))
                    .child(
                        primary("setup-done", p)
                            .when(!done, |d| d.opacity(0.4))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if done {
                                    this.setup = None;
                                    cx.notify();
                                }
                            }))
                            .child("Start using eggbot"),
                    ),
            )
    }

    /// One row of the chat list: message `ix`, or the typing indicator after the last message.
    fn row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(bot) = self.bots.get(self.selected) else { return div().into_any_element() };
        let el = match bot.msgs.get(ix) {
            Some(m) => self.message(bot, ix, m, cx),
            None if bot.waiting() => self.typing(bot).into_any_element(),
            None => div().into_any_element(),
        };
        let (last, hit) = (ix == bot.msgs.len(), self.find_current() == Some(ix));
        // the 8px inset leaves room for the search highlight without moving the text
        div()
            .w_full()
            .flex()
            .justify_center()
            .px_4()
            .pt(px(if ix == 0 { 20. } else { 8. }))
            .when(last, |d| d.pb(px(20.)))
            .child(div().w_full().max_w(px(READ_W + 16.)).px_2().py_1().rounded(px(10.)).when(hit, |d| d.bg(self.p.tint)).child(el))
            .into_any_element()
    }

    fn panel(&self) -> Div {
        let p = self.p;
        div().mx_auto().mt_3().w_full().max_w(px(READ_W)).p_4().flex().flex_col().gap_3().rounded(px(12.)).bg(p.card).border_1().border_color(p.line).shadow(soft_shadow(p))
    }

    /// The bot editor: name, role, egg color. Model and effort live in the composer.
    fn editor(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let current = bot.color();
        let swatches = SHELLS.iter().map(|&c| {
            div()
                .id(("shell", c as usize))
                .size(px(18.))
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
        let label = |t: &'static str| div().text_xs().text_color(p.muted).child(t);
        let field = || div().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line);
        div().px_6().child(
            self.panel()
                .child(div().flex().flex_col().gap_1().child(label("Name")).child(field().child(Input::new(&self.edit_name).appearance(false))))
                .child(div().flex().flex_col().gap_1().child(label("Role")).child(field().child(Textarea::new(&self.edit_role).appearance(false))))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(label("Egg"))
                        .children(swatches)
                        .child(div().flex_1())
                        .when_some(self.edit_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e)))
                        .child(
                            button("cancel-edit", p).on_click(cx.listener(|this, _, _, cx| {
                                this.edit_open = false;
                                cx.notify();
                            })).child("Cancel"),
                        )
                        .child(primary("save-edit", p).on_click(cx.listener(|this, _, window, cx| this.save_edit(window, cx))).child("Save")),
                ),
        )
    }

    /// Title, kickoff, and roster. Start talks to the facilitator; peers join through @Name.
    fn room_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let Some(room) = self.open_room.and_then(|id| self.rooms.iter().find(|r| r.id == id)).cloned() else {
            return div().flex_1().into_any_element();
        };
        let why = crate::room::block(&room.title, &room.kickoff, &room.members, room.facilitator);
        let room_id = room.id;
        let label = |t: &'static str| div().text_xs().text_color(p.muted).child(t);
        let field = || div().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line);
        let members: Vec<_> = self.bots.iter().map(|b| {
            let (bot_id, on, fac) = (b.id, room.members.contains(&b.id), room.facilitator == Some(b.id));
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
                            if let Some(i) = this.bots.iter().position(|b| b.id == bot_id) {
                                this.select(i, window, cx);
                            }
                        }))
                        .child(if unread { "Open · new" } else { "Open" }),
                )
        }).collect();
        let confirming = self.confirm_delete_room == Some(room_id);
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
                    .child(div().flex_1().min_w_0().text_sm().text_color(p.ink).truncate().child(room.title.clone())),
            )
            .when(self.settings_open, |d| d.child(self.settings(cx)))
            .child(
                div().id("room-body").flex_1().overflow_y_scroll().child(
                    div().px_6().pb_8().child(
                        self.panel()
                            .child(div().flex().flex_col().gap_1().child(label("Title")).child(field().child(Input::new(&self.room_title).appearance(false))))
                            .child(div().flex().flex_col().gap_1().child(label("Kickoff")).child(field().child(Textarea::new(&self.room_kickoff).appearance(false))))
                            .child(div().text_xs().text_color(p.muted).child("Start sends this to the facilitator only. They hand work to the other bots with @Name. There is no lead."))
                            .child(div().h(px(1.)).bg(p.line))
                            .child(label("Bots"))
                            .when(self.bots.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("Hatch a bot first, then add it here.")))
                            .children(members)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .pt_1()
                                    .child(div().flex_1())
                                    .when_some(self.room_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e)))
                                    .when(self.room_error.is_none(), |d| d.when_some(self.room_status.clone(), |d, s| d.child(div().flex_1().text_xs().text_color(p.muted).child(s))))
                                    .child(
                                        button("delete-room", p)
                                            .on_click(cx.listener(move |this, _, _, cx| this.delete_room(room_id, cx)))
                                            .child(if confirming { "Delete room?" } else { "Delete" }),
                                    )
                                    .child(
                                        primary("start-room", p)
                                            .when(why.is_some(), |d| d.opacity(0.4))
                                            .on_click(cx.listener(move |this, _, _, cx| this.start_room(cx)))
                                            .child(if room.started { "Start again" } else { "Start" }),
                                    ),
                            ),
                    ),
                ),
            )
            .into_any_element()
    }

    /// Settings (⌘,): start at login, and instructions every bot gets.
    fn settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let label = |t: &'static str| div().text_xs().text_color(p.muted).child(t);
        let login = login::state();
        let note = match (&self.login_error, &login) {
            (Some(e), _) => Some(div().text_xs().text_color(p.err).child(e.clone()).into_any_element()),
            (None, login::State::NeedsApproval) => Some(link("login-approve", p).child("Allow eggbot in System Settings → Login Items").on_click(|_, _, _| login::open_system_settings()).into_any_element()),
            _ => None,
        };
        let on = !matches!(login, login::State::Off);
        div().px_6().child(
            self.panel()
                .child(div().text_sm().text_color(p.ink).child("Settings"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().flex().flex_col().flex_1().child(div().text_sm().text_color(p.ink).child("Start at login")).child(label("Only the menu bar egg appears, and schedules keep running.")))
                        .child(Switch::new("login").checked(on).on_click(cx.listener(|this, on: &bool, _, cx| this.set_login(*on, cx)))),
                )
                .children(note)
                .child(div().h(px(1.)).bg(p.line))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_sm().text_color(p.ink).child("Instructions for all bots"))
                        .child(label("Every bot gets these next to its own role, on Claude and Codex, from its next turn."))
                        .child(div().mt_1().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line).child(Textarea::new(&self.edit_shared).appearance(false))),
                )
                .child(div().h(px(1.)).bg(p.line))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_sm().text_color(p.ink).child("Usage guardrails"))
                        .child(label("Throttle runs one bot per provider. Pause holds new turns and leaves schedules due. The bars stay amber from 80%. A window past its reset counts as empty."))
                        .child(
                            div()
                                .mt_1()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(label("Throttle at"))
                                .child(div().w(px(64.)).px_3().py_1().rounded(px(8.)).border_1().border_color(p.line).child(Input::new(&self.limit_throttle).appearance(false)))
                                .child(label("%"))
                                .child(label("Pause at").ml_2())
                                .child(div().w(px(64.)).px_3().py_1().rounded(px(8.)).border_1().border_color(p.line).child(Input::new(&self.limit_pause).appearance(false)))
                                .child(label("%")),
                        ),
                )
                .when_some(self.settings_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1())
                        .child(
                            button("cancel-settings", p).on_click(cx.listener(|this, _, _, cx| {
                                this.settings_open = false;
                                cx.notify();
                            })).child("Cancel"),
                        )
                        .child(primary("save-settings", p).on_click(cx.listener(|this, _, window, cx| this.save_settings(window, cx))).child("Save")),
                ),
        )
    }

    /// The schedules panel: this bot's schedules and a form to add one.
    fn schedules(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let now = chrono::Local::now();
        let wait = self.schedules_wait(bot);
        let provider = bot.provider;
        let hold = self.breach_of(provider);
        let note_color = if hold.as_ref().is_some_and(|b| b.level == usage::Level::Pause) { p.err } else { p.warn };
        let rows = bot.schedules.iter().map(|s| {
            let id = s.id;
            let due_held = wait && s.due(now);
            let when = if due_held {
                hold.as_ref().map(|br| format!("waiting · {}", usage::short(usage::provider_name(provider), br))).unwrap_or_else(|| "waiting".into())
            } else {
                format!("{} · next {}", s.repeat.label(), s.next_run().format("%a %H:%M"))
            };
            div()
                .flex()
                .items_center()
                .gap_3()
                .text_sm()
                .child(Icon::new(IconName::Clock).size_3p5().text_color(p.muted))
                .child(div().flex_1().truncate().text_color(p.ink).child(s.prompt.clone()))
                .child(div().text_xs().text_color(if due_held { note_color } else { p.muted }).child(when))
                .child(
                    div()
                        .id(("unschedule", id))
                        .text_color(p.muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.ink))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_schedule(id, cx)))
                        .child(Icon::new(IconName::Trash).size_3p5()),
                )
        });
        let kinds = ["Daily", "Weekdays", "Every N hours", "Every N minutes"].iter().enumerate().map(|(k, label)| {
            let on = k == self.sched_kind;
            div()
                .id(("kind", k))
                .px_2()
                .py_1()
                .rounded(px(6.))
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.bg(p.hover).text_color(p.ink))
                .when(!on, |d| d.text_color(p.muted).hover(|d| d.text_color(p.ink)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.sched_kind = k;
                    let hint = if k < 2 { "09:00" } else { "3" };
                    this.sched_value.update(cx, |s, cx| s.set_placeholder(hint, window, cx));
                    cx.notify();
                }))
                .child(*label)
        });
        let field = |state| div().px_3().py_1().rounded(px(8.)).border_1().border_color(p.line).child(Input::new(state).appearance(false));
        div().px_6().child(
            self.panel()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.ink).child(format!("{}'s schedules", bot.name)))
                .when(bot.schedules.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("Nothing scheduled. Each run starts a fresh session with the bot's notes.")))
                .when(wait && !bot.schedules.is_empty(), |d| d.child(div().text_xs().text_color(note_color).child("Due runs wait here instead of starting, and go once the meter drops.")))
                .children(rows)
                .child(div().h(px(1.)).bg(p.line))
                .child(field(&self.sched_prompt))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .children(kinds)
                        .child(div().w(px(90.)).ml_2().child(field(&self.sched_value)))
                        .child(div().flex_1())
                        .child(primary("add-schedule", p).on_click(cx.listener(|this, _, window, cx| this.add_schedule(window, cx))).child("Add")),
                )
                .when(self.sched_kind == 3, |d| d.child(div().text_xs().text_color(p.muted).child("Short intervals use your plan limit quickly.")))
                .when_some(self.sched_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e))),
        )
    }

    /// Shown while the bot works without writing: a wobbling egg and a pulsing label.
    fn typing(&self, bot: &Bot) -> impl IntoElement {
        let label = bot.status.clone().unwrap_or_else(|| "Thinking…".into());
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(self.p.muted)
            .child(egg(format!("typing-{}", bot.id), hex(bot.color()), 14., Mood::Thinking))
            .child(div().child(label).with_animation("pulse", Animation::new(Duration::from_millis(1600)).repeat(), |d, t| d.opacity(0.45 + 0.55 * (t * std::f32::consts::TAU).cos().abs())))
    }

    fn message(&self, bot: &Bot, i: usize, m: &Msg, cx: &mut Context<Self>) -> AnyElement {
        let p = self.p;
        let el = match m {
            Msg::User(t) => div().flex().justify_end().child(div().max_w(relative(0.75)).px_4().py_2().rounded(px(18.)).bg(p.bubble).text_size(px(15.)).line_height(relative(1.5)).text_color(p.ink).child(t.clone())),
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
                        .child(div().text_color(p.ink).child(text.clone())),
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
                    .child(div().text_color(p.ink).child(TextView::markdown(("handoff", bot.id * 100_000 + i), shown).selectable(true)))
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
                    .child(div().flex().items_center().gap_1().text_xs().text_color(p.muted).child(Icon::new(if label == "Fresh start" { IconName::RefreshCw } else { IconName::Clock }).size_3()).child(label.clone()))
                    .child(div().text_color(p.ink).child(prompt.clone())),
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
            Msg::Divider(label) => div().flex().items_center().gap_3().text_xs().text_color(p.muted).child(div().flex_1().h(px(1.)).bg(p.line)).child(label.clone()).child(div().flex_1().h(px(1.)).bg(p.line)),
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
                .child(Icon::new(IconName::CircleAlert).size_4())
                .child(t.clone())
                .when(t.contains("Docker"), |d| d.child(button(("open-setup", i), p).ml_2().on_click(cx.listener(|this, _, _, cx| this.open_setup(cx))).child("Open setup")))
                .when(t.contains("/login") || t.contains("codex login"), |d| {
                    let codex = t.contains("codex login");
                    d.child(button(("sign-in", i), p).ml_2().on_click(cx.listener(move |this, _, _, cx| this.sign_in(codex, cx))).child(if codex { "Sign in to Codex" } else { "Sign in to Claude" }))
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
                            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).size_3())
                            .child(verb.clone())
                            .child(div().truncate().font_family("Menlo").child(target.clone())),
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
        self.sync_list();
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
                this.menu_open = !this.menu_open;
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
            .on_action(cx.listener(|this, _: &StopTurn, window, cx| {
                if this.menu_open {
                    this.menu_open = false;
                    cx.notify();
                } else if this.find_open {
                    this.close_find(window, cx);
                } else {
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
