use super::*;

fn lines(stderr: &str) -> Vec<(Row, State, String)> {
    with(stderr, None, None).into_iter().map(|l| (l.row, l.state, l.text)).collect()
}

fn with(stderr: &str, local: Option<&LocalKey>, installed: Option<&Installed>) -> Vec<Line> {
    let mut list = List::default();
    login_failure(&mut list, stderr, "devbox", "devbox.lan", local, installed);
    list.finish(None, None).rows
}

fn fix(stderr: &str, local: Option<&LocalKey>, installed: Option<&Installed>) -> (String, Option<Fix>) {
    let f = with(stderr, local, installed).into_iter().find(|l| l.state == State::Failed).unwrap();
    (f.text, f.fix)
}

fn probe() -> Probe {
    Probe {
        os: "Linux".into(),
        arch: "x86_64".into(),
        runtime_dir: "/run/user/1000".into(),
        socket: "/run/user/1000/skiff/skiffd.sock".into(),
        socket_live: false,
        status: None,
        managed: None,
        app: String::new(),
        systemd: true,
        unit_enabled: false,
        unit_active: false,
        host_unit_enabled: false,
        linger: "no".into(),
        fallback: false,
        free_kb: 1 << 20,
    }
}

fn server(p: &Probe) -> (bool, Vec<(Row, State, Option<Fix>, String)>) {
    let mut list = List::default();
    let go = server_rows(&mut list, p, "devbox", "2.0.0", None, None);
    (go, list.rows.into_iter().map(|l| (l.row, l.state, l.fix, l.text)).collect())
}

#[test]
fn the_server_rows_install_start_or_leave_alone() {
    // Nothing there: Install.
    let (go, rows) = server(&probe());
    assert!(!go);
    assert_eq!((rows[1].0, rows[1].2), (Row::Skiff, Some(Fix::InstallSkiff)));
    // Another Skiff runs there: used as it is, never an Install.
    let other = Probe { socket_live: true, status: Some(Status { skiff: "1.6.6".into(), skiffd: Some(StatusDaemon { version: "0.6.24".into(), protocol: 27 }) }), ..probe() };
    let (go, rows) = server(&other);
    assert!(go && rows.iter().all(|r| r.2.is_none()), "{rows:?}");
    assert!(rows[1].3.contains("own install"));
    // Ours, stopped: Start. Ours, older: Update. Ours, newer: never down.
    let ours = Probe { managed: Some(Managed { version: "2.0.0".into(), protocol: 27, running: String::new(), build: "e8368d7".into(), running_build: "e8368d7".into() }), ..probe() };
    let (go, rows) = server(&ours);
    assert!(!go);
    assert_eq!((rows[2].0, rows[2].2), (Row::Sessions, Some(Fix::StartSkiff)));
    // The same version from an earlier dev build: Update, to this
    // app's build. A dev app's own build differs from a release.
    let dev = Probe { managed: Some(Managed { version: "2.0.0".into(), protocol: 27, build: "dev".into(), ..Default::default() }), socket_live: true, ..probe() };
    let (_, rows) = server(&dev);
    assert_eq!((rows[1].1, rows[1].2), (State::Warning, Some(Fix::UpdateSkiff)), "{rows:?}");
    assert!(rows[1].3.contains("another build"));
    assert!(super::super::install::same_build(Some("dev-1"), "dev-1") && !super::super::install::same_build(Some("dev-1"), "dev-2") && !super::super::install::same_build(Some("dev-1"), "e8368d7"));
    assert!(super::super::install::same_build(None, "e8368d7") && !super::super::install::same_build(None, "dev") && !super::super::install::same_build(None, "dev-1") && !super::super::install::same_build(None, ""));
    let older = Probe { managed: Some(Managed { version: "1.9.0".into(), protocol: 27, running: String::new(), build: "e8368d7".into(), running_build: "e8368d7".into() }), socket_live: true, ..probe() };
    let (go, rows) = server(&older);
    assert!(go);
    assert_eq!((rows[1].1, rows[1].2), (State::Warning, Some(Fix::UpdateSkiff)));
    assert!(rows[1].3.starts_with("Update available:"));
    let newer_one = Probe { managed: Some(Managed { version: "2.10.0".into(), protocol: 27, running: String::new(), build: "e8368d7".into(), running_build: "e8368d7".into() }), socket_live: true, ..probe() };
    let (_, rows) = server(&newer_one);
    assert_eq!((rows[1].1, rows[1].2), (State::Warning, None));
    assert!(rows[1].3.contains("Update Skiff here"));
    // An update that stopped after it moved `current`: the skiffd that
    // runs is still the old bundle's, and the row goes by it.
    let stopped = Probe { managed: Some(Managed { version: "2.0.0".into(), protocol: 27, running: "1.9.0".into(), build: "e8368d7".into(), running_build: "e8368d7".into() }), ..ours.clone() };
    assert_eq!(stopped.managed_version(), Some("1.9.0"));
    let (_, rows) = server(&Probe { socket_live: true, ..stopped });
    assert_eq!((rows[1].1, rows[1].2), (State::Warning, Some(Fix::UpdateSkiff)), "{rows:?}");
    // Not a Skiff platform: the row says what it found.
    let (go, rows) = server(&Probe { os: "FreeBSD".into(), arch: "amd64".into(), ..probe() });
    assert!(!go && rows[0].3.contains("FreeBSD on amd64"));
    // A Mac server: Install, the same as Linux.
    let (_, rows) = server(&Probe { os: "Darwin".into(), arch: "arm64".into(), systemd: false, ..probe() });
    assert_eq!((rows[1].1, rows[1].2), (State::Failed, Some(Fix::InstallSkiff)));
    // The Skiff app there, closed: no Install, only where to go.
    let (go, rows) = server(&Probe { app: "/Applications/Skiff.app/Contents/MacOS/skiffd".into(), ..probe() });
    assert!(!go && rows[1].1 == State::Failed && rows[1].2.is_none() && rows[1].3.contains("Open it there"), "{rows:?}");
}

#[test]
fn versions_compare_by_number() {
    assert!(newer("2.10.0", "2.9.9") && newer("2.0.1", "2.0.0") && newer("3", "2.9"));
    assert!(!newer("2.0.0", "2.0.0") && !newer("2.0.0-rc.1", "2.0.0") && !newer("1.6.6", "2.0.0"));
}

#[test]
fn keeping_it_running_is_a_warning_with_one_fix() {
    let ours = Probe { managed: Some(Managed { version: "2.0.0".into(), protocol: 27, running: String::new(), build: "e8368d7".into(), running_build: "e8368d7".into() }), unit_enabled: true, host_unit_enabled: true, ..probe() };
    let k = |p: &Probe, refused| { let l = keep_running(p, "devbox", "jose", refused); (l.state, l.fix) };
    assert_eq!(k(&ours, false), (State::Warning, Some(Fix::TurnOnLinger)));
    assert_eq!(k(&ours, true), (State::Warning, None), "refused: plain words only");
    assert_eq!(k(&Probe { linger: "yes".into(), ..ours.clone() }, false), (State::Passed, None));
    assert_eq!(k(&Probe { systemd: false, ..ours.clone() }, false), (State::Warning, None));
    assert_eq!(k(&probe(), false), (State::Passed, None), "not ours: not our business");
    // An install from before skiff-host had its unit: Start adds it.
    assert_eq!(k(&Probe { host_unit_enabled: false, ..ours.clone() }, false), (State::Warning, Some(Fix::StartSkiff)));
    // macOS: the LaunchAgent files decide; linger means nothing there.
    let mac = Probe { os: "Darwin".into(), arch: "arm64".into(), systemd: false, linger: String::new(), ..ours.clone() };
    assert_eq!(k(&mac, false), (State::Passed, None));
    assert_eq!(k(&Probe { unit_enabled: false, ..mac.clone() }, false), (State::Warning, Some(Fix::StartSkiff)));
    assert_eq!(k(&Probe { host_unit_enabled: false, ..mac }, false), (State::Warning, Some(Fix::StartSkiff)));
    // A warning still lets the Host be used.
    let mut list = List::default();
    for row in Row::ALL.into_iter().take(9) {
        list.pass(row, String::new());
    }
    list.rows.push(keep_running(&ours, "devbox", "jose", false));
    assert!(list.finish(None, None).ok);
}

#[test]
fn the_projects_row_is_a_warning_with_start_for_our_install_only() {
    let p = |answer, managed| { let l = projects(answer, managed); (l.state, l.fix, l.text) };
    assert_eq!(p(Ok(" 2.0.0".into()), true), (State::Passed, None, "skiff-host 2.0.0 answers.".into()));
    let down = || Err(anyhow::anyhow!(super::super::data::DataError::NotRunning));
    let (state, fix, text) = p(down(), true);
    assert_eq!((state, fix), (State::Warning, Some(Fix::StartSkiff)));
    assert!(text.contains("Sessions work."), "{text}");
    assert_eq!(p(down(), false).1, None, "not ours: no Start");
    let other = skiff_host_api::Error::IdentityChanged { expected: "a".to_string().try_into().unwrap(), received: "b".to_string().try_into().unwrap() };
    assert_eq!(p(Err(other.into()), true).1, None, "another install: no Start");
}

#[test]
fn a_refused_login_names_the_key_case_and_its_one_fix() {
    let pw = "me@devbox.lan: Permission denied (publickey,password).\n";
    let keys = "me@devbox.lan: Permission denied (publickey).\n";
    let ready = LocalKey::Ready("ssh-ed25519 AAAA".into());
    assert_eq!(fix(pw, Some(&LocalKey::None), None), ("This computer has no SSH key.".into(), Some(Fix::MakeKey)));
    assert_eq!(fix(pw, Some(&LocalKey::Locked("/k".into())), None).1, Some(Fix::UnlockKey));
    assert_eq!(fix(pw, Some(&ready), None).1, Some(Fix::InstallKey));
    assert_eq!(fix(keys, Some(&ready), None).1, Some(Fix::CopyPublicKey));
    let foreign = Installed { added: true, ssh_dir_own: true, file_own: false };
    let (text, f) = fix(pw, Some(&ready), Some(&foreign));
    assert_eq!(f, Some(Fix::FixPermissions));
    assert!(text.contains("cannot read your key file"), "{text}");
    let lost = Installed { added: false, ssh_dir_own: false, file_own: false };
    assert_eq!(fix(pw, Some(&ready), Some(&lost)).1, None, "no fix when ~/.ssh is not the account's");
    // A good install says nothing: the next refusal is plain again.
    let good = Installed { added: true, ssh_dir_own: true, file_own: true };
    assert_eq!(fix(pw, Some(&ready), Some(&good)).1, Some(Fix::InstallKey));
    let unknown = "No ED25519 host key is known for devbox.lan and you have requested strict checking.\nHost key verification failed.\n";
    assert_eq!(fix(unknown, None, None).1, Some(Fix::TrustKey));
    assert_eq!(fix("@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @\n", None, None).1, Some(Fix::ReviewKey));
}

fn failed(stderr: &str) -> (Row, String) {
    let rows = lines(stderr);
    let f = rows.iter().find(|r| r.1 == State::Failed).expect("a failed row");
    // Every row after it is skipped, and nothing repeats ssh's output.
    let at = rows.iter().position(|r| r.1 == State::Failed).unwrap();
    assert!(rows[at + 1..].iter().all(|r| r.1 == State::Skipped), "{rows:?}");
    assert!(!f.2.contains("ssh:") && !f.2.contains(".ssh/"), "{}", f.2);
    (f.0, f.2.clone())
}

#[test]
fn each_ssh_failure_names_one_row_and_one_plain_cause() {
    let cases = [
        ("ssh: Could not resolve hostname devbox.lann: Name or service not known\n", Row::Reach, "cannot find devbox.lan"),
        ("ssh: connect to host 10.0.4.21 port 22: Connection timed out\n", Row::Reach, "in 15 s"),
        ("ssh: connect to host 10.0.4.21 port 22: No route to host\n", Row::Reach, "does not answer"),
        ("@@@\n@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @\nHost key verification failed.\n", Row::HostKey, "host key changed"),
        ("No ED25519 host key is known for devbox.lan and you have requested strict checking.\nHost key verification failed.\n", Row::HostKey, "Not known yet"),
        ("me@devbox.lan: Permission denied (publickey).\n", Row::Login, "known keys only"),
        ("me@devbox.lan: Permission denied (publickey,password).\n", Row::Login, "key is not on the server"),
        ("kex_exchange_identification: read: Connection reset by peer\n", Row::Reach, "does not answer"),
        ("something new\n", Row::Login, "unknown reason"),
    ];
    for (stderr, row, words) in cases {
        let (got, text) = failed(stderr);
        assert_eq!(got, row, "{stderr}");
        assert!(text.contains(words), "{stderr}: {text}");
    }
}

#[test]
fn the_probe_command_is_fixed_and_quotes_its_values() {
    let t = SshTarget { user: Some("me".into()), port: Some(2222), ..SshTarget::new("devbox", "") };
    let command = probe_command();
    let args: Vec<String> = ssh::batch_args(&t, &SshOptions { program: "ssh".into(), config: None }, command)
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect();
    for want in ["BatchMode=yes", "StrictHostKeyChecking=yes", "ControlPath=none", "ForwardAgent=no"] {
        assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
    }
    assert!(!args.iter().any(|a| a.contains("accept-new") || a.contains("StrictHostKeyChecking=no")));
    let n = args.len();
    assert_eq!(&args[n - 7..], ["-l", "me", "-p", "2222", "--", "devbox", "sh -s -- probe"]);
}

/// The probe as the server runs it: here, with `sh`, no ssh. It looks
/// at the one socket, `<runtime dir>/skiff/skiffd.sock`, whatever runs
/// on this machine.
#[test]
fn the_probe_prints_one_object_and_finds_the_socket() {
    let dir = std::env::temp_dir().join(format!("skiff-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("skiff")).unwrap();
    let run = || {
        let mut child = std::process::Command::new("sh")
            .args(["-s", "--", "probe"])
            .env("XDG_RUNTIME_DIR", &dir)
            .env("HOME", &dir)
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), SERVER.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        serde_json::from_slice::<Probe>(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
    };
    let sock = dir.join("skiff/skiffd.sock");
    let none = run();
    assert_eq!((none.runtime_dir.as_str(), none.socket.as_str(), none.socket_live), (&*dir.to_string_lossy(), &*sock.to_string_lossy(), false), "{none:?}");
    // Another name in the folder is not Skiff's socket.
    let _other = std::os::unix::net::UnixListener::bind(dir.join("skiff/skiffd-x.sock")).unwrap();
    assert!(!run().socket_live);
    let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let p = run();
    assert_eq!(p.socket, sock.to_string_lossy());
    assert!(p.socket_live && p.status.is_none(), "{p:?}");
    // The Skiff app's skiffd, when this machine has one (a .deb here).
    assert!(p.app.is_empty() || std::path::Path::new(&p.app).is_file(), "{p:?}");
    assert!(!p.os.is_empty() && !p.arch.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
