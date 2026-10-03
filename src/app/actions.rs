//! Keyboard shortcuts, the menu bar, and what each action does.

use gpui_kit::*;

use super::{Eggbot, Panel};
use crate::set_dock_icon;
use crate::ui::theme::Appearance;

actions!(eggbot, [Quit, CloseWindow, NewBot, FocusInput, PrevBot, NextBot, StopTurn, Dismiss, CycleAppearance, OpenSettings, OpenSetup, Find, ToggleSidebar]);

/// ⌘1…⌘9 selects the bot at that position.
#[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = eggbot)]
struct SelectBot(usize);

/// Shortcuts that work anywhere in the window.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-n", NewBot, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-k", FocusInput, None),
        // ⌘[ / ⌘] are outdent/indent inside text fields, so switching uses ⌃Tab
        KeyBinding::new("ctrl-shift-tab", PrevBot, None),
        KeyBinding::new("ctrl-tab", NextBot, None),
        KeyBinding::new("escape", Dismiss, None),
        KeyBinding::new("cmd-.", StopTurn, None),
        KeyBinding::new("cmd-shift-d", CycleAppearance, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-f", Find, None),
    ]);
    cx.bind_keys((1..=9).map(|n| KeyBinding::new(&format!("cmd-{n}"), SelectBot(n - 1), None)));
}

pub(crate) fn set_menus(appearance: Appearance, sidebar_open: bool, cx: &mut App) {
    let pick = |name: &str, a: Appearance| MenuItem::Action { name: name.to_string().into(), action: Box::new(a), os_action: None, checked: a == appearance, disabled: false };
    cx.set_menus([
        Menu {
            name: "eggbot".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::action("Setup…", OpenSetup),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindow),
                MenuItem::action("Quit eggbot", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::Action { name: "Toggle Sidebar".to_string().into(), action: Box::new(ToggleSidebar), os_action: None, checked: sidebar_open, disabled: false },
                MenuItem::separator(),
                pick("Match System", Appearance::System),
                pick("Light", Appearance::Light),
                pick("Dark", Appearance::Dark),
                MenuItem::separator(),
                MenuItem::action("Next Appearance", CycleAppearance),
            ],
            disabled: false,
        },
    ]);
}

impl Eggbot {
    /// ⌘B hides or shows the bot list. The width stays, and the choice is saved.
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        self.resizing = false;
        set_menus(self.appearance, self.sidebar_open, cx);
        self.save();
        cx.notify();
    }

    /// The root view handles every action, so a shortcut works whatever has focus.
    pub(crate) fn on_actions(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &Quit, window, cx| this.request_quit(window, cx)))
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
    }
}
