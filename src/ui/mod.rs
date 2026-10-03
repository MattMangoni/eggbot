//! Everything eggbot draws: the root view and the widgets every area shares. State and behaviour live in `app/`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::Eggbot;
use crate::ui::theme::Palette;

mod chat;
pub(crate) mod composer;
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

/// Small muted text: field labels and hints.
fn label(t: impl Into<SharedString>, p: Palette) -> Div {
    div().text_xs().text_color(p.muted).child(t.into())
}

/// A hairline between panel sections.
fn divider(p: Palette) -> Div {
    div().h(px(1.)).bg(p.line)
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
        let root = div()
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
            );
        Self::on_actions(root, cx).when(self.sidebar_open, |d| d.child(self.sidebar(cx))).child(self.chat(cx))
    }
}
