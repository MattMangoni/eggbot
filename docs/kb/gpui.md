# GPUI / gpui-kit notes

Versions: `gpui-kit 0.7.0` → `gpui-pre 0.3.7` (Zed snapshot) + `gpui-component 0.7.0` + `gpui-base 0.7.0`.
Plain `gpui 0.2.2` on crates.io is older and incompatible with gpui-component 0.7.

Source to read when unsure: `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.7/examples/` and `gpui-component-0.7.0/src/`.

## Setup

- `gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| { gpui_kit::init(cx); ... })`.
- `gpui_kit::open_window(options, cx, |window, cx| cx.new(...))` mounts the Root for you.
- `use gpui_kit::*;` re-exports gpui. `.when()` needs `use gpui_kit::prelude::FluentBuilder as _;`.
- Icons: `gpui_kit::assets::IconName` (full Lucide set), rendered with `component::Icon::new(..)`.

## Theme

- `Theme::update(cx, |t| { t.background = ..; })` — fields come from `ThemeColor` via Deref. Use `update`, not `global_mut`, so copies and windows stay in sync.

## Input

- `InputState::new(window, cx).placeholder(..)` in `cx.new`; render with `Input::new(&state).appearance(false)` for a bare field.
- Events: `cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, window, cx| ..)`; `InputEvent::PressEnter { shift, secondary }`.
- `state.read(cx).value()`, `state.update(cx, |s, cx| s.set_value("", window, cx))`.

## Markdown

- `component::text::TextView::markdown(id, text).selectable(true)`.

## Animation

- `el.with_animation(id, Animation::new(dur).repeat().with_easing(ease_out_quint()), |el, t| ..)`. State is keyed by id: change the id to restart.
- A one-shot animation keeps its last frame; to switch mode afterwards, re-render (we spawn a timer then `cx.notify()`).
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

## Focus

- Nothing is focused on window open: call `input.update(cx, |s, cx| s.focus(window, cx))` in the view constructor, and again after any click that should return typing to the composer.
