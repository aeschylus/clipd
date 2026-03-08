use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::clipboard::{detect_content_type, detect_source_app, sha256_hex, ClipboardPoller};
use crate::config::Config;
use crate::models::ClipEntry;
use crate::store::Store;

/// Run the main daemon loop.
///
/// This is an async function intended to run inside a Tokio runtime.
/// It polls the clipboard at `config.poll_interval_ms`, stores new entries,
/// and evicts old ones to stay within `config.max_history`.
///
/// Graceful shutdown: the loop exits when the Tokio runtime is dropped
/// or when a SIGTERM/SIGINT is received (handled by main).
pub async fn run(config: Config) -> Result<()> {
    info!("clipd daemon starting");

    // Ensure storage directories exist
    config.ensure_dirs().context("creating storage directories")?;

    let store = Store::open(&config.db_path).context("opening clip store")?;
    let mut poller = ClipboardPoller::new();
    let interval = Duration::from_millis(config.poll_interval_ms);
    let mut ticker = time::interval(interval);
    let mut ticks_since_evict: u64 = 0;

    info!(
        db = %config.db_path.display(),
        poll_ms = config.poll_interval_ms,
        max_history = config.max_history,
        "daemon loop started"
    );

    // Set up graceful shutdown signal handling
    #[cfg(unix)]
    let mut sigterm = {
        use tokio::signal::unix::{signal, SignalKind};
        signal(SignalKind::terminate()).context("registering SIGTERM handler")?
    };

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(e) = tick(&store, &mut poller, &config) {
                    error!("tick error: {:#}", e);
                }

                ticks_since_evict += 1;
                // Evict every ~60 seconds (120 ticks at 500ms default)
                if ticks_since_evict >= 120 {
                    ticks_since_evict = 0;
                    if let Err(e) = store.evict_old(config.max_history) {
                        warn!("eviction error: {:#}", e);
                    } else {
                        debug!("eviction pass complete");
                    }
                }
            }

            // Graceful shutdown on SIGTERM (Unix)
            #[cfg(unix)]
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down");
                break;
            }

            // Graceful shutdown on Ctrl-C
            _ = tokio::signal::ctrl_c() => {
                info!("received SIGINT, shutting down");
                break;
            }
        }
    }

    info!("clipd daemon stopped");
    Ok(())
}

/// One clipboard poll tick.
fn tick(store: &Store, poller: &mut ClipboardPoller, config: &Config) -> Result<()> {
    let Some(content) = poller.poll()? else {
        return Ok(()); // No change
    };

    // Apply minimum length filter
    if content.len() < config.min_content_len {
        return Ok(());
    }

    // Detect source app (best-effort)
    let source_app = detect_source_app();

    // Check if source app is in the ignore list
    if let Some(ref app) = source_app {
        if config.should_ignore_app(app) {
            debug!(app = %app, "ignoring clipboard from blocked app");
            return Ok(());
        }
    }

    let hash = sha256_hex(&content);
    let content_type = detect_content_type(&content);

    let entry = ClipEntry::new(content.clone(), content_type.clone(), hash, source_app.clone());

    match store.insert(&entry) {
        Ok(id) => {
            info!(
                id = id,
                content_type = %content_type,
                source_app = ?source_app,
                preview = %entry.preview(60),
                "captured clip"
            );
        }
        Err(e) => {
            error!("failed to store clip: {:#}", e);
        }
    }

    Ok(())
}

/// Write the daemon PID to the pid file.
pub fn write_pid(pid_path: &Path) -> Result<()> {
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pid = std::process::id();
    std::fs::write(pid_path, pid.to_string())?;
    info!(pid = pid, path = %pid_path.display(), "wrote PID file");
    Ok(())
}

/// Read the PID from the pid file.  Returns None if the file doesn't exist.
pub fn read_pid(pid_path: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Remove the pid file.
pub fn remove_pid(pid_path: &Path) {
    let _ = std::fs::remove_file(pid_path);
}

/// Check if the process with the given PID is alive (Unix).
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    // kill(pid, 0) returns 0 if process exists, ESRCH if not
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
pub fn process_alive(pid: u32) -> bool {
    // On Windows: try OpenProcess
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains(&pid.to_string())
        })
        .unwrap_or(false)
}
