# clipd

**Headless cross-platform clipboard history daemon with metadata capture.**

Inspired by [Paste.app](https://pasteapp.io/) — the gold-standard clipboard manager for Mac/iOS — but designed for server-side tooling, terminal-centric workflows, and environments where a GUI is unavailable or unwanted.

---

## What is clipd?

Every time you press Cmd+C (or Ctrl+C), `clipd` silently captures the clipboard content and stores it with rich metadata:

- **Timestamp** — when the copy happened
- **Source app** — which application was frontmost (Safari, VS Code, Terminal, etc.)
- **Content type** — automatically classified as URL, file path, code, or plain text
- **SHA-256 hash** — for instant deduplication (copying the same text twice creates one entry)
- **Pinboard-style pinning** — mark important clips so they survive history eviction
- **Tags and labels** — organise clips just like Paste.app's pinboards

All data is stored locally in SQLite. No cloud. No telemetry.

---

## How it compares to Paste.app

| Feature | Paste.app | clipd |
|---|---|---|
| Clipboard history | Yes | Yes |
| Source app capture | Yes | Yes |
| Content type detection | Yes | Yes (URL, code, file path, text) |
| Search | Yes (FTS + OCR) | Yes (SQLite FTS5) |
| Pinboards | Yes (visual) | Yes (pin + tag via CLI) |
| iCloud sync | Yes | No (local-only by design) |
| GUI | Yes (beautiful) | No (headless by design) |
| iOS / iPad | Yes | No |
| REST API | No | Planned |
| Lobster integration | No | Planned |
| Cross-platform | macOS + iOS | macOS + Windows + Linux |
| Price | Subscription | Free / open source |

**clipd's niche:** CI servers, headless Macs, remote dev machines, dotfiles setups, and anywhere you want Paste.app's *functionality* without its *interface*.

---

## Installation

### Prerequisites

- Rust 1.75+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- On Linux: `libx11-dev` (for arboard X11 support)
- On Linux (optional): `xdotool` for source app detection

### Build from source

```bash
git clone https://github.com/aeschylus/clipd
cd clipd
cargo install --path .
```

### macOS Permissions

On macOS 14+, `clipd` needs:
- **Accessibility / Automation** (for source app name via `lsappinfo`) — grant in System Settings → Privacy & Security → Automation
- No special permissions needed for clipboard access itself

---

## Usage

### Start the daemon

```bash
# Background (default)
clipd daemon start

# Foreground — useful for systemd/launchd or debugging
clipd daemon start --foreground
```

### Check status

```bash
clipd daemon status
# daemon: running (PID 12345)
# clips stored: 1423
# database: /Users/you/.local/share/clipd/history.db
```

### Browse history

```bash
# Last 20 clips (default)
clipd list

# More clips
clipd list --limit 100

# Search
clipd list --search "github.com"
clipd list --search "def main"

# JSON output (pipe-friendly)
clipd list --format json | jq '.[0]'
```

### Get full content of a clip

```bash
clipd get 42

# Raw content only (pipe-friendly)
clipd get 42 --raw | pbcopy
clipd get 42 --raw | xclip -selection clipboard
```

### Pin a clip (protect from eviction)

```bash
clipd pin 42
clipd pin 42 --unpin
```

### Tag and label clips

```bash
# Add tags
clipd tag 42 work
clipd tag 42 important

# Set a human-readable label (like Paste.app's rename)
clipd label 42 "API key format"
clipd label 42  # (no label text = clear label)
```

### Delete a clip

```bash
clipd delete 42
```

### Export history

```bash
# JSON (default)
clipd export > ~/clipboard-backup.json

# CSV
clipd export --format csv > ~/clipboard-backup.csv

# Last 500 clips only
clipd export --limit 500
```

### Stop the daemon

```bash
clipd daemon stop
```

---

## Storage

| Path | Purpose |
|---|---|
| `~/.local/share/clipd/history.db` | SQLite database (WAL mode) |
| `~/.config/clipd/config.toml` | Configuration |
| `~/.local/share/clipd/clipd.log` | Daemon log |
| `~/.local/run/clipd/clipd.pid` | PID file (runtime) |

---

## Configuration

`~/.config/clipd/config.toml` — all fields are optional (sensible defaults apply):

```toml
# How often to check clipboard for changes (milliseconds)
poll_interval_ms = 500

# Maximum non-pinned history entries
max_history = 10000

# Ignore copies shorter than this (prevents single-char noise)
min_content_len = 2

# Apps whose clipboard writes are never stored (password managers, etc.)
ignored_apps = [
    "1Password",
    "Bitwarden",
    "LastPass",
    "KeePassXC",
    "Keychain",
]

# Override storage paths (optional)
# db_path = "/custom/path/history.db"
# log_path = "/custom/path/clipd.log"
```

---

## Running as a service

### macOS (launchd)

Create `~/Library/LaunchAgents/ai.clipd.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>ai.clipd</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/clipd</string>
        <string>daemon</string>
        <string>start</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/you/.local/share/clipd/clipd.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/you/.local/share/clipd/clipd.log</string>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/ai.clipd.plist
```

### Linux (systemd user service)

Create `~/.config/systemd/user/clipd.service`:

```ini
[Unit]
Description=clipd clipboard history daemon
After=graphical-session.target

[Service]
ExecStart=%h/.cargo/bin/clipd daemon start --foreground
Restart=on-failure
Environment=DISPLAY=:0

[Install]
WantedBy=default.target
```

```bash
systemctl --user enable --now clipd
```

---

## Platform Notes

### macOS
- Source app detection uses `lsappinfo front` (no extra permissions required for app name)
- Full NSWorkspace integration via `objc` crate is planned for image clipboard support

### Linux
- Source app detection uses `xdotool getactivewindow getwindowname` (X11) or `qdbus` (KDE/Wayland)
- `arboard` requires an X11 or Wayland display; on headless servers set `DISPLAY=:0` or use a virtual framebuffer

### Windows
- Source app detection via PowerShell + `GetForegroundWindow`
- Daemon start uses process detach; no Windows Service setup required for personal use

---

## Roadmap

- [ ] REST API server (`clipd serve`) for programmatic access
- [ ] Lobster skill integration (search clipboard from Telegram)
- [ ] Image clipboard support (arboard image API)
- [ ] Shared pinboards via Git sync
- [ ] Browser extension for enriched URL metadata (title, favicon)
- [ ] `clipd watch` — stream new clips in real time (WebSocket)
- [ ] Encryption at rest (SQLCipher)

---

## License

MIT
