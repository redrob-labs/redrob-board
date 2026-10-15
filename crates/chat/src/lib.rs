//! Chat with the shared Redrob Code engine, which drives the open design through tools.
//!
//! The app declares `board_agent::workspace::tool_specs()` on `/v1/chat/completions`, so the
//! engine never executes anything itself: it answers with `tool_calls`, the app runs them on its
//! own [`Workspace`] (the UI thread owns that state) and sends the results back as `tool` messages
//! until the engine answers with text. HTTP runs on a background thread; the UI polls events.
//!
//! The engine is reached only through `redrob-engine-client` (the daemon's record, its Basic
//! secret). There is no second path to a provider: without the engine the palette says so and
//! points at the launcher.

#![forbid(unsafe_code)]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use board_agent::workspace::{ToolSpec, Workspace, tool_specs};
pub use redrob_engine_client::{Record, Unavailable};

/// The app name the daemon groups our requests under.
pub const APP: &str = "board";
/// `redrob/auto` routes through the Console; a local provider id would target a local model.
pub const MODEL: &str = "redrob/auto";
/// Tool rounds per user turn before the loop stops and says so.
pub const MAX_TOOL_ROUNDS: usize = 12;

pub const SYSTEM_PROMPT: &str = "You are the agent inside Redrob Board, a PCB design studio. You drive the open design with the \
tools: describe, list_examples, catalog, write_schematic, load_example, place, route, drc, export_fab. For a new board: look up \
parts with catalog (by role), write_schematic with the MCU, sensors, power and extras the user asked for (defaults are sane), then \
place on the outline they want (default 30 x 25 mm; connectors and buttons on an edge), route, run drc, and export when asked. \
Call describe first when you do not know the state. Report numbers from tool results, never guess them. Answer in the user's \
language, briefly.";

/// Connect to the running engine, starting it when installed but not up.
pub fn connect() -> Result<Record, Unavailable> {
    redrob_engine_client::require_as(APP, 10)
}

/// One message in the OpenAI shape.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(text: &str) -> Message {
        Message { role: "system".into(), content: Some(text.into()), tool_calls: Vec::new(), tool_call_id: None }
    }
    pub fn user(text: &str) -> Message {
        Message { role: "user".into(), content: Some(text.into()), tool_calls: Vec::new(), tool_call_id: None }
    }
    pub fn tool(id: &str, content: String) -> Message {
        Message { role: "tool".into(), content: Some(content), tool_calls: Vec::new(), tool_call_id: Some(id.into()) }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_type() -> String {
    "function".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    /// JSON text, as OpenAI sends it.
    pub arguments: String,
}

#[derive(Deserialize)]
struct Completion {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<ApiError>,
}
#[derive(Deserialize)]
struct Choice {
    message: Message,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct ApiError {
    message: String,
}

/// What one completion gave back.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

/// A client for one engine. `base` is the OpenAI base (`<url>/v1`).
#[derive(Clone, Debug)]
pub struct ChatClient {
    base: String,
    headers: Vec<(&'static str, String)>,
    timeout: Duration,
}

impl ChatClient {
    pub fn for_engine(record: &Record, session: &str) -> ChatClient {
        ChatClient {
            base: redrob_engine_client::openai_base(record),
            headers: redrob_engine_client::app_headers(record, APP, Some(session)),
            timeout: Duration::from_secs(120),
        }
    }

    /// A client for any OpenAI-compatible base with fixed headers (tests use a local server).
    pub fn new(base: String, headers: Vec<(&'static str, String)>) -> ChatClient {
        ChatClient { base, headers, timeout: Duration::from_secs(120) }
    }

    pub fn complete(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<Reply, String> {
        let body = json!({
            "model": MODEL,
            "messages": messages,
            "tools": tools.iter().map(ToolSpec::to_openai).collect::<Vec<_>>(),
            "tool_choice": "auto",
            "stream": false,
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(self.timeout)).http_status_as_error(false).build().into();
        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));
        let mut req = agent.post(&url).header("Content-Type", "application/json");
        for (k, v) in &self.headers {
            req = req.header(*k, v);
        }
        let mut response = req.send(body.to_string().as_bytes()).map_err(|e| format!("engine unreachable: {e}"))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
        parse_reply(status, &text)
    }
}

/// Parse one chat completion into text or tool calls.
pub fn parse_reply(status: u16, text: &str) -> Result<Reply, String> {
    let c: Completion = serde_json::from_str(text).map_err(|e| format!("engine answered {status} with a body that is not a completion: {e}"))?;
    if let Some(e) = c.error {
        return Err(format!("engine error ({status}): {}", e.message));
    }
    if status != 200 {
        return Err(format!("engine answered {status}"));
    }
    let choice = c.choices.into_iter().next().ok_or("engine answered with no choices")?;
    let m = choice.message;
    if !m.tool_calls.is_empty() || choice.finish_reason.as_deref() == Some("tool_calls") {
        return Ok(Reply::ToolCalls(m.tool_calls));
    }
    Ok(Reply::Text(m.content.unwrap_or_default()))
}

/// What the UI sees from the agent thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The engine wants these run on the workspace; answer with [`Agent::tool_results`].
    ToolCalls(Vec<ToolCall>),
    /// Final text of the turn.
    Text(String),
    /// The turn ended with an error (the engine, the network, or the round cap).
    Error(String),
    /// The turn is over (after `Text` or `Error`).
    Done,
}

enum Inbound {
    Prompt(String),
    ToolResults(Vec<(String, String)>),
}

/// The agent: a thread that owns the conversation and talks to the engine.
pub struct Agent {
    to_thread: Sender<Inbound>,
    from_thread: Receiver<Event>,
    pub busy: bool,
}

impl Agent {
    pub fn spawn(client: ChatClient) -> Agent {
        let (to_thread, inbox) = channel::<Inbound>();
        let (outbox, from_thread) = channel::<Event>();
        thread::Builder::new()
            .name("board-chat".into())
            .spawn(move || run(client, &inbox, &outbox))
            .map_err(|e| e.to_string())
            .expect("spawn chat thread");
        Agent { to_thread, from_thread, busy: false }
    }

    pub fn prompt(&mut self, text: &str) {
        self.busy = true;
        let _ = self.to_thread.send(Inbound::Prompt(text.to_string()));
    }

    /// Answer the last `Event::ToolCalls`: `(tool_call_id, result json text)` per call.
    pub fn tool_results(&mut self, results: Vec<(String, String)>) {
        let _ = self.to_thread.send(Inbound::ToolResults(results));
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(ev) = self.from_thread.try_recv() {
            if ev == Event::Done {
                self.busy = false;
            }
            out.push(ev);
        }
        out
    }
}

fn run(client: ChatClient, inbox: &Receiver<Inbound>, outbox: &Sender<Event>) {
    let tools = tool_specs();
    let mut history = vec![Message::system(SYSTEM_PROMPT)];
    while let Ok(msg) = inbox.recv() {
        let Inbound::Prompt(text) = msg else { continue };
        history.push(Message::user(&text));
        let mut rounds = 0;
        loop {
            match client.complete(&history, &tools) {
                Ok(Reply::Text(t)) => {
                    history.push(Message { role: "assistant".into(), content: Some(t.clone()), tool_calls: Vec::new(), tool_call_id: None });
                    let _ = outbox.send(Event::Text(t));
                    break;
                }
                Ok(Reply::ToolCalls(calls)) => {
                    rounds += 1;
                    if rounds > MAX_TOOL_ROUNDS {
                        let _ = outbox.send(Event::Error(format!("stopped after {MAX_TOOL_ROUNDS} tool rounds")));
                        break;
                    }
                    history.push(Message { role: "assistant".into(), content: None, tool_calls: calls.clone(), tool_call_id: None });
                    let _ = outbox.send(Event::ToolCalls(calls.clone()));
                    // Wait for the UI to run them; a new prompt meanwhile cancels this turn.
                    match inbox.recv() {
                        Ok(Inbound::ToolResults(results)) => {
                            for (id, content) in results {
                                history.push(Message::tool(&id, content));
                            }
                        }
                        Ok(Inbound::Prompt(_)) | Err(_) => {
                            let _ = outbox.send(Event::Error("turn cancelled".into()));
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = outbox.send(Event::Error(e));
                    break;
                }
            }
        }
        let _ = outbox.send(Event::Done);
    }
}

/// Run `calls` on a workspace and shape the answers for [`Agent::tool_results`].
pub fn run_tool_calls(ws: &mut Workspace, calls: &[ToolCall]) -> Vec<(String, String)> {
    calls
        .iter()
        .map(|c| {
            let args: Value =
                if c.function.arguments.trim().is_empty() { json!({}) } else { serde_json::from_str(&c.function.arguments).unwrap_or(Value::Null) };
            let out =
                if args.is_object() { ws.run(&c.function.name, &args) } else { Err(format!("{}: arguments are not a JSON object", c.function.name)) };
            let text = match out {
                Ok(v) => v.to_string(),
                Err(e) => json!({"error": e}).to_string(),
            };
            (c.id.clone(), text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A one-shot HTTP/1.1 server that answers the given bodies in order and records requests.
    fn serve(bodies: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let h = thread::spawn(move || {
            let mut seen = Vec::new();
            for body in bodies {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let mut len = None;
                loop {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                    if len.is_none()
                        && let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n")
                    {
                        let head = String::from_utf8_lossy(&buf[..p]).to_string();
                        let cl = head
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()));
                        len = Some((p + 4, cl.unwrap_or(0)));
                    }
                    if let Some((start, cl)) = len
                        && buf.len() >= start + cl
                    {
                        seen.push(String::from_utf8_lossy(&buf).to_string());
                        break;
                    }
                }
                write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                    .unwrap();
            }
            seen
        });
        (format!("http://{addr}/v1"), h)
    }

    fn tool_call_reply(calls: &[(&str, &str, &str)]) -> String {
        let tc: Vec<Value> =
            calls.iter().map(|(id, name, args)| json!({"id": id, "type": "function", "function": {"name": name, "arguments": args}})).collect();
        json!({"choices": [{"message": {"role": "assistant", "content": null, "tool_calls": tc}, "finish_reason": "tool_calls"}]}).to_string()
    }

    fn text_reply(t: &str) -> String {
        json!({"choices": [{"message": {"role": "assistant", "content": t}, "finish_reason": "stop"}]}).to_string()
    }

    #[test]
    fn parse_text_tool_calls_and_errors() {
        assert_eq!(parse_reply(200, &text_reply("hi")).unwrap(), Reply::Text("hi".into()));
        let Reply::ToolCalls(c) = parse_reply(200, &tool_call_reply(&[("c1", "route", "{}")])).unwrap() else { panic!() };
        assert_eq!(c[0].function.name, "route");
        assert!(parse_reply(401, r#"{"error":{"message":"bad secret"}}"#).unwrap_err().contains("bad secret"));
        assert!(parse_reply(502, "<html>").unwrap_err().contains("502"));
    }

    #[test]
    fn a_turn_runs_the_engines_tool_calls_on_the_workspace_and_ends_with_text() {
        let (base, server) = serve(vec![
            tool_call_reply(&[("c1", "load_example", r#"{"name":"sensor-node"}"#)]),
            tool_call_reply(&[
                ("c2", "place", r#"{"width_mm":30,"height_mm":25,"edge_parts":[{"refdes":"J1","edge":"left"}]}"#),
                ("c3", "route", ""),
            ]),
            tool_call_reply(&[("c4", "drc", "{}")]),
            text_reply("Done: 14 parts routed, DRC clean."),
        ]);
        let client = ChatClient::new(base, vec![("Authorization", "Basic x".into()), ("x-redrob-app", APP.into())]);
        let mut agent = Agent::spawn(client);
        let mut ws = Workspace::default();
        agent.prompt("센서 노드 보드 만들어");
        let mut text = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            for ev in agent.poll() {
                match ev {
                    Event::ToolCalls(calls) => {
                        let results = run_tool_calls(&mut ws, &calls);
                        agent.tool_results(results);
                    }
                    Event::Text(t) => text = Some(t),
                    Event::Error(e) => panic!("{e}"),
                    Event::Done => {}
                }
            }
            if text.is_some() && !agent.busy {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(text.as_deref(), Some("Done: 14 parts routed, DRC clean."));
        assert_eq!(ws.board.as_ref().map(|b| b.placements.len()), Some(14));
        assert_eq!(ws.route_complete, Some(true));
        assert!(ws.drc_run && ws.violations.is_empty());
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 4);
        assert!(seen[0].contains("x-redrob-app: board"), "{}", seen[0]);
        assert!(seen[0].contains("\"model\":\"redrob/auto\""));
        assert!(seen[0].contains("\"tools\":["));
        // The fourth request carries the whole exchange: three tool-call rounds and their results.
        assert_eq!(seen[3].matches("\"role\":\"tool\"").count(), 4);
        // Tool results travel as JSON text inside the message content, hence the escaping.
        assert!(seen[3].contains(r#"\"placed\":14"#), "{}", seen[3]);
    }

    #[test]
    fn engine_errors_end_the_turn() {
        let (base, _server) = serve(vec![json!({"error": {"message": "no provider"}}).to_string()]);
        let mut agent = Agent::spawn(ChatClient::new(base, Vec::new()));
        agent.prompt("hi");
        let mut err = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while err.is_none() && std::time::Instant::now() < deadline {
            for ev in agent.poll() {
                if let Event::Error(e) = ev {
                    err = Some(e);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(err.unwrap().contains("no provider"));
    }

    #[test]
    fn bad_tool_arguments_become_error_results_not_panics() {
        let mut ws = Workspace::default();
        let calls =
            vec![ToolCall { id: "a".into(), kind: "function".into(), function: FunctionCall { name: "place".into(), arguments: "[1,2]".into() } }];
        let r = run_tool_calls(&mut ws, &calls);
        assert!(r[0].1.contains("not a JSON object"));
    }
}
