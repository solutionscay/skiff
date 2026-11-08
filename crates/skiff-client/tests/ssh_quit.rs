//! The quit path ends every ssh child of this app. A fake `ssh` (Python,
//! never OpenSSH, no network) binds the `-L` path and runs until it is
//! killed; the typing link's run ignores SIGTERM, so SIGKILL must follow.
//! Its own test binary: `end_all` ends every ssh child of the process.
//! Skips when this machine has no python3.

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use skiff_client::host::ssh::{self, BatchRun, Carries, SshForward, SshOptions, SshTarget};

const FAKE: &str = r#"#!/usr/bin/python3
import socket,sys,signal,time
if 'RequestTTY=force' in sys.argv: signal.signal(signal.SIGTERM, signal.SIG_IGN)
if '-L' in sys.argv:
 s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[sys.argv.index('-L')+1].split(':')[0]); s.listen()
time.sleep(60)
"#;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_ends_every_ssh_child() {
    if !Path::new("/usr/bin/python3").is_file() {
        eprintln!("skipped: no /usr/bin/python3");
        return;
    }
    // Short: the forward's socket path must fit in sun_path.
    let dir = Dir(PathBuf::from("/tmp").join(format!("sq-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    let program = dir.0.join("fake-ssh");
    std::fs::write(&program, FAKE).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let options = SshOptions { program, config: None };
    let target = SshTarget::new("fixture", "/fixture.sock");
    let parent = dir.0.join("fwd");

    let mut data = SshForward::open_for(&target, &options, &parent, Carries::Data).await.unwrap();
    let mut typing = SshForward::open_for(&target, &options, &parent, Carries::Typing).await.unwrap();
    let batch = {
        let (target, options) = (target.clone(), options.clone());
        tokio::spawn(async move {
            ssh::run_batch(&target, &options, "true".into(), b"", Duration::from_secs(30)).await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(data.is_alive() && typing.is_alive() && !batch.is_finished());

    let start = Instant::now();
    tokio::task::spawn_blocking(|| ssh::end_all(ssh::QUIT_WAIT)).await.unwrap();
    assert!(start.elapsed() < ssh::QUIT_WAIT + Duration::from_millis(200), "{:?}", start.elapsed());

    // SIGKILL lands at once; give the kernel a moment to mark the exit.
    let until = Instant::now() + Duration::from_secs(2);
    while (data.is_alive() || typing.is_alive()) && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!data.is_alive(), "the data link's ssh runs on");
    assert!(!typing.is_alive(), "the typing link's ssh runs on");
    let run = tokio::time::timeout(Duration::from_secs(2), batch).await;
    assert!(matches!(run, Ok(Ok(Ok(BatchRun::Done(_) | BatchRun::Ssh(_))))), "the batch run's ssh runs on");
}
