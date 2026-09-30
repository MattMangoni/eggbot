use std::f32::consts::TAU;
use std::time::Duration;

use gpui_kit::*;

/// One frame of an egg: every animation just produces one of these.
#[derive(Clone, Copy)]
struct Pose {
    rot: f32,
    squash: f32,
    blink: f32,
    look: f32,
    crack: f32,
    face: f32,
}

const REST: Pose = Pose { rot: 0., squash: 0., blink: 0., look: 0., crack: 0., face: 1. };
pub const HATCH: Duration = Duration::from_millis(1800);
pub const BOING: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    Unborn,
    Hatching,
    Idle,
    Thinking,
    /// No animation, for eggs that repeat many times (chat history).
    Still,
    /// Squash-and-stretch after a click; the counter restarts the animation.
    Boing(u32),
}

pub fn egg(id: impl Into<SharedString>, color: Hsla, w: f32, mood: Mood) -> AnyElement {
    let id: SharedString = id.into();
    let base = div().w(px(w)).h(px(w * 1.3));
    let draw = move |el: Div, pose: Pose| el.child(paint(color, w, pose));
    match mood {
        Mood::Unborn => draw(base, Pose { face: 0., ..REST }).into_any_element(),
        Mood::Still => draw(base, REST).into_any_element(),
        Mood::Boing(n) => base
            .with_animation(
                ElementId::Name(format!("{id}-boing-{n}").into()),
                Animation::new(BOING),
                move |el, t| draw(el, Pose { squash: 0.28 * (-5. * t).exp() * (t * TAU * 2.5).sin(), ..REST }),
            )
            .into_any_element(),
        Mood::Hatching => base
            .with_animation(
                ElementId::Name(format!("{id}-hatch").into()),
                Animation::new(HATCH),
                move |el, t| draw(el, hatch(t)),
            )
            .into_any_element(),
        Mood::Idle => base
            .with_animation(
                ElementId::Name(format!("{id}-idle").into()),
                Animation::new(Duration::from_millis(4700)).repeat(),
                move |el, t| {
                    let blink = if (0.9..0.95).contains(&t) { ((t - 0.9) / 0.05 * TAU / 2.).sin() } else { 0. };
                    draw(el, Pose { squash: 0.018 * (t * TAU * 2.).sin(), blink, ..REST })
                },
            )
            .into_any_element(),
        Mood::Thinking => base
            .with_animation(
                ElementId::Name(format!("{id}-think").into()),
                Animation::new(Duration::from_millis(1400)).repeat(),
                move |el, t| {
                    let s = (t * TAU).sin();
                    draw(el, Pose { rot: 0.1 * s, squash: 0.04 * (t * TAU * 2.).sin().abs(), look: s, ..REST })
                },
            )
            .into_any_element(),
    }
}

/// Drop in, wobble, crack, open eyes.
fn hatch(t: f32) -> Pose {
    let seg = |a: f32, b: f32| ((t - a) / (b - a)).clamp(0., 1.);
    let pop = seg(0., 0.2);
    let wobble = seg(0.2, 0.6);
    let crack = seg(0.55, 0.75);
    let open = seg(0.75, 1.);
    // underdamped spring so the egg lands with a little bounce
    let land = 1. - (-7. * pop).exp() * (pop * 14.).cos();
    Pose {
        rot: 0.22 * (wobble * TAU * 3.).sin() * (1. - wobble),
        squash: 0.25 * (1. - land) + 0.06 * (open * TAU / 2.).sin(),
        blink: 1. - open,
        look: 0.,
        crack: crack * (1. - open),
        face: open.max(0.001),
    }
}

fn paint(color: Hsla, size: f32, pose: Pose) -> impl IntoElement {
    canvas(|_, _, _| {}, move |b, _, window, _| {
        let (w, h) = (b.size.width.as_f32(), b.size.height.as_f32());
        let (ox, oy) = (b.origin.x.as_f32(), b.origin.y.as_f32());
        let s = w * 0.92;
        // unit egg coordinates (x in 0..1, y in 0..1.3), pivot at the bottom centre
        let (sin, cos) = pose.rot.sin_cos();
        let sx = 1. + pose.squash * 0.6;
        let sy = 1. - pose.squash;
        let at = |x: f32, y: f32| {
            let (dx, dy) = ((x - 0.5) * s * sx, (y - 1.3) * s * sy);
            point(px(ox + w / 2. + dx * cos - dy * sin), px(oy + h - 0.5 + dx * sin + dy * cos))
        };

        let mut shadow = PathBuilder::fill();
        ellipse(&mut shadow, point(px(ox + w / 2.), px(oy + h - 1.)), s * 0.34, s * 0.05);
        paint_path(window, shadow, black().opacity(0.12));

        let mut shell = PathBuilder::fill();
        shell.move_to(at(0.5, 0.));
        // narrow top, widest point below the middle
        shell.cubic_bezier_to(at(1., 0.82), at(0.76, 0.), at(1., 0.4));
        shell.cubic_bezier_to(at(0.5, 1.3), at(1., 1.12), at(0.8, 1.3));
        shell.cubic_bezier_to(at(0., 0.82), at(0.2, 1.3), at(0., 1.12));
        shell.cubic_bezier_to(at(0.5, 0.), at(0., 0.4), at(0.24, 0.));
        shell.close();
        paint_path(window, shell, color);

        let mut gloss = PathBuilder::fill();
        ellipse(&mut gloss, at(0.3, 0.34), s * 0.08, s * 0.12);
        paint_path(window, gloss, white().opacity(0.45));

        let ink = hsla(30. / 360., 0.12, 0.16, pose.face);
        // small eggs get relatively bigger eyes so the face still reads
        let k = (44. / size).clamp(1., 1.6);
        let eye_h = s * 0.07 * k * (1. - pose.blink).max(0.12);
        for x in [0.36, 0.64] {
            let mut eye = PathBuilder::fill();
            ellipse(&mut eye, at(x + pose.look * 0.04, 0.8), s * 0.055 * k, eye_h);
            paint_path(window, eye, ink);
        }
        for x in [0.25, 0.75] {
            let mut cheek = PathBuilder::fill();
            ellipse(&mut cheek, at(x, 0.95), s * 0.07, s * 0.035);
            paint_path(window, cheek, hsla(10. / 360., 0.9, 0.7, 0.35 * pose.face));
        }

        if pose.crack > 0. {
            let zig = [(0.08, 0.62), (0.22, 0.54), (0.34, 0.66), (0.48, 0.55), (0.6, 0.67), (0.74, 0.55), (0.92, 0.63)];
            let n = ((zig.len() - 1) as f32 * pose.crack).ceil() as usize;
            let mut crack = PathBuilder::stroke(px(s * 0.03));
            crack.move_to(at(zig[0].0, zig[0].1));
            for &(x, y) in &zig[1..=n] {
                crack.line_to(at(x, y));
            }
            paint_path(window, crack, hsla(30. / 360., 0.2, 0.25, 0.8));
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

fn paint_path(window: &mut Window, p: PathBuilder, color: Hsla) {
    if let Ok(path) = p.build() {
        window.paint_path(path, color);
    }
}
