#!/usr/bin/env bash
# clipd build script — works on any Debian/Ubuntu server
# Builds the clipd CLI daemon binary for the current platform.
#
# Usage:
#   ./scripts/build.sh              # build release binary
#   ./scripts/build.sh --debug      # build debug binary
#   ./scripts/build.sh --install    # build + install to /usr/local/bin
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_MODE="release"
INSTALL=false

# Parse args
for arg in "$@"; do
  case "$arg" in
    --debug)    BUILD_MODE="debug" ;;
    --install)  INSTALL=true ;;
    --help|-h)
      echo "Usage: $0 [--debug] [--install]"
      echo ""
      echo "  --debug    Build debug binary (faster compile, slower runtime)"
      echo "  --install  Copy binary to /usr/local/bin/clipd after build"
      exit 0
      ;;
  esac
done

# ── System dependencies ──────────────────────────────────────────────────────

need() {
  command -v "$1" &>/dev/null
}

install_rust() {
  echo "→ Installing Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
}

echo "=== clipd build script ==="
echo "Platform: $(uname -s) $(uname -m)"
echo "Build mode: $BUILD_MODE"
echo ""

# Debian system deps
if command -v apt-get &>/dev/null; then
  MISSING_PKGS=()
  for pkg in build-essential pkg-config libssl-dev; do
    dpkg -s "$pkg" &>/dev/null || MISSING_PKGS+=("$pkg")
  done
  if [[ ${#MISSING_PKGS[@]} -gt 0 ]]; then
    echo "→ Installing system packages: ${MISSING_PKGS[*]}"
    sudo apt-get update -qq
    sudo apt-get install -y -qq "${MISSING_PKGS[@]}"
  fi
fi

# Rust
if ! need rustc; then
  install_rust
else
  echo "→ Rust $(rustc --version) found"
fi

if ! need cargo; then
  source "$HOME/.cargo/env"
fi

# ── Build ─────────────────────────────────────────────────────────────────────

cd "$REPO_ROOT"

CARGO_FLAGS=(--package clipd --manifest-path clipd-cli/Cargo.toml)
if [[ "$BUILD_MODE" == "release" ]]; then
  CARGO_FLAGS+=(--release)
fi

echo ""
echo "→ Building clipd CLI (${BUILD_MODE})..."
cargo build "${CARGO_FLAGS[@]}"

# Locate binary
if [[ "$BUILD_MODE" == "release" ]]; then
  BINARY="$REPO_ROOT/target/release/clipd"
else
  BINARY="$REPO_ROOT/target/debug/clipd"
fi

echo ""
echo "✓ Build succeeded: $BINARY"
echo "  Version: $("$BINARY" --version)"

# ── Install ───────────────────────────────────────────────────────────────────

if [[ "$INSTALL" == true ]]; then
  DEST="/usr/local/bin/clipd"
  echo ""
  echo "→ Installing to $DEST..."
  sudo cp "$BINARY" "$DEST"
  sudo chmod 755 "$DEST"
  echo "✓ Installed: $(clipd --version)"
  echo ""
  echo "Start the daemon:"
  echo "  clipd daemon start"
fi

echo ""
echo "Done."
