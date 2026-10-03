mod app;
mod claude;
mod codex;
mod egg;
mod group;
mod handoff;
mod login;
mod memory;
mod notify;
mod room;
mod sandbox;
mod schedule;
mod skills;
mod tray;
mod ui;
mod usage;

use app::Eggbot;
use gpui_kit::assets::Assets;
use gpui_kit::component::Theme;
use gpui_kit::component::select::SelectItem;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

actions!(eggbot, [Quit, CloseWindow, NewBot, FocusInput, PrevBot, NextBot, StopTurn, Dismiss, CycleAppearance, OpenSettings, OpenSetup, Find, ToggleSidebar]);

/// ⌘1…⌘9 selects the bot at that position.
#[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = eggbot)]
struct SelectBot(usize);

/// Light or dark, chosen in the View menu.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = eggbot)]
enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }
}

// the default bundle has only the component icons; add the extra ones we use
gpui_kit::assets::icon_assets!(ExtraIcons, [Clock, Trash, Pencil, CircleCheck, CircleDashed]);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Hsla,
    /// Translucent: the window behind it is blurred (vibrancy).
    side: Hsla,
    card: Hsla,
    ink: Hsla,
    muted: Hsla,
    line: Hsla,
    hover: Hsla,
    /// Selected row and meter tracks: readable over the frosted sidebar.
    tint: Hsla,
    bubble: Hsla,
    ok: Hsla,
    warn: Hsla,
    err: Hsla,
}

impl Palette {
    fn light() -> Self {
        Self {
            bg: hex(0xFFFFFF),
            side: hsla(0., 0., 1., 0.45),
            card: hex(0xFFFFFF),
            ink: hex(0x0D0D0D),
            muted: hex(0x6B6B6B),
            line: hex(0xE5E5E5),
            hover: hsla(0., 0., 0., 0.05),
            tint: hsla(0., 0., 0., 0.09),
            bubble: hex(0xF4F4F4),
            ok: hex(0x16A34A),
            warn: hex(0xD97706),
            err: hex(0xDC2626),
        }
    }

    fn dark() -> Self {
        Self {
            bg: hex(0x0F0F0F),
            side: hsla(0., 0., 0.05, 0.25),
            card: hex(0x171717),
            ink: hex(0xECECEC),
            muted: hex(0xA3A3A3),
            line: hex(0x2A2A2A),
            hover: hsla(0., 0., 1., 0.06),
            tint: hsla(0., 0., 1., 0.11),
            bubble: hex(0x1F1F1F),
            ok: hex(0x4ADE80),
            warn: hex(0xFBBF24),
            err: hex(0xF87171),
        }
    }

    /// Follows the macOS appearance and pushes our colors into gpui-component.
    fn apply(window: &mut Window, cx: &mut App) -> Self {
        Theme::sync_system_appearance(Some(window), cx);
        let p = if Theme::global(cx).is_dark() { Self::dark() } else { Self::light() };
        // separate update: the mode switch above reloads the stock colors
        Theme::update(cx, |t| {
            // gpui-component's root paints this over the whole window; clear it so the sidebar blur shows
            t.background = transparent_black();
            t.foreground = p.ink;
            t.border = p.line;
            t.input = p.line;
            t.primary = p.ink;
            t.primary_foreground = p.bg;
            t.ring = p.muted;
            t.caret = p.ink;
            t.selection = p.muted.opacity(0.3);
            t.muted = p.bubble;
            t.muted_foreground = p.muted;
            t.accent = p.hover;
            t.popover = p.card;
            t.list_hover = p.hover;
            t.list_active = p.hover;
        });
        p
    }
}

/// A dropdown option: what is shown, and what is stored (None = the provider's default).
#[derive(Clone)]
struct Choice {
    value: Option<String>,
    label: SharedString,
}

impl SelectItem for Choice {
    type Value = Option<String>;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

const MODELS: [(Option<&str>, &str); 5] = [(None, "Default"), (Some("fable"), "Fable"), (Some("opus"), "Opus"), (Some("sonnet"), "Sonnet"), (Some("haiku"), "Haiku")];

impl Eggbot {
    /// Forces the whole app light or dark (vibrancy, menus and popovers follow) and refreshes the View menu.
    fn set_appearance(&mut self, appearance: Appearance, window: &mut Window, cx: &mut Context<Self>) {
        use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
        self.appearance = appearance;
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            let name = match appearance {
                Appearance::System => None,
                Appearance::Light => Some(unsafe { NSAppearanceNameAqua }),
                Appearance::Dark => Some(unsafe { NSAppearanceNameDarkAqua }),
            };
            let look = name.and_then(NSAppearance::appearanceNamed);
            NSApplication::sharedApplication(mtm).setAppearance(look.as_deref());
        }
        self.p = Palette::apply(window, cx);
        set_menus(appearance, self.sidebar_open, cx);
        self.save();
        cx.notify();
    }

    /// ⌘B hides or shows the bot list. The width stays, and the choice is saved.
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        self.resizing = false;
        set_menus(self.appearance, self.sidebar_open, cx);
        self.save();
        cx.notify();
    }
}

/// Puts macOS's "sidebar" material behind the whole window; eggbot's opaque main area paints over it.
fn add_vibrancy(window: &Window) {
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let (Some(mtm), Ok(handle)) = (objc2::MainThreadMarker::new(), HasWindowHandle::window_handle(window)) else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: GPUI's AppKit handle points at its live NSView, and we are on the main thread
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let Some(parent) = (unsafe { view.superview() }) else { return };
    let effect = NSVisualEffectView::initWithFrame(mtm.alloc(), parent.bounds());
    effect.setMaterial(NSVisualEffectMaterial::Sidebar);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::FollowsWindowActiveState);
    effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    parent.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, Some(view));
}

fn set_menus(appearance: Appearance, sidebar_open: bool, cx: &mut App) {
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

/// The Dock icon shows only while the window is visible; the menu bar egg is always there.
fn set_dock_icon(visible: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let policy = if visible { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
        NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
    }
}

#[cfg(test)]
mod persist_tests {

    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
}

fn main() {
    // apps opened from Finder get a bare PATH; docker lives in Homebrew, /usr/local/bin or OrbStack's own folder
    let (path, home) = (std::env::var("PATH").unwrap_or_default(), std::env::var("HOME").unwrap_or_default());
    // SAFETY: still single-threaded, before GPUI starts
    unsafe { std::env::set_var("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{home}/.orbstack/bin:{path}")) };
    gpui_kit::application().with_assets(AppAssets).run(|cx| {
        gpui_kit::init(cx);
        // opened by macOS at login: start quietly, with only the menu bar egg
        let at_login = login::launched_at_login();
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
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1080.), px(720.)), cx))),
            // transparent, with a native vibrancy view behind (GPUI's own Blurred has no effect here)
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: Some(TitlebarOptions { title: Some("eggbot".into()), appears_transparent: true, traffic_light_position: Some(point(px(16.), px(16.))) }),
            show: !at_login,
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            add_vibrancy(window);
            cx.new(|cx| app::Eggbot::new(window, cx))
        })
        .unwrap();
        if at_login {
            set_dock_icon(false);
        } else {
            cx.activate(true);
        }
    });
}

#[cfg(test)]
mod tests {}
