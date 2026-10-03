//! Due schedules and the usage guard: plan meters, pause and throttle checks, and starting scheduled runs.

use std::time::Duration;

use gpui_kit::*;

use super::Eggbot;
use super::bot::{Bot, Msg};
use crate::claude::{Meter, Provider};
use crate::{handoff, sandbox, usage};

const QUIET: &str = "\n\n(This is a scheduled run. If nothing here needs the user's attention, reply with exactly QUIET and nothing else.)";

impl Eggbot {
    /// The background tick: resumes queued work, then starts due schedules. The first check comes soon after launch,
    /// so runs missed while eggbot was closed happen once.
    pub(crate) fn start_ticker(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut wait = Duration::from_secs(3);
            loop {
                cx.background_executor().timer(wait).await;
                let needs_docker = match this.update(cx, |this, _| this.bots.iter().any(|b| !b.busy() && !b.queue.is_empty())) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                // docker info can block; only ask when a handoff is actually waiting
                if needs_docker {
                    let online = cx.background_executor().spawn(async { sandbox::running() }).await;
                    if this.update(cx, |this, cx| this.pump_queues(online, None, cx)).is_err() {
                        break;
                    }
                }
                if this.update(cx, |this, cx| this.run_due(cx)).is_err() {
                    break;
                }
                wait = Duration::from_secs(20);
            }
        })
        .detach();
    }

    /// Starts due schedules the usage guard is willing to run. Held ones keep their anchor.
    pub(crate) fn run_due(&mut self, cx: &mut Context<Self>) {
        let before = self.guard_levels();
        let started = self.release_schedules(cx);
        if started || before != self.guard_levels() {
            cx.notify();
        }
    }

    /// Moves due schedules onto a bot when the guard allows. Held ones keep their anchor, so they stay due.
    pub(crate) fn release_schedules(&mut self, cx: &mut Context<Self>) -> bool {
        let now = chrono::Local::now();
        let levels = [Provider::Claude, Provider::Codex].map(|p| (p, self.breach_of(p).map(|b| b.level).unwrap_or(usage::Level::Ok)));
        let mut taken: Vec<Provider> = self.bots.iter().filter(|b| b.busy()).map(|b| b.provider).collect();
        for b in &self.bots {
            if !b.queue.is_empty() && !taken.contains(&b.provider) {
                taken.push(b.provider);
            }
        }
        let mut due = vec![];
        for b in &mut self.bots {
            let level = levels.iter().find(|(p, _)| *p == b.provider).map(|(_, l)| *l).unwrap_or(usage::Level::Ok);
            let provider_taken = taken.contains(&b.provider);
            if !usage::schedule_action(level, b.busy(), !b.queue.is_empty(), provider_taken) {
                continue;
            }
            let mut fired = false;
            for s in b.schedules.iter_mut().filter(|s| s.due(now)) {
                s.anchor = now.timestamp();
                b.msgs.push(Msg::Scheduled { prompt: s.prompt.clone(), label: s.repeat.label() });
                due.push((b.id, format!("{}{QUIET}", s.prompt)));
                fired = true;
            }
            if fired && level == usage::Level::Throttle && !taken.contains(&b.provider) {
                taken.push(b.provider);
            }
        }
        if due.is_empty() {
            return false;
        }
        for (id, prompt) in due {
            self.deliver(id, handoff::Pending::schedule(prompt), cx);
        }
        self.save();
        true
    }

    pub(crate) fn set_meter(&mut self, meter: Meter, cx: &mut Context<Self>) {
        let provider = meter.provider;
        let before = self.breach_of(provider).map(|b| b.level);
        self.meters.retain(|m| m.provider != meter.provider);
        self.meters.push(meter);
        self.meters.sort_by_key(|m| m.provider.label());
        let after = self.breach_of(provider);
        if after.as_ref().is_some_and(|b| b.level == usage::Level::Pause) && before != Some(usage::Level::Pause) {
            self.announce_pause(provider, after.as_ref().unwrap());
        }
        self.release_schedules(cx);
        self.resume_queues(cx);
        self.save();
    }

    fn announce_pause(&mut self, provider: Provider, breach: &usage::Breach) {
        let Some(id) = self.bots.iter().find(|b| b.provider == provider).map(|b| b.id) else { return };
        let text = usage::explain(usage::provider_name(provider), breach, self.pause, self.throttle);
        self.alert(id, &format!("{} usage paused", usage::provider_name(provider)), &text);
    }

    pub(crate) fn breach_of(&self, provider: Provider) -> Option<usage::Breach> {
        let now = chrono::Local::now().timestamp();
        self.meters.iter().find(|m| m.provider == provider).and_then(|m| usage::breach(&m.windows, now, self.throttle, self.pause))
    }

    /// Pause blocks every new turn. Throttle blocks a second bot of the same provider.
    pub(crate) fn may_start(&self, provider: Provider) -> bool {
        match self.breach_of(provider).map(|b| b.level) {
            Some(usage::Level::Pause) => false,
            Some(usage::Level::Throttle) => !self.bots.iter().any(|b| b.provider == provider && b.busy()),
            _ => true,
        }
    }

    fn guard_levels(&self) -> [usage::Level; 2] {
        [Provider::Claude, Provider::Codex].map(|p| self.breach_of(p).map(|b| b.level).unwrap_or(usage::Level::Ok))
    }

    /// Due schedules of this bot are sitting out a throttle or a pause.
    pub(crate) fn schedules_wait(&self, bot: &Bot) -> bool {
        let Some(breach) = self.breach_of(bot.provider) else { return false };
        let provider_taken = self.bots.iter().any(|b| b.id != bot.id && b.provider == bot.provider && (b.busy() || !b.queue.is_empty()));
        !usage::schedule_action(breach.level, bot.busy(), !bot.queue.is_empty(), provider_taken)
    }
}
