use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for clipd.
///
/// Loaded from `~/.config/clipd/config.toml`.  All fields have sensible
/// defaults so a fresh install works without any configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// How often (milliseconds) to poll the clipboard for changes.
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,

    /// Maximum number of non-pinned history entries to retain.
    #[serde(default = "default_max_history")]
    pub max_history: usize,

    /// Where the SQLite database lives.
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,

    /// PID file path (used for daemon management).
    #[serde(default = "default_pid_path")]
    pub pid_path: PathBuf,

    /// Log file path.
    #[serde(default = "default_log_path")]
    pub log_path: PathBuf,

    /// Minimum content length to capture (filters accidental single-char copies).
    #[serde(default = "default_min_content_len")]
    pub min_content_len: usize,

    /// Applications to ignore (e.g. password managers).
    /// Entries are matched case-insensitively against the source app name.
    #[serde(default)]
    pub ignored_apps: Vec<String>,
}

fn default_poll_interval_ms() -> u64 {
    500
}

fn default_max_history() -> usize {
    10_000
}

fn default_db_path() -> PathBuf {
    data_dir().join("history.db")
}

fn default_pid_path() -> PathBuf {
    runtime_dir().join("clipd.pid")
}

fn default_log_path() -> PathBuf {
    data_dir().join("clipd.log")
}

fn default_min_content_len() -> usize {
    1
}

/// Returns `~/.local/share/clipd/` (XDG data home on Linux/macOS).
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("clipd")
}

/// Returns a suitable runtime directory for PID files.
pub fn runtime_dir() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(|| data_dir())
        .join("clipd")
}

/// Returns `~/.config/clipd/config.toml`.
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("clipd")
        .join("config.toml")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_ms: default_poll_interval_ms(),
            max_history: default_max_history(),
            db_path: default_db_path(),
            pid_path: default_pid_path(),
            log_path: default_log_path(),
            min_content_len: default_min_content_len(),
            ignored_apps: vec![
                // Common password managers — their content should never be stored
                "1Password".to_string(),
                "Bitwarden".to_string(),
                "LastPass".to_string(),
                "KeePassXC".to_string(),
                "Keychain".to_string(),
            ],
        }
    }
}

impl Config {
    /// Load config from disk, falling back to defaults if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Config::default());
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config file {}", path.display()))?;

        let cfg: Config = toml::from_str(&raw)
            .with_context(|| format!("parsing config file {}", path.display()))?;

        Ok(cfg)
    }

    /// Write the current config to disk, creating parent directories as needed.
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw)?;
        Ok(())
    }

    /// Ensure all required directories exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.pid_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Returns true if the given app name should be ignored.
    pub fn should_ignore_app(&self, app: &str) -> bool {
        let lower = app.to_lowercase();
        self.ignored_apps
            .iter()
            .any(|ignored| lower.contains(&ignored.to_lowercase()))
    }
}
