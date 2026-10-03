//! Panels above the main pane (bot editor, settings, skills, schedules) and the first-run setup checklist.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::theme::hex;
use super::{button, divider, field, heading, label, link, primary};
use crate::app::bot::{Bot, SHELLS};
use crate::app::setup::{Check, Setup};
use crate::app::{Eggbot, Panel};
use crate::egg::{Mood, egg};
use crate::usage;
use crate::{login, sandbox};

impl Eggbot {
    /// The first-run checklist: each row turns green on its own as the checks pass.
    pub(crate) fn setup_view(&self, s: &Setup, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().text_color(p.ink).child(title))
                        .child(div().text_xs().text_color(if failed { p.err } else { p.muted }).child(line)),
                )
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
            .child(row(
                "setup-engine",
                "Docker engine",
                &s.engine,
                "Docker is installed.",
                "Each bot runs in its own container. Colima is free and open source.",
                Some(("Install Colima", Box::new(|this: &mut Eggbot, cx: &mut Context<Eggbot>| this.setup_action(|s| &mut s.engine, "Installing in Terminal…", sandbox::install_engine, cx)))),
            ))
            .child(row(
                "setup-running",
                "Docker running",
                &s.running,
                "Docker is running.",
                "Start your Docker engine.",
                ready(&s.engine).then(|| ("Start Docker", Box::new(|this: &mut Eggbot, cx: &mut Context<Eggbot>| this.setup_action(|s| &mut s.running, "Starting Docker…", sandbox::wake, cx)) as Act)),
            ))
            .child(row(
                "setup-image",
                "Bot machine",
                &s.image,
                "The bot machine is ready.",
                "The image every bot runs in. Built once, in about a minute.",
                ready(&s.running).then(|| {
                    ("Build", Box::new(|this: &mut Eggbot, cx: &mut Context<Eggbot>| this.setup_action(|s| &mut s.image, "Building, about a minute…", || sandbox::ready(&|_| {}), cx)) as Act)
                }),
            ))
            .child(divider(p).my_1())
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

    /// The bot editor: name, role, egg color. Model and effort live in the composer.
    pub(crate) fn editor(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
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
        div().px_6().child(
            self.panel()
                .child(div().flex().flex_col().gap_1().child(label("Name", p)).child(field(p).child(Input::new(&self.edit_name).appearance(false))))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(label("Role", p))
                        .child(field(p).child(Textarea::new(&self.edit_role).appearance(false)))
                        .child(label("Skills sit under the composer and go out with this role.", p)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(label("Egg", p))
                        .children(swatches)
                        .child(div().flex_1())
                        .child(
                            button("cancel-edit", p)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.panel = Panel::None;
                                    cx.notify();
                                }))
                                .child("Cancel"),
                        )
                        .child(primary("save-edit", p).on_click(cx.listener(|this, _, window, cx| this.save_edit(window, cx))).child("Save")),
                )
                .when_some(self.edit_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e))),
        )
    }

    /// Settings (⌘,): start at login, and instructions every bot gets.
    pub(crate) fn settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let login = login::state();
        let note = match (&self.login_error, &login) {
            (Some(e), _) => Some(div().text_xs().text_color(p.err).child(e.clone()).into_any_element()),
            (None, login::State::NeedsApproval) => {
                Some(link("login-approve", p).child("Allow eggbot in System Settings → Login Items").on_click(|_, _, _| login::open_system_settings()).into_any_element())
            }
            _ => None,
        };
        let on = !matches!(login, login::State::Off);
        div().px_6().child(
            self.panel()
                .child(heading("Settings", p))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().flex().flex_col().flex_1().child(heading("Start at login", p)).child(label("Only the menu bar egg appears, and schedules keep running.", p)))
                        .child(Switch::new("login").checked(on).on_click(cx.listener(|this, on: &bool, _, cx| this.set_login(*on, cx)))),
                )
                .children(note)
                .child(divider(p))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(heading("Instructions for all bots", p))
                        .child(label("Every bot gets these next to its own role, from its next turn.", p))
                        .child(field(p).mt_1().child(Textarea::new(&self.edit_shared).appearance(false))),
                )
                .child(divider(p))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(heading("Usage guardrails", p))
                        .child(label("Throttle runs one bot per provider. Pause holds new turns and leaves schedules due. Bars turn amber at 80%.", p))
                        .child(
                            div()
                                .mt_1()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(label("Throttle at", p))
                                .child(field(p).w(px(64.)).child(Input::new(&self.limit_throttle).appearance(false)))
                                .child(label("%", p))
                                .child(label("Pause at", p).ml_2())
                                .child(field(p).w(px(64.)).child(Input::new(&self.limit_pause).appearance(false)))
                                .child(label("%", p)),
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
                            button("cancel-settings", p)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.panel = Panel::None;
                                    cx.notify();
                                }))
                                .child("Cancel"),
                        )
                        .child(primary("save-settings", p).on_click(cx.listener(|this, _, window, cx| this.save_settings(window, cx))).child("Save")),
                ),
        )
    }

    /// This bot's skills: the preset's defaults, then whatever the user added, edited, or removed.
    pub(crate) fn skills(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let editing = self.skill_at;
        let rows = bot.skills.iter().enumerate().map(|(i, s)| {
            let on = editing == Some(i);
            let line = s.body.lines().next().unwrap_or("");
            let mut preview: String = line.chars().take(72).collect();
            if line.chars().count() > 72 {
                preview.push('…');
            }
            div()
                .id(("skill", i))
                .flex()
                .items_center()
                .gap_3()
                .px_2()
                .py_1()
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |d| d.bg(p.hover))
                .hover(|d| d.bg(p.hover))
                .on_click(cx.listener(move |this, _, window, cx| this.edit_skill(i, window, cx)))
                .child(Icon::new(IconName::BookOpen).size_3p5().text_color(p.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_sm().text_color(p.ink).truncate().child(s.name.clone()))
                        .child(div().text_xs().text_color(p.muted).truncate().child(preview)),
                )
                .child(
                    div()
                        .id(("unskill", i))
                        .text_color(p.muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(p.ink))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.remove_skill(i, window, cx);
                        }))
                        .child(Icon::new(IconName::Trash).size_3p5()),
                )
        });
        let saving = self.skill_at.is_some();
        div().px_6().child(
            self.panel()
                .child(heading(format!("{}'s skills", bot.name), p))
                .child(label("Procedures this bot follows from its next turn.", p))
                .when(bot.skills.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("No skills yet. Add one below.")))
                .child(div().flex().flex_col().gap_1().children(rows))
                .child(divider(p))
                .child(div().flex().flex_col().gap_1().child(label("Name", p)).child(field(p).child(Input::new(&self.skill_name).appearance(false))))
                .child(div().flex().flex_col().gap_1().child(label("Instructions", p)).child(field(p).child(Textarea::new(&self.skill_body).appearance(false))))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when_some(self.skill_error.clone(), |d, e| d.child(div().flex_1().min_w_0().text_xs().text_color(p.err).child(e)))
                        .when(self.skill_error.is_none(), |d| d.child(div().flex_1()))
                        .when(saving, |d| {
                            d.child(
                                button("clear-skill", p)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.clear_skill_form(window, cx);
                                        cx.notify();
                                    }))
                                    .child("Cancel"),
                            )
                        })
                        .child(primary("save-skill", p).on_click(cx.listener(|this, _, window, cx| this.save_skill(window, cx))).child(if saving { "Save" } else { "Add" })),
                ),
        )
    }

    /// The schedules panel: this bot's schedules and a form to add one.
    pub(crate) fn schedules(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(div().flex_1().min_w_0().truncate().text_color(p.ink).child(s.prompt.clone()))
                .child(div().flex_none().text_xs().text_color(if due_held { note_color } else { p.muted }).child(when))
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
        div().px_6().child(
            self.panel()
                .child(heading(format!("{}'s schedules", bot.name), p))
                .when(bot.schedules.is_empty(), |d| d.child(div().text_sm().text_color(p.muted).child("Nothing scheduled. Each run starts a fresh session with the bot's notes.")))
                .when(wait && !bot.schedules.is_empty(), |d| d.child(div().text_xs().text_color(note_color).child("Due runs wait here instead of starting, and go once the meter drops.")))
                .child(div().flex().flex_col().gap_2().children(rows))
                .child(divider(p))
                .child(div().flex().flex_col().gap_1().child(label("Task", p)).child(field(p).child(Input::new(&self.sched_prompt).appearance(false))))
                .child(
                    div()
                        .flex()
                        .gap_1()
                        // the chips fill the height of the time field, so both centre on the same line
                        .child(div().flex().flex_col().gap_1().child(label("Repeat", p)).child(div().flex_1().flex().items_center().gap_1().children(kinds)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .ml_2()
                                .child(label(["At", "At", "Hours", "Minutes"][self.sched_kind.min(3)], p))
                                .child(field(p).w(px(90.)).child(Input::new(&self.sched_value).appearance(false))),
                        )
                        .child(div().flex_1())
                        .child(primary("add-schedule", p).self_end().on_click(cx.listener(|this, _, window, cx| this.add_schedule(window, cx))).child("Add")),
                )
                .when(self.sched_kind == 3, |d| d.child(label("Short intervals use your plan limit quickly.", p)))
                .when_some(self.sched_error.clone(), |d, e| d.child(div().text_xs().text_color(p.err).child(e))),
        )
    }
}
