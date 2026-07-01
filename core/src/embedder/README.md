# Spire Embedder — Text Embedding Pipeline

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![Candle](https://img.shields.io/badge/candle-0.11-orange)](https://github.com/huggingface/candle)

The embedder module provides local, on-device text embedding using [Candle](https://github.com/huggingface/candle), a minimalist ML framework for Rust. It runs the `sentence-transformers/all-MiniLM-L6-v2` model entirely on-device — no cloud API calls, no data leaves your machine.

---

## Architecture

```
embedder/
├── mod.rs              # Embedder trait definition + re-exports
└── candle_embedder.rs  # Candle-based implementation (all-MiniLM-L6-v2)
```

### Embedder Trait (`mod.rs`)

The `Embedder` trait defines the contract for text-to-vector conversion:

```rust
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding>;
    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>>;
    fn dimensions(&self) -> usize;
}
```

| Method | Description |
|--------|-------------|
| `embed` | Embed a single text string, returning an `Embedding` with vector, hash, and metadata |
| `embed_batch` | Embed multiple texts in one call (batched inference for efficiency) |
| `dimensions` | Return the embedding vector dimension (384 for all-MiniLM-L6-v2) |

### CandleEmbedder (`candle_embedder.rs`)

The primary implementation uses Candle to load and run the BERT-based model:

- **Model**: `sentence-transformers/all-MiniLM-L6-v2` — a 384-dimensional, L2-normalized sentence embedding model
- **Weights**: ~85 MB, downloaded from Hugging Face Hub on first run, cached at `~/.cache/huggingface/`
- **Inference**: Mean-pooling over BERT token embeddings, followed by L2 normalization
- **Acceleration**: CPU by default; Metal GPU on Apple Silicon (opt-in via `SPIRE_USE_METAL=1`)

#### `create_embedder()` Factory

```rust
pub fn create_embedder() -> Result<CandleEmbedder>
```

Downloads model weights (if not cached), loads the tokenizer, and initializes the Candle model. Returns an error if model loading fails (e.g., network unavailable on first run).

---

## Embedding Data Model

Defined in [`models/embedding.rs`](../models/embedding.rs):

```rust
pub struct Embedding {
    pub vector: Vec<f32>,           // 384-dimensional, L2-normalized
    pub text: String,               // Original input text
    pub text_hash: String,          // MD5 hex digest for deduplication
    pub token_count: usize,         // Number of tokens after tokenization
    pub dimensions: usize,          // Always 384
    pub model_name: String,         // "sentence-transformers/all-MiniLM-L6-v2"
    pub generated_at: DateTime<Utc>, // Timestamp of embedding generation
}
```

---

## Usage

### In the Actor System

The `MemoryGraphActor` receives an `Arc<dyn Embedder>` at construction and spawns embedding tasks via `tokio::spawn`:

```rust
// In MemoryGraphActor::handle for StoreNode:
let embedder = self.embedder.clone();
let text = format!("{}: {}", node_input.name, desc);
tokio::spawn(async move {
    match embedder.embed(&text).await {
        Ok(embedding) => { /* store embedding */ }
        Err(e) => tracing::warn!("Embedding failed: {}", e),
    }
});
```

### Standalone

```rust
use spire_rust::embedder::candle_embedder::create_embedder;

let embedder = create_embedder()?;
let embedding = embedder.embed("Your text here").await?;
println!("Vector (first 5 dims): {:?}", &embedding.vector[..5]);
println!("Dimensions: {}", embedding.dimensions);
```

---

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `SPIRE_USE_METAL` | (unset) | Set to `1` to enable Metal GPU acceleration on Apple Silicon |

### Metal GPU Support

On Apple Silicon (M1/M2/M3/M4), Candle can use Metal for GPU-accelerated inference:

```bash
SPIRE_USE_METAL=1 cargo run
```

**Note**: Some operations may not be fully supported on Metal. If you encounter errors, fall back to CPU by unsetting the variable.

---

## Performance

| Metric | Value |
|--------|-------|
| Model | all-MiniLM-L6-v2 |
| Vector dimensions | 384 |
| Model size | ~85 MB |
| Inference (CPU, M1 Pro) | ~10-20 ms per text |
| Inference (Metal GPU, M1 Pro) | ~5-10 ms per text |
| Batch inference | Up to 4x throughput vs. single |

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `candle-core` | 0.11 | ML inference engine (CPU + Metal) |
| `candle-transformers` | 0.11 | BERT model architecture |
| `candle-nn` | 0.11 | Neural network building blocks |
| `hf-hub` | 1.0.0-rc.1 | Hugging Face Hub client for model downloads |
| `tokenizers` | 0.21 | Hugging Face tokenizers (WordPiece for BERT) |
| `md-5` | 0.10 | MD5 hashing for text deduplication |

---

## Testing

```bash
# Run embedding tests (requires model download, ~85 MB)
cargo test -- --ignored

# Run all other tests (no model download needed)
cargo test
```

Tests that require model download are marked `#[ignore]` to avoid slow first-run downloads during normal development.

---

## Related

- [Models README](../models/README.md) — Embedding data types and the `Embedder` trait
- [Actors README](../actors/README.md) — How the embedder is used by `MemoryGraphActor`
- [Core README](../../README.md) — Project overview
