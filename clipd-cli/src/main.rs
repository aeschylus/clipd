use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::process;

use clipd_core::config::{self, Config};
use clipd_core::daemon::{process_alive, read_pid, remove_pid, write_pid};
use clipd_core::store::Store;

// ─── CLI definition ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "clipd",
    version,
    about = "Headless cross-platform clipboard history daemon",
    long_about = "clipd monitors your clipboard in the background, stores every unique copy \
                  with full metadata (source app, timestamp, content type), and exposes a \
                  search/query CLI — like Paste.app but without a GUI."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonCommands,
    },

    /// List recent clipboard entries
    List {
        /// Maximum number of entries to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,

        /// Full-text search term
        #[arg(short, long)]
        search: Option<String>,

        /// Output format (table, json, csv)
        #[arg(short, long, default_value = "table")]
        format: OutputFormat,
    },

    /// Print full content of a clip by ID
    Get {
        /// Clip ID (from `clipd list`)
        id: i64,

        /// Output raw content only (no metadata)
        #[arg(short, long)]
        raw: bool,
    },

    /// Pin or unpin a clip (pinned clips are never evicted)
    Pin {
        /// Clip ID
        id: i64,

        /// Unpin instead of pin
        #[arg(long)]
        unpin: bool,
    },

    /// Add a tag to a clip
    Tag {
        /// Clip ID
        id: i64,

        /// Tag to add
        tag: String,
    },

    /// Set a human-readable label on a clip (like Paste.app rename)
    Label {
        /// Clip ID
        id: i64,

        /// Label text (omit to clear the label)
        label: Option<String>,
    },

    /// Delete a clip by ID
    Delete {
        /// Clip ID
        id: i64,
    },

    /// Export clipboard history
    Export {
        /// Output format (json, csv)
        #[arg(short, long, default_value = "json")]
        format: ExportFormat,

        /// Maximum number of entries (default: all)
        #[arg(short = 'n', long)]
        limit: Option<usize>,
    },

    /// Show current configuration and storage paths
    Config,
}

#[derive(Subcommand)]
enum DaemonCommands {
    /// Start the daemon in the background (forks a new process)
    Start {
        /// Stay in foreground (useful for systemd / launchd / debugging)
        #[arg(long)]
        foreground: bool,
    },

    /// Stop the running daemon
    Stop,

    /// Show daemon status and statistics
    Status,
}

#[derive(clap::ValueEnum, Clone)]
enum OutputFormat {
    Table,
    Json,
    Csv,
}

#[derive(clap::ValueEnum, Clone)]
enum ExportFormat {
    Json,
    Csv,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    // Initialize logging (respects RUST_LOG env var)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("clipd=info".parse().unwrap()),
        )
        .with_target(false)
        .compact()
        .init();

    if let Err(e) = run(cli) {
        eprintln!("error: {:#}", e);
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let config = Config::load().context("loading configuration")?;

    match cli.command {
        Commands::Daemon { action } => handle_daemon(action, config),
        Commands::List { limit, search, format } => {
            handle_list(limit, search.as_deref(), format, &config)
        }
        Commands::Get { id, raw } => handle_get(id, raw, &config),
        Commands::Pin { id, unpin } => handle_pin(id, unpin, &config),
        Commands::Tag { id, tag } => handle_tag(id, tag, &config),
        Commands::Label { id, label } => handle_label(id, label, &config),
        Commands::Delete { id } => handle_delete(id, &config),
        Commands::Export { format, limit } => handle_export(format, limit, &config),
        Commands::Config => handle_config(&config),
    }
}

// ─── Command handlers ────────────────────────────────────────────────────────

fn handle_daemon(action: DaemonCommands, config: Config) -> Result<()> {
    match action {
        DaemonCommands::Start { foreground } => {
            // Check if already running
            if let Some(pid) = read_pid(&config.pid_path) {
                if process_alive(pid) {
                    bail!("daemon is already running (PID {})", pid);
                }
                // Stale PID file
                remove_pid(&config.pid_path);
            }

            if foreground {
                // Run in foreground — write PID then block
                write_pid(&config.pid_path)?;
                let pid_path = config.pid_path.clone();
                let result = start_daemon_foreground(config);
                remove_pid(&pid_path);
                result
            } else {
                // Daemonize: re-exec self with `--foreground` in background
                daemon_start_background(&config)
            }
        }

        DaemonCommands::Stop => {
            let Some(pid) = read_pid(&config.pid_path) else {
                bail!("daemon is not running (no PID file at {})", config.pid_path.display());
            };

            if !process_alive(pid) {
                eprintln!("daemon process {} is not alive, removing stale PID file", pid);
                remove_pid(&config.pid_path);
                return Ok(());
            }

            #[cfg(unix)]
            {
                use std::time::Duration;
                // Send SIGTERM
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
                // Wait up to 5s for exit
                for _ in 0..50 {
                    std::thread::sleep(Duration::from_millis(100));
                    if !process_alive(pid) {
                        remove_pid(&config.pid_path);
                        println!("daemon stopped");
                        return Ok(());
                    }
                }
                bail!("daemon did not stop within 5 seconds (PID {})", pid);
            }

            #[cfg(not(unix))]
            {
                // Windows: taskkill
                let status = process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .status()?;
                if status.success() {
                    remove_pid(&config.pid_path);
                    println!("daemon stopped");
                } else {
                    bail!("failed to stop daemon (PID {})", pid);
                }
                Ok(())
            }
        }

        DaemonCommands::Status => {
            let running = if let Some(pid) = read_pid(&config.pid_path) {
                if process_alive(pid) {
                    println!("daemon: running (PID {})", pid);
                    true
                } else {
                    println!("daemon: not running (stale PID file)");
                    false
                }
            } else {
                println!("daemon: not running");
                false
            };

            if running || config.db_path.exists() {
                match Store::open(&config.db_path) {
                    Ok(store) => {
                        let count = store.count().unwrap_or(0);
                        println!("clips stored: {}", count);
                        println!("database: {}", config.db_path.display());
                    }
                    Err(e) => eprintln!("could not open database: {}", e),
                }
            }

            Ok(())
        }
    }
}

fn start_daemon_foreground(config: Config) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building Tokio runtime")?;
    rt.block_on(clipd_core::daemon::run(config))
}

/// On Unix, re-exec the current binary with `daemon start --foreground`,
/// detached from the terminal via double-fork.
#[cfg(unix)]
fn daemon_start_background(config: &Config) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe().context("resolving current executable path")?;

    let _child = process::Command::new(&exe)
        .args(["daemon", "start", "--foreground"])
        .stdin(process::Stdio::null())
        .stdout(std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.log_path)
            .unwrap_or_else(|_| unsafe { std::fs::File::from_raw_fd(1) }))
        .stderr(std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.log_path)
            .unwrap_or_else(|_| unsafe { std::fs::File::from_raw_fd(2) }))
        // Double-fork via setsid to fully detach from the terminal
        .process_group(0)
        .spawn()
        .context("spawning background daemon process")?;

    // Give the child a moment to write its PID file before we print the message
    std::thread::sleep(std::time::Duration::from_millis(200));
    println!("daemon started (log: {})", config.log_path.display());
    Ok(())
}

#[cfg(not(unix))]
fn daemon_start_background(config: &Config) -> Result<()> {
    let exe = std::env::current_exe().context("resolving current executable path")?;

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.log_path)?;

    process::Command::new(&exe)
        .args(["daemon", "start", "--foreground"])
        .stdin(process::Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file)
        .spawn()
        .context("spawning background daemon process")?;

    std::thread::sleep(std::time::Duration::from_millis(200));
    println!("daemon started (log: {})", config.log_path.display());
    Ok(())
}

fn handle_list(
    limit: usize,
    search: Option<&str>,
    format: OutputFormat,
    config: &Config,
) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    let clips = store.list(limit, search)?;

    if clips.is_empty() {
        println!("(no clips found)");
        return Ok(());
    }

    match format {
        OutputFormat::Table => {
            println!("{:<6} {:<10} {:<20} {:<8} {}", "ID", "TYPE", "TIME", "PINNED", "PREVIEW");
            println!("{}", "-".repeat(90));
            for clip in &clips {
                println!(
                    "{:<6} {:<10} {:<20} {:<8} {}",
                    clip.id,
                    clip.content_type,
                    clip.created_at.format("%Y-%m-%d %H:%M"),
                    if clip.pinned { "yes" } else { "" },
                    clip.preview(48),
                );
            }
            println!("\n{} clip(s)", clips.len());
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&clips)?);
        }
        OutputFormat::Csv => {
            println!("id,content_type,created_at,pinned,source_app,preview");
            for clip in &clips {
                println!(
                    "{},{},{},{},{},\"{}\"",
                    clip.id,
                    clip.content_type,
                    clip.created_at.to_rfc3339(),
                    clip.pinned,
                    clip.source_app.as_deref().unwrap_or(""),
                    clip.preview(80).replace('"', "\"\""),
                );
            }
        }
    }
    Ok(())
}

fn handle_get(id: i64, raw: bool, config: &Config) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    let Some(clip) = store.get(id)? else {
        bail!("clip {} not found", id);
    };

    if raw {
        print!("{}", clip.content);
        return Ok(());
    }

    println!("ID:          {}", clip.id);
    println!("Type:        {}", clip.content_type);
    println!("Created:     {}", clip.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("Pinned:      {}", clip.pinned);
    if let Some(app) = &clip.source_app {
        println!("Source app:  {}", app);
    }
    if let Some(label) = &clip.label {
        println!("Label:       {}", label);
    }
    if !clip.tags.is_empty() {
        println!("Tags:        {}", clip.tags.join(", "));
    }
    println!("Hash:        {}…", &clip.hash[..16]);
    println!("\n--- Content ---\n{}", clip.content);

    Ok(())
}

fn handle_pin(id: i64, unpin: bool, config: &Config) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    let pinned = !unpin;
    if store.set_pinned(id, pinned)? {
        println!("clip {} {}pinned", id, if pinned { "" } else { "un" });
    } else {
        bail!("clip {} not found", id);
    }
    Ok(())
}

fn handle_tag(id: i64, tag: String, config: &Config) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    store.add_tag(id, &tag)?;
    println!("tagged clip {} with \"{}\"", id, tag);
    Ok(())
}

fn handle_label(id: i64, label: Option<String>, config: &Config) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    store.set_label(id, label.as_deref())?;
    match label {
        Some(l) => println!("labelled clip {} as \"{}\"", l, l),
        None => println!("cleared label on clip {}", id),
    }
    Ok(())
}

fn handle_delete(id: i64, config: &Config) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    if store.delete(id)? {
        println!("deleted clip {}", id);
    } else {
        bail!("clip {} not found", id);
    }
    Ok(())
}

fn handle_export(format: ExportFormat, limit: Option<usize>, config: &Config) -> Result<()> {
    let store = Store::open(&config.db_path)?;
    let clips = if let Some(n) = limit {
        store.list(n, None)?
    } else {
        store.export_all()?
    };

    match format {
        ExportFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&clips)?);
        }
        ExportFormat::Csv => {
            println!("id,content_type,created_at,pinned,source_app,hash,label,tags,content");
            for clip in &clips {
                println!(
                    "{},{},{},{},{},{},{},{},\"{}\"",
                    clip.id,
                    clip.content_type,
                    clip.created_at.to_rfc3339(),
                    clip.pinned,
                    clip.source_app.as_deref().unwrap_or(""),
                    clip.hash,
                    clip.label.as_deref().unwrap_or(""),
                    clip.tags.join(";"),
                    clip.content.replace('"', "\"\""),
                );
            }
        }
    }
    Ok(())
}

fn handle_config(config: &Config) -> Result<()> {
    println!("Configuration file:  {}", config::config_path().display());
    println!("Database:            {}", config.db_path.display());
    println!("PID file:            {}", config.pid_path.display());
    println!("Log file:            {}", config.log_path.display());
    println!("Poll interval:       {}ms", config.poll_interval_ms);
    println!("Max history:         {}", config.max_history);
    println!("Min content length:  {}", config.min_content_len);
    println!("Ignored apps:        {}", config.ignored_apps.join(", "));
    Ok(())
}

// Needed for the unsafe file descriptor trick in daemon_start_background
#[cfg(unix)]
use std::os::unix::io::FromRawFd;
