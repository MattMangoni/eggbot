//! First-run setup and sign-in: the checklist and its re-check loop, setup actions, the sign-in watch, and the Codex account query.

use std::time::Duration;

use gpui_kit::*;

use super::bot::Msg;
use super::{Eggbot, Panel};
use crate::claude::Provider;
use crate::{codex, sandbox};

/// One row of the setup checklist.
#[derive(Clone, PartialEq)]
pub(crate) enum Check {
    Unknown,
    Ok,
    Missing,
    /// An action is running (install, start, build, sign-in); the label says what.
    Busy(&'static str),
    Failed(String),
}

/// The first-run checklist: Docker engine, Docker running, bot image, Claude and Codex sign-in.
#[derive(Clone)]
pub(crate) struct Setup {
    pub(crate) engine: Check,
    pub(crate) running: Check,
    pub(crate) image: Check,
    pub(crate) claude: Check,
    pub(crate) codex: Check,
}

impl Setup {
    /// Docker works, the image exists, and at least one provider is signed in.
    pub(crate) fn done(&self) -> bool {
        [&self.engine, &self.running, &self.image].iter().all(|c| **c == Check::Ok) && (self.claude == Check::Ok || self.codex == Check::Ok)
    }
}

impl Check {
    /// The row after one more check: passed is Ok; a running action or its error stays until the check passes.
    fn after(&self, passed: bool) -> Check {
        match (self, passed) {
            (_, true) => Check::Ok,
            (Check::Busy(_) | Check::Failed(_), false) => self.clone(),
            _ => Check::Missing,
        }
    }
}

/// The error asks the user to sign in to `provider` (Claude says `/login`, Codex says `codex login`).
pub(crate) fn needs_login(error: &str, provider: Provider) -> bool {
    error.contains(if provider == Provider::Codex { "codex login" } else { "/login" })
}

/// Turns the latest "not signed in" error for `provider` into a green notice that can send its prompt again.
fn mark_signed_in(msgs: &mut [Msg], provider: Provider) {
    let Some(k) = msgs.iter().rposition(|m| matches!(m, Msg::Error(t) if needs_login(t, provider))) else { return };
    let prompt = msgs[..k].iter().rev().find_map(|m| match m {
        Msg::User(t) => Some(t.clone()),
        Msg::Handoff { prompt, .. } | Msg::Scheduled { prompt, .. } | Msg::Kickoff { prompt, .. } => Some(prompt.clone()),
        _ => None,
    });
    msgs[k] = Msg::SignedIn { provider, prompt };
}

impl Eggbot {
    /// The login finishes in Terminal; check every 5 s for 5 minutes and tell the user when it works.
    fn watch_sign_in(&mut self, provider: Provider, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..60 {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let ok = cx
                    .background_executor()
                    .spawn(async move {
                        match provider {
                            Provider::Claude => sandbox::claude_signed_in(),
                            Provider::Codex => codex::account().is_ok(),
                        }
                    })
                    .await;
                if ok {
                    this.update(cx, |this, cx| this.signed_in(provider, cx)).ok();
                    return;
                }
            }
        })
        .detach();
    }

    /// Turns each bot's latest "not signed in" error for `provider` into a green notice with "Send again".
    fn signed_in(&mut self, provider: Provider, cx: &mut Context<Self>) {
        for bot in &mut self.bots {
            mark_signed_in(&mut bot.msgs, provider);
        }
        if provider == Provider::Codex {
            self.codex_query = None;
            self.refresh_codex(1, cx);
        }
        self.save();
        cx.notify();
    }

    /// Codex usage and model list, from a throwaway container (no turn needed).
    /// With `tries` > 1 it keeps asking every 5 s, e.g. while the user finishes signing in.
    pub(crate) fn refresh_codex(&mut self, tries: u32, cx: &mut Context<Self>) {
        if self.codex_query == Some(None) {
            return;
        }
        self.codex_query = Some(None);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut answer = Err(String::new());
            for attempt in 0..tries {
                if attempt > 0 {
                    cx.background_executor().timer(Duration::from_secs(5)).await;
                }
                answer = cx.background_executor().spawn(async { codex::account() }).await;
                if answer.is_ok() {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                match answer {
                    Ok((meter, models)) => {
                        this.set_meter(meter, cx);
                        this.codex_models = models;
                        this.codex_query = None;
                        this.selects_stale = true;
                        this.save();
                    }
                    Err(e) => this.codex_query = Some(Some(e)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_setup(&mut self, cx: &mut Context<Self>) {
        if self.setup.is_some() {
            return;
        }
        let unknown = Check::Unknown;
        self.setup = Some(Setup { engine: unknown.clone(), running: unknown.clone(), image: unknown.clone(), claude: unknown.clone(), codex: unknown });
        self.panel = Panel::None;
        cx.notify();
        // re-check every few seconds while the checklist is open; installs and sign-ins finish outside eggbot
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(before)) = this.update(cx, |this, _| this.setup.clone()) else { return };
                let after = cx
                    .background_executor()
                    .spawn(async move {
                        let engine = before.engine.after(sandbox::installed());
                        let running = before.running.after(engine == Check::Ok && sandbox::running());
                        let image = before.image.after(running == Check::Ok && sandbox::image_ready());
                        // sign-in checks start a container, so they stop once they pass
                        let signed = |c: &Check, check: &dyn Fn() -> bool| if *c == Check::Ok { Check::Ok } else { c.after(image == Check::Ok && check()) };
                        let claude = signed(&before.claude, &sandbox::claude_signed_in);
                        let codex = signed(&before.codex, &|| codex::account().is_ok());
                        Setup { engine, running, image, claude, codex }
                    })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.setup.is_some() {
                            this.setup = Some(after);
                            cx.notify()
                        }
                    })
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(4)).await;
            }
        })
        .detach();
    }

    /// Runs a setup action off the main thread; its row shows `label` until the next check, or the error.
    pub(crate) fn setup_action(&mut self, row: fn(&mut Setup) -> &mut Check, label: &'static str, action: fn() -> Result<(), String>, cx: &mut Context<Self>) {
        let Some(setup) = &mut self.setup else { return };
        *row(setup) = Check::Busy(label);
        cx.notify();
        let task = cx.background_executor().spawn(async move { action() });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                this.update(cx, |this, cx| {
                    if let Some(setup) = &mut this.setup {
                        *row(setup) = Check::Failed(e);
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn sign_in(&mut self, codex: bool, cx: &mut Context<Self>) {
        self.watch_sign_in(if codex { Provider::Codex } else { Provider::Claude }, cx);
        let Some(id) = self.bots.get(self.selected).map(|b| b.id) else { return };
        let opening = cx.background_executor().spawn(async move { sandbox::ready(&|_| {}).and_then(|_| sandbox::sign_in(codex)) });
        cx.spawn(async move |this, cx| {
            if let Err(e) = opening.await {
                this.update(cx, |this, cx| {
                    if let Some(b) = this.bot_mut(id) {
                        b.msgs.push(Msg::Error(format!("Could not open the sign-in: {e}")));
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
    use core::prelude::v1::test;

    #[test]
    fn a_setup_row_keeps_its_action_until_the_check_passes() {
        assert!(Check::Unknown.after(true) == Check::Ok);
        assert!(Check::Unknown.after(false) == Check::Missing);
        assert!(Check::Busy("Building…").after(false) == Check::Busy("Building…"));
        assert!(Check::Failed("no brew".into()).after(false) == Check::Failed("no brew".into()));
        assert!(Check::Failed("no brew".into()).after(true) == Check::Ok);
    }

    #[test]
    fn signing_in_turns_the_latest_error_into_send_again() {
        let claude = "Not logged in · Please run /login";
        let mut msgs = vec![
            Msg::User("first".into()),
            Msg::Error(claude.into()),
            Msg::Scheduled { prompt: "nightly".into(), label: "Every day at 09:00".into() },
            Msg::Error(claude.into()),
            Msg::Error(crate::codex::NOT_SIGNED_IN.into()),
        ];
        assert!(needs_login(claude, Provider::Claude) && !needs_login(claude, Provider::Codex));
        mark_signed_in(&mut msgs, Provider::Claude);
        assert!(matches!(&msgs[3], Msg::SignedIn { provider: Provider::Claude, prompt: Some(p) } if p == "nightly"));
        // an older error, and the other provider's, stay as they were
        assert!(matches!(&msgs[1], Msg::Error(_)) && matches!(&msgs[4], Msg::Error(_)));
        let mut unrelated = vec![Msg::Error("boom".into())];
        mark_signed_in(&mut unrelated, Provider::Codex);
        assert!(matches!(&unrelated[0], Msg::Error(_)));
    }
}
