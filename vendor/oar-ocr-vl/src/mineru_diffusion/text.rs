//! SDAR block-diffusion text decoder.
//!
//! Architecturally a Qwen2-style decoder (RMSNorm, SwiGLU MLP, GQA, RoPE) with
//! two departures that matter for block diffusion:
//!
//! * **QK-norm** — per-head RMSNorm is applied to queries and keys before RoPE.
//! * **Non-causal attention** — there is no built-in causal mask. Causality is
//!   imposed at *block* granularity by the caller: the prompt is prefilled with
//!   an explicit block-causal mask and each generation block attends fully to
//!   the committed KV cache plus itself (no mask needed).
//!
//! The [`SdarKvCache`] supports the diffusion access pattern: denoising passes
//! read the committed cache and transiently append the in-flight block's KV
//! without storing it (`store_kv = false`); once a block is fully decoded it is
//! committed once (`store_kv = true`).

use super::config::SdarConfig;
use crate::attention::{
    RotaryEmbedding, flash_attention, scaled_dot_product_attention_gqa,
    segmented_scaled_dot_product_attention_gqa,
};
use crate::error::Error;
use crate::kv_trim::TrimmableKvCache;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing, rotate_half};
use candle_core::{DType, Device, Tensor};
use candle_nn::{
    Embedding, Linear, Module, RmsNorm, VarBuilder, embedding, linear, linear_no_bias, rms_norm,
};

fn proc_err(msg: &'static str, e: candle_core::Error) -> Error {
    candle_to_ocr_processing(crate::error::ProcessingStage::TensorOperation, msg, e)
}

/// Per-layer committed KV (rope already applied to keys).
/// Shape `(batch=1, num_kv_heads, len, head_dim)`.
#[derive(Debug, Clone)]
struct LayerKvCache {
    /// Immutable prefix inherited from another request. Tensor views are
    /// reference counted by Candle, so fork is O(layers) metadata and does not
    /// copy K/V data. Only `committed` is ever mutated by this branch.
    shared_prefix: Option<(Tensor, Tensor)>,
    committed: TrimmableKvCache,
}

impl LayerKvCache {
    fn new() -> Self {
        Self::with_capacity(0)
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            shared_prefix: None,
            committed: TrimmableKvCache::new(2, capacity),
        }
    }

    #[cfg(test)]
    fn full_kv(
        &mut self,
        k: &Tensor,
        v: &Tensor,
        store_kv: bool,
    ) -> candle_core::Result<(Tensor, Tensor)> {
        let (local_k, local_v) = if store_kv {
            if self.committed.storage_capacity() == 0 && self.committed.max_seq_len() > 0 {
                self.committed.initialize_storage(k)?;
            }
            self.committed.append(k, v)?
        } else {
            match (self.committed.k(), self.committed.v()) {
                (Some(committed_k), Some(committed_v)) => (
                    Tensor::cat(&[committed_k, k], 2)?,
                    Tensor::cat(&[committed_v, v], 2)?,
                ),
                _ => (k.clone(), v.clone()),
            }
        };
        match &self.shared_prefix {
            Some((prefix_k, prefix_v)) => Ok((
                Tensor::cat(&[prefix_k, &local_k], 2)?,
                Tensor::cat(&[prefix_v, &local_v], 2)?,
            )),
            None => Ok((local_k, local_v)),
        }
    }

    fn kv_segments(
        &mut self,
        k: &Tensor,
        v: &Tensor,
        store_kv: bool,
    ) -> candle_core::Result<Vec<(Tensor, Tensor)>> {
        let (local_k, local_v) = if store_kv {
            if self.committed.storage_capacity() == 0 && self.committed.max_seq_len() > 0 {
                self.committed.initialize_storage(k)?;
            }
            self.committed.append(k, v)?
        } else {
            match (self.committed.k(), self.committed.v()) {
                (Some(committed_k), Some(committed_v)) => (
                    Tensor::cat(&[committed_k, k], 2)?,
                    Tensor::cat(&[committed_v, v], 2)?,
                ),
                _ => (k.clone(), v.clone()),
            }
        };
        let mut segments = Vec::with_capacity(2);
        if let Some((prefix_k, prefix_v)) = &self.shared_prefix {
            segments.push((prefix_k.clone(), prefix_v.clone()));
        }
        segments.push((local_k, local_v));
        Ok(segments)
    }

    fn seq_len(&self) -> usize {
        self.shared_prefix_len() + self.committed.current_seq_len()
    }

    fn shared_prefix_len(&self) -> usize {
        self.shared_prefix.as_ref().map_or(0, |(k, _)| {
            k.dim(2)
                .expect("LayerKvCache shared-prefix K must remain rank-4")
        })
    }

    fn fork_at(&self, len: usize) -> candle_core::Result<Self> {
        if len > self.seq_len() {
            candle_core::bail!(
                "cannot fork KV at {len}, cache length is {}",
                self.seq_len()
            )
        }
        let prefix_len = self.shared_prefix_len();
        let shared_prefix = if len == 0 {
            None
        } else if len <= prefix_len {
            let (k, v) = self.shared_prefix.as_ref().expect("non-empty prefix");
            Some((k.narrow(2, 0, len)?, v.narrow(2, 0, len)?))
        } else {
            let local_len = len - prefix_len;
            let local_k = self.committed.k().ok_or_else(|| {
                candle_core::Error::Msg("missing local K while forking cache".into())
            })?;
            let local_v = self.committed.v().ok_or_else(|| {
                candle_core::Error::Msg("missing local V while forking cache".into())
            })?;
            let local_k = local_k.narrow(2, 0, local_len)?;
            let local_v = local_v.narrow(2, 0, local_len)?;
            match &self.shared_prefix {
                Some((prefix_k, prefix_v)) => Some((
                    Tensor::cat(&[prefix_k, &local_k], 2)?,
                    Tensor::cat(&[prefix_v, &local_v], 2)?,
                )),
                None => Some((local_k, local_v)),
            }
        };
        Ok(Self {
            shared_prefix,
            committed: TrimmableKvCache::new(2, 0),
        })
    }

    fn trim_to(&mut self, len: usize) -> candle_core::Result<()> {
        if len >= self.seq_len() {
            return Ok(());
        }
        let prefix_len = self.shared_prefix_len();
        if len < prefix_len {
            let (k, v) = self.shared_prefix.as_ref().expect("non-empty prefix");
            self.shared_prefix = if len == 0 {
                None
            } else {
                Some((k.narrow(2, 0, len)?, v.narrow(2, 0, len)?))
            };
            self.committed.reset();
        } else {
            self.committed.trim_to(len - prefix_len)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct AttentionMode {
    store_kv: bool,
    is_causal: bool,
}

/// Committed KV cache across all decoder layers.
#[derive(Debug, Clone)]
pub struct SdarKvCache {
    layers: Vec<LayerKvCache>,
}

impl SdarKvCache {
    pub fn new(num_layers: usize) -> Self {
        Self {
            layers: (0..num_layers).map(|_| LayerKvCache::new()).collect(),
        }
    }

    /// Create a cache with stable per-layer backing storage. HPD uses this for
    /// the parent request so every child fork can keep a view of the same
    /// allocation even while the parent continues decoding.
    pub(crate) fn with_capacity(num_layers: usize, capacity: usize) -> Self {
        Self {
            layers: (0..num_layers)
                .map(|_| LayerKvCache::with_capacity(capacity))
                .collect(),
        }
    }

    /// Return the committed sequence length. All decoder layers are kept in
    /// lockstep, so reading the first layer is sufficient.
    pub(crate) fn seq_len(&self) -> usize {
        self.layers.first().map_or(0, LayerKvCache::seq_len)
    }

    /// Fork a request at an exact token boundary. Boundaries within one backing
    /// segment produce immutable Tensor views and an empty private tail, so
    /// appending to either branch cannot corrupt the other. Recursively forking
    /// past an inherited prefix materializes that prefix plus the selected
    /// private tail because this cache currently stores only one shared segment.
    pub(crate) fn fork_at(&self, len: usize) -> Result<Self, Error> {
        let layers = self
            .layers
            .iter()
            .map(|layer| layer.fork_at(len))
            .collect::<candle_core::Result<Vec<_>>>()
            .map_err(|e| candle_to_ocr_inference("Qwen3", "fork causal KV cache", e))?;
        Ok(Self { layers })
    }

    pub(crate) fn shared_prefix_len(&self) -> usize {
        self.layers
            .first()
            .map_or(0, LayerKvCache::shared_prefix_len)
    }

    /// Roll every layer back to a shared prefix. This is used by models with
    /// speculative or forked decoding to discard unaccepted/tail tokens while
    /// retaining the already-computed prefix KV.
    pub(crate) fn trim_to(&mut self, len: usize) -> Result<(), Error> {
        for layer in &mut self.layers {
            layer
                .trim_to(len)
                .map_err(|e| candle_to_ocr_inference("Qwen3", "trim causal KV cache", e))?;
        }
        Ok(())
    }
}

fn apply_rope(x: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
    let a = x
        .broadcast_mul(cos)
        .map_err(|e| proc_err("SDAR rope x*cos", e))?;
    let b = rotate_half(x)?
        .broadcast_mul(sin)
        .map_err(|e| proc_err("SDAR rope rotate_half*sin", e))?;
    (a + b).map_err(|e| proc_err("SDAR rope sum", e))
}

#[derive(Debug)]
struct SdarAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    num_heads: usize,
    num_kv_heads: usize,
    head_dim: usize,
    scale: f64,
}

impl SdarAttention {
    fn load(cfg: &SdarConfig, vb: VarBuilder) -> Result<Self, Error> {
        if !cfg
            .num_attention_heads
            .is_multiple_of(cfg.num_key_value_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "MinerU-Diffusion: num_attention_heads ({}) must be divisible by num_key_value_heads ({})",
                    cfg.num_attention_heads, cfg.num_key_value_heads
                ),
            });
        }
        let head_dim = cfg.head_dim()?;
        let q_dim = cfg.num_attention_heads * head_dim;
        let kv_dim = cfg.num_key_value_heads * head_dim;
        let mk = |i: usize, o: usize, name: &str, vb: VarBuilder| -> Result<Linear, Error> {
            if cfg.attention_bias {
                linear(i, o, vb)
            } else {
                linear_no_bias(i, o, vb)
            }
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", name, e))
        };
        let q_proj = mk(cfg.hidden_size, q_dim, "load q_proj", vb.pp("q_proj"))?;
        let k_proj = mk(cfg.hidden_size, kv_dim, "load k_proj", vb.pp("k_proj"))?;
        let v_proj = mk(cfg.hidden_size, kv_dim, "load v_proj", vb.pp("v_proj"))?;
        let o_proj = mk(q_dim, cfg.hidden_size, "load o_proj", vb.pp("o_proj"))?;
        let q_norm = rms_norm(head_dim, cfg.rms_norm_eps, vb.pp("q_norm"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load q_norm", e))?;
        let k_norm = rms_norm(head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load k_norm", e))?;
        Ok(Self {
            q_proj,
            k_proj,
            v_proj,
            o_proj,
            q_norm,
            k_norm,
            num_heads: cfg.num_attention_heads,
            num_kv_heads: cfg.num_key_value_heads,
            head_dim,
            scale: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn forward(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        mask: Option<&Tensor>,
        cache: &mut LayerKvCache,
        mode: AttentionMode,
    ) -> Result<Tensor, Error> {
        let (b, s, _) = hidden
            .dims3()
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "attn dims", e))?;

        let project =
            |proj: &Linear, heads: usize, norm: Option<&RmsNorm>| -> Result<Tensor, Error> {
                let x = proj
                    .forward(hidden)
                    .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "attn proj", e))?;
                let x = x
                    .reshape((b, s, heads, self.head_dim))
                    .map_err(|e| proc_err("SDAR attn reshape", e))?;
                let x = match norm {
                    Some(n) => n.forward(&x).map_err(|e| {
                        candle_to_ocr_inference("MinerU-Diffusion", "attn qk-norm", e)
                    })?,
                    None => x,
                };
                x.transpose(1, 2)
                    .map_err(|e| proc_err("SDAR attn transpose", e))?
                    .contiguous()
                    .map_err(|e| proc_err("SDAR attn contiguous", e))
            };

        let q = project(&self.q_proj, self.num_heads, Some(&self.q_norm))?;
        let k = project(&self.k_proj, self.num_kv_heads, Some(&self.k_norm))?;
        let v = project(&self.v_proj, self.num_kv_heads, None)?;

        let q = apply_rope(&q, cos, sin)?;
        let k = apply_rope(&k, cos, sin)?;

        // Committed causal/decode KV uses appendable backing storage. SDAR's
        // in-flight denoising block remains transient and is concatenated only
        // for that attention pass.
        let segments = cache
            .kv_segments(&k, &v, mode.store_kv)
            .map_err(|e| proc_err("SDAR kv cache update", e))?;

        let n_rep = self.num_heads / self.num_kv_heads;
        let flash = if mask.is_none() && segments.len() == 1 {
            flash_attention(
                &q,
                &segments[0].0,
                &segments[0].1,
                self.scale,
                mode.is_causal,
            )
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "flash attention", e))?
        } else {
            None
        };
        let attn = match flash {
            Some(attn) => attn,
            None if segments.len() == 1 => scaled_dot_product_attention_gqa(
                &q,
                &segments[0].0,
                &segments[0].1,
                mask,
                self.scale,
                mode.is_causal,
                n_rep,
            )
            .map_err(|e| {
                candle_to_ocr_inference("MinerU-Diffusion", "grouped-query attention", e)
            })?,
            None => {
                let refs = segments.iter().map(|(k, v)| (k, v)).collect::<Vec<_>>();
                segmented_scaled_dot_product_attention_gqa(
                    &q,
                    &refs,
                    mask,
                    self.scale,
                    mode.is_causal,
                    n_rep,
                )
                .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "segmented GQA", e))?
            }
        };
        let attn = attn
            .transpose(1, 2)
            .map_err(|e| proc_err("SDAR attn out transpose", e))?
            .reshape((b, s, self.num_heads * self.head_dim))
            .map_err(|e| proc_err("SDAR attn out reshape", e))?;
        self.o_proj
            .forward(&attn)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "o_proj", e))
    }

    fn forward_batch(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        caches: &mut [&mut LayerKvCache],
        mode: AttentionMode,
    ) -> Result<Tensor, Error> {
        let (b, s, _) = hidden
            .dims3()
            .map_err(|e| candle_to_ocr_inference("Qwen3", "batched attn dims", e))?;
        if b != caches.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "Qwen3 batched attention has {b} rows but {} caches",
                    caches.len()
                ),
            });
        }
        let project =
            |proj: &Linear, heads: usize, norm: Option<&RmsNorm>| -> Result<Tensor, Error> {
                let x = proj
                    .forward(hidden)
                    .map_err(|e| candle_to_ocr_inference("Qwen3", "batched attn proj", e))?;
                let x = x
                    .reshape((b, s, heads, self.head_dim))
                    .map_err(|e| proc_err("batched Qwen3 attn reshape", e))?;
                let x = match norm {
                    Some(n) => n
                        .forward(&x)
                        .map_err(|e| candle_to_ocr_inference("Qwen3", "batched attn qk-norm", e))?,
                    None => x,
                };
                x.transpose(1, 2)
                    .and_then(|x| x.contiguous())
                    .map_err(|e| proc_err("batched Qwen3 attn transpose", e))
            };
        let q = project(&self.q_proj, self.num_heads, Some(&self.q_norm))?;
        let k = project(&self.k_proj, self.num_kv_heads, Some(&self.k_norm))?;
        let v = project(&self.v_proj, self.num_kv_heads, None)?;
        let cos = cos
            .narrow(0, 0, 1)
            .and_then(|x| x.squeeze(0))
            .and_then(|x| x.unsqueeze(1))
            .map_err(|e| proc_err("batched Qwen3 rope cos shape", e))?;
        let sin = sin
            .narrow(0, 0, 1)
            .and_then(|x| x.squeeze(0))
            .and_then(|x| x.unsqueeze(1))
            .map_err(|e| proc_err("batched Qwen3 rope sin shape", e))?;
        let q = apply_rope(&q, &cos, &sin)?;
        let k = apply_rope(&k, &cos, &sin)?;
        let n_rep = self.num_heads / self.num_kv_heads;
        let mut rows = Vec::with_capacity(b);
        // Projection and MLP work stay batched, but variable-length forked KV
        // currently requires one segmented-attention launch sequence per row.
        // A future ragged/paged kernel can remove this remaining launch loop.
        for (row, cache) in caches.iter_mut().enumerate() {
            let q_row = q
                .narrow(0, row, 1)
                .map_err(|e| proc_err("batch Q row", e))?;
            let k_row = k
                .narrow(0, row, 1)
                .map_err(|e| proc_err("batch K row", e))?;
            let v_row = v
                .narrow(0, row, 1)
                .map_err(|e| proc_err("batch V row", e))?;
            let segments = cache
                .kv_segments(&k_row, &v_row, mode.store_kv)
                .map_err(|e| proc_err("batched Qwen3 KV update", e))?;
            let refs = segments.iter().map(|(k, v)| (k, v)).collect::<Vec<_>>();
            rows.push(
                segmented_scaled_dot_product_attention_gqa(
                    &q_row,
                    &refs,
                    None,
                    self.scale,
                    mode.is_causal,
                    n_rep,
                )
                .map_err(|e| candle_to_ocr_inference("Qwen3", "batched segmented GQA", e))?,
            );
        }
        let row_refs = rows.iter().collect::<Vec<_>>();
        let attn = Tensor::cat(&row_refs, 0)
            .and_then(|x| x.transpose(1, 2))
            .and_then(|x| x.reshape((b, s, self.num_heads * self.head_dim)))
            .map_err(|e| proc_err("batched Qwen3 attention output", e))?;
        self.o_proj
            .forward(&attn)
            .map_err(|e| candle_to_ocr_inference("Qwen3", "batched o_proj", e))
    }
}

#[derive(Debug)]
struct SdarMlp {
    gate: Linear,
    up: Linear,
    down: Linear,
}

impl SdarMlp {
    fn load(cfg: &SdarConfig, vb: VarBuilder) -> Result<Self, Error> {
        let gate = linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("gate_proj"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load gate_proj", e))?;
        let up = linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("up_proj"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load up_proj", e))?;
        let down = linear_no_bias(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_proj"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load down_proj", e))?;
        Ok(Self { gate, up, down })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let gate = self
            .gate
            .forward(x)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "gate_proj", e))?;
        let gate = candle_nn::ops::silu(&gate).map_err(|e| proc_err("SDAR silu", e))?;
        let up = self
            .up
            .forward(x)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "up_proj", e))?;
        let prod = (gate * up).map_err(|e| proc_err("SDAR mlp mul", e))?;
        self.down
            .forward(&prod)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "down_proj", e))
    }
}

#[derive(Debug)]
struct SdarLayer {
    input_layernorm: RmsNorm,
    self_attn: SdarAttention,
    post_attention_layernorm: RmsNorm,
    mlp: SdarMlp,
}

impl SdarLayer {
    fn load(cfg: &SdarConfig, vb: VarBuilder) -> Result<Self, Error> {
        let input_layernorm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load input_layernorm", e))?;
        let post_attention_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_attention_layernorm"),
        )
        .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load post_attn_ln", e))?;
        let self_attn = SdarAttention::load(cfg, vb.pp("self_attn"))?;
        let mlp = SdarMlp::load(cfg, vb.pp("mlp"))?;
        Ok(Self {
            input_layernorm,
            self_attn,
            post_attention_layernorm,
            mlp,
        })
    }

    fn forward(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        mask: Option<&Tensor>,
        cache: &mut LayerKvCache,
        mode: AttentionMode,
    ) -> Result<Tensor, Error> {
        let normed = self
            .input_layernorm
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "input_layernorm", e))?;
        let attn = self
            .self_attn
            .forward(&normed, cos, sin, mask, cache, mode)?;
        let hidden = (hidden + attn).map_err(|e| proc_err("SDAR attn residual", e))?;
        let normed = self
            .post_attention_layernorm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "post_attn_ln", e))?;
        let mlp = self.mlp.forward(&normed)?;
        (hidden + mlp).map_err(|e| proc_err("SDAR mlp residual", e))
    }

    fn forward_batch(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        caches: &mut [&mut LayerKvCache],
        mode: AttentionMode,
    ) -> Result<Tensor, Error> {
        let normed = self
            .input_layernorm
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference("Qwen3", "batched input_layernorm", e))?;
        let attn = self
            .self_attn
            .forward_batch(&normed, cos, sin, caches, mode)?;
        let hidden = (hidden + attn).map_err(|e| proc_err("batched Qwen3 attn residual", e))?;
        let normed = self
            .post_attention_layernorm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("Qwen3", "batched post-attn norm", e))?;
        let mlp = self.mlp.forward(&normed)?;
        (hidden + mlp).map_err(|e| proc_err("batched Qwen3 MLP residual", e))
    }
}

/// SDAR decoder stack + tied/untied LM head.
pub struct SdarModel {
    embed_tokens: Embedding,
    layers: Vec<SdarLayer>,
    norm: RmsNorm,
    lm_head: Linear,
    rotary: RotaryEmbedding,
    dtype: DType,
    device: Device,
}

impl SdarModel {
    /// `vb` should point at `language_model` (so `vb.pp("model")` is the
    /// decoder and `vb.pp("lm_head")` the output projection).
    pub fn load(cfg: &SdarConfig, vb: VarBuilder, dtype: DType) -> Result<Self, Error> {
        let model_vb = vb.pp("model");
        let embed_tokens = embedding(cfg.vocab_size, cfg.hidden_size, model_vb.pp("embed_tokens"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load embed_tokens", e))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            layers.push(SdarLayer::load(cfg, model_vb.pp(format!("layers.{i}")))?);
        }
        let norm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, model_vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load norm", e))?;
        let lm_head = if cfg.tie_word_embeddings {
            Linear::new(embed_tokens.embeddings().clone(), None)
        } else {
            linear_no_bias(cfg.hidden_size, cfg.vocab_size, vb.pp("lm_head"))
                .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load lm_head", e))?
        };
        let head_dim = cfg.head_dim()?;
        let rotary = RotaryEmbedding::new_dynamic(head_dim, cfg.rope_theta, vb.device())?;
        Ok(Self {
            embed_tokens,
            layers,
            norm,
            lm_head,
            rotary,
            dtype,
            device: vb.device().clone(),
        })
    }

    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }

    /// Embed token ids `(1, S)` into `(1, S, hidden)`.
    pub fn embed(&self, input_ids: &Tensor) -> Result<Tensor, Error> {
        self.embed_tokens
            .forward(input_ids)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "embed", e))
    }

    fn rope_for(&self, positions: &[i64]) -> Result<(Tensor, Tensor), Error> {
        let s = positions.len();
        let pos = Tensor::from_vec(positions.to_vec(), (1, 1, s), &self.device)
            .map_err(|e| proc_err("SDAR position_ids tensor", e))?;
        self.rotary.forward_multi_axis(&pos, self.dtype)
    }

    fn rope_for_batch(&self, positions: &[Vec<i64>]) -> Result<(Tensor, Tensor), Error> {
        let batch = positions.len();
        let seq = positions.first().map_or(0, Vec::len);
        if batch == 0 || seq == 0 || positions.iter().any(|row| row.len() != seq) {
            return Err(Error::InvalidInput {
                message: "Qwen3 batched positions must be a non-empty rectangle".to_string(),
            });
        }
        let flat = positions.iter().flatten().copied().collect::<Vec<_>>();
        let pos = Tensor::from_vec(flat, (1, batch, seq), &self.device)
            .map_err(|e| proc_err("Qwen3 batched position tensor", e))?;
        self.rotary.forward_multi_axis(&pos, self.dtype)
    }

    /// Run the decoder over `inputs_embeds` `(1, S, hidden)` at the given
    /// absolute `positions`, returning the post-norm hidden states `(1, S, hidden)`.
    /// `mask` is an additive attention bias `(1, 1, S, kv_len)` or `None` for
    /// full attention. `kv_len` is the complete logical cache length, including
    /// any inherited shared prefix and the branch-private tail. When `store_kv`
    /// is set, each layer commits its KV.
    pub fn forward(
        &self,
        inputs_embeds: &Tensor,
        positions: &[i64],
        mask: Option<&Tensor>,
        cache: &mut SdarKvCache,
        store_kv: bool,
    ) -> Result<Tensor, Error> {
        self.forward_inner(inputs_embeds, positions, mask, cache, store_kv, false)
    }

    /// Run the same Qwen3 decoder in ordinary autoregressive causal mode.
    ///
    /// SDAR itself uses block-level masks and therefore calls [`Self::forward`]
    /// with non-causal attention. Multimodal models that share its Qwen3 +
    /// QK-norm backbone can use this path for flash-attention prefill and
    /// token-by-token decoding without duplicating the decoder stack.
    pub(crate) fn forward_causal(
        &self,
        inputs_embeds: &Tensor,
        positions: &[i64],
        cache: &mut SdarKvCache,
        store_kv: bool,
    ) -> Result<Tensor, Error> {
        self.forward_inner(inputs_embeds, positions, None, cache, store_kv, true)
    }

    /// Continuous-batching decode for independent requests with distinct,
    /// forkable KV caches. Rows must have the same query length (ordinary
    /// decode uses 1; P-MTP verification uses `k + 1`), while their cached
    /// prefix lengths may differ.
    pub(crate) fn forward_causal_batch(
        &self,
        inputs_embeds: &Tensor,
        positions: &[Vec<i64>],
        caches: &mut [&mut SdarKvCache],
    ) -> Result<Tensor, Error> {
        let (batch, seq, _) = inputs_embeds
            .dims3()
            .map_err(|e| candle_to_ocr_inference("Qwen3", "batched input shape", e))?;
        if batch != positions.len()
            || batch != caches.len()
            || positions.iter().any(|p| p.len() != seq)
        {
            return Err(Error::InvalidInput {
                message: format!(
                    "Qwen3 batch mismatch: embeddings={batch}x{seq}, positions={}, caches={}",
                    positions.len(),
                    caches.len()
                ),
            });
        }
        let (cos, sin) = self.rope_for_batch(positions)?;
        let mut hidden = inputs_embeds.clone();
        let mode = AttentionMode {
            store_kv: true,
            is_causal: true,
        };
        for (layer_index, layer) in self.layers.iter().enumerate() {
            let mut layer_caches = caches
                .iter_mut()
                .map(|cache| &mut cache.layers[layer_index])
                .collect::<Vec<_>>();
            hidden = layer.forward_batch(&hidden, &cos, &sin, &mut layer_caches, mode)?;
        }
        self.norm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("Qwen3", "batched final norm", e))
    }

    fn forward_inner(
        &self,
        inputs_embeds: &Tensor,
        positions: &[i64],
        mask: Option<&Tensor>,
        cache: &mut SdarKvCache,
        store_kv: bool,
        is_causal: bool,
    ) -> Result<Tensor, Error> {
        let (cos, sin) = self.rope_for(positions)?;
        let mut hidden = inputs_embeds.clone();
        let mode = AttentionMode {
            store_kv,
            is_causal,
        };
        for (layer, layer_cache) in self.layers.iter().zip(cache.layers.iter_mut()) {
            hidden = layer.forward(&hidden, &cos, &sin, mask, layer_cache, mode)?;
        }
        self.norm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "final norm", e))
    }

    /// Project hidden states `(1, S, hidden)` to logits `(1, S, vocab)`.
    pub fn lm_logits(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        self.lm_head
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "lm_head", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_cache_appends_in_place_between_growths() -> candle_core::Result<()> {
        let device = Device::Cpu;
        let mut cache = LayerKvCache::new();
        let prompt = Tensor::zeros((1, 2, 3, 4), DType::F32, &device)?;
        let token = Tensor::ones((1, 2, 1, 4), DType::F32, &device)?;

        let (prompt_k, _) = cache.full_kv(&prompt, &prompt, true)?;
        assert_eq!(prompt_k.dims(), &[1, 2, 3, 4]);
        assert_eq!(cache.committed.storage_capacity(), 3);

        let (first_decode_k, _) = cache.full_kv(&token, &token, true)?;
        assert_eq!(first_decode_k.dims(), &[1, 2, 4, 4]);
        assert_eq!(cache.committed.storage_capacity(), 6);

        let (second_decode_k, _) = cache.full_kv(&token, &token, true)?;
        assert_eq!(second_decode_k.dims(), &[1, 2, 5, 4]);
        assert_eq!(cache.committed.storage_capacity(), 6);

        let transient = Tensor::zeros((1, 2, 2, 4), DType::F32, &device)?;
        let (transient_k, _) = cache.full_kv(&transient, &transient, false)?;
        assert_eq!(transient_k.dims(), &[1, 2, 7, 4]);
        assert_eq!(cache.committed.current_seq_len(), 5);
        Ok(())
    }

    #[test]
    fn forked_cache_shares_prefix_and_owns_its_tail() -> Result<(), Error> {
        let device = Device::Cpu;
        let mut parent = SdarKvCache::with_capacity(2, 8);
        for layer in &mut parent.layers {
            let prefix = Tensor::zeros((1, 2, 4, 4), DType::F32, &device).unwrap();
            layer.full_kv(&prefix, &prefix, true).unwrap();
        }
        let mut child = parent.fork_at(3)?;
        assert_eq!(parent.seq_len(), 4);
        assert_eq!(child.seq_len(), 3);
        assert_eq!(child.shared_prefix_len(), 3);

        for layer in &mut parent.layers {
            let tail = Tensor::ones((1, 2, 2, 4), DType::F32, &device).unwrap();
            layer.full_kv(&tail, &tail, true).unwrap();
        }
        for layer in &mut child.layers {
            let tail = Tensor::ones((1, 2, 1, 4), DType::F32, &device).unwrap();
            layer.full_kv(&tail, &tail, true).unwrap();
        }
        assert_eq!(parent.seq_len(), 6);
        assert!(
            parent
                .layers
                .iter()
                .all(|layer| layer.committed.storage_capacity() == 8)
        );
        assert_eq!(child.seq_len(), 4);
        assert_eq!(child.shared_prefix_len(), 3);
        Ok(())
    }
}
