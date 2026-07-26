#!/usr/bin/env bash
#
# analyze.sh — Convenience wrapper for project analysis.
#
# This script is a stub — the project-analyzer CLI tool has been removed.
# Analysis is now handled internally by spire-core's analyzer module.
#
# Usage:
#   ./scripts/analyze.sh [path]
#
# The path argument is accepted for backward compatibility but ignored.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_PATH="${1:-$ROOT}"

echo "analyze.sh: project analysis is now handled internally by spire-core."
echo "analyze.sh: (the standalone project-analyzer CLI has been removed)"
echo "analyze.sh: target path: $TARGET_PATH"
