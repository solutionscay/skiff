//! Runs an agent's own command for one read: a one-shot command whose JSON
//! comes on stdout, or a stdio JSON-RPC server asked a few calls and then
//! stopped. Every process is killed when the read ends or times out.
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// A read that takes longer is stopped.
pub const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// The most stdout a read keeps.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

/// The program of an agent, and how a message names the command.
#[derive(Debug, Clone)]
pub struct Program {
    /// The full path skiffd found on its PATH, or the command's first word.
    pub path: PathBuf,
}

impl Program {
    /// A command for `args`, started in the account's home with the
    /// program's own folder first on PATH: a Node agent finds the `node`
    /// installed beside it, as in a pane.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        if let Some(home) = dirs::home_dir() {
            cmd.current_dir(home);
        }
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let mut paths = vec![dir.to_path_buf()];
            paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        cmd
    }
}

/// `label` names the command in a message: "claude -p /usage".
fn did_not_start(label: &str, e: &std::io::Error) -> String {
    format!("{label} did not start: {e}")
}

/// Runs `args` to the end and answers its stdout as JSON. A non-zero exit
/// with no JSON is a failed read.
pub async fn json_output(program: &Program, args: &[&str], env: &[(&str, &str)], label: &str) -> Result<Value, String> {
    let mut cmd = program.command(args);
    cmd.envs(env.iter().copied());
    let mut child = cmd.spawn().map_err(|e| did_not_start(label, &e))?;
    let mut out = Vec::new();
    let stdout = child.stdout.take().ok_or_else(|| format!("{label} gave no output"))?;
    let read = async {
        stdout.take(MAX_OUTPUT).read_to_end(&mut out).await.map_err(|e| format!("{label}: {e}"))?;
        child.wait().await.map_err(|e| format!("{label}: {e}"))
    };
    let status = tokio::time::timeout(READ_TIMEOUT, read).await.map_err(|_| no_answer(label))??;
    match serde_json::from_slice::<Value>(&out) {
        Ok(v) => Ok(v),
        Err(_) if !status.success() => Err(exited(label, status)),
        Err(_) => Err(format!("{label} gave no JSON")),
    }
}

fn no_answer(label: &str) -> String {
    format!("{label} gave no answer in {} s", READ_TIMEOUT.as_secs())
}

fn exited(label: &str, status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("{label} exited with code {code}"),
        None => format!("{label} was stopped by a signal"),
    }
}

/// A JSON-RPC 2.0 server on a child's stdin and stdout, one message a line.
pub struct Rpc {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    label: String,
    next: u64,
}

impl Rpc {
    pub fn start(program: &Program, args: &[&str], label: &str) -> Result<Self, String> {
        let mut cmd = program.command(args);
        cmd.stdin(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| did_not_start(label, &e))?;
        let stdin = child.stdin.take().ok_or_else(|| format!("{label} has no input"))?;
        let stdout = child.stdout.take().ok_or_else(|| format!("{label} gave no output"))?;
        Ok(Self { child, stdin, lines: BufReader::new(stdout).lines(), label: label.into(), next: 1 })
    }

    async fn send(&mut self, message: Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
        line.push(b'\n');
        self.stdin.write_all(&line).await.map_err(|_| self.ended_text())?;
        self.stdin.flush().await.map_err(|_| self.ended_text())
    }

    /// A notification: no answer.
    pub async fn notify(&mut self, method: &str) -> Result<(), String> {
        self.send(json!({ "jsonrpc": "2.0", "method": method })).await
    }

    /// Calls `method` and answers its `result`. Other messages are skipped.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        loop {
            let Some(line) = self.lines.next_line().await.map_err(|e| format!("{}: {e}", self.label))? else {
                return Err(self.ended().await);
            };
            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error.get("message").and_then(Value::as_str).unwrap_or("an error");
                return Err(format!("{} answered {method} with {text}", self.label));
            }
            return message.get("result").cloned().ok_or_else(|| format!("{} answered {method} with no result", self.label));
        }
    }

    fn ended_text(&self) -> String {
        format!("{} closed its input", self.label)
    }

    /// The server closed its output: its exit, when it has one.
    async fn ended(&mut self) -> String {
        match tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await {
            Ok(Ok(status)) => exited(&self.label, status),
            _ => format!("{} closed its output", self.label),
        }
    }

    /// Stops the server.
    pub async fn stop(mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

/// `read` with the read timeout, the server stopped after it either way.
pub async fn with_rpc<T>(
    program: &Program,
    args: &[&str],
    label: &str,
    read: impl AsyncFnOnce(&mut Rpc) -> Result<T, String>,
) -> Result<T, String> {
    let mut rpc = Rpc::start(program, args, label)?;
    let out = tokio::time::timeout(READ_TIMEOUT, read(&mut rpc)).await.unwrap_or_else(|_| Err(no_answer(label)));
    rpc.stop().await;
    out
}

/// The program for a command line: its first word.
pub fn first_word(command: &str) -> Option<PathBuf> {
    command.split_whitespace().next().map(|w| Path::new(w).to_path_buf())
}
