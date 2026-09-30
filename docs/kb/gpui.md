# GPUI / gpui-kit notes

Versions: `gpui-kit 0.7.0` → `gpui-pre 0.3.7` (Zed snapshot) + `gpui-component 0.7.0` + `gpui-base 0.7.0`.
Plain `gpui 0.2.2` on crates.io is older and incompatible with gpui-component 0.7.

Source to read when unsure: `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.7/examples/` and `gpui-component-0.7.0/src/`.

## Setup

- `gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| { gpui_kit::init(cx); ... })`.
- `gpui_kit::open_window(options, cx, |window, cx| cx.new(...))` mounts the Root for you.
- `use gpui_kit::*;` re-exports gpui. `.when()` needs `use gpui_kit::prelude::FluentBuilder as _;`.
- Icons: `gpui_kit::assets::IconName` names the full Lucide set, but `Assets` embeds ONLY the ~101 icons in `gpui-kit-assets/default-icons.txt`. Others (e.g. `Clock`, `Trash`) render as nothing unless added: `icon_assets!(ExtraIcons, [Clock, Trash])` + an `AppAssets` source that tries `ExtraIcons` then `Assets` (see `src/main.rs`).

## Theme

- `Theme::update(cx, |t| { t.background = ..; })` — fields come from `ThemeColor` via Deref. Use `update`, not `global_mut`, so copies and windows stay in sync (it also re-installs markdown TextView colors).
- Dark mode: `Theme::sync_system_appearance(Some(window), cx)` first (it reloads stock colors), then our colors in a separate `Theme::update`. Re-run on `cx.observe_window_appearance(window, ..)`. See `Palette::apply` in `src/main.rs`.

## Input

- `InputState::new(window, cx).placeholder(..)` in `cx.new`; render with `Input::new(&state).appearance(false)` for a bare field.
- Events: `cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, window, cx| ..)`; `InputEvent::PressEnter { shift, secondary }`.
- `state.read(cx).value()`, `state.update(cx, |s, cx| s.set_value("", window, cx))`.

## Markdown

- `component::text::TextView::markdown(id, text).selectable(true)`.

## Animation

- `el.with_animation(id, Animation::new(dur).repeat().with_easing(ease_out_quint()), |el, t| ..)`. State is keyed by id: change the id to restart.
- A one-shot animation keeps its last frame; to switch mode afterwards, re-render (we spawn a timer then `cx.notify()`).
- A spring retargets smoothly when `.to(..)` changes between renders (same id) — used for the sidebar selection card.
- Springs exist: `el.with_spring(id, SpringAnimation::new(SpringConfig::new(170., damping, 1.)).to(value), |el, v| ..)`.
- Divs cannot rotate (`Transformation` is svg-only). For rotation, paint with `canvas` + `PathBuilder` and transform points yourself (see `src/egg.rs`).

## Drawing

- `canvas(prepaint, paint)`; in paint: `PathBuilder::fill()` / `::stroke(px(w))`, `move_to`, `line_to`, `cubic_bezier_to(to, ctrl_a, ctrl_b)` (note: `to` first), `close`, `build()`, `window.paint_path(path, color)`.

## Async

- `cx.spawn(async move |this, cx| { .. this.update(cx, |this, cx| ..) })`.
- Timers: clone the executor first (`let ex = cx.background_executor().clone();`) — borrowing `cx` in a closure blocks `this.update(cx, ..)`.

## Window

- Transparent titlebar: `TitlebarOptions { appears_transparent: true, traffic_light_position: Some(point(px(16.), px(16.))), .. }`; leave ~44px top padding.

## Screenshots (dev)

- `screencapture -l<windowID>` needs Screen Recording permission for the terminal (Ghostty), granted and then restart the terminal.
- `Window::render_to_image()` exists behind gpui's `test-support` feature (offscreen alternative, not used).
- Screenshots and keystrokes run outside the tool sandbox (`screencapture` fails inside it even with permission).
- Drive the UI for tests: `osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "eggbot") to true' -e 'tell application "System Events" to keystroke "hi"' -e 'tell application "System Events" to key code 36'`.

## Scroll

- After loading history or switching chats, call `scroll.scroll_to_bottom()`; the handle applies it on the next layout.

## Focus

- Nothing is focused on window open: call `input.update(cx, |s, cx| s.focus(window, cx))` in the view constructor, and again after any click that should return typing to the composer.

## Background app (menu bar)

- GPUI has no tray API; we use `tray-icon` 0.26 (+ its `muda` menus). Build the icon inside `application().run`, on the main thread. Use `with_icon_templated` / `set_icon_templated` (the `*_as_template` calls are deprecated).
- Menu clicks: `MenuEvent::set_event_handler` → `async_channel` → a `cx.spawn_in(window, …)` loop with `update_in`.
- Scripted clicks on the status item (System Events) do NOT open a tray-icon menu; only real mouse clicks do.
- Keep the view alive when the window "closes": `window.on_window_should_close(cx, |_, cx| { cx.hide(); …; false })`.
- Dock icon on/off: `objc2_app_kit::NSApplication::sharedApplication(mtm).setActivationPolicy(Regular|Accessory)` (safe fn in objc2-app-kit 0.3).
- ⌘Q/⌘W: `actions!` + `cx.bind_keys` + `cx.set_menus`; handle with `.on_action(cx.listener(..))` on the root div.
- Confirm dialogs: `window.prompt(PromptLevel::Warning, msg, Some(detail), &["Quit", "Cancel"], cx)` → oneshot with the button index.

## Dropdowns

- `component::select::{Select, SelectState, SelectEvent, SelectItem}`. Custom item = struct implementing `SelectItem` (`type Value`, `title()`, `value()`); see `Choice` in `src/main.rs`.
- `SelectState::new(Vec<Item>, None, window, cx)`; refill with `set_items(items, window, cx)` + `set_selected_value(&v, window, cx)` (both need `window`, so eggbot refills in `render` when `selects_stale`).
- Listen with `cx.subscribe_in(&state, window, |this, _, ev: &SelectEvent<Vec<Item>>, _, cx| …)`; `SelectEvent::Confirm(Some(value))`.

## Traps (2026-10-01)

- Setting `.hover(..)` twice on one element panics: "hover style already set" (div.rs:843). Helpers that set hover (`button()` in `src/ui.rs`) must not get a second `.hover`; use a separate helper (`primary()`).
- A lazy `.map(..)` iterator of children that captures `cx` must be `.collect()`ed before `cx` is used again in the same builder chain.
- gpui-component's window root paints `theme.background` over the whole window. For a see-through window set `Theme.background = transparent_black()` and paint the opaque areas yourself.
- `WindowBackgroundAppearance::Blurred` did nothing on macOS 27. Use `Transparent` plus a native `NSVisualEffectView` (material `Sidebar`, blending `BehindWindow`) inserted below GPUI's NSView (`add_vibrancy` in `src/main.rs`). Get the NSView via `raw_window_handle::HasWindowHandle::window_handle(window)` — call it as a trait function, because GPUI's own `Window::window_handle()` shadows it.
- `Select` fills its parent; wrap it in a fixed-width `div` to size it (`.w()` on the Select does not reach the trigger).
- Test clicks: `scratchpad/click X Y` (Swift, CGEvent) works on GPUI views; System Events clicks do not.
- gpui-component binds ⌘[ / ⌘] (outdent/indent) inside text fields, and context bindings beat global ones: don't use them for app shortcuts. eggbot uses ⌘1…9 and ⌃Tab / ⌃⇧Tab.
- Parameterized actions: `#[derive(Clone, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)] #[action(namespace = …)]` needs `schemars` as a direct dependency.
- Multi-line composer: `TextareaState::new(..).submit_on_enter(true)` + `set_auto_grow(1, 8, cx)` → Enter emits `PressEnter`, Shift+Enter inserts a newline.
- Soft shadows: pass a `Vec<BoxShadow>` to `.shadow(..)` (hairline + wide faint blur); the stock `shadow_sm/md` look harsh on white.
- Forced light/dark: set `NSApplication.appearance` (`NSAppearanceNameAqua` / `DarkAqua`, nil = system). The window's effective appearance follows, so `Theme::sync_system_appearance`, the vibrancy view, menus and popovers all switch together.
- Multi-line `Textarea` adds its own inset (`Size::input_px()`, 10px at medium) even with `appearance(false)`; count it when aligning with neighbours.
- Checked menu items: build `MenuItem::Action { checked, .. }` directly and call `cx.set_menus` again when the state changes.
- Starting hidden: `WindowOptions { show: false, .. }`; `window.activate_window()` later shows it. GPUI's `run` callback runs inside `applicationDidFinishLaunching`, so `NSAppleEventManager.currentAppleEvent` still holds the launch event there.
- objc2-foundation hides `NSAppleEventDescriptor.eventID` / `paramDescriptorForKeyword` behind the large `objc2-core-services` feature; a raw `msg_send!` avoids it.
- System Events keystrokes go to whatever app is in front: set eggbot frontmost in the same osascript call, or test text lands in the terminal.
