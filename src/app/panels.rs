//! Panel forms: settings, the bot editor, skills, and schedules. One panel is open at a time.

use gpui_kit::*;

use super::state::SHARED;
use super::{Eggbot, Panel};
use crate::claude::Provider;
use crate::{login, schedule, skills, usage};

impl Eggbot {
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone().unwrap_or_else(|| SHARED.into());
        self.edit_shared.update(cx, |s, cx| {
            s.set_value(shared, window, cx);
            s.focus(window, cx);
        });
        let (throttle, pause) = (usage::percent(self.throttle).to_string(), usage::percent(self.pause).to_string());
        self.limit_throttle.update(cx, |s, cx| s.set_value(throttle, window, cx));
        self.limit_pause.update(cx, |s, cx| s.set_value(pause, window, cx));
        (self.panel, self.login_error, self.settings_error) = (Panel::Settings, None, None);
        cx.notify();
    }

    /// Saves the instructions for all bots; each bot gets them from its next turn (both providers).
    pub(crate) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.edit_shared.read(cx).value().trim().to_string();
        let limits = usage::parse_limits(&self.limit_throttle.read(cx).value(), &self.limit_pause.read(cx).value());
        let Ok((throttle, pause)) = limits else {
            self.settings_error = limits.err().map(str::to_string);
            cx.notify();
            return;
        };
        self.shared = (text != SHARED).then_some(text);
        (self.throttle, self.pause) = (throttle, pause);
        (self.panel, self.settings_error) = (Panel::None, None);
        self.save();
        // a looser limit can let held schedules and queued turns go
        self.release_schedules(cx);
        self.resume_queues(cx);
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn set_login(&mut self, on: bool, cx: &mut Context<Self>) {
        self.login_error = login::set(on).err();
        cx.notify();
    }

    pub(crate) fn add_schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.sched_prompt.read(cx).value().trim().to_string();
        let value = self.sched_value.read(cx).value().to_string();
        let repeat = match (prompt.is_empty(), schedule::Repeat::parse(self.sched_kind, &value)) {
            (true, _) => Err("Write what the bot should do"),
            (false, r) => r,
        };
        match (repeat, self.bots.get_mut(self.selected)) {
            (Ok(repeat), Some(bot)) => {
                let id = bot.schedules.iter().map(|s| s.id + 1).max().unwrap_or(0);
                bot.schedules.push(schedule::Schedule { id, prompt, repeat, anchor: chrono::Local::now().timestamp() });
                self.sched_error = None;
                self.sched_prompt.update(cx, |s, cx| s.set_value("", window, cx));
                self.save();
            }
            (Err(e), _) => self.sched_error = Some(e.into()),
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get(self.selected) else { return };
        let (name, role) = (bot.name.clone(), bot.role().to_string());
        self.edit_name.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
        });
        self.edit_role.update(cx, |s, cx| s.set_value(role, window, cx));
        self.panel = Panel::Editor;
        self.edit_error = None;
        self.selects_stale = true;
        if self.bots[self.selected].provider == Provider::Codex && self.codex_models.is_empty() && self.codex_query != Some(None) {
            self.refresh_codex(1, cx);
        }
        cx.notify();
    }

    pub(crate) fn save_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.edit_name.read(cx).value().trim().to_string();
        let role = self.edit_role.read(cx).value().trim().to_string();
        let Some(id) = self.bots.get(self.selected).map(|b| b.id) else { return };
        let taken = self.bots.iter().any(|b| b.id != id && b.name.eq_ignore_ascii_case(&name));
        self.edit_error = match () {
            _ if name.is_empty() || name.chars().count() > 24 => Some("Use a name of 1 to 24 characters".into()),
            _ if taken => Some("Another bot already has this name (@mentions need unique names)".into()),
            _ if role.is_empty() => Some("Write a role, even a short one".into()),
            _ => None,
        };
        if self.edit_error.is_none()
            && let Some(bot) = self.bots.get_mut(self.selected)
        {
            bot.name = name;
            bot.role = (role != bot.preset().role).then_some(role);
            self.panel = Panel::None;
            self.save();
            self.input.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn remove_schedule(&mut self, id: usize, cx: &mut Context<Self>) {
        if let Some(bot) = self.bots.get_mut(self.selected) {
            bot.schedules.retain(|s| s.id != id);
            self.save();
            cx.notify();
        }
    }

    pub(crate) fn toggle_skills(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = if self.panel == Panel::Skills { Panel::None } else { Panel::Skills };
        if self.panel == Panel::Skills {
            self.clear_skill_form(window, cx);
            self.skill_name.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn toggle_schedules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = if self.panel == Panel::Schedules { Panel::None } else { Panel::Schedules };
        self.sched_error = None;
        if self.panel == Panel::Schedules {
            self.sched_prompt.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Panel::None;
        self.focus_main(window, cx);
        cx.notify();
    }

    pub(crate) fn clear_skill_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.skill_at = None;
        self.skill_error = None;
        self.skill_name.update(cx, |s, cx| s.set_value("", window, cx));
        self.skill_body.update(cx, |s, cx| s.set_value("", window, cx));
    }

    pub(crate) fn edit_skill(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(skill) = self.bots.get(self.selected).and_then(|b| b.skills.get(index)).cloned() else { return };
        self.skill_at = Some(index);
        self.skill_error = None;
        self.skill_name.update(cx, |s, cx| {
            s.set_value(skill.name, window, cx);
            s.focus(window, cx);
        });
        self.skill_body.update(cx, |s, cx| s.set_value(skill.body, window, cx));
        cx.notify();
    }

    /// Writes the form onto this bot's own list. The next turn sends it with the role.
    pub(crate) fn save_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.skill_name.read(cx).value().to_string();
        let body = self.skill_body.read(cx).value().to_string();
        let at = self.skill_at;
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        match skills::upsert(&mut bot.skills, at, &name, &body) {
            Ok(()) => {
                self.save();
                self.clear_skill_form(window, cx);
            }
            Err(e) => self.skill_error = Some(e.into()),
        }
        cx.notify();
    }

    pub(crate) fn remove_skill(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get_mut(self.selected) else { return };
        if !skills::remove(&mut bot.skills, index) {
            return;
        }
        if self.skill_at == Some(index) {
            self.clear_skill_form(window, cx);
        } else if let Some(at) = self.skill_at.filter(|at| *at > index) {
            self.skill_at = Some(at - 1);
        }
        self.save();
        cx.notify();
    }
}
