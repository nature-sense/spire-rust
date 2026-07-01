use std::sync::Arc;

use spire_rust::embedder::candle_embedder::CandleEmbedder;
use spire_rust::models::embedding::Embedder;

/// Helper to create an embedder outside the tokio runtime (hf-hub uses block_on internally).
fn create_embedder() -> CandleEmbedder {
    // Run in a separate thread to avoid tokio runtime conflict with hf-hub's block_on
    std::thread::spawn(|| CandleEmbedder::new().expect("Failed to create CandleEmbedder"))
        .join()
        .expect("Thread panicked")
}

/// Helper to call the async Embedder::embed() trait method from a sync context.
fn embed_sync(embedder: &CandleEmbedder, text: &str) -> spire_rust::models::embedding::Embedding {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(<CandleEmbedder as Embedder>::embed(embedder, text))
        .expect("Failed to embed text")
}

/// Helper to call the async Embedder::embed_batch() trait method from a sync context.
fn embed_batch_sync(
    embedder: &CandleEmbedder,
    texts: &[String],
) -> Vec<spire_rust::models::embedding::Embedding> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(<CandleEmbedder as Embedder>::embed_batch(embedder, texts))
        .expect("Failed to embed batch")
}

/// Test that the CandleEmbedder can be instantiated and produces a
/// 384-dimensional normalized vector for a known text.
///
/// This test downloads the model weights on first run (~85 MB to
/// `~/.cache/huggingface/`), so it may take a few seconds the first time.
#[test]
#[ignore = "Requires model download (~85 MB) — run explicitly with `cargo test -- --ignored`"]
fn test_embedder_creates_384_dim_vector() {
    let embedder = create_embedder();
    let embedding = embed_sync(&embedder, "Hello, world! This is a test sentence for embedding.");

    // Verify dimensions
    assert_eq!(
        embedding.dimensions,
        384,
        "Embedding should be 384-dimensional"
    );
    assert_eq!(
        embedding.vector.len(),
        384,
        "Vector length should be 384"
    );

    // Verify model name
    assert!(
        embedding.model_name.contains("all-MiniLM-L6-v2"),
        "Model name should contain 'all-MiniLM-L6-v2'"
    );

    // Verify text hash is present
    assert!(!embedding.text_hash.is_empty(), "Text hash should not be empty");

    // Verify token count
    assert!(
        embedding.token_count > 0,
        "Token count should be > 0"
    );

    // Verify timestamp
    let age = chrono::Utc::now() - embedding.generated_at;
    assert!(
        age.num_seconds() < 60,
        "Embedding should have been generated recently"
    );
}

#[test]
#[ignore = "Requires model download (~85 MB) — run explicitly with `cargo test -- --ignored`"]
fn test_embedder_vector_is_l2_normalized() {
    let embedder = create_embedder();
    let embedding = embed_sync(&embedder, "Test sentence for normalization check.");

    // Calculate Euclidean norm
    let norm: f32 = embedding.vector.iter().map(|x| x * x).sum::<f32>().sqrt();

    // Should be approximately 1.0 (allow small floating point error)
    assert!(
        (norm - 1.0).abs() < 1e-4,
        "L2 norm should be approximately 1.0, got {}",
        norm
    );
}

#[test]
#[ignore = "Requires model download (~85 MB) — run explicitly with `cargo test -- --ignored`"]
fn test_embedder_batch_works() {
    let embedder = create_embedder();

    let texts = vec![
        "First test sentence.".to_string(),
        "Second test sentence with more words.".to_string(),
        "Third sentence, slightly different.".to_string(),
    ];

    let embeddings = embed_batch_sync(&embedder, &texts);

    assert_eq!(
        embeddings.len(),
        3,
        "Should return 3 embeddings for 3 texts"
    );

    for (i, emb) in embeddings.iter().enumerate() {
        assert_eq!(
            emb.vector.len(),
            384,
            "Embedding {} should be 384-dimensional",
            i
        );
        let norm: f32 = emb.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "Embedding {} norm should be ~1.0, got {}",
            i,
            norm
        );
    }
}

#[test]
#[ignore = "Requires model download (~85 MB) — run explicitly with `cargo test -- --ignored`"]
fn test_embedder_similar_texts_have_similar_embeddings() {
    let embedder = create_embedder();

    let text_a = "The cat sat on the mat.";
    let text_b = "A cat is sitting on a mat.";
    let text_c = "Quantum physics explains particle behavior.";

    let emb_a = embed_sync(&embedder, text_a);
    let emb_b = embed_sync(&embedder, text_b);
    let emb_c = embed_sync(&embedder, text_c);

    // Cosine similarity (vectors are L2-normalized, so dot product = cosine)
    let sim_ab: f32 = emb_a
        .vector
        .iter()
        .zip(emb_b.vector.iter())
        .map(|(a, b)| a * b)
        .sum();

    let sim_ac: f32 = emb_a
        .vector
        .iter()
        .zip(emb_c.vector.iter())
        .map(|(a, b)| a * b)
        .sum();

    // Similar texts should be more similar than dissimilar ones
    assert!(
        sim_ab > sim_ac,
        "Similar texts should have higher cosine similarity ({} vs {})",
        sim_ab,
        sim_ac
    );
}

#[test]
#[ignore = "Requires model download (~85 MB) — run explicitly with `cargo test -- --ignored`"]
fn test_embedder_arc_can_be_shared() {
    let embedder = Arc::new(create_embedder());

    // Simulate sharing across actors
    let embedder_clone = Arc::clone(&embedder);

    let text = "Testing shared embedder across threads.";
    let emb1 = embed_sync(&embedder, text);
    let emb2 = embed_sync(&embedder_clone, text);

    // Same text should produce the same embedding
    assert_eq!(emb1.vector, emb2.vector, "Same text should produce identical embeddings");
}
