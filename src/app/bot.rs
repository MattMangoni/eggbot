//! The bot model: presets, chat messages, the saved bot, and hatching, deleting, reordering, and mounting folders.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::{Eggbot, Panel};
use crate::app::state::data_dir;
use crate::claude::Provider;
use crate::egg::Mood;
use crate::ui::composer::parse_model_value;
use crate::{claude, group, handoff, room, sandbox, schedule, skills};

pub(crate) struct Preset {
    pub(crate) name: &'static str,
    pub(crate) blurb: &'static str,
    pub(crate) color: u32,
    pub(crate) role: &'static str,
}

pub(crate) const PRESETS: [Preset; 4] = [
    Preset {
        name: "Reviewer",
        blurb: "Reads diffs, finds bugs, weighs risk",
        color: 0xF5C6A5,
        role: "You are Reviewer, a code reviewer living in eggbot. Read code and diffs carefully. Find bugs, security issues and risky changes. For each finding give the file, the line, why it matters and a concrete fix, ranked by severity. Do not edit files unless asked.",
    },
    Preset {
        name: "Implementer",
        blurb: "Writes and changes code",
        color: 0xC6DDB8,
        role: "You are Implementer, a software engineer living in eggbot. Write and change code with the smallest correct diff. Match the existing style, reuse what exists, and explain briefly what you changed.",
    },
    Preset {
        name: "Designer",
        blurb: "UI and UX critique and polish",
        color: 0xD6CAF0,
        role: "You are Designer, a UI and UX specialist living in eggbot. Critique and improve hierarchy, spacing, typography, color, motion, accessibility and interaction states. Give concrete, actionable suggestions.",
    },
    Preset { name: "Custom", blurb: "A blank bot you shape yourself", color: 0xF3DF9C, role: "You are a helpful bot living in eggbot." },
];

pub(crate) const SHELLS: [u32; 8] = [0xF5C6A5, 0xC6DDB8, 0xD6CAF0, 0xF3DF9C, 0xB9D8EA, 0xF2B8C6, 0xCFE3D8, 0xE3D2B9];

#[derive(Serialize, Deserialize)]
pub(crate) enum Msg {
    User(String),
    Bot(String),
    Tool {
        id: String,
        verb: String,
        target: String,
        detail: String,
        open: bool,
    },
    Error(String),
    /// Work handed over by another bot; `paused` when the chain hit the hop limit.
    Handoff {
        from: String,
        color: u32,
        prompt: String,
        text: String,
        paused: bool,
        open: bool,
        /// Set when this hop stays inside a room. Continue chain keeps it on the next turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        room: Option<usize>,
    },
    Sent {
        to: String,
    },
    /// Marks where a fresh session began.
    Divider(String),
    /// A turn started by a schedule, shown where a user message would be.
    Scheduled {
        prompt: String,
        label: String,
    },
    /// Replaces a "not signed in" error once the login works; `prompt` is what failed, for "Send again".
    SignedIn {
        provider: Provider,
        prompt: Option<String>,
    },
    /// A room kickoff, shown in the facilitator's chat. `prompt` is what the bot was told.
    Kickoff {
        room_id: usize,
        room: String,
        text: String,
        prompt: String,
    },
}

/// Bot text written at and after `from`. Earlier lines belong to a previous turn.
pub(crate) fn reply_text(msgs: &[Msg], from: usize) -> String {
    let from = from.min(msgs.len());
    msgs[from..].iter().filter_map(|m| if let Msg::Bot(t) = m { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n\n")
}

/// `base`, or `base 2`, `base 3`… The first one `taken` does not claim. Bots, rooms, and groups share this rule.
pub(crate) fn unique_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..).map(|i| if i == 1 { base.to_string() } else { format!("{base} {i}") }).find(|n| !taken(n)).unwrap()
}

/// Where a bot dragged from row `from` lands when dropped above row `before` (`before` = len: the end).
/// None when nothing would change: onto itself, just below itself, or out of range.
pub(crate) fn drop_index(len: usize, from: usize, before: usize) -> Option<usize> {
    if from >= len || before > len || before == from || before == from + 1 {
        return None;
    }
    Some(if before > from { before - 1 } else { before })
}

impl Msg {
    /// The text search looks in: what people and bots wrote, not tool output.
    pub(crate) fn searchable(&self) -> Option<&str> {
        match self {
            Msg::User(t) | Msg::Bot(t) | Msg::Handoff { text: t, .. } | Msg::Scheduled { prompt: t, .. } | Msg::Kickoff { text: t, .. } => Some(t),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Bot {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) preset: usize,
    /// User-picked folders, each mounted at `/work/<name>`.
    #[serde(default)]
    pub(crate) folders: Vec<sandbox::Mount>,
    /// Single folder from before multi-mount. Folded into `folders` on load.
    #[serde(default, skip_serializing)]
    pub(crate) folder: Option<PathBuf>,
    // renamed when bots moved into containers: host sessions cannot resume there
    #[serde(rename = "sandbox_session")]
    pub(crate) session: Option<String>,
    #[serde(default)]
    pub(crate) provider: Provider,
    /// Codex thread id, kept apart from the Claude session so switching provider loses neither.
    #[serde(default)]
    pub(crate) thread: Option<String>,
    pub(crate) msgs: Vec<Msg>,
    #[serde(default)]
    pub(crate) schedules: Vec<schedule::Schedule>,
    /// Edits made in the bot editor; None = the preset's value.
    #[serde(default)]
    pub(crate) role: Option<String>,
    #[serde(default)]
    pub(crate) color: Option<u32>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
    /// Copied from the preset at hatch, then owned by this bot.
    #[serde(default)]
    pub(crate) skills: Vec<skills::Skill>,
    #[serde(skip)]
    pub(crate) run: Option<Arc<claude::Handle>>,
    #[serde(skip)]
    pub(crate) status: Option<String>,
    #[serde(skip)]
    pub(crate) stopped: bool,
    /// Handoff hops that led to the current turn (0 = started by the user).
    #[serde(skip)]
    pub(crate) hops: u32,
    /// `msgs` index where the running turn's output starts. A queued kickoff is not always the last marker.
    #[serde(skip)]
    pub(crate) reply_from: usize,
    /// Turns waiting for this one to finish (handoffs, and schedules that arrived mid-turn).
    #[serde(default)]
    pub(crate) queue: Vec<handoff::Pending>,
    /// The running turn uses a throwaway session (schedules): its session id and context are not kept.
    #[serde(skip)]
    pub(crate) fresh_turn: bool,
    /// The running turn saves notes before a fresh start; on success the session is dropped.
    #[serde(skip)]
    pub(crate) refreshing: bool,
    /// Codex: the role last delivered to the current thread; a different role is sent again once.
    #[serde(default)]
    pub(crate) codex_role: Option<String>,
    /// Role sent with the running turn; becomes `codex_role` when the turn succeeds.
    #[serde(skip)]
    pub(crate) pending_role: Option<String>,
    /// Tokens in the main session's context and the model's window (0 = unknown).
    #[serde(default)]
    pub(crate) context: (u64, u64),
    /// Something arrived that the user has not seen yet.
    #[serde(default)]
    pub(crate) unread: bool,
    /// The running turn. Retried after a sign-in clash, and restored when it is a handoff.
    #[serde(default)]
    pub(crate) current: Option<handoff::Pending>,
    /// The running turn is already that one retry.
    #[serde(skip)]
    pub(crate) retried: bool,
    /// Newest-first names this bot successfully handed work to. A paused chain is not recorded.
    #[serde(default)]
    pub(crate) recent: Vec<String>,
    /// Composer text kept while another bot is selected. Not saved across restarts.
    #[serde(skip)]
    pub(crate) draft: String,
}

impl Bot {
    pub(crate) fn preset(&self) -> &'static Preset {
        &PRESETS[self.preset.min(PRESETS.len() - 1)]
    }

    pub(crate) fn color(&self) -> u32 {
        self.color.unwrap_or(self.preset().color)
    }

    pub(crate) fn role(&self) -> &str {
        self.role.as_deref().unwrap_or(self.preset().role)
    }

    /// Sidebar subtitle: the preset blurb, or the start of an edited role.
    pub(crate) fn blurb(&self) -> String {
        match &self.role {
            Some(r) => r.lines().next().unwrap_or_default().chars().take(60).collect(),
            None => self.preset().blurb.to_string(),
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.run.is_some()
    }

    pub(crate) fn mood(&self) -> Mood {
        if self.busy() { Mood::Thinking } else { Mood::Still }
    }

    /// True while the bot works but is not writing text (thinking or running a tool).
    pub(crate) fn waiting(&self) -> bool {
        self.busy() && !matches!(self.msgs.last(), Some(Msg::Bot(_)))
    }
}

impl Eggbot {
    pub(crate) fn hatch(&mut self, preset: usize) {
        self.leave_room();
        self.leave_group();
        self.panel = Panel::None;
        self.skill_at = None;
        let base = PRESETS[preset].name;
        let name = unique_name(base, |n| self.bots.iter().any(|b| b.name == n));
        self.bots.push(Bot {
            id: self.next_id,
            name,
            preset,
            // copied by preset name, so this bot keeps its own list after hatch
            skills: skills::defaults(PRESETS[preset].name),
            ..Bot::default()
        });
        self.next_id += 1;
        self.selected = self.bots.len() - 1;
        self.selects_stale = true;
        self.save();
    }

    /// Moves bot `from` to just above row `before` (`before` = len: the end); the selection stays on the same bot.
    pub(crate) fn move_bot(&mut self, from: usize, before: usize, cx: &mut Context<Self>) {
        self.dragging = None;
        cx.notify();
        let Some(at) = drop_index(self.bots.len(), from, before) else { return };
        let selected = self.bots[self.selected].id;
        let bot = self.bots.remove(from);
        self.bots.insert(at, bot);
        self.selected = self.bots.iter().position(|b| b.id == selected).unwrap_or(0);
        self.save();
        cx.notify();
    }

    /// Deletes the bot, its container and its scratch folder; never a mounted folder.
    pub(crate) fn delete(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(i) = self.bots.iter().position(|b| b.id == id) else { return };
        if let Some(run) = &self.bots[i].run {
            run.stop();
        }
        self.bots.remove(i);
        for room in &mut self.rooms {
            (room.members, room.facilitator) = room::forget(std::mem::take(&mut room.members), room.facilitator, id);
        }
        for group in &mut self.groups {
            group.members = group::forget(std::mem::take(&mut group.members), id);
        }
        self.selected = self.selected.min(self.bots.len().saturating_sub(1));
        self.confirm_delete = None;
        self.save();
        let scratch = data_dir().join("bots").join(id.to_string());
        cx.background_executor()
            .spawn(async move {
                sandbox::remove(id);
                let _ = std::fs::remove_dir_all(scratch);
            })
            .detach();
        cx.notify();
    }

    pub(crate) fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let Some(bot) = self.bots.get(self.selected) else { return };
        if bot.folders.len() >= sandbox::MAX_MOUNTS {
            self.folder_error = Some(format!("A bot can mount at most {} folders.", sandbox::MAX_MOUNTS));
            cx.notify();
            return;
        }
        let id = bot.id;
        let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Mount folders".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            if paths.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                let show = this.bots.get(this.selected).is_some_and(|b| b.id == id);
                if let Some(b) = this.bot_mut(id) {
                    let err = sandbox::add_mounts(&mut b.folders, &paths).err();
                    if show {
                        this.folder_error = err;
                    }
                }
                this.save();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn remove_folder(&mut self, id: usize, index: usize, cx: &mut Context<Self>) {
        if let Some(bot) = self.bot_mut(id)
            && index < bot.folders.len()
        {
            bot.folders.remove(index);
        }
        self.folder_error = None;
        self.save();
        cx.notify();
    }

    /// The composer's model dropdown changed. Effort levels differ per model, so effort goes back to the default.
    pub(crate) fn pick_model(&mut self, value: &str, cx: &mut Context<Self>) {
        let (provider, model) = parse_model_value(value);
        let Some(b) = self.bots.get_mut(self.selected) else { return };
        if b.provider != provider {
            b.provider = provider;
            // the meter tracks the provider's session; it refills on the next turn
            b.context = (0, 0);
        }
        b.model = model.map(str::to_string);
        b.effort = None;
        if provider == Provider::Codex && self.codex_models.is_empty() {
            self.refresh_codex(1, cx);
        }
        self.selects_stale = true;
        self.save();
        cx.notify();
    }

    pub(crate) fn pick_effort(&mut self, effort: Option<String>, cx: &mut Context<Self>) {
        if let Some(b) = self.bots.get_mut(self.selected) {
            b.effort = effort;
            self.save();
            cx.notify();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // `gpui_kit::*` also exports GPUI's own `test` macro; keep the standard one
    use core::prelude::v1::test;

    #[test]
    fn reply_text_ignores_lines_from_before_the_turn() {
        let msgs = vec![
            Msg::Bot("earlier".into()),
            Msg::Kickoff { room_id: 1, room: "Standup".into(), text: "go".into(), prompt: "prompt".into() },
            Msg::Bot("partial".into()),
            Msg::User("meanwhile".into()),
            Msg::Bot("other".into()),
        ];
        assert_eq!(reply_text(&msgs, 4), "other");
        assert_eq!(reply_text(&msgs, msgs.len()), "");
    }

    #[test]
    fn a_new_name_skips_the_taken_ones() {
        assert_eq!(unique_name("Room", |_| false), "Room");
        let taken = ["Reviewer", "Reviewer 2"];
        assert_eq!(unique_name("Reviewer", |n| taken.contains(&n)), "Reviewer 3");
        assert_eq!(unique_name("Group", |n| n == "Group 2"), "Group");
    }

    #[test]
    fn a_dropped_bot_lands_above_the_row_under_the_pointer() {
        assert_eq!(drop_index(3, 0, 2), Some(1));
        assert_eq!(drop_index(3, 2, 0), Some(0));
        // the space below the list moves it to the end
        assert_eq!(drop_index(3, 0, 3), Some(2));
        assert_eq!(drop_index(3, 1, 1), None);
        assert_eq!(drop_index(3, 1, 2), None);
        assert_eq!(drop_index(3, 3, 0), None);
    }
}
