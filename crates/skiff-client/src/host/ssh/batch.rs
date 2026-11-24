//! One bounded batch SSH command and its progress.
use std::{process::Stdio, time::Duration};

use tokio::{io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader}, process::Command};

use super::{batch_args, SshChild, SshError, SshOptions, SshTarget};

/// The Host protocol never accepts a frame larger than this. Batch SSH runs
/// carry either a probe or one command result, so accepting more can only let
/// a remote peer exhaust this process.
pub(super) const MAX_BATCH_OUTPUT: usize = 8 * 1024 * 1024;
/// Stderr is line-oriented for diagnostics and progress. A line has no
/// protocol value beyond this bound.
pub(super) const MAX_STDERR_LINE: usize = 16 * 1024;

/// What one batch run gave.
pub enum BatchRun {
    /// The command ran: its stdout.
    Done(String),
    /// ssh stopped before the command ran (exit 255): its stderr, for the cause.
    Ssh(String),
    TimedOut,
}

/// Runs `command` over one strict, batch ssh run, with `stdin`, within
/// `timeout`. Err: ssh itself could not be run.
pub async fn run_batch(
    target: &SshTarget,
    options: &SshOptions,
    command: String,
    stdin: &[u8],
    timeout: Duration,
) -> Result<BatchRun, SshError> {
    run_batch_steps(target, options, command, stdin, timeout, None).await
}

/// What a server step prints on stderr, one line per step, for a person
/// who waits: `skiff-step: <words>`.
const STEP: &str = "skiff-step: ";

/// What a batch run shows while it runs.
pub enum BatchEvent<'a> {
    /// A line the server printed as `skiff-step: <words>`.
    Step(&'a str),
    /// One of ssh's own lines that [`shown_line`] lets through.
    Ssh(String),
    /// Bytes of stdin sent so far, and the whole.
    Sent(u64, u64),
}

/// How much stdin goes per write, so the bytes sent can be shown.
const CHUNK: usize = 64 * 1024;

/// [`run_batch`]. With `show`, ssh logs at DEBUG1, and the server's step
/// lines, ssh's allowlisted lines and the bytes sent go to `show` as they
/// happen. The rest of stderr stays for the cause, as without it.
pub async fn run_batch_steps(
    target: &SshTarget,
    options: &SshOptions,
    command: String,
    stdin: &[u8],
    timeout: Duration,
    show: Option<&(dyn Fn(BatchEvent) + Send + Sync)>,
) -> Result<BatchRun, SshError> {
    let mut args = batch_args(target, options, command);
    if show.is_some() {
        for a in args.iter_mut().filter(|a| *a == "LogLevel=INFO") {
            *a = "LogLevel=DEBUG1".into();
        }
    }
    let child = SshChild::spawn(
        Command::new(&options.program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .map_err(|e| SshError::Run(format!("Skiff cannot run ssh: {e}")))?;
    let (mut input, mut stdout, stderr) = {
        let mut c = child.lock();
        (
            c.stdin.take().expect("piped"),
            c.stdout.take().expect("piped"),
            c.stderr.take().expect("piped"),
        )
    };
    let emit = |e: BatchEvent| {
        if let Some(f) = show {
            f(e)
        }
    };
    let run = async {
        // A failed write shows as ssh's own failure below.
        let write = async {
            let total = stdin.len() as u64;
            let mut last = 0u64;
            for chunk in stdin.chunks(CHUNK) {
                if input.write_all(chunk).await.is_err() {
                    break;
                }
                last += chunk.len() as u64;
                if total > 4 * CHUNK as u64
                    && (last.is_multiple_of(4 * CHUNK as u64) || last == total)
                {
                    emit(BatchEvent::Sent(last, total));
                }
            }
            drop(input);
            Ok::<_, SshError>(())
        };
        let read_out = read_stdout(&mut stdout);
        let read_err = async {
            let mut reader = BufReader::new(stderr);
            let mut rest = String::new();
            while let Some(line) = read_stderr_line(&mut reader).await? {
                match line.strip_prefix(STEP) {
                    Some(words) => emit(BatchEvent::Step(words.trim())),
                    None => {
                        if let Some(shown) = shown_line(&line) {
                            emit(BatchEvent::Ssh(shown));
                        }
                        if !line.starts_with("debug") {
                            rest.push_str(&line);
                            rest.push('\n');
                        }
                    }
                }
            }
            Ok::<_, SshError>(rest)
        };
        let (_, out, err) = tokio::try_join!(write, read_out, read_err)?;
        Ok::<_, SshError>((child.wait().await, out, err))
    };
    let (status, out, err) = match tokio::time::timeout(timeout, run).await {
        Ok(Ok(done)) => done,
        Ok(Err(error)) => {
            let _ = child.lock().start_kill();
            let _ = child.wait().await;
            return Err(error);
        }
        Err(_) => return Ok(BatchRun::TimedOut),
    };
    let status = status.map_err(|e| SshError::Run(format!("Skiff cannot run ssh: {e}")))?;
    if status.code() == Some(255) {
        return Ok(BatchRun::Ssh(err));
    }
    Ok(BatchRun::Done(String::from_utf8_lossy(&out).into_owned()))
}

pub(super) async fn read_stdout<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, SshError> {
    let mut output = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|e| SshError::Run(format!("Skiff cannot read ssh output: {e}")))?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > MAX_BATCH_OUTPUT {
            return Err(SshError::OutputTooLarge);
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

pub(super) async fn read_stderr_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<String>, SshError> {
    let mut bytes = Vec::new();
    let count = (&mut *reader)
        .take(MAX_STDERR_LINE as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|e| SshError::Run(format!("Skiff cannot read ssh diagnostics: {e}")))?;
    if count == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_STDERR_LINE
        || bytes.last() != Some(&b'\n') && bytes.len() == MAX_STDERR_LINE + 1
    {
        return Err(SshError::OutputTooLarge);
    }
    Ok(Some(
        String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    ))
}

/// One of ssh's own stderr lines as a person may see it, or `None`. Only a
/// short allowlist passes: connecting, the connection, a known host key,
/// authenticated, and forwarding, without the `debug1: ` prefix, and ssh's
/// connect failures. Never a key path, a key, an agent or a password line.
pub fn shown_line(line: &str) -> Option<String> {
    let l = line.trim();
    let debug = l.strip_prefix("debug1: ");
    let keep = match debug {
        Some(d) => {
            (d.starts_with("Connecting to ") && d.contains(" port "))
                || d.starts_with("Connection established")
                || (d.starts_with("Host '") && d.contains("is known and matches"))
                || d.starts_with("Authenticated to ")
                || d.starts_with("Local connections to ")
                || d.starts_with("Local forwarding listening on ")
        }
        None => {
            // VERBOSE lines, which DEBUG1 also prints, come without a prefix.
            l.starts_with("Authenticated to ")
                || l.starts_with("ssh: connect to host ")
                || l.starts_with("ssh: Could not resolve hostname ")
                || l.starts_with("Host key verification failed")
                || (l.contains("Permission denied (") && !l.contains('/'))
                || l.starts_with("Connection closed by ")
                || l.starts_with("Connection timed out")
        }
    };
    let text = debug.unwrap_or(l);
    // A key or identity file never shows, whatever the line.
    (keep && !text.contains("/.ssh/") && !text.contains("identity")).then(|| text.to_string())
}
