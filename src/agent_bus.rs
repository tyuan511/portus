//! Bidirectional Agent Control Bus: Unix domain socket server, CLI (`Portus ctl`),
//! and shared session snapshot registry so AI agent CLIs (Claude Code hooks, Codex,
//! Gemini CLI, OpenCode, or MCP tools) can report status/progress, trigger Portus
//! notification rings, open splits, or inspect active terminal sessions.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_channel::Sender;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSessionSnapshot {
    pub session_id: String,
    pub title: String,
    pub is_ssh: bool,
    pub host: Option<String>,
    pub cwd: Option<String>,
    pub state: String,
    pub progress: Option<u8>,
    pub attention_message: Option<String>,
    pub unread: bool,
}

#[derive(Clone, Debug)]
pub enum AgentControlCommand {
    Notify {
        session_id: Option<String>,
        title: Option<String>,
        body: String,
    },
    SetStatus {
        session_id: Option<String>,
        state: String,
        message: Option<String>,
        progress: Option<u8>,
    },
    SplitPane {
        cwd: Option<String>,
    },
    OpenFile {
        path: String,
    },
}

#[derive(Debug, Deserialize)]
struct BusRequest {
    op: String,
    session_id: Option<String>,
    title: Option<String>,
    body: Option<String>,
    state: Option<String>,
    message: Option<String>,
    progress: Option<u8>,
    cwd: Option<String>,
    path: Option<String>,
}

pub struct AgentBus {
    sessions: Arc<Mutex<Vec<AgentSessionSnapshot>>>,
    commands: Sender<AgentControlCommand>,
}

impl AgentBus {
    pub fn start(commands: Sender<AgentControlCommand>) -> Arc<Self> {
        let socket_path = default_socket_path();
        let bus = Arc::new(Self {
            sessions: Arc::new(Mutex::new(Vec::new())),
            commands,
        });
        #[cfg(unix)]
        {
            let bus_bg = Arc::clone(&bus);
            std::thread::Builder::new()
                .name("portus-agent-bus".into())
                .spawn(move || run_unix_server(bus_bg, &socket_path))
                .ok();
        }
        bus
    }

    pub fn update_sessions(&self, next: Vec<AgentSessionSnapshot>) {
        if let Ok(mut guard) = self.sessions.lock() {
            *guard = next;
        }
    }

    pub fn sessions_snapshot(&self) -> Vec<AgentSessionSnapshot> {
        self.sessions
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn send_command(&self, cmd: AgentControlCommand) -> Result<(), String> {
        self.commands
            .try_send(cmd)
            .map_err(|_| "Portus workspace is not accepting commands.".to_owned())
    }

    fn handle_request_json(&self, line: &str) -> Value {
        let req: BusRequest = match serde_json::from_str(line.trim()) {
            Ok(req) => req,
            Err(err) => return json!({ "ok": false, "error": format!("Invalid JSON: {err}") }),
        };

        match req.op.as_str() {
            "list" => {
                let sessions = self.sessions_snapshot();
                json!({ "ok": true, "sessions": sessions })
            }
            "notify" => {
                let body = req
                    .body
                    .or(req.message)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                if body.is_empty() {
                    return json!({ "ok": false, "error": "Missing notification body" });
                }
                match self.send_command(AgentControlCommand::Notify {
                    session_id: req.session_id,
                    title: req.title,
                    body,
                }) {
                    Ok(()) => json!({ "ok": true }),
                    Err(err) => json!({ "ok": false, "error": err }),
                }
            }
            "status" => {
                let state = req.state.unwrap_or_else(|| "running".to_owned());
                match self.send_command(AgentControlCommand::SetStatus {
                    session_id: req.session_id,
                    state,
                    message: req.message.or(req.body),
                    progress: req.progress.map(|p| p.min(100)),
                }) {
                    Ok(()) => json!({ "ok": true }),
                    Err(err) => json!({ "ok": false, "error": err }),
                }
            }
            "split" => match self.send_command(AgentControlCommand::SplitPane { cwd: req.cwd }) {
                Ok(()) => json!({ "ok": true }),
                Err(err) => json!({ "ok": false, "error": err }),
            },
            "open" => {
                let Some(path) = req.path.filter(|p| !p.trim().is_empty()) else {
                    return json!({ "ok": false, "error": "Missing path" });
                };
                match self.send_command(AgentControlCommand::OpenFile { path }) {
                    Ok(()) => json!({ "ok": true }),
                    Err(err) => json!({ "ok": false, "error": err }),
                }
            }
            other => json!({ "ok": false, "error": format!("Unknown op: {other}") }),
        }
    }
}

pub fn default_socket_path() -> PathBuf {
    if let Ok(dir) = crate::settings::config_dir() {
        return dir.join("agent.sock");
    }
    std::env::temp_dir().join("portus-agent.sock")
}

#[cfg(unix)]
fn run_unix_server(bus: Arc<AgentBus>, path: &Path) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(path);
    let Ok(listener) = UnixListener::bind(path) else {
        return;
    };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));

    for stream in listener.incoming().flatten() {
        let bus = Arc::clone(&bus);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let reply = bus.handle_request_json(&line);
            let mut writer = &stream;
            let _ = writeln!(writer, "{reply}");
        });
    }
}

/// Runs `Portus ctl <subcommand> ...` synchronously against the running Portus
/// desktop instance over the Unix socket.
pub fn run_cli_subcommand(args: &[String]) -> i32 {
    if args.is_empty() || matches!(args[0].as_str(), "-h" | "--help" | "help") {
        print_ctl_usage();
        return 0;
    }

    let sub = args[0].as_str();
    let rest = &args[1..];
    let default_session = std::env::var("PORTUS_SESSION_ID")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let payload = match sub {
        "list" => json!({ "op": "list" }),
        "notify" => {
            let mut title: Option<String> = None;
            let mut body: Option<String> = None;
            let mut session_id = default_session;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--title" | "-t" if i + 1 < rest.len() => {
                        title = Some(rest[i + 1].clone());
                        i += 2;
                    }
                    "--body" | "-b" | "--message" | "-m" if i + 1 < rest.len() => {
                        body = Some(rest[i + 1].clone());
                        i += 2;
                    }
                    "--session" | "-s" if i + 1 < rest.len() => {
                        session_id = Some(rest[i + 1].clone());
                        i += 2;
                    }
                    other if !other.starts_with('-') && body.is_none() => {
                        body = Some(other.to_owned());
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            let body = body.unwrap_or_else(|| "Agent needs your attention".to_owned());
            json!({
                "op": "notify",
                "session_id": session_id,
                "title": title,
                "body": body,
            })
        }
        "status" => {
            let mut state = "running".to_owned();
            let mut message: Option<String> = None;
            let mut progress: Option<u8> = None;
            let mut session_id = default_session;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--state" if i + 1 < rest.len() => {
                        state = rest[i + 1].clone();
                        i += 2;
                    }
                    "--message" | "-m" if i + 1 < rest.len() => {
                        message = Some(rest[i + 1].clone());
                        i += 2;
                    }
                    "--progress" | "-p" if i + 1 < rest.len() => {
                        progress = rest[i + 1].parse::<u8>().ok().map(|v| v.min(100));
                        i += 2;
                    }
                    "--session" | "-s" if i + 1 < rest.len() => {
                        session_id = Some(rest[i + 1].clone());
                        i += 2;
                    }
                    other if !other.starts_with('-') => {
                        state = other.to_owned();
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            json!({
                "op": "status",
                "session_id": session_id,
                "state": state,
                "message": message,
                "progress": progress,
            })
        }
        "split" => {
            let mut cwd: Option<String> = None;
            let mut i = 0;
            while i < rest.len() {
                if matches!(rest[i].as_str(), "--cwd" | "-d") && i + 1 < rest.len() {
                    cwd = Some(rest[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            json!({ "op": "split", "cwd": cwd })
        }
        "open" => {
            let path = rest.first().cloned().unwrap_or_default();
            json!({ "op": "open", "path": path })
        }
        _ => {
            eprintln!("Unknown ctl subcommand: {sub}");
            print_ctl_usage();
            return 2;
        }
    };

    let socket = std::env::var("PORTUS_SOCKET")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_socket_path);

    match send_socket_request(&socket, &payload) {
        Ok(reply) => {
            println!("{reply}");
            0
        }
        Err(err) => {
            eprintln!("Portus ctl error: {err}");
            1
        }
    }
}

#[cfg(unix)]
fn send_socket_request(socket: &Path, payload: &Value) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket)
        .map_err(|e| format!("Could not connect to {}: {e}", socket.display()))?;
    let serialized = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    writeln!(stream, "{serialized}").map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim().to_owned())
}

#[cfg(not(unix))]
fn send_socket_request(_socket: &Path, _payload: &Value) -> Result<String, String> {
    Err("Unix socket agent bus is only supported on Unix platforms.".to_owned())
}

fn print_ctl_usage() {
    println!(
        "Portus Agent Control CLI\n\n\
         Usage:\n  \
         Portus ctl notify [--title TITLE] [--body MSG] [--session ID]\n  \
         Portus ctl status [--state running|waiting|idle|error] [--message MSG] [--progress 0..100] [--session ID]\n  \
         Portus ctl split [--cwd DIR]\n  \
         Portus ctl open <PATH>\n  \
         Portus ctl list"
    );
}

/// Generates a ready-to-paste Claude Code `~/.claude/settings.json` hooks
/// snippet wired to this Portus executable's `ctl` subcommand.
pub fn build_claude_hooks_snippet() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "/Applications/Portus.app/Contents/MacOS/Portus".to_owned());
    let quoted_exe = shell_words::quote(&exe);
    let snippet = json!({
        "hooks": {
            "Notification": [
                {
                    "matcher": "",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{quoted_exe} ctl notify --title \"Claude Code\" --body \"Needs your attention\"")
                        }
                    ]
                }
            ],
            "Stop": [
                {
                    "matcher": "",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{quoted_exe} ctl status --state idle --message \"Task finished\"")
                        }
                    ]
                }
            ]
        }
    });
    serde_json::to_string_pretty(&snippet).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_handles_list_and_notify_over_unix_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("agent-test.sock");
        let (tx, rx) = async_channel::unbounded();
        let bus = Arc::new(AgentBus {
            sessions: Arc::new(Mutex::new(vec![AgentSessionSnapshot {
                session_id: "sess-1".into(),
                title: "portus".into(),
                is_ssh: false,
                host: None,
                cwd: Some("/tmp".into()),
                state: "running".into(),
                progress: Some(42),
                attention_message: None,
                unread: false,
            }])),
            commands: tx,
        });

        let bus_srv = Arc::clone(&bus);
        let sock_srv = sock.clone();
        std::thread::spawn(move || run_unix_server(bus_srv, &sock_srv));

        for _ in 0..40 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let list_reply = send_socket_request(&sock, &json!({ "op": "list" })).unwrap();
        let list_val: Value = serde_json::from_str(&list_reply).unwrap();
        assert_eq!(list_val["ok"], true);
        assert_eq!(list_val["sessions"][0]["session_id"], "sess-1");

        let notify_reply = send_socket_request(
            &sock,
            &json!({ "op": "notify", "session_id": "sess-1", "title": "Claude", "body": "Approve?" }),
        )
        .unwrap();
        let notify_val: Value = serde_json::from_str(&notify_reply).unwrap();
        assert_eq!(notify_val["ok"], true);

        let cmd = rx.try_recv().unwrap();
        match cmd {
            AgentControlCommand::Notify {
                session_id,
                title,
                body,
            } => {
                assert_eq!(session_id.as_deref(), Some("sess-1"));
                assert_eq!(title.as_deref(), Some("Claude"));
                assert_eq!(body, "Approve?");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
