# clipd

**Clipboard history manager with global hotkey, floating panel UI, and headless daemon.**

clipd stores every clipboard copy locally with rich metadata — source app, timestamp, content type — and puts it at your fingertips with a global hotkey (Cmd+Shift+V) and a dark, search-driven floating panel.

---

## Quick install (macOS)

### Homebrew (CLI only)

```bash
brew tap aeschylus/clipd
brew install clipd
clipd daemon start
```

### macOS App (menu bar + global hotkey)

Download the latest `clipd.app.dmg` from [Releases](https://github.com/aeschylus/clipd/releases) and drag to Applications.

Or build from source (see below).

---

## What's in the box

| Component | What it does |
|---|---|
| `clipd-core` | Rust library: clipboard polling daemon, SQLite store, models |
| `clipd` (CLI) | Full-featured terminal interface: list, search, pin, tag, export |
| `clipd.app` | macOS app: menu bar icon, global hotkey, dark floating panel UI |

---

## macOS App

### Usage

- **Cmd+Shift+V** — toggle the clipboard panel
- **Left-click tray icon** — toggle panel
- **Right-click tray icon** — menu (Show / Quit)

### Panel controls

| Key | Action |
|---|---|
| Type anything | Search clipboard history (real-time FTS) |
| Arrow Up / Down | Navigate list |
| Enter | Paste selected clip |
| Cmd+Delete | Delete selected clip |
| Escape | Dismiss panel |
| Click item | Paste and dismiss |

### Permissions (macOS)

- **Automation / AppleEvents** — for detecting which app you copied from (`lsappinfo`)
- **Accessibility** — for the auto-paste feature (Cmd+V simulation via osascript)

Grant in: *System Settings → Privacy & Security → Accessibility / Automation → clipd*

---

## CLI

### Start the daemon

```bash
# Background (default)
clipd daemon start

# Foreground — useful for launchd or debugging
clipd daemon start --foreground
```

### Browse history

```bash
# Last 20 clips
clipd list

# Search
clipd list --search "github.com"
clipd list --search "def main"

# JSON output
clipd list --format json | jq '.[0]'
```

### Get full content

```bash
clipd get 42
clipd get 42 --raw | pbcopy
```

### Organize

```bash
clipd pin 42            # protect from eviction
clipd pin 42 --unpin
clipd tag 42 work
clipd label 42 "API key format"
clipd delete 42
```

### Export

```bash
clipd export > backup.json
clipd export --format csv > backup.csv
```

### Stop

```bash
clipd daemon stop
```

---

## Configuration

`~/.config/clipd/config.toml` (all fields optional):

```toml
poll_interval_ms = 500
max_history = 10000
min_content_len = 2

ignored_apps = [
    "1Password",
    "Bitwarden",
    "LastPass",
    "KeePassXC",
]
```

---

## Storage

| Path | Purpose |
|---|---|
| `~/.local/share/clipd/history.db` | SQLite (WAL mode) |
| `~/.config/clipd/config.toml` | Configuration |
| `~/.local/share/clipd/clipd.log` | Daemon log |

---

## Building from source

### Prerequisites

- **Rust 1.75+**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Node.js 18+** (for Tauri CLI): `brew install node`
- **Tauri CLI v2**: `cargo install tauri-cli --version "^2"`
- **Xcode Command Line Tools** (macOS): `xcode-select --install`

### Build the CLI only

```bash
git clone https://github.com/aeschylus/clipd
cd clipd
cargo build --release --package clipd
./target/release/clipd --version
```

### Build the macOS app

```bash
git clone https://github.com/aeschylus/clipd
cd clipd
cargo tauri build
# Output: target/release/bundle/macos/clipd.app
open target/release/bundle/macos/
```

### Development mode (hot reload)

```bash
cargo tauri dev
```

---

## Workspace structure

```
clipd/
├── Cargo.toml          # workspace root
├── clipd-core/         # shared library (daemon, store, models, config)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── clipboard.rs   # polling, content-type detection, source-app
│       ├── config.rs      # Config struct, load/save, directory resolution
│       ├── daemon.rs      # async polling loop, PID management
│       ├── models.rs      # ClipEntry, ContentType
│       └── store.rs       # SQLite store with FTS5
├── clipd-cli/          # `clipd` binary (depends on clipd-core)
│   ├── Cargo.toml
│   └── src/main.rs
├── src-tauri/          # Tauri macOS app shell
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json  # window config, bundle id, tray
│   └── src/
│       ├── main.rs      # Tauri app setup, tray, global shortcut
│       └── commands.rs  # IPC: list_clips, search_clips, paste_clip, …
├── ui/                 # Vanilla JS + CSS frontend
│   ├── index.html
│   ├── style.css       # Dark Bisque aesthetic (#09090b)
│   └── main.js         # Search, list rendering, keyboard nav
└── Formula/
    └── clipd.rb        # Homebrew formula
```

---

## Running as a service (macOS launchd)

Create `~/Library/LaunchAgents/ai.clipd.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>ai.clipd</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/clipd</string>
        <string>daemon</string>
        <string>start</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/ai.clipd.plist
```

Note: The `clipd.app` already auto-starts its own embedded daemon — you only need the launchd plist when using the headless CLI daemon without the app.

---

## Comparison with Paste.app

| Feature | Paste.app | clipd |
|---|---|---|
| Clipboard history | Yes | Yes |
| Source app capture | Yes | Yes |
| Content type detection | Yes | Yes |
| Full-text search | Yes | Yes (SQLite FTS5) |
| Global hotkey | Yes | Yes (Cmd+Shift+V) |
| Floating panel | Yes (beautiful) | Yes (dark minimal) |
| Menu bar app | Yes | Yes |
| Pinboards / tags | Yes (visual) | Yes (CLI + panel) |
| iCloud sync | Yes | No (local-only) |
| iOS / iPad | Yes | No |
| Cross-platform | macOS + iOS | macOS + Linux + Windows (CLI) |
| Price | Subscription | Free / open source |

---

## Roadmap

- [ ] REST API (`clipd serve`) for programmatic access
- [ ] Configurable hotkey in `config.toml`
- [ ] Image clipboard support
- [ ] Lobster Telegram skill integration
- [ ] Browser extension for enriched URL metadata
- [ ] Encryption at rest (SQLCipher)
- [ ] Shared pinboards via Git sync

---

## License

MIT
