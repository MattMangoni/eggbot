//! The composer: message box, model and effort dropdowns, send and stop, usage banner, and the schedules/skills row.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::select::{Select, SelectItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{label, link, soft_shadow};
use crate::app::bot::Bot;
use crate::app::{Eggbot, Panel};
use crate::claude::Provider;
use crate::usage;

/// A dropdown option: what is shown, and what is stored (None = the provider's default).
#[derive(Clone)]
pub(crate) struct Choice {
    pub(crate) value: Option<String>,
    pub(crate) label: SharedString,
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

/// Model dropdown value: "claude", "claude:opus", "codex", or "codex:<model id>".
pub(crate) fn model_value(provider: Provider, model: Option<&str>) -> String {
    match model {
        Some(m) => format!("{}:{m}", provider.label()),
        None => provider.label().to_string(),
    }
}

/// The reverse of `model_value`. Anything that is not Codex is Claude.
pub(crate) fn parse_model_value(value: &str) -> (Provider, Option<&str>) {
    let (provider, model) = match value.split_once(':') {
        Some((p, m)) => (p, Some(m)),
        None => (value, None),
    };
    (if provider == "codex" { Provider::Codex } else { Provider::Claude }, model)
}

/// "Claude", or "Claude · Opus" once a model is named.
fn model_label(provider: Provider, name: Option<&str>) -> String {
    let who = usage::provider_name(provider);
    name.map_or_else(|| who.to_string(), |n| format!("{who} · {n}"))
}

impl Eggbot {
    /// Fills the model and effort dropdowns for the selected bot (options depend on provider and model).
    pub(crate) fn sync_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selects_stale = false;
        let Some(bot) = self.bots.get(self.selected) else { return };
        let choice = |provider, model: Option<&str>, name: Option<&str>| Choice { value: Some(model_value(provider, model)), label: model_label(provider, name).into() };
        let models: Vec<Choice> = MODELS
            .iter()
            .map(|(alias, label)| choice(Provider::Claude, *alias, alias.map(|_| *label)))
            .chain(std::iter::once(choice(Provider::Codex, None, None)))
            .chain(self.codex_models.iter().map(|m| choice(Provider::Codex, Some(&m.id), Some(&m.name))))
            .collect();
        let current = model_value(bot.provider, bot.model.as_deref());
        let levels: Vec<String> = match bot.provider {
            Provider::Claude => ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec(),
            Provider::Codex => self.codex_models.iter().find(|m| bot.model.as_ref().map_or(m.default, |id| *id == m.id)).map(|m| m.efforts.clone()).unwrap_or_default(),
        };
        let capital = |l: &str| l[..1].to_uppercase() + &l[1..];
        let efforts: Vec<Choice> =
            std::iter::once(Choice { value: None, label: "Default effort".into() }).chain(levels.iter().map(|l| Choice { value: Some(l.clone()), label: capital(l).into() })).collect();
        let effort = bot.effort.clone();
        self.model_select.update(cx, |s, cx| {
            s.set_items(models, window, cx);
            s.set_selected_value(&Some(current), window, cx);
        });
        self.effort_select.update(cx, |s, cx| {
            s.set_items(efforts, window, cx);
            s.set_selected_value(&effort, window, cx);
        });
    }

    /// The input card with the model and effort dropdowns, and the folder/schedules row under it.
    pub(crate) fn composer(&self, bot: &Bot, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let busy = bot.busy();
        let paused = self.paused(bot.provider);
        let send = div()
            .id("send")
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(p.ink)
            .text_color(p.bg)
            .cursor_pointer()
            .when(paused, |d| d.opacity(0.4).cursor_default())
            .when(!paused, |d| d.hover(|d| d.opacity(0.8)))
            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
            .child(Icon::new(IconName::ArrowUp).size_4());
        let stop = busy.then(|| {
            div()
                .id("stop")
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(p.line)
                .cursor_pointer()
                .hover(|d| d.bg(p.hover))
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                .child(div().size(px(10.)).rounded(px(2.)).bg(p.ink))
        });

        // dropdowns hug their label, like the references
        let model_label = self
            .model_select
            .read(cx)
            .selected_value()
            .cloned()
            .flatten()
            .map(|v| {
                let (provider, model) = parse_model_value(&v);
                let name = model.and_then(|m| MODELS.iter().find(|(a, _)| *a == Some(m)).map(|(_, l)| l.to_string()).or_else(|| self.codex_models.iter().find(|x| x.id == m).map(|x| x.name.clone())));
                model_label(provider, name.as_deref())
            })
            .unwrap_or_else(|| "Claude".into());
        let effort_label = bot.effort.clone().unwrap_or_else(|| "Default effort".into());
        let fit = |label: &str| (label.chars().count() as f32 * 6.2 + 28.).clamp(48., 260.);
        let codex_note = (bot.provider == Provider::Codex && self.codex_models.is_empty()).then(|| self.codex_query.clone()).flatten();
        let note = codex_note.map(|failed| match failed {
            None => label("Loading Codex models…", p).into_any_element(),
            Some(e) if e.contains("codex login") => {
                link("codex-sign-in", p).child("Codex is not signed in · Sign in").on_click(cx.listener(|this, _, _, cx| this.sign_in(true, cx))).into_any_element()
            }
            Some(e) => link("codex-retry", p).child(format!("Codex: {e} · Retry")).on_click(cx.listener(|this, _, _, cx| this.refresh_codex(1, cx))).into_any_element(),
        });

        let skills_label = match bot.skills.len() {
            0 => "Skills".to_string(),
            1 => "1 skill".to_string(),
            n => format!("{n} skills"),
        };
        let waiting = self.schedules_wait(bot) && bot.schedules.iter().any(|s| s.due(chrono::Local::now()));
        let schedules = match (bot.schedules.len(), waiting) {
            (0, _) => "Schedules".to_string(),
            (1, false) => "1 schedule".to_string(),
            (1, true) => "1 schedule · waiting".to_string(),
            (n, false) => format!("{n} schedules"),
            (n, true) => format!("{n} schedules · waiting"),
        };
        // the meter already says "one at a time"; the banner appears when this bot cannot start
        let guard = self.breach_of(bot.provider).filter(|br| br.level == usage::Level::Pause || !self.may_start(bot.provider) || !bot.queue.is_empty()).map(|br| {
            let color = if br.level == usage::Level::Pause { p.err } else { p.warn };
            let mut text = usage::explain(usage::provider_name(bot.provider), &br, self.pause, self.throttle);
            if !bot.queue.is_empty() {
                text.push_str(" Waiting work starts once the limit allows it.");
            }
            div()
                .mb_2()
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(12.))
                .bg(p.card)
                .border_1()
                .border_color(p.line)
                .text_xs()
                .text_color(color)
                .child(Icon::new(IconName::CircleAlert).size_3p5().flex_none())
                .child(div().flex_1().min_w_0().child(text))
        });

        div()
            .flex()
            .flex_col()
            .children(guard)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(20.))
                    .bg(p.card)
                    .border_1()
                    .border_color(p.line)
                    .shadow(soft_shadow(p))
                    // the multi-line textarea adds its own 10px inset, so text lines up with the dropdown labels
                    .child(div().px_2().pt_2().child(Textarea::new(&self.input).appearance(false)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_3()
                            .pb_3()
                            // Select fills its parent, so a fixed-width box sets its size
                            .child(div().flex_none().w(px(fit(&model_label))).child(Select::new(&self.model_select).appearance(false).xsmall().menu_width(px(240.)).menu_max_h(px(320.))))
                            .child(div().w(px(1.)).h(px(14.)).bg(p.line))
                            .child(div().flex_none().w(px(fit(&effort_label))).child(Select::new(&self.effort_select).appearance(false).xsmall().menu_width(px(160.))))
                            .child(div().flex_1())
                            .child(div().flex().items_center().gap_2().children(stop).child(send)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2p5()
                    .pt_1()
                    .child(
                        link("clock", p)
                            .when(self.panel == Panel::Schedules, |d| d.bg(p.hover).text_color(p.ink))
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_schedules(window, cx)))
                            .child(Icon::new(IconName::Clock).size_3())
                            .child(schedules),
                    )
                    .child(
                        link("skills", p)
                            .when(self.panel == Panel::Skills, |d| d.bg(p.hover).text_color(p.ink))
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_skills(window, cx)))
                            .child(Icon::new(IconName::BookOpen).size_3())
                            .child(skills_label),
                    )
                    .child(div().flex_1())
                    .when_some(self.folder_error.clone(), |d, e| d.child(div().min_w_0().text_xs().text_color(p.err).truncate().child(e)))
                    .children(note),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
    use core::prelude::v1::test;

    #[test]
    fn model_values_round_trip_and_name_the_model() {
        for (provider, model) in [(Provider::Claude, None), (Provider::Claude, Some("opus")), (Provider::Codex, None), (Provider::Codex, Some("gpt-5.5-codex"))] {
            assert_eq!(parse_model_value(&model_value(provider, model)), (provider, model));
        }
        assert_eq!(model_value(Provider::Claude, Some("opus")), "claude:opus");
        assert_eq!(parse_model_value("anything"), (Provider::Claude, None));
        assert_eq!(model_label(Provider::Codex, Some("GPT-5")), "Codex · GPT-5");
        assert_eq!(model_label(Provider::Claude, None), "Claude");
    }
}
