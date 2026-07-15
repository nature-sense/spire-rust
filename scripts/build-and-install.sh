#!/usr/bin/env bash
#
# build-and-install.sh — Build all Rust binaries + extension JS,
# kill any running subprocesses, and install into VS Code.
#
# Usage:  ./scripts/build-and-install.sh
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="darwin-arm64"
EXT_DIR="$HOME/.vscode/extensions/naturesense.spire-extension-0.1.0"
BIN_DIR="$EXT_DIR/bin/$PLATFORM"
DIST_DIR="$EXT_DIR/dist"
MEDIA_DIR="$EXT_DIR/media"

echo "=== Spire Build & Install ==="
echo "Root:     $ROOT"
echo "Platform: $PLATFORM"
echo "Target:   $EXT_DIR"
echo ""

# ── Step 1: Kill any running subprocesses ──────────────────────────────────
echo "--- Killing any running spire-core / mcp-* processes ---"
pkill -f "spire-core" 2>/dev/null || true
pkill -f "mcp-git"    2>/dev/null || true
pkill -f "mcp-process" 2>/dev/null || true
pkill -f "mcp-search"    2>/dev/null || true
pkill -f "mcp-terminal"  2>/dev/null || true
pkill -f "mcp-filesystem"  2>/dev/null || true
sleep 1

# Verify they're gone
if pgrep -f "spire-core" >/dev/null 2>&1; then
  echo "WARNING: spire-core still running, forcing kill..."
  pkill -9 -f "spire-core" 2>/dev/null || true
  sleep 1
fi
echo "All subprocesses stopped."
echo ""

# ── Step 2: Build Rust binaries ────────────────────────────────────────────
echo "--- Building Rust binaries (release) ---"
cd "$ROOT/rust"
cargo build --release -p spire-core
cargo build --release -p mcp-git
cargo build --release -p mcp-process
cargo build --release -p mcp-search
cargo build --release -p mcp-terminal
cargo build --release -p mcp-filesystem
cd "$ROOT"
echo "Rust build complete."
echo ""

# ── Step 3: Pre-download embedding model ───────────────────────────────────
echo "--- Pre-downloading embedding model ---"
"$ROOT/scripts/download-embedding-model.sh"
echo "Embedding model ready."
echo ""

# ── Step 4: Build extension JS ─────────────────────────────────────────────
echo "--- Building extension JS bundle ---"
cd "$ROOT/ts/spire-extension"
npm run build
echo "Extension JS build complete."
echo ""

# ── Step 4: Create target directories ──────────────────────────────────────
echo "--- Creating extension directories ---"
mkdir -p "$BIN_DIR"
mkdir -p "$DIST_DIR"
mkdir -p "$MEDIA_DIR"
echo "Directories ready."
echo ""

# ── Step 5: Copy binaries ──────────────────────────────────────────────────
echo "--- Copying binaries ---"
cp "$ROOT/rust/target/release/spire-core"    "$BIN_DIR/spire-core"
cp "$ROOT/rust/target/release/mcp-git"       "$BIN_DIR/mcp-git"
cp "$ROOT/rust/target/release/mcp-process"   "$BIN_DIR/mcp-process"
cp "$ROOT/rust/target/release/mcp-search"    "$BIN_DIR/mcp-search"
cp "$ROOT/rust/target/release/mcp-terminal"  "$BIN_DIR/mcp-terminal"
cp "$ROOT/rust/target/release/mcp-filesystem"  "$BIN_DIR/mcp-filesystem"
chmod +x "$BIN_DIR/spire-core" "$BIN_DIR/mcp-git" "$BIN_DIR/mcp-process" "$BIN_DIR/mcp-search" "$BIN_DIR/mcp-terminal" "$BIN_DIR/mcp-filesystem"
echo "Binaries copied to $BIN_DIR"
echo ""

# ── Step 6: Copy extension JS ──────────────────────────────────────────────
echo "--- Copying extension JS ---"
cp "$ROOT/ts/spire-extension/dist/extension.js" "$DIST_DIR/extension.js"
echo "Extension JS copied."
echo ""

# ── Step 7: Copy package.json, media, and webview files ────────────────────
echo "--- Copying package.json, media, and webview files ---"
cp "$ROOT/ts/spire-extension/package.json" "$EXT_DIR/package.json"
cp "$ROOT/ts/spire-extension/media/"*.png "$MEDIA_DIR/" 2>/dev/null || true
cp "$ROOT/ts/spire-extension/media/"*.svg "$MEDIA_DIR/" 2>/dev/null || true

# Copy webview files (needed for the chat panel)
WEBVIEW_DIR="$EXT_DIR/src/webview"
mkdir -p "$WEBVIEW_DIR"
cp "$ROOT/ts/spire-extension/src/webview/style.css" "$WEBVIEW_DIR/style.css"
cp "$ROOT/ts/spire-extension/src/webview/app.js"    "$WEBVIEW_DIR/app.js"
echo "Webview files copied."
echo "Package files copied."
echo ""

# ── Step 8: Also update workspace binary dir (for dev mode) ────────────────
echo "--- Updating workspace binary dir (dev mode) ---"
DEV_BIN_DIR="$ROOT/ts/spire-extension/bin/$PLATFORM"
mkdir -p "$DEV_BIN_DIR"
cp "$ROOT/rust/target/release/spire-core"    "$DEV_BIN_DIR/spire-core"
cp "$ROOT/rust/target/release/mcp-git"       "$DEV_BIN_DIR/mcp-git"
cp "$ROOT/rust/target/release/mcp-process"   "$DEV_BIN_DIR/mcp-process"
cp "$ROOT/rust/target/release/mcp-search"    "$DEV_BIN_DIR/mcp-search"
cp "$ROOT/rust/target/release/mcp-terminal"  "$DEV_BIN_DIR/mcp-terminal"
cp "$ROOT/rust/target/release/mcp-filesystem"  "$DEV_BIN_DIR/mcp-filesystem"
chmod +x "$DEV_BIN_DIR/spire-core" "$DEV_BIN_DIR/mcp-git" "$DEV_BIN_DIR/mcp-process" "$DEV_BIN_DIR/mcp-search" "$DEV_BIN_DIR/mcp-terminal" "$DEV_BIN_DIR/mcp-filesystem"
echo "Workspace binaries updated."
echo ""

# ── Step 9: Clean up stale lock files (NOT the WAL/snapshot data) ──────────
echo "--- Cleaning up stale lock files ---"
# Remove any stale lock files but keep the WAL and snapshot data
# Check project-local .spire/data directory
if [ -n "${SPIRE_PROJECT_ROOT:-}" ] && [ -d "$SPIRE_PROJECT_ROOT/.spire/data" ]; then
    rm -f "$SPIRE_PROJECT_ROOT/.spire/data/"*.lock 2>/dev/null || true
fi
echo "Stale lock files cleaned (persisted data preserved)."
echo ""


# ── Step 10: Reload VS Code window ─────────────────────────────────────────
echo "--- Reloading VS Code window ---"
code --command "workbench.action.reloadWindow" 2>/dev/null || true
echo ""

echo "=== Build & Install Complete ==="
echo ""
echo "The extension has been installed to: $EXT_DIR"
echo "VS Code window has been reloaded."
echo ""
echo "If the chat webview doesn't appear, open it via:"
echo "  Cmd+Shift+P → 'Spire: Open Chat'"
echo ""
echo "Note: Persisted data (config, graph) is preserved across builds."
echo "If you need to reset, delete the .spire/ directory in your project root manually."
