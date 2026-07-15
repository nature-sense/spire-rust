// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

pub mod candle_embedder;

pub use candle_embedder::{CandleEmbedder, create_embedder};

use anyhow::Result;
use async_trait::async_trait;
use tracing::warn;

use crate::models::embedding::{Embedder, Embedding};

/// A no-op embedder that returns zero vectors.
///
/// Used as a fallback when the real CandleEmbedder fails to load
/// (e.g. network unavailable during first run, or model download failure).
/// This allows the system to start in degraded mode rather than crashing.
#[derive(Debug)]
pub struct NoopEmbedder;

#[async_trait]
impl Embedder for NoopEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        warn!("NoopEmbedder: returning zero vector for '{}'", text);
        Ok(Embedding::new(vec![0.0f32; 384], text, "noop"))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        warn!("NoopEmbedder: returning zero vectors for {} text(s)", texts.len());
        Ok(texts
            .iter()
            .map(|text| Embedding::new(vec![0.0f32; 384], text, "noop"))
            .collect())
    }

    fn dimensions(&self) -> usize {
        384
    }
}
