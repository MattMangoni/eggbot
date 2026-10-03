//! Everything eggbot draws. State and behaviour live in `main.rs`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{Eggbot, Panel};
use crate::ui::theme::{Appearance, Palette};
use crate::{CloseWindow, CycleAppearance, Dismiss, Find, FocusInput, NewBot, NextBot, OpenSettings, OpenSetup, PrevBot, Quit, SelectBot, StopTurn, ToggleSidebar, set_dock_icon};

mod chat;
mod composer;
mod group_view;
mod panels;
mod room_view;
mod sidebar;
pub(crate) mod theme;
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
    fn panel(&self) -> Div {
        let p = self.p;
        div().mx_auto().mt_3().w_full().max_w(px(READ_W)).p_4().flex().flex_col().gap_3().rounded(px(12.)).bg(p.card).border_1().border_color(p.line).shadow(soft_shadow(p))
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
