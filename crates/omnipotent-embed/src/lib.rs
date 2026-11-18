//! Source-independent passage preparation, batch planning and embedding.
pub mod chunkers;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};
use embeddinggemma2::{config::ModelConfig, ops, text::TextModel, weights::Weights};
use omnipotent_format::{
    Article, DocumentId,
    package::{ResourceKind, SearchEntry},
};
use std::{path::Path, sync::Arc};
use tokenizers::Tokenizer;

pub const MAX_INPUT_TOKENS: usize = 384;
pub const BATCH_TOKENS: usize = 2048;
pub const MAX_BATCH_SIZE: usize = 128;
pub const DIMENSIONS: usize = 768;
pub const MODEL: &str = "google/embeddinggemma-2";
pub const MODEL_REVISION: &str = "f9972ac22e696a105c482202646979e33e4b5a1e";
pub const DOCUMENT_TEMPLATE: &str = "title: {title} | text: {heading}{text}";
pub const QUERY_TEMPLATE: &str = "task: search result | query: {query}";

pub struct PreparedPassage {
    pub entry: SearchEntry,
    pub tokens: Vec<u32>,
}

pub struct Chunker {
    tokenizer: Tokenizer,
}
impl Chunker {
    pub fn load(path: &Path) -> Result<Self> {
        let mut tokenizer =
            Tokenizer::from_file(path).map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        tokenizer
            .with_truncation(None)
            .map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        tokenizer.with_padding(None);
        Ok(Self { tokenizer })
    }
    pub fn chunk(
        &self,
        kind: ResourceKind,
        article: &Article,
        document: DocumentId,
    ) -> Result<Vec<PreparedPassage>> {
        match kind {
            ResourceKind::Article => chunkers::article::chunk(self, article, document),
            ResourceKind::Publication => anyhow::bail!("publication chunking is not implemented"), // TODO: xberg.
            ResourceKind::Map | ResourceKind::Media | ResourceKind::Data => {
                anyhow::bail!("pillar chunking is not implemented")
            } // TODO.
        }
    }
    pub fn tokens(&self, input: &str) -> Result<Vec<u32>> {
        Ok(self
            .tokenizer
            .encode(input, true)
            .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))?
            .get_ids()
            .to_vec())
    }
}

/// Descriptors may be sorted independently of token/text storage. Call after sorting
/// by (token_count, passage_id); returns the next adjacent-length batch size.
pub fn batch_len(lengths: impl IntoIterator<Item = usize>, maximum: usize) -> Result<usize> {
    ensure!(
        (1..=MAX_BATCH_SIZE).contains(&maximum),
        "batch size must be 1..={MAX_BATCH_SIZE}"
    );
    let mut count = 0;
    let mut longest = 0;
    for length in lengths.into_iter().take(maximum) {
        ensure!(
            (1..=MAX_INPUT_TOKENS).contains(&length),
            "prepared input must have 1..={MAX_INPUT_TOKENS} tokens"
        );
        longest = longest.max(length);
        if (count + 1) * longest > BATCH_TOKENS {
            break;
        }
        count += 1;
    }
    Ok(count)
}

enum Backend {
    Cpu(Box<TextModel>),
    #[cfg(feature = "cuda")]
    Cuda(Box<embeddinggemma2::gpu::text::GpuText>),
}

pub struct Encoder {
    backend: Backend,
    hidden: usize,
}
impl Encoder {
    /// Original safetensors, always FP32. Auto selects CUDA when it initialises;
    /// an inference failure is returned to the caller, never retried on CPU.
    pub fn load(directory: &Path, device: &str) -> Result<Self> {
        ensure!(
            ["auto", "cpu", "cuda"].contains(&device),
            "device must be auto, cpu or cuda (Metal is not supported)"
        );
        let config = ModelConfig::load(directory)?;
        ensure!(
            config.text_config.embedding_dim == DIMENSIONS,
            "expected 768-dimensional model"
        );
        let hidden = config.text_config.hidden_size;
        let weights = Arc::new(Weights::open(&directory.join("model.safetensors"))?);
        let text = TextModel::load(&config.text_config, Arc::clone(&weights))
            .context("loading text backbone")?;
        if device != "cpu" {
            #[cfg(feature = "cuda")]
            {
                use embeddinggemma2::{
                    Precision,
                    gpu::{text::GpuText, try_open},
                };
                match try_open(0, Precision::F32) {
                    Ok(gpu) => match GpuText::from_cpu(text, Arc::new(gpu)) {
                        Ok(model) => {
                            eprintln!("device=cuda precision=FP32 backend=embeddinggemma2-rust");
                            return Ok(Self {
                                backend: Backend::Cuda(Box::new(model)),
                                hidden,
                            });
                        }
                        Err(error) if device == "cuda" => {
                            return Err(error.context("loading CUDA text model"));
                        }
                        Err(error) => {
                            eprintln!("CUDA model initialisation failed: {error}; using CPU");
                            let text = TextModel::load(&config.text_config, weights)?;
                            return Ok(Self {
                                backend: Backend::Cpu(Box::new(text)),
                                hidden,
                            });
                        }
                    },
                    Err(error) if device == "cuda" => {
                        return Err(error.context(
                            "CUDA initialisation; install NVIDIA drivers, cuBLAS and NVRTC",
                        ));
                    }
                    Err(error) => eprintln!("CUDA unavailable: {error}; using CPU"),
                }
            }
            #[cfg(not(feature = "cuda"))]
            ensure!(
                device != "cuda",
                "this executable was built without CUDA support"
            );
        }
        eprintln!("device=cpu precision=FP32 backend=embeddinggemma2-rust");
        Ok(Self {
            backend: Backend::Cpu(Box::new(text)),
            hidden,
        })
    }

    /// Uses the backend's public token-input primitives so persisted token rows
    /// are never re-tokenised. The upstream encoder isolates packed sequences.
    pub fn embed(&self, rows: &[Vec<u32>]) -> Result<Vec<[f32; DIMENSIONS]>> {
        ensure!(!rows.is_empty(), "empty inference batch");
        ensure!(
            batch_len(rows.iter().map(Vec::len), MAX_BATCH_SIZE)? == rows.len(),
            "batch exceeds passage or padded-token budget"
        );
        let lengths: Vec<_> = rows.iter().map(Vec::len).collect();
        let total: usize = lengths.iter().sum();
        let mut inputs = vec![0.; total * self.hidden];
        let mut offset = 0;
        for row in rows {
            let destination = &mut inputs[offset * self.hidden..(offset + row.len()) * self.hidden];
            match &self.backend {
                Backend::Cpu(model) => model.embed_tokens(row, destination)?,
                #[cfg(feature = "cuda")]
                Backend::Cuda(model) => model.embed_tokens(row, destination)?,
            }
            offset += row.len();
        }
        let outputs = match &self.backend {
            Backend::Cpu(model) => {
                ops::run_sized(total, || model.forward_packed(&inputs, &lengths))
            }
            #[cfg(feature = "cuda")]
            Backend::Cuda(model) => model.forward_packed(&inputs, &lengths)?,
        };
        let mut offset = 0;
        rows.iter()
            .map(|row| {
                let mut pooled = ops::mean_rows(
                    &outputs[offset * DIMENSIONS..(offset + row.len()) * DIMENSIONS],
                    row.len(),
                    DIMENSIONS,
                );
                offset += row.len();
                ops::l2_normalize(&mut pooled);
                ensure!(
                    pooled.iter().all(|x| x.is_finite()),
                    "non-finite model output"
                );
                pooled
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("model returned the wrong dimension"))
            })
            .collect()
    }
}
