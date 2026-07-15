#!/usr/bin/env bash
#
# download-embedding-model.sh — Pre-download the embedding model
# into the HuggingFace cache so it's available at runtime without
# network access (avoiding blocking I/O inside the Tokio runtime).
#
# The model is cached at ~/.cache/huggingface/hub/ by default.
# Subsequent runs of spire-core will load from cache instantly.
#
# Usage:  ./scripts/download-embedding-model.sh
#
set -euo pipefail

MODEL_ID="sentence-transformers/all-MiniLM-L6-v2"
CACHE_DIR="${HF_HOME:-$HOME/.cache/huggingface}/hub"

echo "=== Downloading embedding model ==="
echo "Model:    $MODEL_ID"
echo "Cache:    $CACHE_DIR"
echo ""

# Check if model is already cached
MODEL_DIR="$CACHE_DIR/models--sentence-transformers--all-MiniLM-L6-v2"
if [ -d "$MODEL_DIR" ] && [ -f "$MODEL_DIR/snapshots/"*"/model.safetensors" ] 2>/dev/null; then
    echo "✓ Model already cached at $MODEL_DIR"
    echo "  (delete this directory to force re-download)"
    exit 0
fi

# Prefer huggingface-hub CLI (pip install huggingface-hub)
if command -v huggingface-cli &>/dev/null; then
    echo "Using huggingface-cli to download..."
    huggingface-cli download "$MODEL_ID" --local-dir-use-symlinks False
    echo "✓ Download complete via huggingface-cli"
    exit 0
fi

# Fallback: use Python with huggingface-hub library
if command -v python3 &>/dev/null; then
    echo "Using Python huggingface-hub library..."
    python3 -c "
import sys
try:
    from huggingface_hub import snapshot_download
    snapshot_download('$MODEL_ID')
    print('✓ Download complete via Python huggingface-hub')
except ImportError:
    print('huggingface_hub not installed, trying pip install...')
    sys.exit(1)
" 2>&1 || {
    echo "Trying pip install huggingface-hub..."
    pip3 install -q huggingface-hub 2>/dev/null || pip install -q huggingface-hub 2>/dev/null || true
    python3 -c "
from huggingface_hub import snapshot_download
snapshot_download('$MODEL_ID')
print('✓ Download complete via Python huggingface-hub')
" 2>&1
}
    exit 0
fi

# Last resort: use curl to download individual files into the cache structure
echo "Using curl to download model files..."
SNAPSHOT_DIR="$MODEL_DIR/snapshots/$(date +%s)"
mkdir -p "$SNAPSHOT_DIR"

BASE_URL="https://huggingface.co/$MODEL_ID/resolve/main"

for file in "config.json" "tokenizer.json" "model.safetensors"; do
    echo "  Downloading $file..."
    curl -fSL "$BASE_URL/$file" -o "$SNAPSHOT_DIR/$file" || {
        echo "ERROR: Failed to download $file"
        rm -rf "$MODEL_DIR"
        exit 1
    }
done

# Create the refs file so hf-hub can find the snapshot
mkdir -p "$MODEL_DIR/refs"
echo "$(basename "$SNAPSHOT_DIR")" > "$MODEL_DIR/refs/main"

echo ""
echo "✓ Model downloaded to $SNAPSHOT_DIR"
echo "  Total size: $(du -sh "$SNAPSHOT_DIR" | cut -f1)"
