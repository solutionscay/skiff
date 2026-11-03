use super::*;

#[tokio::test]
async fn oversized_stdout_is_rejected_without_returning_it() {
    let (mut writer, mut reader) = tokio::io::duplex(MAX_BATCH_OUTPUT + 1);
    let data = vec![b'x'; MAX_BATCH_OUTPUT + 1];
    let write = tokio::spawn(async move { writer.write_all(&data).await });
    assert_eq!(
        read_stdout(&mut reader).await,
        Err(SshError::OutputTooLarge)
    );
    write.await.unwrap().unwrap();
}

#[tokio::test]
async fn oversized_unterminated_stderr_line_is_rejected() {
    let (mut writer, reader) = tokio::io::duplex(MAX_STDERR_LINE + 1);
    let data = vec![b'x'; MAX_STDERR_LINE + 1];
    let write = tokio::spawn(async move { writer.write_all(&data).await });
    let mut reader = BufReader::new(reader);
    assert_eq!(
        read_stderr_line(&mut reader).await,
        Err(SshError::OutputTooLarge)
    );
    write.await.unwrap().unwrap();
}

fn target() -> SshTarget {
    SshTarget::new("devbox", "/run/user/501/skiff/skiffd.sock")
}

#[test]
fn the_forward_keeps_strict_trust_and_never_takes_the_destination_as_an_option() {
    let args = forward_args(
        &target(),
        Path::new("/tmp/skiff-x/skiffd.sock"),
        &SshOptions {
            program: "ssh".into(),
            config: None,
        },
    );
    let args: Vec<String> = args.into_iter().map(|a| a.into_string().unwrap()).collect();
    for want in [
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "UpdateHostKeys=no",
        "ExitOnForwardFailure=yes",
        "ControlPath=none",
        "ForwardAgent=no",
    ] {
        let at = args
            .iter()
            .position(|a| a == want)
            .unwrap_or_else(|| panic!("no {want}: {args:?}"));
        assert_eq!(args[at - 1], "-o");
    }
    // Nothing turns host key checking off or accepts a new key.
    assert!(!args.iter().any(|a| a.contains("StrictHostKeyChecking=no")
        || a.contains("accept-new")
        || a.contains("UserKnownHostsFile")));
    assert!(!args.contains(&"-F".to_string()));
    let n = args.len();
    assert_eq!(
        &args[n - 5..],
        [
            "-N",
            "-L",
            "/tmp/skiff-x/skiffd.sock:/run/user/501/skiff/skiffd.sock",
            "--",
            "devbox"
        ]
    );
    let with = forward_args(
        &target(),
        Path::new("/x"),
        &SshOptions {
            program: "ssh".into(),
            config: Some("/fixture/ssh_config".into()),
        },
    );
    assert_eq!(
        with[..2],
        [OsString::from("-F"), OsString::from("/fixture/ssh_config")]
    );
    // User and port go before `--`, and only when set.
    let set = SshTarget {
        user: Some("jose".into()),
        port: Some(2222),
        ..target()
    };
    let args: Vec<String> = forward_args(&set, Path::new("/x"), &SshOptions::default())
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect();
    let n = args.len();
    assert_eq!(&args[n - 9..n - 5], ["-l", "jose", "-p", "2222"]);
    assert!(SshTarget {
        user: Some("-oX".into()),
        ..target()
    }
    .validate()
    .is_err());
    assert!(SshTarget {
        port: Some(0),
        ..target()
    }
    .validate()
    .is_err());
}

#[test]
fn only_allowlisted_ssh_lines_show_and_never_a_key_path() {
    let shown = |l: &str| shown_line(l);
    assert_eq!(
        shown("debug1: Connecting to p0p [192.168.1.40] port 22.").as_deref(),
        Some("Connecting to p0p [192.168.1.40] port 22.")
    );
    assert_eq!(
        shown("debug1: Connection established.").as_deref(),
        Some("Connection established.")
    );
    assert!(shown("debug1: Host 'p0p' is known and matches the ED25519 host key.").is_some());
    assert!(
        shown("debug1: Authenticated to p0p ([192.168.1.40]:22) using \"publickey\".").is_some()
    );
    assert!(shown("ssh: connect to host p0p port 22: Connection timed out").is_some());
    for hidden in [
        "debug1: Will attempt key: /home/me/.ssh/id_ed25519 ED25519 SHA256:abc",
        "debug1: Offering public key: /home/me/.ssh/id_ed25519",
        "debug1: identity file /home/me/.ssh/id_rsa type -1",
        "debug1: Reading configuration data /home/me/.ssh/config",
        "debug1: client_input_global_request: rtype keepalive",
        "Load key \"/home/me/.ssh/id\": bad permissions",
        "debug1: Server host key: ssh-ed25519 SHA256:abc",
    ] {
        assert_eq!(shown(hidden), None, "{hidden}");
    }
}

#[test]
fn targets_that_ssh_could_misread_are_refused() {
    assert!(target().validate().is_ok());
    assert!(SshTarget {
        destination: "me@devbox".into(),
        ..target()
    }
    .validate()
    .is_ok());
    for d in ["", "-oProxyCommand=x", "dev box", "dev\nbox"] {
        assert!(
            SshTarget {
                destination: d.into(),
                ..target()
            }
            .validate()
            .is_err(),
            "{d:?}"
        );
    }
    for s in ["relative.sock", "/a:b.sock", "/a\n.sock", ""] {
        assert!(
            SshTarget {
                socket: s.into(),
                ..target()
            }
            .validate()
            .is_err(),
            "{s:?}"
        );
    }
    // Spaces are fine: a macOS socket lives under Application Support.
    assert!(SshTarget {
        socket: "/Users/me/Library/Application Support/skiff/skiffd.sock".into(),
        ..target()
    }
    .validate()
    .is_ok());
}

#[test]
fn ssh_output_maps_to_one_cause_without_repeating_it() {
    let t = target();
    let cases = [
        ("@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\nHost key verification failed.\n", SshError::HostKeyChanged("devbox".into())),
        ("No ED25519 host key is known for [127.0.0.1]:42022 and you have requested strict checking.\nHost key verification failed.\n", SshError::HostKeyUnknown("devbox".into())),
        ("me@devbox: Permission denied (publickey).\n", SshError::AuthFailed("devbox".into())),
        ("channel 2: open failed: administratively prohibited: open failed\n", SshError::ForwardingRefused("devbox".into())),
        // OpenSSH 9.7 prints this for a missing socket and for disabled forwarding alike.
        ("channel 1: open failed: connect failed: open failed\n", SshError::NoSkiffd { destination: "devbox".into(), socket: t.socket.clone() }),
        ("ssh: Could not resolve hostname devbox: nodename nor servname provided\n", SshError::Unreachable("devbox".into())),
    ];
    for (stderr, want) in cases {
        assert_eq!(classify(stderr, &t), Some(want), "{stderr}");
    }
    assert_eq!(
        classify("Load key \"/Users/me/.ssh/id\": bad permissions\n", &t),
        None
    );
    // The probe splits the one line in two.
    let refused = SshError::NoSkiffd {
        destination: "devbox".into(),
        socket: t.socket.clone(),
    };
    assert_eq!(refused.clone().refine(false), refused);
    assert_eq!(
        refused.refine(true),
        SshError::ForwardingRefused("devbox".into())
    );
    // The messages name the destination and never ssh's own lines.
    let text = SshError::AuthFailed("devbox".into()).to_string();
    assert!(text.contains("devbox") && !text.contains(".ssh"));
}

#[test]
fn a_shared_or_foreign_directory_is_refused_not_repaired() {
    let dir = std::env::temp_dir().join(format!("skiff-ssh-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    private_dir(&dir).unwrap();
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_dir(&dir).is_err());
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o755
    );
    std::fs::remove_dir(&dir).unwrap();
}
