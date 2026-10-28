use portable_pty::CommandBuilder;
use skiff_core::session::SessionSpec;
use std::path::Path;
use super::{default_shell, SHELL_HANDOFF_TITLE};

/// The command for the PTY, and whether it is the shell itself.
pub(super) fn launch_command(spec: &SessionSpec, command: &str, cwd: &Path, id: &str) -> (CommandBuilder, bool) {
    // A peek tool runs as given and ends with its program: the peek closes then.
    if spec.peek {
        let mut cmd = CommandBuilder::new(command);
        cmd.args(&spec.args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // `sh -c` sets no size, and tools such as delta read it to lay out.
        cmd.env("COLUMNS", spec.cols.to_string());
        cmd.env("LINES", spec.rows.to_string());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        return (cmd, false);
    }
    // An alias from the user's rc files stands for a command line.
    let (program, args) = match skiff_core::alias::expand(command) {
        Some(w) => (w[0].clone(), [&w[1..], &spec.args[..]].concat()),
        None => (command.to_string(), spec.args.clone()),
    };
    // A program that is not the shell hands the terminal to a shell when it
    // exits, so Ctrl+C in an agent leaves a prompt instead of a dead pane.
    // Ctrl+C signals the whole foreground group, the wrapper included; the
    // `trap :` keeps the wrapper alive, and unlike `trap ''` the child does
    // not inherit it, so the agent still gets SIGINT as normal. The title
    // carries the program's exit code, which the hand-off would lose.
    // Keep the daemon's prepared environment. A login shell resets PATH;
    // another Ctrl+C during profile loading can leave agent commands missing.
    let shell = default_shell();
    let is_shell = program == shell
        || std::path::Path::new(&program).file_name() == std::path::Path::new(&shell).file_name();
    let (program, args) = if is_shell {
        (program, args)
    } else {
        let mut wrapped = vec![
            "-c".to_string(),
            format!(
                "trap : INT; \"$@\"; printf '\\033]0;{SHELL_HANDOFF_TITLE}:%s\\007' \"$?\"; exec \"$SKIFF_SHELL\" -i"
            ),
            "skiff".to_string(),
        ];
        // The client's variables go to the program alone, through `env`, so
        // the fallback shell after it does not keep them.
        if !spec.env.is_empty() {
            wrapped.push("env".to_string());
            wrapped.extend(spec.env.iter().map(|(k, v)| format!("{k}={v}")));
        }
        wrapped.push(program);
        wrapped.extend(args);
        ("/bin/sh".to_string(), wrapped)
    };
    let mut cmd = CommandBuilder::new(&program);
    cmd.args(&args);
    cmd.env("SKIFF_SHELL", &shell);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // So the `skiff` CLI in a pane knows its own session and reaches this daemon.
    cmd.env("SKIFF_SESSION", id);
    cmd.env("SKIFF_SOCKET", crate::paths::socket());
    // Prefer the CLI shipped beside this daemon over a dev CLI on PATH.
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        let mut paths = vec![dir];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        if let Ok(path) = std::env::join_paths(paths) {
            cmd.env("PATH", path);
        }
    }
    if is_shell {
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
    }
    (cmd, is_shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_cli_directory_comes_first_on_path() {
        let (cmd, _) = launch_command(&SessionSpec::default(), "opencode", Path::new("/"), "s1");
        let path = cmd.get_env("PATH").expect("pane PATH");
        let paths: Vec<_> = std::env::split_paths(path).collect();
        assert_eq!(paths[0], std::env::current_exe().unwrap().parent().unwrap());
        assert_eq!(paths[1..], std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>());
    }

    #[test]
    fn variables_go_to_the_program_alone() {
        let spec = SessionSpec { args: vec!["--x".into()], env: vec![("OPENCODE_CONFIG_CONTENT".into(), r#"{"instructions":["/a b.md"]}"#.into())], ..Default::default() };
        let (cmd, shell) = launch_command(&spec, "opencode", Path::new("/"), "s1");
        assert!(!shell);
        let argv: Vec<String> = cmd.get_argv().iter().map(|w| w.to_string_lossy().into_owned()).collect();
        // `env` in the wrapper: the fallback shell after the program does not keep them.
        assert_eq!(argv[3..], ["skiff", "env", r#"OPENCODE_CONFIG_CONTENT={"instructions":["/a b.md"]}"#, "opencode", "--x"]);
        assert!(cmd.get_env("OPENCODE_CONFIG_CONTENT").is_none());
        let plain = SessionSpec::default();
        let (cmd, _) = launch_command(&plain, "opencode", Path::new("/"), "s1");
        assert!(!cmd.get_argv().iter().any(|w| w == "env"));
    }
}
