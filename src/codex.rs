//! Runs one Codex turn through `codex app-server` (JSON-RPC over stdio) and turns its notifications into events.

use std::io::{BufRead, BufReader, Lines, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::claude::{Ev, Handle, Meter, Provider, Turn, Window, spawn};
use crate::sandbox;

/// A model this account can use, with the effort levels it supports.
#[derive(Clone)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
    pub default: bool,
}

/// Shown in the chat; the UI offers a "Sign in to Codex" button for errors containing `codex login`.
pub const NOT_SIGNED_IN: &str = "Not signed in to Codex · run codex login";

struct Rpc {
    stdin: Arc<Mutex<ChildStdin>>,
    lines: Lines<BufReader<ChildStdout>>,
    next: u64,
}

impl Rpc {
    fn start(mut cmd: Command) -> Result<(Child, Rpc), String> {
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("Could not run docker: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut rpc = Rpc { stdin, lines, next: 0 };
        rpc.call("initialize", json!({ "clientInfo": { "name": "eggbot", "version": env!("CARGO_PKG_VERSION") } }), &mut |_| {})?;
        write(&rpc.stdin, &json!({ "jsonrpc": "2.0", "method": "initialized" }));
        Ok((child, rpc))
    }

    /// Sends a request and returns its result; notifications that arrive meanwhile go to `note`.
    fn call(&mut self, method: &str, params: Value, note: &mut dyn FnMut(&Value)) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        write(&self.stdin, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        for line in self.lines.by_ref().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v.get("method").is_some() {
                note(&v);
            } else if v["id"] == id {
                return match v.get("error") {
                    Some(e) => Err(e["message"].as_str().unwrap_or("codex error").to_string()),
                    None => Ok(v["result"].clone()),
                };
            }
        }
        Err("codex app-server stopped".into())
    }
}

fn write(stdin: &Mutex<ChildStdin>, msg: &Value) {
    let mut s = stdin.lock().unwrap();
    let _ = writeln!(s, "{msg}");
    let _ = s.flush();
}

/// Runs one Codex turn in the bot's container.
pub fn run(t: Turn) -> (Arc<Handle>, async_channel::Receiver<Ev>) {
    spawn(t.bot, move |h, send| turn(&t, h, send))
}

fn turn(t: &Turn, h: &Handle, send: &dyn Fn(Ev)) -> Result<bool, String> {
    let name = sandbox::ensure(t.bot, &t.mount, &|s| send(Ev::Status(s.to_string())))?;
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-i", &name, "codex", "app-server"]);
    let (child, mut rpc) = Rpc::start(cmd)?;
    *h.child.lock().unwrap() = Some(child);
    let result = converse(t, h, send, &mut rpc);
    if let Some(mut c) = h.child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    result
}

fn converse(t: &Turn, h: &Handle, send: &dyn Fn(Ev), rpc: &mut Rpc) -> Result<bool, String> {
    let mut forward = |v: &Value| parse(v).into_iter().for_each(send);
    if rpc.call("account/read", json!({}), &mut forward)?["account"].is_null() {
        return Err(NOT_SIGNED_IN.into());
    }
    if let Ok(r) = rpc.call("account/rateLimits/read", Value::Null, &mut forward) {
        send(Ev::Usage(meter(&r["rateLimits"])));
    }
    // the container is the sandbox, so Codex runs with full access inside it
    let opts = json!({ "cwd": "/work", "sandbox": "danger-full-access", "approvalPolicy": "never", "developerInstructions": t.role, "model": t.model });
    let resumed = t.session.as_ref().and_then(|id| {
        let mut p = opts.clone();
        p["threadId"] = json!(id);
        rpc.call("thread/resume", p, &mut forward).ok()
    });
    let thread = match resumed {
        Some(r) => r,
        None => rpc.call("thread/start", opts, &mut forward)?,
    };
    let thread_id = thread["thread"]["id"].as_str().unwrap_or_default().to_string();
    send(Ev::Session(thread_id.clone()));
    let started = rpc.call("turn/start", json!({ "threadId": thread_id, "input": [{ "type": "text", "text": t.prompt }], "effort": t.effort }), &mut forward)?;
    let interrupt = json!({ "jsonrpc": "2.0", "id": 0, "method": "turn/interrupt", "params": { "threadId": thread_id, "turnId": started["turn"]["id"] } });
    let stdin = rpc.stdin.clone();
    *h.interrupt.lock().unwrap() = Some(Box::new(move || write(&stdin, &interrupt)));
    for line in rpc.lines.by_ref().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let evs = parse(&v);
        let done = evs.iter().any(|e| matches!(e, Ev::Done { .. }));
        evs.into_iter().for_each(send);
        if done {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Turns one app-server notification into events.
pub fn parse(v: &Value) -> Vec<Ev> {
    let p = &v["params"];
    let item = &p["item"];
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
    match (v["method"].as_str().unwrap_or_default(), item["type"].as_str().unwrap_or_default()) {
        ("item/started", "agentMessage") => vec![Ev::TextStart],
        ("item/agentMessage/delta", _) => vec![Ev::Text(s(&p["delta"]))],
        ("item/started", kind) => match tool(kind, item) {
            Some((name, target)) => vec![Ev::Tool { id: s(&item["id"]), name, target }],
            None => vec![],
        },
        ("item/completed", kind) if tool(kind, item).is_some() => {
            let content = match kind {
                "commandExecution" => s(&item["aggregatedOutput"]),
                "fileChange" => item["changes"].as_array().map(|c| c.iter().map(|c| s(&c["path"])).collect::<Vec<_>>().join("\n")).unwrap_or_default(),
                _ => item.get("error").filter(|e| !e.is_null()).map(|e| e.to_string()).unwrap_or_else(|| "done".into()),
            };
            vec![Ev::ToolResult { id: s(&item["id"]), content: content.chars().take(2000).collect() }]
        }
        ("account/rateLimits/updated", _) => vec![Ev::Usage(meter(&p["rateLimits"]))],
        ("turn/completed", _) => {
            let turn = &p["turn"];
            let error = match turn["status"].as_str() {
                Some("failed") => Some(turn["error"]["message"].as_str().unwrap_or("Codex turn failed").to_string()),
                Some("interrupted") => Some("Codex turn interrupted".into()),
                _ => None,
            };
            vec![Ev::Done { error }]
        }
        _ => vec![],
    }
}

/// Tool-log name and target for the item kinds worth showing.
fn tool(kind: &str, item: &Value) -> Option<(String, String)> {
    let s = |k: &str| item[k].as_str().unwrap_or_default().to_string();
    Some(match kind {
        "commandExecution" => ("Shell".into(), unwrap_shell(&s("command"))),
        "fileChange" => ("Edit".into(), item["changes"][0]["path"].as_str().map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).unwrap_or_default()),
        "mcpToolCall" => ("Tool".into(), s("tool")),
        "dynamicToolCall" => ("Tool".into(), s("tool")),
        "webSearch" => ("Search".into(), s("query")),
        _ => return None,
    })
}

/// `/bin/zsh -lc 'cat note.txt'` → `cat note.txt`.
fn unwrap_shell(cmd: &str) -> String {
    match cmd.split_once(" -lc ") {
        Some((_, inner)) => inner.trim_matches('\'').to_string(),
        None => cmd.to_string(),
    }
}

fn meter(r: &Value) -> Meter {
    let windows = ["primary", "secondary"]
        .iter()
        .filter_map(|k| {
            let w = r.get(*k).filter(|w| !w.is_null())?;
            let mins = w["windowDurationMins"].as_i64().unwrap_or(0);
            let label = match mins {
                10080 => "week".to_string(),
                m if m > 0 && m % 1440 == 0 => format!("{}d", m / 1440),
                m if m > 0 && m % 60 == 0 => format!("{}h", m / 60),
                m => format!("{m}m"),
            };
            Some(Window { label, used: w["usedPercent"].as_f64().unwrap_or(0.) as f32 / 100., reset: w["resetsAt"].as_i64().unwrap_or(0) })
        })
        .collect();
    Meter { provider: Provider::Codex, windows, at: chrono::Local::now().timestamp() }
}

/// Plan usage and the models this account can use, from a throwaway container.
pub fn account() -> Result<(Meter, Vec<Model>), String> {
    sandbox::ready(&|_| {})?;
    let (mut child, mut rpc) = Rpc::start(sandbox::codex_oneshot())?;
    let result = (|| {
        if rpc.call("account/read", json!({}), &mut |_| {})?["account"].is_null() {
            return Err(NOT_SIGNED_IN.to_string());
        }
        let limits = rpc.call("account/rateLimits/read", Value::Null, &mut |_| {})?;
        let models = rpc.call("model/list", json!({}), &mut |_| {})?;
        let models = models["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["hidden"] != true)
            .map(|m| Model {
                id: m["id"].as_str().unwrap_or_default().to_string(),
                name: m["displayName"].as_str().unwrap_or_default().to_string(),
                efforts: m["supportedReasoningEfforts"].as_array().into_iter().flatten().filter_map(|e| e["reasoningEffort"].as_str().map(str::to_string)).collect(),
                default: m["isDefault"] == true,
            })
            .collect();
        Ok((meter(&limits["rateLimits"]), models))
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_notifications() {
        let lines = [
            r#"{"method":"item/started","params":{"item":{"type":"agentMessage","id":"m1","text":"","phase":"commentary"}}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"I’ll"}}"#,
            r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"e1","command":"/bin/zsh -lc 'cat note.txt'","aggregatedOutput":""}}}"#,
            r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"e1","command":"/bin/zsh -lc 'cat note.txt'","aggregatedOutput":"hello from probe\n","exitCode":0}}}"#,
            r#"{"method":"item/started","params":{"item":{"type":"reasoning","id":"r1"}}}"#,
            r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":10080,"resetsAt":1791188704},"secondary":{"usedPercent":40,"windowDurationMins":300,"resetsAt":5}}}}"#,
            r#"{"method":"turn/completed","params":{"turn":{"id":"t1","status":"failed","error":{"message":"boom"}}}}"#,
        ];
        let mut evs: Vec<Ev> = lines.iter().flat_map(|l| parse(&serde_json::from_str(l).unwrap())).collect();
        for e in &mut evs {
            if let Ev::Usage(m) = e {
                m.at = 0;
            }
        }
        let window = |label: &str, used, reset| Window { label: label.into(), used, reset };
        assert_eq!(
            evs,
            vec![
                Ev::TextStart,
                Ev::Text("I’ll".into()),
                Ev::Tool { id: "e1".into(), name: "Shell".into(), target: "cat note.txt".into() },
                Ev::ToolResult { id: "e1".into(), content: "hello from probe\n".into() },
                Ev::Usage(Meter { provider: Provider::Codex, windows: vec![window("week", 0.12, 1791188704), window("5h", 0.4, 5)], at: 0 }),
                Ev::Done { error: Some("boom".into()) },
            ]
        );
    }
}
