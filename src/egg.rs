use std::f32::consts::TAU;
use std::time::Duration;

use gpui_kit::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    /// At rest: no animation.
    Still,
    /// The bot is working: a gentle wobble and glancing eyes.
    Thinking,
}

/// A small vector egg with two eyes, `w` pixels wide.
pub fn egg(id: impl Into<SharedString>, color: Hsla, w: f32, mood: Mood) -> AnyElement {
    let id: SharedString = id.into();
    let base = div().w(px(w)).h(px(w * 1.3)).flex_none();
    match mood {
        Mood::Still => base.child(paint(color, w, 0., 0.)).into_any_element(),
        Mood::Thinking => base
            .with_animation(ElementId::Name(format!("{id}-think").into()), Animation::new(Duration::from_millis(1400)).repeat(), move |el, t| {
                let s = (t * TAU).sin();
                el.child(paint(color, w, 0.1 * s, s))
            })
            .into_any_element(),
    }
}

fn paint(color: Hsla, size: f32, rot: f32, look: f32) -> impl IntoElement {
    canvas(|_, _, _| {}, move |b, _, window, _| {
        let (w, h) = (b.size.width.as_f32(), b.size.height.as_f32());
        let (ox, oy) = (b.origin.x.as_f32(), b.origin.y.as_f32());
        // unit egg coordinates (x in 0..1, y in 0..1.3), rotated around the bottom centre
        let (sin, cos) = rot.sin_cos();
        let at = |x: f32, y: f32| {
            let (dx, dy) = ((x - 0.5) * w, (y - 1.3) * w);
            point(px(ox + w / 2. + dx * cos - dy * sin), px(oy + h + dx * sin + dy * cos))
        };

        let mut shell = PathBuilder::fill();
        shell.move_to(at(0.5, 0.));
        shell.cubic_bezier_to(at(1., 0.82), at(0.76, 0.), at(1., 0.4));
        shell.cubic_bezier_to(at(0.5, 1.3), at(1., 1.12), at(0.8, 1.3));
        shell.cubic_bezier_to(at(0., 0.82), at(0.2, 1.3), at(0., 1.12));
        shell.cubic_bezier_to(at(0.5, 0.), at(0., 0.4), at(0.24, 0.));
        shell.close();
        fill(window, shell, color);

        // small eggs get relatively bigger eyes so the face still reads
        let k = (44. / size).clamp(1., 1.6);
        for x in [0.36, 0.64] {
            let mut eye = PathBuilder::fill();
            ellipse(&mut eye, at(x + look * 0.04, 0.8), w * 0.05 * k, w * 0.065 * k);
            fill(window, eye, hsla(0., 0., 0.1, 0.85));
        }
    })
    .size_full()
}

fn ellipse(p: &mut PathBuilder, c: Point<Pixels>, rx: f32, ry: f32) {
    let k = 0.5523;
    let (x, y) = (c.x.as_f32(), c.y.as_f32());
    let pt = |dx: f32, dy: f32| point(px(x + dx), px(y + dy));
    p.move_to(pt(rx, 0.));
    p.cubic_bezier_to(pt(0., ry), pt(rx, ry * k), pt(rx * k, ry));
    p.cubic_bezier_to(pt(-rx, 0.), pt(-rx * k, ry), pt(-rx, ry * k));
    p.cubic_bezier_to(pt(0., -ry), pt(-rx, -ry * k), pt(-rx * k, -ry));
    p.cubic_bezier_to(pt(rx, 0.), pt(rx * k, -ry), pt(rx, -ry * k));
    p.close();
}

fn fill(window: &mut Window, p: PathBuilder, color: Hsla) {
    if let Ok(path) = p.build() {
        window.paint_path(path, color);
    }
}
