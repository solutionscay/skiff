//! Remote Host connection and identity pinning.

use std::{path::PathBuf, sync::Arc, time::Duration};
use skiff_client::host::local::{HostSelection, SelectedHost};
use super::{App, Warning};

/// A remote Host: connect through its forward, check the identity it reports
/// against its pin, and never start, reload, stop or replace anything. A
/// version mismatch is reported, not acted on. Every failure leaves its cause
/// in the warning, so the status and the picker say why, in words that never
/// repeat ssh's own output.
pub(super) async fn connect_remote(app: &App, selection: &HostSelection, guard: &mut Option<Arc<SelectedHost>>) -> Result<(Arc<SelectedHost>, bool), String> {
    let destination = selection.ssh_target().map(|t| t.destination.clone()).unwrap_or_default();
    let arrival = match selection.ssh_target() {
        Some(t) => {
            let arrival = crate::hosts::update_on_connect(t).await;
            // The probe that ran for it found the Host's one socket: the
            // forward goes there. Without it, the connect probes itself.
            if let Some(socket) = crate::hosts::found_socket(t) {
                selection.set_remote_socket(socket);
            }
            arrival
        }
        None => None,
    };
    match try_remote(app, selection, &destination, arrival.as_ref(), crate::hosts::path()).await {
        Ok(host) => {
            // The passive note: "skiffd 2.1 (updated from 2.0)".
            if let Some(from) = arrival.and_then(|a| a.updated_from) {
                *app.replaced.lock().await = Some(from);
            }
            if let Some(t) = selection.ssh_target().cloned() {
                tauri::async_runtime::spawn_blocking(move || crate::hosts::note_login(&t, true));
            }
            *app.changed.lock().unwrap() = None;
            *app.live.lock().unwrap() = Some(host.connection());
            *guard = Some(host.clone());
            Ok((host, false))
        }
        Err(e) => {
            let e = refine_refused(e, selection).await;
            let (warning, message) = remote_failure(&e, &destination);
            if let (Some(_), Some(t)) = (&warning, selection.ssh_target().cloned()) {
                tauri::async_runtime::spawn_blocking(move || crate::hosts::note_login(&t, false));
            }
            if let Some(skiff_host_api::Error::IdentityChanged { received, .. }) = e.downcast_ref() {
                *app.changed.lock().unwrap() = Some((destination.clone(), received.clone()));
            }
            // The wait between tries is not a cause: the last cause stays.
            if let Some(w) = warning {
                *app.warning.lock().await = Some(w);
            }
            Err(message)
        }
    }
}

/// A refused channel names two causes. The probe looks at the socket: when
/// one is there, the sshd blocks the forward. Any other error stays.
async fn refine_refused(e: anyhow::Error, selection: &HostSelection) -> anyhow::Error {
    use skiff_client::host::{check, ssh::SshError};
    let (Some(SshError::NoSkiffd { .. }), Some(t)) = (e.downcast_ref::<SshError>(), selection.ssh_target()) else { return e };
    let options = skiff_client::host::ssh::SshOptions::default();
    match check::run_probe(t, &options).await {
        Ok(check::ProbeRun::Probed(p, _)) => match e.downcast::<SshError>() {
            Ok(ssh) => ssh.refine(p.socket_live).into(),
            Err(e) => e,
        },
        _ => e,
    }
}

/// The warning and the message for a failed remote connection. Only known
/// causes are named; anything else gets one plain line with what to check.
/// `None` warning: a wait between tries, which keeps the last cause.
pub(super) fn remote_failure(e: &anyhow::Error, destination: &str) -> (Option<Warning>, String) {
    use skiff_client::host::ssh::SshError;
    let warn = |kind: &'static str, message: String| (Some(Warning { kind, message: message.clone(), sessions: None }), message);
    if let Some(e) = e.downcast_ref::<SshError>() {
        return match e {
            SshError::Waiting { .. } => (None, e.to_string()),
            _ => warn("remote", e.to_string()),
        };
    }
    match e.downcast_ref::<skiff_host_api::Error>() {
        Some(skiff_host_api::Error::IdentityChanged { .. }) => warn(
            "identity",
            format!(
                "{destination} answers as another Skiff Host than before. Skiff stopped and sends nothing to it. \
                 If you expect this, as after Skiff was reinstalled there, accept the new identity."
            ),
        ),
        Some(skiff_host_api::Error::MalformedContract { .. }) => warn(
            "remote",
            format!("Skiff cannot read the replies from the sessions service on {destination}. Settings › Hosts checks {destination}."),
        ),
        _ if e.downcast_ref::<RemoteSilent>().is_some() => warn(
            "remote",
            format!("skiffd on {destination} did not answer within {} s. Settings › Hosts checks {destination}.", REMOTE_HELLO_TIMEOUT.as_secs()),
        ),
        _ => warn(
            "remote",
            format!("Cannot reach skiffd on {destination} over ssh. Settings › Hosts checks {destination} and finds the cause."),
        ),
    }
}

/// How long a remote skiffd may take to answer the first Ping.
pub(super) const REMOTE_HELLO_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) async fn try_remote(app: &App, selection: &HostSelection, destination: &str, arrival: Option<&crate::hosts::Arrival>, file: PathBuf) -> anyhow::Result<Arc<SelectedHost>> {
    let mut c = selection.connect().await?;
    let info = match tokio::time::timeout(REMOTE_HELLO_TIMEOUT, c.daemon_info()).await {
        Ok(info) => info?,
        Err(_) => anyhow::bail!(RemoteSilent),
    };
    skiff_client::host::ssh::connect_note(format!("skiffd {}, protocol {}", info.version, info.protocol));
    let name = arrival.map_or(destination, |a| a.name.as_str());
    let managed = arrival.is_some_and(|a| a.managed);
    *app.warning.lock().await =
        crate::hosts::version_warning(name, arrival.and_then(|a| a.version.as_deref()), info.protocol, managed).map(|(kind, message)| Warning { kind, message, sessions: None });
    let first = selection.pinned().is_none();
    let host = Arc::new(c.activate().await?);
    if first {
        // The first identity becomes the Host's pin before anything uses it:
        // stored in hosts.toml first, then pinned here. Until the store
        // succeeds the selection stays unpinned, so every try stores again
        // and none connects on an identity only this window holds.
        let id = host.connection().hello().host.host_id.clone();
        let target = selection.ssh_target().cloned().ok_or_else(|| anyhow::anyhow!("{destination} is not a remote Host"))?;
        let pinned = id.clone();
        let stored = tauri::async_runtime::spawn_blocking(move || crate::hosts::pin_first(&file, &target, &id)).await?;
        match stored {
            Ok(()) => selection.repin(pinned),
            Err(e) => {
                // Another window or run stored another identity first: that
                // one is the pin from now on, here too.
                if let Some(skiff_host_api::Error::IdentityChanged { expected, .. }) = e.downcast_ref() {
                    selection.repin(expected.clone());
                }
                return Err(e);
            }
        }
    }
    Ok(host)
}

/// The remote skiffd did not answer the first Ping in time.
#[derive(Debug)]
pub(super) struct RemoteSilent;
impl std::fmt::Display for RemoteSilent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no answer within {}s", REMOTE_HELLO_TIMEOUT.as_secs())
    }
}
impl std::error::Error for RemoteSilent {}

#[cfg(test)]
mod tests {
    use crate::connection::{ensure_host, data_now, local_selection, replace_selection, status_now};

    /// A remote Host is reached, never managed: a failing SSH endpoint gets
    /// an explanation, and the app starts, reloads and signals nothing here.
    #[test]
    fn a_remote_endpoint_never_runs_local_lifecycle_and_reports_why() {
        use super::*;
        let _env = crate::reload::tests::ENV.lock().unwrap_or_else(|e| e.into_inner());
        tokio::runtime::Runtime::new().unwrap().block_on(async {
        let dir = std::env::temp_dir().join(format!("skiff-remote-life-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        // A local "skiffd" and "ssh" that only leave a mark.
        let mark = |name: &str| {
            let script = dir.join(name);
            std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {}/{name}.ran\nexit 255\n", dir.display())).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            script
        };
        let old_daemon = std::env::var_os("SKIFF_DAEMON");
        std::env::set_var("SKIFF_DAEMON", mark("skiffd"));
        let old_data = std::env::var_os("SKIFF_HOST_BIN");
        std::env::set_var("SKIFF_HOST_BIN", mark("skiff-host"));
        let app = App::default();
        let target = skiff_client::host::ssh::SshTarget::new("devbox", "/run/user/1/skiff/skiffd.sock");
        // Review P2: an unknown host key from a fresh state reaches the status.
        let ssh = mark("ssh");
        let text = std::fs::read_to_string(&ssh).unwrap().replace("exit 255", "echo 'Host key verification failed.' >&2\nexit 255");
        std::fs::write(&ssh, text).unwrap();
        let options = skiff_client::host::ssh::SshOptions { program: ssh, config: None };
        app.set_selection(HostSelection::ssh(target, options, dir.join("fwd"), None).unwrap());
        assert!(app.remote());
        assert!(app.selection().process_control().is_none());
        let started = std::time::Instant::now();
        let status = status_now(&app).await;
        assert!(!status.connected && !status.spawned && status.pid.is_none());
        assert_eq!(status.socket, "devbox:/run/user/1/skiff/skiffd.sock");
        let cause = status.warning.as_ref().expect("the cause reaches the status");
        assert_eq!(cause.kind, "remote");
        assert!(cause.message.contains("does not know the host key of devbox") && !cause.message.contains("verification failed"), "{}", cause.message);
        // One ssh run, no retry loop, and a second try waits.
        assert!(started.elapsed() < Duration::from_secs(5));
        let e = ensure_host(&app).await.err().unwrap();
        assert!(e.contains("tries again"), "{e}");
        // The wait keeps the cause: the status still says what to fix.
        let again = status_now(&app).await;
        assert!(again.warning.is_some_and(|w| w.message.contains("host key")));
        assert_eq!(std::fs::read_to_string(dir.join("ssh.ran")).unwrap().lines().count(), 1);
        assert!(!dir.join("skiffd.ran").exists(), "the local daemon binary ran for a remote Host");
        // M4: no data-plane service is started for a remote Host, here or there.
        assert!(data_now(&app).await.is_err());
        assert!(!dir.join("skiff-host.ran").exists(), "the data-plane binary ran for a remote Host");
        // Switching back to this Mac drops the remote selection and its forward.
        replace_selection(&app, local_selection()).await;
        assert!(!app.remote());
        match old_daemon {
            Some(v) => std::env::set_var("SKIFF_DAEMON", v),
            None => std::env::remove_var("SKIFF_DAEMON"),
        }
        match old_data {
            Some(v) => std::env::set_var("SKIFF_HOST_BIN", v),
            None => std::env::remove_var("SKIFF_HOST_BIN"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        });
    }

    /// Review P2: each remote failure has a cause the status can show, in
    /// words that never repeat ssh's output; a wait keeps the last cause.
    #[test]
    fn remote_failures_name_a_cause_and_a_wait_keeps_the_last_one() {
        use skiff_client::host::ssh::SshError;
        let d = "devbox";
        let cause = |e: anyhow::Error| super::remote_failure(&e, d);
        for e in [
            SshError::HostKeyUnknown(d.into()),
            SshError::HostKeyChanged(d.into()),
            SshError::AuthFailed(d.into()),
            SshError::NoSkiffd { destination: d.into(), socket: "/s.sock".into() },
            SshError::ForwardingRefused(d.into()),
        ] {
            let text = e.to_string();
            let (w, message) = cause(e.into());
            assert_eq!(w.map(|w| (w.kind, w.message)), Some(("remote", text.clone())));
            assert_eq!(message, text);
        }
        let (w, _) = cause(SshError::Waiting { destination: d.into(), seconds: 4 }.into());
        assert!(w.is_none());
        let id = |s: &str| skiff_host_api::HostId::try_from(s.to_string()).unwrap();
        let (w, _) = cause(skiff_host_api::Error::IdentityChanged { expected: id("A"), received: id("B") }.into());
        assert_eq!(w.unwrap().kind, "identity");
        // An unknown failure names no path or ssh line: one line, what to check.
        let (w, _) = cause(anyhow::anyhow!("connect /var/folders/x/T/skiff-501-1/9-0/skiffd.sock: No such file"));
        let w = w.unwrap();
        assert!(w.message.contains("Settings › Hosts") && !w.message.contains("/var/folders"), "{}", w.message);
    }

    /// Residual P1, failure injection: the first identity's store fails (a
    /// directory sits where the new hosts.toml is written). The connection is
    /// refused and the selection stays unpinned, on a retry too; once the
    /// store works, the retry stores the pin before it connects. A Host that
    /// then answers as B is refused here and by a selection rebuilt from the
    /// file. A fake `ssh` only (a Python Unix-socket server), never OpenSSH.
    #[test]
    fn a_first_identity_connects_only_once_it_is_stored_even_on_a_retry() {
        use super::*;
        if !std::path::Path::new("/usr/bin/python3").is_file() {
            eprintln!("skipped: no /usr/bin/python3");
            return;
        }
        // Short: the forward's socket path must fit in sun_path.
        let dir = PathBuf::from("/tmp").join(format!("sd-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let id_file = dir.join("remote-id");
        let script = format!(
            "#!/usr/bin/python3\n\
             import socket,sys,json,threading\n\
             spec=sys.argv[sys.argv.index('-L')+1]\n\
             s=socket.socket(socket.AF_UNIX); s.bind(spec.split(':')[0]); s.listen()\n\
             def serve(c):\n\
             \x20with c, c.makefile('rb') as f:\n\
             \x20 for line in f:\n\
             \x20  req=json.loads(line)\n\
             \x20  if req.get('cmd')=='ping':\n\
             \x20   hid=open('{id}').read()\n\
             \x20   data={{'id':req['id'],'type':'pong','version':'{v}','protocol':{p},'identity':{{'host_id':hid,'instance_id':'run','os':'fixture','account':'fixture'}}}}\n\
             \x20   c.sendall((json.dumps(data)+'\\n').encode())\n\
             while True:\n\
             \x20c,_=s.accept();threading.Thread(target=serve,args=(c,),daemon=True).start()\n",
            id = id_file.display(),
            v = env!("CARGO_PKG_VERSION"),
            p = skiff_core::PROTOCOL,
        );
        let ssh = dir.join("fake-ssh");
        std::fs::write(&ssh, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&id_file, "A").unwrap();
        let file = dir.join("hosts.toml");
        std::fs::write(&file, "selected = \"h_fixture\"\n[[host]]\nid = \"h_fixture\"\nname = \"fixture\"\ndestination = \"fixture\"\nsocket = \"/fixture.sock\"\n").unwrap();
        let target = skiff_client::host::ssh::SshTarget::new("fixture", "/fixture.sock");
        let options = skiff_client::host::ssh::SshOptions { program: ssh, config: None };
        let selection = |pin: Option<&str>| {
            let pin = pin.map(|p| skiff_host_api::HostId::try_from(p.to_string()).unwrap());
            HostSelection::ssh(target.clone(), options.clone(), dir.join("fwd"), pin).unwrap()
        };
        let disk_pin = || crate::hosts::load(&file).unwrap().hosts[0].host_id.clone();
        let changed = |r: &anyhow::Result<Arc<SelectedHost>>| matches!(r.as_ref().err().and_then(|e| e.downcast_ref()), Some(skiff_host_api::Error::IdentityChanged { .. }));
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let app = App::default();
            let first = selection(None);
            // Only the store fails: the new file cannot be written beside.
            let obstruction = file.with_extension(format!("toml.{}.tmp", std::process::id()));
            std::fs::create_dir(&obstruction).unwrap();
            for attempt in 0..2 {
                let r = try_remote(&app, &first, "fixture", None, file.clone()).await;
                assert!(r.is_err(), "attempt {attempt} connected with no stored pin");
                assert_eq!(first.pinned(), None, "attempt {attempt} pinned an unstored identity");
                assert_eq!(disk_pin(), None);
            }
            std::fs::remove_dir(&obstruction).unwrap();
            let retry = try_remote(&app, &first, "fixture", None, file.clone()).await;
            assert!(retry.is_ok(), "{:?}", retry.err());
            assert_eq!(disk_pin().as_deref(), Some("A"), "the retry connected without storing its pin");
            assert_eq!(first.pinned().map(|p| p.as_str().to_string()).as_deref(), Some("A"));
            drop(retry);
            // Another Host answers there now.
            std::fs::write(&id_file, "B").unwrap();
            assert!(changed(&try_remote(&app, &first, "fixture", None, file.clone()).await), "the same selection accepted B");
            drop(first);
            let rebuilt = selection(disk_pin().as_deref());
            let r = try_remote(&app, &rebuilt, "fixture", None, file.clone()).await;
            assert!(changed(&r), "a rebuilt selection accepted B: {:?}", r.err());
            assert_eq!(disk_pin().as_deref(), Some("A"));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
