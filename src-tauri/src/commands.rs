// IPC commands exposed to the WebView frontend via Tauri's invoke mechanism.
//
// All commands are pure in the sense that they take explicit parameters and
// return Results — side effects (DB reads, clipboard writes) are isolated here
// at the system boundary.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use clipd_core::{config::Config, models::ClipEntry, store::Store};

// ─── Serialisable view type ───────────────────────────────────────────────────

/// A lightweight representation of a clip sent to the frontend.
/// We avoid sending the full raw content for the list view (saves bandwidth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipView {
    pub id: i64,
    pub preview: String,
    pub full_content: String,
    pub content_type: String,
    pub source_app: Option<String>,
    pub created_at: String,
    pub pinned: bool,
    pub label: Option<String>,
    pub tags: Vec<String>,
    pub type_icon: String,
}

impl From<ClipEntry> for ClipView {
    fn from(clip: ClipEntry) -> Self {
        let type_icon = match clip.content_type.as_str() {
            "url" => "link",
            "code" => "code",
            "file_path" => "folder",
            "image" => "image",
            _ => "text",
        }
        .to_string();

        ClipView {
            preview: clip.preview(120),
            full_content: clip.content.clone(),
            content_type: clip.content_type.to_string(),
            source_app: clip.source_app,
            created_at: clip.created_at.format("%H:%M · %b %d").to_string(),
            pinned: clip.pinned,
            label: clip.label,
            tags: clip.tags,
            type_icon,
            id: clip.id,
        }
    }
}

// ─── Helper: open the store ───────────────────────────────────────────────────

fn open_store() -> Result<Store, String> {
    let config = Config::load().map_err(|e| format!("config error: {e}"))?;
    Store::open(&config.db_path).map_err(|e| format!("store error: {e}"))
}

// ─── Commands ─────────────────────────────────────────────────────────────────

/// Return the most recent clips (up to `limit`).
#[tauri::command]
pub fn list_clips(limit: Option<usize>) -> Result<Vec<ClipView>, String> {
    let store = open_store()?;
    let n = limit.unwrap_or(50);
    store
        .list(n, None)
        .map(|clips| clips.into_iter().map(ClipView::from).collect())
        .map_err(|e| e.to_string())
}

/// Search clips by full-text query.
#[tauri::command]
pub fn search_clips(query: String, limit: Option<usize>) -> Result<Vec<ClipView>, String> {
    let store = open_store()?;
    let n = limit.unwrap_or(50);
    let term = if query.trim().is_empty() { None } else { Some(query.as_str()) };
    store
        .list(n, term)
        .map(|clips| clips.into_iter().map(ClipView::from).collect())
        .map_err(|e| e.to_string())
}

/// Fetch a single clip by ID (returns full content).
#[tauri::command]
pub fn get_clip(id: i64) -> Result<Option<ClipView>, String> {
    let store = open_store()?;
    store
        .get(id)
        .map(|opt| opt.map(ClipView::from))
        .map_err(|e| e.to_string())
}

/// Write the clip's content to the system clipboard and signal the UI to dismiss.
///
/// Uses `arboard` directly rather than the Tauri plugin so we avoid a round-trip
/// through JS and keep the paste action synchronous.
#[tauri::command]
pub async fn paste_clip(app: AppHandle, id: i64) -> Result<(), String> {
    let store = open_store()?;
    let clip = store
        .get(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("clip {} not found", id))?;

    // Write to clipboard
    let mut board = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    board.set_text(&clip.content).map_err(|e| e.to_string())?;

    // Hide the panel
    if let Some(window) = app.get_webview_window("clipd-panel") {
        let _ = window.hide();
    }

    // Simulate Cmd+V paste via shell (macOS only)
    // We use osascript to tell the previously frontmost app to paste.
    #[cfg(target_os = "macos")]
    {
        std::thread::spawn(|| {
            // Small delay to allow the panel to hide and the previous app to focus
            std::thread::sleep(std::time::Duration::from_millis(120));
            let _ = std::process::Command::new("osascript")
                .args(["-e", "tell application \"System Events\" to keystroke \"v\" using command down"])
                .output();
        });
    }

    Ok(())
}

/// Delete a clip by ID.
#[tauri::command]
pub fn delete_clip(id: i64) -> Result<bool, String> {
    let store = open_store()?;
    store.delete(id).map_err(|e| e.to_string())
}

/// Toggle pin status for a clip.
#[tauri::command]
pub fn toggle_pin(id: i64, pinned: bool) -> Result<bool, String> {
    let store = open_store()?;
    store.set_pinned(id, pinned).map_err(|e| e.to_string())
}

/// Return the current clipd configuration as a JSON-serialisable map.
#[tauri::command]
pub fn get_config() -> Result<serde_json::Value, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "db_path": config.db_path.display().to_string(),
        "poll_interval_ms": config.poll_interval_ms,
        "max_history": config.max_history,
        "min_content_len": config.min_content_len,
        "ignored_apps": config.ignored_apps,
    }))
}
