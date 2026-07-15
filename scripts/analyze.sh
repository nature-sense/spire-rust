#!/usr/bin/env bash
#
# analyze.sh — Convenience wrapper for the project-analyzer tool.
#
# Usage:
#   ./scripts/analyze.sh [path] [--format json|pretty] [--no-ignore]
#
# Examples:
#   ./scripts/analyze.sh . --format pretty
#   ./scripts/analyze.sh /some/project --format json
#
# The first argument is the path to analyze (defaults to .).
# All remaining arguments are passed through to project-analyzer.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# If the first arg doesn't start with --, treat it as the path
# and resolve it to an absolute path before cd'ing into rust/
if [[ $# -gt 0 && "$1" != --* ]]; then
    TARGET_PATH="$(cd "$ROOT" && realpath "$1" 2>/dev/null || echo "$ROOT/$1")"
    shift
    cd "$ROOT/rust"
    exec cargo run -p project-analyzer -- "$TARGET_PATH" "$@"
else
    cd "$ROOT/rust"
    exec cargo run -p project-analyzer -- "$ROOT" "$@"
fi
