use anyhow::{bail, Context, Result};
use skiff_core::socket::socket_path;
use skiffd::{server, session::SessionPool, shellenv};
use tokio::net::{UnixListener, UnixStream};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Before the runtime starts any thread: it sets environment variables.
    shellenv::import();
    tokio::runtime::Runtime::new()?.block_on(run())
}

async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let path = socket_path();
    // Only a directory we create gets locked down. Never chmod a shared one like /tmp.
    if let Some(dir) = path.parent().filter(|d| !d.exists()) {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    if path.exists() {
        // A live socket means another daemon. A dead one is leftover from a crash.
        if UnixStream::connect(&path).await.is_ok() {
            bail!("skiffd is already running at {}", path.display());
        }
        std::fs::remove_file(&path)?;
    }

    let listener =
        UnixListener::bind(&path).with_context(|| format!("bind {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    let pool = SessionPool::new();
    pool.restore();
    pool.spawn_saver();
    pool.spawn_idle_watcher();
    pool.spawn_flusher();
    pool.spawn_away_watcher();
    pool.spawn_foreground_watcher();
    tracing::info!("skiffd {} listening on {}", skiff_core::VERSION, path.display());

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let pool = pool.clone();
                tokio::spawn(async move {
                    if let Err(e) = server::handle(stream, pool).await {
                        tracing::debug!("client closed: {e:#}");
                    }
                });
            }
            _ = &mut ctrl_c => break,
            _ = term.recv() => break,
        }
    }

    tracing::info!("skiffd shutting down");
    pool.save_now();
    let _ = std::fs::remove_file(&path);
    Ok(())
}
