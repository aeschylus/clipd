use anyhow::Result;
use sha2::{Digest, Sha256};
use std::process::Command;

use crate::models::ContentType;

/// Detect the semantic content type of a clipboard string.
///
/// Heuristics (in priority order):
///   1. URL: starts with http/https/ftp or looks like a domain
///   2. File path: starts with `/` or `~/` or `C:\` etc.
///   3. Code: contains keywords, brackets, indentation, shebangs
///   4. Plain text: fallback
pub fn detect_content_type(content: &str) -> ContentType {
    let trimmed = content.trim();

    if is_url(trimmed) {
        return ContentType::Url;
    }

    if is_file_path(trimmed) {
        return ContentType::FilePath;
    }

    if is_code(trimmed) {
        return ContentType::Code;
    }

    ContentType::PlainText
}

fn is_url(s: &str) -> bool {
    // Match common URL schemes
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("ftp://")
        || s.starts_with("ftps://")
        || s.starts_with("ssh://")
        || s.starts_with("git://")
        // Bare domains like "example.com/path"
        || (s.contains('.') && !s.contains(' ') && {
            let parts: Vec<&str> = s.splitn(2, '.').collect();
            parts.len() == 2 && !parts[0].is_empty() && parts[0].len() <= 63
        } && (s.contains('/') || {
            // Check for TLD-like ending
            let after_dot = s.split('.').last().unwrap_or("");
            matches!(after_dot, "com" | "org" | "net" | "io" | "dev" | "app" | "ai" | "co")
        }))
}

fn is_file_path(s: &str) -> bool {
    // Unix absolute paths
    if s.starts_with('/') {
        return true;
    }
    // macOS home-relative
    if s.starts_with("~/") {
        return true;
    }
    // Windows absolute paths
    if s.len() >= 3 {
        let bytes = s.as_bytes();
        if bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
            return true;
        }
    }
    // UNC paths
    if s.starts_with("\\\\") {
        return true;
    }
    false
}

fn is_code(s: &str) -> bool {
    // Shebang line
    if s.starts_with("#!") {
        return true;
    }
    // Function / method / class definitions (many languages)
    let code_patterns = [
        "fn ", "func ", "function ", "def ", "class ", "struct ",
        "impl ", "interface ", "async ", "await ", "import ", "use ",
        "const ", "let ", "var ", "pub ", "private ", "public ",
        "return ", "if (", "if(", "for (", "for(", "while (",
        "=>", "->", "::", "&&", "||",
    ];
    let matches = code_patterns.iter().filter(|&&p| s.contains(p)).count();
    if matches >= 2 {
        return true;
    }
    // Has indentation (multiple lines with leading spaces/tabs)
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() >= 3 {
        let indented = lines
            .iter()
            .filter(|l| l.starts_with("    ") || l.starts_with('\t'))
            .count();
        if indented >= 2 {
            return true;
        }
    }
    // Brackets balance check (relaxed: just has { } or [ ] or ( ))
    let has_braces = s.contains('{') && s.contains('}');
    let has_parens = s.contains('(') && s.contains(')');
    if has_braces && has_parens && s.contains('\n') {
        return true;
    }
    false
}

/// Compute SHA-256 of content and return as lowercase hex string.
pub fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Attempt to detect which application currently owns the clipboard /
/// was the most recent foreground window.
///
/// This is inherently platform-specific and best-effort.  Returns None
/// gracefully if detection fails.
pub fn detect_source_app() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        detect_source_app_macos()
    }
    #[cfg(target_os = "linux")]
    {
        detect_source_app_linux()
    }
    #[cfg(target_os = "windows")]
    {
        detect_source_app_windows()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// macOS: use `osascript` to query the frontmost application name.
///
/// Alternatively, `lsappinfo front` works without Accessibility perms for app name.
#[cfg(target_os = "macos")]
fn detect_source_app_macos() -> Option<String> {
    // Prefer lsappinfo (faster, no Accessibility permission needed for name)
    if let Ok(output) = Command::new("lsappinfo")
        .args(["front"])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        // lsappinfo output: `"ApplicationType" = "Foreground"  "CFBundleName" = "Safari"`
        for line in s.lines() {
            if line.contains("CFBundleName") {
                if let Some(name) = line.split('"').nth(3) {
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }

    // Fallback: osascript (requires Automation permission on macOS 10.14+)
    if let Ok(output) = Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to get name of first process whose frontmost is true"])
        .output()
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }

    None
}

/// Linux (X11/Wayland): query active window name via `xdotool` or `/proc`.
#[cfg(target_os = "linux")]
fn detect_source_app_linux() -> Option<String> {
    // Try xdotool (works on X11)
    if let Ok(output) = Command::new("xdotool")
        .args(["getactivewindow", "getwindowname"])
        .output()
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }

    // Try qdbus / kdotool for KDE (Wayland)
    if let Ok(output) = Command::new("qdbus")
        .args(["org.kde.KWin", "/KWin", "activeWindow"])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }

    None
}

/// Windows: use PowerShell to get the foreground window process name.
#[cfg(target_os = "windows")]
fn detect_source_app_windows() -> Option<String> {
    let script = r#"
        Add-Type @'
        using System;
        using System.Runtime.InteropServices;
        public class Win32 {
            [DllImport("user32.dll")]
            public static extern IntPtr GetForegroundWindow();
            [DllImport("user32.dll")]
            public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
        }
'@
        $hwnd = [Win32]::GetForegroundWindow()
        $pid = 0
        [Win32]::GetWindowThreadProcessId($hwnd, [ref]$pid) | Out-Null
        (Get-Process -Id $pid -ErrorAction SilentlyContinue).MainWindowTitle
    "#;

    if let Ok(output) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Holds clipboard polling state between ticks.
pub struct ClipboardPoller {
    last_hash: Option<String>,
}

impl ClipboardPoller {
    pub fn new() -> Self {
        Self { last_hash: None }
    }

    /// Poll the clipboard.  Returns `Some(content)` if the clipboard changed
    /// since the last poll, `None` if it is the same or unavailable.
    pub fn poll(&mut self) -> Result<Option<String>> {
        let mut clipboard = arboard::Clipboard::new()?;

        let content = match clipboard.get_text() {
            Ok(text) => text,
            Err(arboard::Error::ContentNotAvailable) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        if content.is_empty() {
            return Ok(None);
        }

        let hash = sha256_hex(&content);
        if self.last_hash.as_deref() == Some(&hash) {
            return Ok(None); // No change
        }

        self.last_hash = Some(hash);
        Ok(Some(content))
    }
}

impl Default for ClipboardPoller {
    fn default() -> Self {
        Self::new()
    }
}
