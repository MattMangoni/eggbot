//! Runs one turn of the official `claude` CLI and turns its stream-json output into events.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::sandbox;

#[derive(Debug, PartialEq)]
pub enum Ev {
    /// Sandbox progress before claude starts (waking Colima, building the image…).
    Status(String),
    Session(String),
    TextStart,
    Text(String),
    Tool { id: String, name: String, target: String },
    ToolResult { id: String, content: String },
    Usage { five_hour: f32, seven_day: f32 },
    Done { error: Option<String> },
}

/// A running turn; `stop` ends it inside the container too.
pub struct Handle {
    bot: usize,
    child: Mutex<Option<Child>>,
}

impl Handle {
    pub fn stop(self: &Arc<Self>) {
        let this = self.clone();
        // docker calls block; keep them off the UI thread
        std::thread::spawn(move || {
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
    pub prompt: String,
    pub role: String,
    pub session: Option<String>,
}

/// Runs one `claude -p` turn in the bot's container; events arrive on the channel, which closes at the end.
pub fn run(t: Turn) -> (Arc<Handle>, async_channel::Receiver<Ev>) {
    let handle = Arc::new(Handle { bot: t.bot, child: Mutex::new(None) });
    let (tx, rx) = async_channel::unbounded();
    let h = handle.clone();
    std::thread::spawn(move || {
        let send = |ev| drop(tx.send_blocking(ev));
        let error = match turn(&t, &h, &send) {
            Ok(true) => return,
            Ok(false) => "claude ended without a result".to_string(),
            Err(e) => e,
        };
        send(Ev::Done { error: Some(error) });
    });
    (handle, rx)
}

/// Ok(true) when claude reported its own result (success or error).
fn turn(t: &Turn, h: &Handle, send: &dyn Fn(Ev)) -> Result<bool, String> {
    let name = sandbox::ensure(t.bot, &t.mount, &|s| send(Ev::Status(s.to_string())))?;
    let mut cmd = Command::new("docker");
    cmd.args(["exec", &name, "claude", "-p", &t.prompt, "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
        .args(["--append-system-prompt", &t.role])
        // clean bots with full power inside their own machine; no tools that reach outside it
        .args(["--setting-sources", "project,local", "--strict-mcp-config", "--permission-mode", "bypassPermissions"])
        .args(["--disallowedTools", "RemoteTrigger,CronCreate,CronDelete,ScheduleWakeup,PushNotification"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(s) = &t.session {
        cmd.args(["--resume", s]);
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
            let f = |k: &str| w[k]["utilization"].as_f64().unwrap_or(0.) as f32;
            vec![Ev::Usage { five_hour: f("five_hour"), seven_day: f("seven_day") }]
        }
        "result" => {
            let error = (v["is_error"] == true).then(|| s("/result"));
            vec![Ev::Session(s("/session_id")), Ev::Done { error }]
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
            r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.03},"seven_day":{"utilization":0.5}}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hi"}}}"#,
            r#"{"type":"result","is_error":false,"result":"Hi","session_id":"abc"}"#,
            "not json",
        ];
        let evs: Vec<Ev> = lines.iter().flat_map(|l| parse(l)).collect();
        assert_eq!(
            evs,
            vec![
                Ev::Session("abc".into()),
                Ev::Tool { id: "t1".into(), name: "Read".into(), target: "note.txt".into() },
                Ev::ToolResult { id: "t1".into(), content: "1\thello".into() },
                Ev::Usage { five_hour: 0.03, seven_day: 0.5 },
                Ev::TextStart,
                Ev::Text("Hi".into()),
                Ev::Session("abc".into()),
                Ev::Done { error: None },
            ]
        );
    }
}
