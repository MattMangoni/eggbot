//! Runs one turn of the official `claude` CLI and turns its stream-json output into events.
//! Also holds the types both providers share (see `codex.rs`).

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sandbox;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Provider {
    #[default]
    Claude,
    Codex,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }
}

/// Plan usage for one provider: named windows (e.g. "5h", "week") with 0..1 used and reset time.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Meter {
    pub provider: Provider,
    pub windows: Vec<Window>,
    /// Unix seconds when this was reported.
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Window {
    pub label: String,
    pub used: f32,
    pub reset: i64,
}

#[derive(Debug, PartialEq)]
pub enum Ev {
    /// Sandbox progress before the agent starts (starting Docker, building the image…).
    Status(String),
    Session(String),
    TextStart,
    Text(String),
    Tool { id: String, name: String, target: String },
    ToolResult { id: String, content: String },
    Usage(Meter),
    /// Tokens in the session's context now, and/or the model's context window.
    Context { used: Option<u64>, window: Option<u64> },
    Done { error: Option<String> },
}

type Interrupt = Box<dyn FnOnce() + Send>;

/// A running turn; `stop` ends it inside the container too.
pub struct Handle {
    bot: usize,
    pub(crate) child: Mutex<Option<Child>>,
    /// Graceful stop (Codex `turn/interrupt`); without it the process is killed.
    pub(crate) interrupt: Mutex<Option<Interrupt>>,
}

impl Handle {
    pub fn stop(self: &Arc<Self>) {
        let this = self.clone();
        // docker calls block; keep them off the UI thread
        std::thread::spawn(move || {
            if let Some(graceful) = this.interrupt.lock().unwrap().take() {
                return graceful();
            }
            sandbox::interrupt(this.bot);
            if let Some(c) = this.child.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
        });
    }
}

pub struct Turn {
    pub bot: usize,
    pub mount: PathBuf,
    /// The bot's own folder for NOTES.md, mounted at /memory.
    pub memory: PathBuf,
    pub prompt: String,
    pub role: String,
    /// Claude session id or Codex thread id to continue.
    pub session: Option<String>,
    /// Model alias or id; None = the provider's default.
    pub model: Option<String>,
    /// Reasoning effort level; None = the provider's default.
    pub effort: Option<String>,
    /// Codex only: send `role` with this turn (new thread, or the role changed since it was last sent).
    pub send_role: bool,
}

/// Runs `turn` on a thread; events arrive on the channel, which closes at the end.
/// `turn` returns Ok(true) when the agent reported its own end (success or error).
pub fn spawn(bot: usize, turn: impl FnOnce(&Handle, &dyn Fn(Ev)) -> Result<bool, String> + Send + 'static) -> (Arc<Handle>, async_channel::Receiver<Ev>) {
    let handle = Arc::new(Handle { bot, child: Mutex::new(None), interrupt: Mutex::new(None) });
    let (tx, rx) = async_channel::unbounded();
    let h = handle.clone();
    std::thread::spawn(move || {
        let send = |ev| drop(tx.send_blocking(ev));
        let error = match turn(&h, &send) {
            Ok(true) => return,
            Ok(false) => "the agent ended without a result".to_string(),
            Err(e) => e,
        };
        send(Ev::Done { error: Some(error) });
    });
    (handle, rx)
}

/// Runs one `claude -p` turn in the bot's container.
pub fn run(t: Turn) -> (Arc<Handle>, async_channel::Receiver<Ev>) {
    spawn(t.bot, move |h, send| turn(&t, h, send))
}

/// Ok(true) when claude reported its own result (success or error).
fn turn(t: &Turn, h: &Handle, send: &dyn Fn(Ev)) -> Result<bool, String> {
    let name = sandbox::ensure(t.bot, &t.mount, &t.memory, &|s| send(Ev::Status(s.to_string())))?;
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-e", &sandbox::tz(), &name, "claude", "-p", &t.prompt, "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
        .args(["--append-system-prompt", &t.role])
        // clean bots with full power inside their own machine; no tools that reach outside it
        .args(["--setting-sources", "project,local", "--strict-mcp-config", "--permission-mode", "bypassPermissions"])
        // without this a resumed session keeps the role it started with, ignoring edits
        .args(["--system-prompt-snapshot", "off"])
        .args(["--disallowedTools", "RemoteTrigger,CronCreate,CronDelete,ScheduleWakeup,PushNotification"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(s) = &t.session {
        cmd.args(["--resume", s]);
    }
    if let Some(m) = &t.model {
        cmd.args(["--model", m]);
    }
    if let Some(e) = &t.effort {
        cmd.args(["--effort", e]);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Could not run docker: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    *h.child.lock().unwrap() = Some(child);

    let mut done = false;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        for ev in parse(&line) {
            done |= matches!(ev, Ev::Done { .. });
            send(ev);
        }
    }
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    let status = h.child.lock().unwrap().take().map(|mut c| c.wait());
    match status {
        _ if done => Ok(true),
        Some(Ok(s)) if !s.success() => Err(format!("claude stopped ({s}). {}", err.trim())),
        _ => Ok(false),
    }
}

pub fn parse(line: &str) -> Vec<Ev> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    let s = |p: &str| v.pointer(p).and_then(Value::as_str).unwrap_or_default().to_string();
    match v["type"].as_str().unwrap_or_default() {
        "system" if v["subtype"] == "init" => vec![Ev::Session(s("/session_id"))],
        "stream_event" => match v["event"]["type"].as_str().unwrap_or_default() {
            "content_block_start" if v["event"]["content_block"]["type"] == "text" => vec![Ev::TextStart],
            "content_block_delta" if v["event"]["delta"]["type"] == "text_delta" => vec![Ev::Text(s("/event/delta/text"))],
            // the last request's full input + output is what the session occupies now
            "message_delta" => {
                let u = &v["event"]["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                vec![Ev::Context { used: Some(n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + n("output_tokens")), window: None }]
            }
            _ => vec![],
        },
        "assistant" => blocks(&v)
            .filter(|b| b["type"] == "tool_use")
            .map(|b| Ev::Tool { id: str_of(&b["id"]), name: str_of(&b["name"]), target: target(&b["input"]) })
            .collect(),
        "user" => blocks(&v)
            .filter(|b| b["type"] == "tool_result")
            .map(|b| Ev::ToolResult { id: str_of(&b["tool_use_id"]), content: result_text(&b["content"]) })
            .collect(),
        "rate_limit_event" => {
            let w = &v["rate_limit_info"]["unifiedWindows"];
            let window = |label: &str, k: &str| Window { label: label.into(), used: w[k]["utilization"].as_f64().unwrap_or(0.) as f32, reset: w[k]["resetsAt"].as_i64().unwrap_or(0) };
            vec![Ev::Usage(Meter { provider: Provider::Claude, windows: vec![window("5h", "five_hour"), window("7d", "seven_day")], at: chrono::Local::now().timestamp() })]
        }
        "result" => {
            let error = (v["is_error"] == true).then(|| s("/result"));
            let window = v["modelUsage"].as_object().and_then(|m| m.values().filter_map(|u| u["contextWindow"].as_u64()).max());
            let mut evs = vec![Ev::Session(s("/session_id"))];
            if window.is_some() {
                evs.push(Ev::Context { used: None, window });
            }
            evs.push(Ev::Done { error });
            evs
        }
        _ => vec![],
    }
}

fn blocks(v: &Value) -> impl Iterator<Item = &Value> {
    v["message"]["content"].as_array().into_iter().flatten()
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// The one input field worth showing next to the tool name.
fn target(input: &Value) -> String {
    ["file_path", "path", "pattern", "command", "url", "query", "description"]
        .iter()
        .find_map(|k| input[k].as_str())
        .map(|t| t.rsplit('/').next().filter(|_| t.starts_with('/')).unwrap_or(t).to_string())
        .unwrap_or_default()
}

fn result_text(c: &Value) -> String {
    let text = match c {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    // ponytail: hard cut keeps huge file dumps out of the chat; add "show more" if it matters
    text.chars().take(2000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_turn() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"abc"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/x/note.txt"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"tool_use_id":"t1","type":"tool_result","content":"1\thello"}]}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.03,"resetsAt":100},"seven_day":{"utilization":0.5,"resetsAt":200}}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hi"}}}"#,
            r#"{"type":"result","is_error":false,"result":"Hi","session_id":"abc"}"#,
            "not json",
        ];
        let mut evs: Vec<Ev> = lines.iter().flat_map(|l| parse(l)).collect();
        for e in &mut evs {
            if let Ev::Usage(m) = e {
                m.at = 0;
            }
        }
        let window = |label: &str, used, reset| Window { label: label.into(), used, reset };
        let usage = Ev::Usage(Meter { provider: Provider::Claude, windows: vec![window("5h", 0.03, 100), window("7d", 0.5, 200)], at: 0 });
        assert_eq!(
            evs,
            vec![
                Ev::Session("abc".into()),
                Ev::Tool { id: "t1".into(), name: "Read".into(), target: "note.txt".into() },
                Ev::ToolResult { id: "t1".into(), content: "1\thello".into() },
                usage,
                Ev::TextStart,
                Ev::Text("Hi".into()),
                Ev::Session("abc".into()),
                Ev::Done { error: None },
            ]
        );
    }
}
