//! The menu bar egg: shows who is working and keeps eggbot reachable when the window is closed.

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub(crate) enum Action {
    Open,
    Bot(usize),
    Quit,
}

pub(crate) struct Tray {
    icon: TrayIcon,
    /// Still, wobble left, wobble right, still with an unread dot.
    frames: [Icon; 4],
    frame: usize,
    /// (id, name, busy, unread) the menu was last built from; rebuilt only when it changes.
    shown: Vec<(usize, String, bool, bool)>,
}

impl Tray {
    /// Menu clicks go to `tx`.
    pub(crate) fn new(tx: async_channel::Sender<Action>) -> Option<Self> {
        let frames = [egg_icon(0., false), egg_icon(-0.2, false), egg_icon(0.2, false), egg_icon(0., true)];
        let icon = TrayIconBuilder::new().with_icon_templated(frames[0].clone()).with_tooltip("eggbot").build().map_err(|e| eprintln!("eggbot: no menu bar icon: {e}")).ok()?;
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let action = match e.id.0.as_str() {
                "open" => Action::Open,
                "quit" => Action::Quit,
                id => match id.strip_prefix("bot:").and_then(|n| n.parse().ok()) {
                    Some(n) => Action::Bot(n),
                    None => return,
                },
            };
            let _ = tx.send_blocking(action);
        }));
        let mut tray = Self { icon, frames, frame: 0, shown: vec![] };
        tray.update(vec![], 0);
        Some(tray)
    }

    /// Called on a timer: wobbles the egg while any bot works and refreshes the menu when needed.
    pub(crate) fn update(&mut self, bots: Vec<(usize, String, bool, bool)>, tick: usize) {
        let busy = bots.iter().any(|b| b.2);
        let frame = match (busy, bots.iter().any(|b| b.3)) {
            (true, _) => 1 + tick % 2,
            (false, true) => 3,
            _ => 0,
        };
        if frame != self.frame {
            let _ = self.icon.set_icon_templated(Some(self.frames[frame].clone()));
            self.frame = frame;
        }
        if bots == self.shown && tick > 0 {
            return;
        }
        let menu = Menu::new();
        let _ = menu.append(&MenuItem::with_id("open", "Open eggbot", true, None));
        let _ = menu.append(&PredefinedMenuItem::separator());
        for (id, name, busy, unread) in &bots {
            let label = match (busy, unread) {
                (true, _) => format!("● {name} — working"),
                (false, true) => format!("○ {name} — new"),
                _ => format!("○ {name}"),
            };
            let _ = menu.append(&MenuItem::with_id(format!("bot:{id}"), label, true, None));
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("quit", "Quit eggbot", true, None));
        self.icon.set_menu(Some(Box::new(menu)));
        self.shown = bots;
    }
}

/// A 36×36 (18pt @2x) template egg with two eye holes, tilted by `angle` radians around its base; `dot` adds an unread badge.
fn egg_icon(angle: f32, dot: bool) -> Icon {
    const N: usize = 36;
    let (sin, cos) = angle.sin_cos();
    let inside = |x: f32, y: f32| {
        // rotate back around the bottom centre, then test the egg shape in unit space
        let (dx, dy) = (x - 18., y - 32.);
        let (ux, uy) = (dx * cos + dy * sin, -dx * sin + dy * cos + 32.);
        let (cy, half_h) = (19., 13.);
        let t = ((uy - cy) / half_h).clamp(-1., 1.);
        let half_w = 11. * (1. - 0.25 * (-t).max(0.).powi(2)); // narrower top
        let shell = (ux / half_w).powi(2) + ((uy - cy) / half_h).powi(2) <= 1.;
        let eye = |ex: f32| (ux - ex).powi(2) + (uy - 21.).powi(2) <= 2.6f32.powi(2);
        // the badge sits top right, with a clear ring cut out of the shell around it
        let badge = (x - 30.).powi(2) + (y - 7.).powi(2);
        if dot && badge <= 4.5f32.powi(2) {
            return true;
        }
        shell && !eye(-4.5) && !eye(4.5) && !(dot && badge <= 6.5f32.powi(2))
    };
    let mut rgba = vec![0u8; N * N * 4];
    for py in 0..N {
        for px in 0..N {
            // 4×4 supersampling for smooth edges
            let hits = (0..16).filter(|s| inside(px as f32 + (s % 4) as f32 / 4. + 0.125, py as f32 + (s / 4) as f32 / 4. + 0.125)).count();
            rgba[(py * N + px) * 4 + 3] = (hits * 255 / 16) as u8;
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("valid icon size")
}
