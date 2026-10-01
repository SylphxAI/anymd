//! DFlash parallel draft model for HunyuanOCR 1.5.
//!
//! The draft consumes intermediate features from the target decoder as cached
//! context K/V. Its queries are one target-produced bonus token followed by a
//! block of mask tokens. All mask positions are predicted in one non-causal
//! pass and are then verified by the target model in one causal pass.

#[cfg(feature = "cuda")]
use super::dynamic_kv::{
    DynamicPagedKvAppend, FusedAddRmsNormBf16, FusedRmsNormRopeBf16, FusedRopeBf16,
    FusedSiluMulBf16,
};
use crate::attention::{RotaryEmbedding, flash_attention, scaled_dot_product_attention_gqa};
#[cfg(feature = "cuda")]
use crate::cuda_kernels::ArgmaxFirstBf16;
#[cfg(feature = "cuda")]
use crate::decoder_graph::{
    CudaGraphDrainGuard, CudaGraphKvLengths, cuda_graph_error, drop_and_drain,
    report_stashed_cuda_error, sync_graph_tensor,
};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing, rotate_half};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{Linear, Module, RmsNorm, VarBuilder, linear_no_bias, rms_norm};
use serde::Deserialize;
use std::cell::RefCell;
use std::path::Path;

const ROPE_CACHE_LEN: usize = 16_384;

fn tensor_err(message: &'static str, error: candle_core::Error) -> Error {
    candle_to_ocr_processing(
        crate::error::ProcessingStage::TensorOperation,
        message,
        error,
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct DFlashTargetConfig {
    pub mask_token_id: u32,
    pub target_layer_ids: Vec<usize>,
}

/// Configuration stored in `dflash/config.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct DFlashConfig {
    pub block_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub dflash_config: DFlashTargetConfig,
}

impl DFlashConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "HunyuanOCR DFlash", "config.json")
    }

    fn validate(&self) -> Result<(), Error> {
        if self.block_size < 2
            || self.num_hidden_layers == 0
            || self.dflash_config.target_layer_ids.is_empty()
        {
            return Err(Error::Config {
                message: "HunyuanOCR DFlash: block size must be at least 2, and layer count and target layers must be non-zero".to_string(),
            });
        }
        if !self
            .num_attention_heads
            .is_multiple_of(self.num_key_value_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR DFlash: {} attention heads is not divisible by {} KV heads",
                    self.num_attention_heads, self.num_key_value_heads
                ),
            });
        }
        Ok(())
    }
}

const CONTEXT_KV_INITIAL_CAPACITY: usize = 16_384;
const CONTEXT_KV_PAGE_SIZE: usize = 32;

#[derive(Debug)]
struct ContextKv {
    storage: Option<(Tensor, Tensor)>,
    block_table: Option<Tensor>,
    k: Option<Tensor>,
    v: Option<Tensor>,
    len: usize,
    capacity: usize,
}

impl Default for ContextKv {
    fn default() -> Self {
        Self {
            storage: None,
            block_table: None,
            k: None,
            v: None,
            len: 0,
            capacity: CONTEXT_KV_INITIAL_CAPACITY,
        }
    }
}

impl ContextKv {
    fn reset(&mut self) {
        self.k = None;
        self.v = None;
        self.len = 0;
    }

    #[cfg(feature = "cuda")]
    fn reset_graph_storage(&mut self) {
        self.storage = None;
        self.block_table = None;
        self.capacity = CONTEXT_KV_INITIAL_CAPACITY;
        self.reset();
    }

    fn ensure_capacity(
        &mut self,
        template_k: &Tensor,
        template_v: &Tensor,
        required: usize,
    ) -> Result<(), Error> {
        let (_, heads, _, head_dim) = template_k
            .dims4()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: context template shape", e))?;
        let reusable = self.storage.as_ref().is_some_and(|(storage_k, storage_v)| {
            storage_k.dtype() == template_k.dtype()
                && storage_v.dtype() == template_v.dtype()
                && storage_k.device().same_device(template_k.device())
                && storage_v.device().same_device(template_v.device())
                && storage_k.dims()
                    == [
                        self.capacity / CONTEXT_KV_PAGE_SIZE,
                        CONTEXT_KV_PAGE_SIZE,
                        heads,
                        head_dim,
                    ]
                && storage_v.dims() == storage_k.dims()
        });
        if self.storage.is_some() && !reusable {
            self.storage = None;
            self.block_table = None;
            self.reset();
        }
        if self.storage.is_none() {
            self.capacity =
                self.capacity.max(required).div_ceil(CONTEXT_KV_PAGE_SIZE) * CONTEXT_KV_PAGE_SIZE;
            let blocks = self.capacity / CONTEXT_KV_PAGE_SIZE;
            let shape = (blocks, CONTEXT_KV_PAGE_SIZE, heads, head_dim);
            self.storage = Some((
                Tensor::zeros(shape, template_k.dtype(), template_k.device())
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: allocate context K", e))?,
                Tensor::zeros(shape, template_v.dtype(), template_v.device())
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: allocate context V", e))?,
            ));
            self.block_table = Some(
                Tensor::new((0..blocks as u32).collect::<Vec<_>>(), template_k.device())
                    .and_then(|x| x.reshape((1, blocks)))
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: create KV block table", e))?,
            );
        } else if required > self.capacity {
            let new_capacity = (self.capacity * 2)
                .max(required)
                .div_ceil(CONTEXT_KV_PAGE_SIZE)
                * CONTEXT_KV_PAGE_SIZE;
            let grow_by = new_capacity - self.capacity;
            let (old_k, old_v) = self.storage.as_ref().expect("context storage initialized");
            let old_k = old_k
                .reshape((self.capacity, heads, head_dim))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: flatten context K", e))?;
            let old_v = old_v
                .reshape((self.capacity, heads, head_dim))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: flatten context V", e))?;
            let extra_k = Tensor::zeros(
                (grow_by, heads, head_dim),
                template_k.dtype(),
                template_k.device(),
            )
            .map_err(|e| tensor_err("HunyuanOCR DFlash: grow context K", e))?;
            let extra_v = Tensor::zeros(
                (grow_by, heads, head_dim),
                template_v.dtype(),
                template_v.device(),
            )
            .map_err(|e| tensor_err("HunyuanOCR DFlash: grow context V", e))?;
            let blocks = new_capacity / CONTEXT_KV_PAGE_SIZE;
            self.storage = Some((
                Tensor::cat(&[&old_k, &extra_k], 0)
                    .and_then(|x| x.reshape((blocks, CONTEXT_KV_PAGE_SIZE, heads, head_dim)))
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: expand context K", e))?,
                Tensor::cat(&[&old_v, &extra_v], 0)
                    .and_then(|x| x.reshape((blocks, CONTEXT_KV_PAGE_SIZE, heads, head_dim)))
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: expand context V", e))?,
            ));
            self.block_table = Some(
                Tensor::new((0..blocks as u32).collect::<Vec<_>>(), template_k.device())
                    .and_then(|x| x.reshape((1, blocks)))
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: grow KV block table", e))?,
            );
            self.capacity = new_capacity;
        }
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn initialize_storage(&mut self, template: &Tensor) -> Result<(), Error> {
        self.ensure_capacity(template, template, self.capacity)
    }

    #[cfg(feature = "cuda")]
    fn storage(&self) -> Option<(&Tensor, &Tensor, &Tensor)> {
        self.storage
            .as_ref()
            .zip(self.block_table.as_ref())
            .map(|((k, v), table)| (k, v, table))
    }

    fn append(&mut self, k: &Tensor, v: &Tensor) -> Result<(), Error> {
        let added = k
            .dim(2)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: context K length", e))?;
        let required = self.len + added;
        self.ensure_capacity(k, v, required)?;
        // `ensure_capacity` may have discarded incompatible storage and reset
        // the logical cache, so do not retain the pre-reset length.
        let required = self.len + added;
        let (storage_k, storage_v) = self.storage.as_mut().expect("context storage initialized");
        let (_, heads, _, head_dim) = k
            .dims4()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: append context shape", e))?;
        let k = k
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: page context K", e))?;
        let v = v
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: page context V", e))?;
        let flat_k = storage_k
            .reshape((self.capacity, heads, head_dim))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: flatten storage K", e))?;
        let flat_v = storage_v
            .reshape((self.capacity, heads, head_dim))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: flatten storage V", e))?;
        flat_k
            .slice_set(&k, 0, self.len)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: append context K", e))?;
        flat_v
            .slice_set(&v, 0, self.len)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: append context V", e))?;
        self.len = required;
        self.k = Some(
            flat_k
                .narrow(0, 0, self.len)
                .and_then(|x| x.transpose(0, 1))
                .and_then(|x| x.unsqueeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: view context K", e))?,
        );
        self.v = Some(
            flat_v
                .narrow(0, 0, self.len)
                .and_then(|x| x.transpose(0, 1))
                .and_then(|x| x.unsqueeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: view context V", e))?,
        );
        Ok(())
    }

    /// Place the transient bonus+mask K/V immediately after the accepted
    /// context. The logical context length is unchanged, so the next accepted
    /// append overwrites this tail instead of copying the whole history.
    fn with_queries(&mut self, k: &Tensor, v: &Tensor) -> Result<(Tensor, Tensor), Error> {
        if self.len == 0 {
            return Err(Error::InvalidInput {
                message: "HunyuanOCR DFlash: context cache is empty".to_string(),
            });
        }
        let query_len = k
            .dim(2)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: query K length", e))?;
        let required = self.len + query_len;
        self.ensure_capacity(k, v, required)?;
        let (storage_k, storage_v) = self.storage.as_mut().expect("context storage initialized");
        let (_, heads, _, head_dim) = k
            .dims4()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: query context shape", e))?;
        let k = k
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: page query K", e))?;
        let v = v
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: page query V", e))?;
        let flat_k = storage_k
            .reshape((self.capacity, heads, head_dim))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: flatten query storage K", e))?;
        let flat_v = storage_v
            .reshape((self.capacity, heads, head_dim))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: flatten query storage V", e))?;
        flat_k
            .slice_set(&k, 0, self.len)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: write query K", e))?;
        flat_v
            .slice_set(&v, 0, self.len)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: write query V", e))?;
        Ok((
            flat_k
                .narrow(0, 0, required)
                .and_then(|x| x.transpose(0, 1))
                .and_then(|x| x.unsqueeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: view query K", e))?,
            flat_v
                .narrow(0, 0, required)
                .and_then(|x| x.transpose(0, 1))
                .and_then(|x| x.unsqueeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: view query V", e))?,
        ))
    }
}

fn apply_rope(x: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
    let a = x
        .broadcast_mul(cos)
        .map_err(|e| tensor_err("HunyuanOCR DFlash: rope x*cos", e))?;
    let b = rotate_half(x)?
        .broadcast_mul(sin)
        .map_err(|e| tensor_err("HunyuanOCR DFlash: rope rotate_half*sin", e))?;
    (a + b).map_err(|e| tensor_err("HunyuanOCR DFlash: rope sum", e))
}

#[derive(Debug)]
struct DFlashAttention {
    qkv_proj: Linear,
    o_proj: Linear,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    scale: f64,
}

impl DFlashAttention {
    fn load(cfg: &DFlashConfig, vb: VarBuilder) -> Result<Self, Error> {
        let q_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_attention_heads * cfg.head_dim,
            vb.pp("q_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load q_proj", e))?;
        let k_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("k_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load k_proj", e))?;
        let v_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("v_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load v_proj", e))?;
        let o_proj = linear_no_bias(
            cfg.num_attention_heads * cfg.head_dim,
            cfg.hidden_size,
            vb.pp("o_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load o_proj", e))?;
        let q_norm = rms_norm(cfg.head_dim, cfg.rms_norm_eps, vb.pp("q_norm"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load q_norm", e))?;
        let k_norm = rms_norm(cfg.head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load k_norm", e))?;
        let qkv_weight = Tensor::cat(&[q_proj.weight(), k_proj.weight(), v_proj.weight()], 0)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: fuse QKV weights", e))?;
        Ok(Self {
            qkv_proj: Linear::new(qkv_weight, None),
            o_proj,
            q_norm,
            k_norm,
            num_heads: cfg.num_attention_heads,
            num_kv_heads: cfg.num_key_value_heads,
            num_kv_groups: cfg.num_attention_heads / cfg.num_key_value_heads,
            head_dim: cfg.head_dim,
            scale: (cfg.head_dim as f64).powf(-0.5),
        })
    }

    fn shape_projected(
        &self,
        projected: &Tensor,
        heads: usize,
        norm: Option<&RmsNorm>,
    ) -> Result<Tensor, Error> {
        let (batch, seq_len, _) = projected
            .dims3()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: projected dims", e))?;
        let projected = projected
            .reshape((batch, seq_len, heads, self.head_dim))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: projection reshape", e))?;
        let projected = match norm {
            Some(norm) => norm
                .forward(&projected)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "Q/K norm", e))?,
            None => projected,
        };
        projected
            .transpose(1, 2)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: projection transpose", e))
    }

    fn qkv_weights(&self) -> Result<(Tensor, Tensor), Error> {
        let q_width = self.num_heads * self.head_dim;
        let kv_width = self.num_kv_heads * self.head_dim;
        let k = self
            .qkv_proj
            .weight()
            .narrow(0, q_width, kv_width)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: K weight view", e))?;
        let v = self
            .qkv_proj
            .weight()
            .narrow(0, q_width + kv_width, kv_width)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: V weight view", e))?;
        Ok((k, v))
    }

    fn append_projected_context(
        &self,
        raw_k: &Tensor,
        raw_v: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cache: &mut ContextKv,
    ) -> Result<(), Error> {
        let k = self.shape_projected(raw_k, self.num_kv_heads, Some(&self.k_norm))?;
        let k = apply_rope(&k, cos, sin)?;
        let v = self.shape_projected(raw_v, self.num_kv_heads, None)?;
        cache.append(&k, &v)
    }

    fn project_qkv(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
    ) -> Result<(Tensor, Tensor, Tensor), Error> {
        #[cfg(not(feature = "cuda"))]
        let _ = cos_sin;
        let q_width = self.num_heads * self.head_dim;
        let kv_width = self.num_kv_heads * self.head_dim;
        let qkv = self
            .qkv_proj
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "QKV projection", e))?;
        #[cfg(feature = "cuda")]
        if qkv.device().is_cuda()
            && qkv.dtype() == DType::BF16
            && self.head_dim == 128
            && cos_sin.is_some()
        {
            let cos_sin = cos_sin.expect("checked above");
            let q = qkv
                .apply_op3_no_bwd(
                    cos_sin,
                    self.q_norm.weight(),
                    &FusedRmsNormRopeBf16 {
                        projection_width: q_width + 2 * kv_width,
                        projection_offset: 0,
                        num_heads: self.num_heads,
                        query_len: hidden.dim(1).map_err(|e| {
                            tensor_err("HunyuanOCR DFlash: fused Q query length", e)
                        })?,
                        head_dim: self.head_dim,
                        eps: self.q_norm.eps() as f32,
                        include_v: false,
                    },
                )
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused Q RMSNorm RoPE", e))?;
            let kv = qkv
                .apply_op3_no_bwd(
                    cos_sin,
                    self.k_norm.weight(),
                    &FusedRmsNormRopeBf16 {
                        projection_width: q_width + 2 * kv_width,
                        projection_offset: q_width,
                        num_heads: self.num_kv_heads,
                        query_len: hidden.dim(1).map_err(|e| {
                            tensor_err("HunyuanOCR DFlash: fused KV query length", e)
                        })?,
                        head_dim: self.head_dim,
                        eps: self.k_norm.eps() as f32,
                        include_v: true,
                    },
                )
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused KV RMSNorm RoPE", e))?;
            let k = kv
                .narrow(0, 0, 1)
                .and_then(|x| x.squeeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused K view", e))?;
            let v = kv
                .narrow(0, 1, 1)
                .and_then(|x| x.squeeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused V view", e))?;
            return Ok((q, k, v));
        }
        let q_raw = qkv
            .narrow(2, 0, q_width)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: Q projection slice", e))?;
        let k_raw = qkv
            .narrow(2, q_width, kv_width)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: K projection slice", e))?;
        let v_raw = qkv
            .narrow(2, q_width + kv_width, kv_width)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: V projection slice", e))?;
        let q = self.shape_projected(&q_raw, self.num_heads, Some(&self.q_norm))?;
        let q = self.apply_query_rope(&q, cos, sin, cos_sin)?;
        let query_k = self.shape_projected(&k_raw, self.num_kv_heads, Some(&self.k_norm))?;
        let query_k = self.apply_query_rope(&query_k, cos, sin, cos_sin)?;
        let query_v = self.shape_projected(&v_raw, self.num_kv_heads, None)?;

        Ok((q, query_k, query_v))
    }

    fn apply_query_rope(
        &self,
        input: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        #[cfg(not(feature = "cuda"))]
        let _ = cos_sin;
        #[cfg(feature = "cuda")]
        if input.device().is_cuda()
            && input.dtype() == DType::BF16
            && cos_sin.is_some()
            && self.head_dim.is_multiple_of(2)
        {
            let output = Tensor::zeros(input.shape(), input.dtype(), input.device())
                .map_err(|e| tensor_err("HunyuanOCR DFlash: allocate fused RoPE output", e))?;
            output
                .inplace_op3(input, cos_sin.expect("checked above"), &FusedRopeBf16)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused BF16 RoPE", e))?;
            return Ok(output);
        }
        apply_rope(input, cos, sin)
    }

    fn attend_projected(
        &self,
        q: &Tensor,
        query_k: &Tensor,
        query_v: &Tensor,
        cache: &mut ContextKv,
    ) -> Result<Tensor, Error> {
        let (k, v) = cache.with_queries(query_k, query_v)?;
        // Compute GQA without physically repeating the full context K/V cache
        // across all query heads.
        let output = match flash_attention(q, &k, &v, self.scale, false)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "flash attention", e))?
        {
            Some(output) => output,
            None => scaled_dot_product_attention_gqa(
                q,
                &k,
                &v,
                None,
                self.scale,
                false,
                self.num_kv_groups,
            )
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "grouped attention", e))?,
        };
        Ok(output)
    }

    #[cfg(feature = "cuda")]
    fn attend_projected_dynamic(
        &self,
        q: &Tensor,
        query_k: &Tensor,
        query_v: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
        cache: &ContextKv,
    ) -> Result<Tensor, Error> {
        let (_, _, query_len, _) = q
            .dims4()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: dynamic Q dimensions", e))?;
        let (cache_k, cache_v, block_table) = cache.storage().ok_or_else(|| Error::Config {
            message: "HunyuanOCR DFlash: dynamic context storage is not initialized".to_string(),
        })?;
        let append = DynamicPagedKvAppend {
            query_len,
            cache_len: CONTEXT_KV_INITIAL_CAPACITY,
        };
        cache_k
            .inplace_op3(query_k, kv_lengths, &append)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: dynamic query K append", e))?;
        cache_v
            .inplace_op3(query_v, kv_lengths, &append)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: dynamic query V append", e))?;

        let q = q
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: dynamic Q layout", e))?;
        candle_flash_attn::flash_attn_varlen_paged_windowed(
            &q,
            cache_k,
            cache_v,
            query_lengths,
            kv_lengths,
            block_table,
            None,
            query_len,
            CONTEXT_KV_INITIAL_CAPACITY,
            self.scale as f32,
            None,
            None,
            CONTEXT_KV_PAGE_SIZE,
            None,
        )
        .and_then(|x| x.transpose(0, 1))
        .and_then(|x| x.unsqueeze(0))
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "dynamic flash attention", e))
    }

    fn flatten_attention_output(&self, output: &Tensor) -> Result<Tensor, Error> {
        let (batch, _, query_len, _) = output
            .dims4()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: attention output dimensions", e))?;
        output
            .transpose(1, 2)
            .and_then(|x| x.reshape((batch, query_len, self.num_heads * self.head_dim)))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: attention output reshape", e))
    }

    fn project_attention_output(&self, output: &Tensor) -> Result<Tensor, Error> {
        self.o_proj
            .forward(output)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "o_proj", e))
    }
}

#[derive(Debug)]
struct DFlashMlp {
    gate_up_proj: Linear,
    down_proj: Linear,
    intermediate_size: usize,
}

impl DFlashMlp {
    fn load(cfg: &DFlashConfig, vb: VarBuilder) -> Result<Self, Error> {
        let gate_proj = linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("gate_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load gate_proj", e))?;
        let up_proj = linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("up_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load up_proj", e))?;
        let down_proj = linear_no_bias(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load down_proj", e))?;
        let gate_up_weight = Tensor::cat(&[gate_proj.weight(), up_proj.weight()], 0)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: fuse gate/up weights", e))?;
        Ok(Self {
            gate_up_proj: Linear::new(gate_up_weight, None),
            down_proj,
            intermediate_size: cfg.intermediate_size,
        })
    }

    fn forward(&self, input: &Tensor) -> Result<Tensor, Error> {
        let gate_up = self
            .gate_up_proj
            .forward(input)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "MLP gate/up", e))?;
        let gate = gate_up
            .narrow(2, 0, self.intermediate_size)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "MLP gate", e))?;
        let up = gate_up
            .narrow(2, self.intermediate_size, self.intermediate_size)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "MLP up", e))?;
        #[cfg(feature = "cuda")]
        let hidden = if gate.device().is_cuda() && gate.dtype() == DType::BF16 {
            gate.apply_op2_no_bwd(&up, &FusedSiluMulBf16)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused MLP SiLU*up", e))?
        } else {
            let gate = candle_nn::ops::silu(&gate)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "MLP SiLU", e))?;
            (gate * up).map_err(|e| tensor_err("HunyuanOCR DFlash: MLP multiply", e))?
        };
        #[cfg(not(feature = "cuda"))]
        let hidden = {
            let gate = candle_nn::ops::silu(&gate)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "MLP SiLU", e))?;
            (gate * up).map_err(|e| tensor_err("HunyuanOCR DFlash: MLP multiply", e))?
        };
        self.down_proj
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "MLP down", e))
    }
}

#[derive(Debug)]
struct DFlashLayer {
    input_layernorm: RmsNorm,
    self_attn: DFlashAttention,
    post_attention_layernorm: RmsNorm,
    mlp: DFlashMlp,
}

impl DFlashLayer {
    fn load(cfg: &DFlashConfig, vb: VarBuilder) -> Result<Self, Error> {
        let input_layernorm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load input_layernorm", e))?;
        let post_attention_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_attention_layernorm"),
        )
        .map_err(|e| {
            candle_to_ocr_inference("HunyuanOCR DFlash", "load post_attention_layernorm", e)
        })?;
        Ok(Self {
            input_layernorm,
            self_attn: DFlashAttention::load(cfg, vb.pp("self_attn"))?,
            post_attention_layernorm,
            mlp: DFlashMlp::load(cfg, vb.pp("mlp"))?,
        })
    }

    fn forward(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
        cache: &mut ContextKv,
    ) -> Result<Tensor, Error> {
        let (q, k, v) = self.project_qkv_eager(hidden, cos, sin, cos_sin)?;
        let attention = self.self_attn.attend_projected(&q, &k, &v, cache)?;
        let attention = self.self_attn.flatten_attention_output(&attention)?;
        self.post_attention_eager(hidden, &attention)
    }

    fn project_qkv_eager(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
    ) -> Result<(Tensor, Tensor, Tensor), Error> {
        let normed = self
            .input_layernorm
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "input norm", e))?;
        self.self_attn.project_qkv(&normed, cos, sin, cos_sin)
    }

    fn post_attention_eager(&self, hidden: &Tensor, attention: &Tensor) -> Result<Tensor, Error> {
        let attention = self.self_attn.project_attention_output(attention)?;
        #[cfg(feature = "cuda")]
        if hidden.device().is_cuda()
            && hidden.dtype() == DType::BF16
            && hidden.is_contiguous()
            && attention.is_contiguous()
        {
            let packed = hidden
                .apply_op3_no_bwd(
                    &attention,
                    self.post_attention_layernorm.weight(),
                    &FusedAddRmsNormBf16 {
                        eps: self.post_attention_layernorm.eps() as f32,
                    },
                )
                .map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR DFlash", "fused residual RMSNorm", e)
                })?;
            let residual = packed
                .narrow(0, 0, 1)
                .and_then(|x| x.squeeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused residual view", e))?;
            let normed = packed
                .narrow(0, 1, 1)
                .and_then(|x| x.squeeze(0))
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused RMSNorm view", e))?;
            let mlp = self.mlp.forward(&normed)?;
            return (residual + mlp).map_err(|e| tensor_err("HunyuanOCR DFlash: MLP residual", e));
        }
        let hidden = (hidden + attention)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: attention residual", e))?;
        let normed = self
            .post_attention_layernorm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "post-attention norm", e))?;
        let mlp = self.mlp.forward(&normed)?;
        (hidden + mlp).map_err(|e| tensor_err("HunyuanOCR DFlash: MLP residual", e))
    }

    #[cfg(feature = "cuda")]
    fn forward_dynamic(
        &self,
        hidden: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
        cache: &ContextKv,
    ) -> Result<Tensor, Error> {
        let (q, k, v) = self.project_qkv_eager(hidden, cos, sin, cos_sin)?;
        let attention = self.self_attn.attend_projected_dynamic(
            &q,
            &k,
            &v,
            query_lengths,
            kv_lengths,
            cache,
        )?;
        let attention = self.self_attn.flatten_attention_output(&attention)?;
        self.post_attention_eager(hidden, &attention)
    }
}

#[cfg(feature = "cuda")]
struct DFlashCudaGraph {
    // Dispose via `dispose` so capture-touched buffers are never returned to
    // the stream-ordered allocator (see SingleTokenDecoderCudaGraph::dispose).
    graph: candle_core::cuda_backend::cudarc::driver::CudaGraph,
    query_input: Tensor,
    cos_input: Tensor,
    sin_input: Tensor,
    _query_lengths: Tensor,
    kv_lengths: CudaGraphKvLengths,
    hidden_output: Tensor,
    proposals_output: Tensor,
}

#[cfg(feature = "cuda")]
impl DFlashCudaGraph {
    fn dispose(self) {
        let Self {
            graph,
            query_input,
            cos_input,
            sin_input,
            _query_lengths,
            kv_lengths,
            hidden_output,
            proposals_output,
        } = self;
        let device = query_input.device().clone();
        report_stashed_cuda_error(&device, "CUDA graph disposal");
        drop_and_drain(graph, &device);
        drop_and_drain(proposals_output, &device);
        drop_and_drain(hidden_output, &device);
        drop_and_drain(kv_lengths, &device);
        drop_and_drain(_query_lengths, &device);
        drop_and_drain(sin_input, &device);
        drop_and_drain(cos_input, &device);
        drop_and_drain(query_input, &device);
    }
}

#[cfg(feature = "cuda")]
impl std::fmt::Debug for DFlashCudaGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DFlashCudaGraph")
            .field("query_input", &self.query_input.shape())
            .field("hidden_output", &self.hidden_output.shape())
            .field("proposals_output", &self.proposals_output.shape())
            .finish_non_exhaustive()
    }
}

/// Loaded DFlash draft and its incremental target-context K/V caches.
pub(crate) struct DFlashModel {
    #[cfg(feature = "cuda")]
    decode_graph: RefCell<Option<DFlashCudaGraph>>,
    cfg: DFlashConfig,
    fc: Linear,
    hidden_norm: RmsNorm,
    layers: Vec<DFlashLayer>,
    norm: RmsNorm,
    rotary: RotaryEmbedding,
    rope_cos: Tensor,
    rope_sin: Tensor,
    context_kv_proj: Linear,
    lm_head_weight_t: Tensor,
    caches: RefCell<Vec<ContextKv>>,
    dtype: DType,
    device: Device,
    // Must stay the last field: it drops last and drains CUDA errors the
    // other fields' frees may stash (see CudaGraphDrainGuard).
    #[cfg(feature = "cuda")]
    _drain_guard: CudaGraphDrainGuard,
}

impl DFlashModel {
    pub(crate) fn from_dir(
        model_dir: impl AsRef<Path>,
        dtype: DType,
        device: &Device,
        lm_head_weight: &Tensor,
    ) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = DFlashConfig::from_path(model_dir.join("config.json"))?;
        cfg.validate()?;
        let (target_vocab_size, target_hidden_size) = lm_head_weight
            .dims2()
            .map_err(|e| tensor_err("HunyuanOCR DFlash: target LM head shape", e))?;
        if target_vocab_size != cfg.vocab_size || target_hidden_size != cfg.hidden_size {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR DFlash: target LM head shape {target_vocab_size}x{target_hidden_size} does not match draft vocab/hidden {}x{}",
                    cfg.vocab_size, cfg.hidden_size
                ),
            });
        }
        let lm_head_weight_t = lm_head_weight
            .transpose(0, 1)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: transpose target LM head", e))?;
        let files = crate::utils::collect_safetensors(model_dir, "HunyuanOCR DFlash")?;
        // SAFETY: the checkpoint files remain mapped for the lifetime of the tensors.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&files, dtype, device)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load safetensors", e))?
        };
        let fc = linear_no_bias(
            cfg.hidden_size * cfg.dflash_config.target_layer_ids.len(),
            cfg.hidden_size,
            vb.pp("fc"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load fc", e))?;
        let hidden_norm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("hidden_norm"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load hidden_norm", e))?;
        let norm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "load norm", e))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for index in 0..cfg.num_hidden_layers {
            layers.push(DFlashLayer::load(&cfg, vb.pp(format!("layers.{index}")))?);
        }
        let mut context_weights = Vec::with_capacity(cfg.num_hidden_layers * 2);
        for layer in &layers {
            let (k, v) = layer.self_attn.qkv_weights()?;
            context_weights.push(k);
            context_weights.push(v);
        }
        let context_weight_refs: Vec<&Tensor> = context_weights.iter().collect();
        let context_kv_weight = Tensor::cat(&context_weight_refs, 0)
            .and_then(|x| x.contiguous())
            .map_err(|e| tensor_err("HunyuanOCR DFlash: fuse context K/V weights", e))?;
        let context_kv_proj = Linear::new(context_kv_weight, None);
        let rotary = RotaryEmbedding::new_dynamic(cfg.head_dim, cfg.rope_theta, device)?;
        let rope_positions = Tensor::arange(0i64, ROPE_CACHE_LEN as i64, device)
            .and_then(|x| x.reshape((1, 1, ROPE_CACHE_LEN)))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: rope cache positions", e))?;
        let (rope_cos, rope_sin) = rotary.forward_multi_axis(&rope_positions, dtype)?;
        let caches = (0..cfg.num_hidden_layers)
            .map(|_| ContextKv::default())
            .collect();
        let model = Self {
            #[cfg(feature = "cuda")]
            decode_graph: RefCell::new(None),
            cfg,
            fc,
            hidden_norm,
            layers,
            norm,
            rotary,
            rope_cos,
            rope_sin,
            context_kv_proj,
            lm_head_weight_t,
            caches: RefCell::new(caches),
            dtype,
            device: device.clone(),
            #[cfg(feature = "cuda")]
            _drain_guard: CudaGraphDrainGuard::new(device),
        };
        model.prepare_cuda_graph()?;
        Ok(model)
    }

    pub(crate) fn config(&self) -> &DFlashConfig {
        &self.cfg
    }

    pub(crate) fn prepare_cuda_graph(&self) -> Result<(), Error> {
        #[cfg(feature = "cuda")]
        {
            if std::env::var_os("OAR_HUNYUAN_DISABLE_CUDA_GRAPH").is_some() {
                self.invalidate_cuda_graph();
                return Ok(());
            }
            // `DynamicPagedKvAppend` accepts BF16 only. Other dtype
            // overrides retain the eager path instead of failing model load.
            if self.device.is_cuda()
                && self.dtype == DType::BF16
                && self.decode_graph.borrow().is_none()
            {
                let mut caches = self.caches.borrow_mut();
                if caches.iter().any(|cache| cache.len != 0) {
                    return Ok(());
                }
                // A long-context eager fallback may have grown the physical
                // cache beyond the graph's fixed 16K stride. At the next page
                // boundary it is safe to shrink and capture fresh pointers.
                for cache in caches.iter_mut() {
                    if cache.capacity != CONTEXT_KV_INITIAL_CAPACITY {
                        cache.reset_graph_storage();
                    }
                }
                drop(caches);
                self.capture_cuda_graph()?;
            }
        }
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn invalidate_cuda_graph(&self) {
        // A captured graph owns raw pointers into the fixed-size cache. Once
        // that cache must grow, the graph cannot safely be reused, including
        // after a later page-level reset.
        if let Some(graph) = self.decode_graph.borrow_mut().take() {
            graph.dispose();
        }
    }

    fn rope(&self, start: usize, len: usize) -> Result<(Tensor, Tensor), Error> {
        if start + len <= ROPE_CACHE_LEN {
            let cos = self
                .rope_cos
                .narrow(2, start, len)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: rope cos slice", e))?;
            let sin = self
                .rope_sin
                .narrow(2, start, len)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: rope sin slice", e))?;
            return Ok((cos, sin));
        }
        let positions: Vec<i64> = (start..start + len).map(|x| x as i64).collect();
        let positions = Tensor::from_vec(positions, (1, 1, len), &self.device)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: position tensor", e))?;
        self.rotary.forward_multi_axis(&positions, self.dtype)
    }

    fn transform_target(&self, aux_hidden: &Tensor) -> Result<Tensor, Error> {
        let hidden = self
            .fc
            .forward(aux_hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "target fc", e))?;
        self.hidden_norm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "target hidden norm", e))
    }

    fn append_projected_context(
        &self,
        target: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        caches: &mut [ContextKv],
    ) -> Result<(), Error> {
        let projected = self
            .context_kv_proj
            .forward(target)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "context K/V", e))?;
        let kv_width = self.cfg.num_key_value_heads * self.cfg.head_dim;
        for (index, (layer, cache)) in self.layers.iter().zip(caches.iter_mut()).enumerate() {
            let offset = index * 2 * kv_width;
            let raw_k = projected
                .narrow(2, offset, kv_width)
                .and_then(|x| x.contiguous())
                .map_err(|e| tensor_err("HunyuanOCR DFlash: context K slice", e))?;
            let raw_v = projected
                .narrow(2, offset + kv_width, kv_width)
                .and_then(|x| x.contiguous())
                .map_err(|e| tensor_err("HunyuanOCR DFlash: context V slice", e))?;
            layer
                .self_attn
                .append_projected_context(&raw_k, &raw_v, cos, sin, cache)?;
        }
        Ok(())
    }

    pub(crate) fn reset_context(&self, aux_hidden: &Tensor) -> Result<(), Error> {
        #[cfg(feature = "cuda")]
        let _cuda_htod_cache = match &self.device {
            Device::Cuda(device) => Some(device.enable_cuda_graph_htod_cache()),
            _ => None,
        };
        let len = aux_hidden
            .dim(1)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: target context length", e))?;
        #[cfg(feature = "cuda")]
        if len > CONTEXT_KV_INITIAL_CAPACITY {
            self.invalidate_cuda_graph();
        }
        let target = self.transform_target(aux_hidden)?;
        let (cos, sin) = self.rope(0, len)?;
        let mut caches = self.caches.borrow_mut();
        for cache in caches.iter_mut() {
            cache.reset();
        }
        self.append_projected_context(&target, &cos, &sin, &mut caches)
    }

    pub(crate) fn append_context(&self, aux_hidden: &Tensor) -> Result<(), Error> {
        #[cfg(feature = "cuda")]
        let _cuda_htod_cache = match &self.device {
            Device::Cuda(device) => Some(device.enable_cuda_graph_htod_cache()),
            _ => None,
        };
        let added = aux_hidden
            .dim(1)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: appended context length", e))?;
        if added == 0 {
            return Ok(());
        }
        let start = self.context_len();
        #[cfg(feature = "cuda")]
        if start.saturating_add(added) > CONTEXT_KV_INITIAL_CAPACITY {
            self.invalidate_cuda_graph();
        }
        let target = self.transform_target(aux_hidden)?;
        let (cos, sin) = self.rope(start, added)?;
        let mut caches = self.caches.borrow_mut();
        self.append_projected_context(&target, &cos, &sin, &mut caches)
    }

    pub(crate) fn context_len(&self) -> usize {
        self.caches.borrow().first().map_or(0, |cache| cache.len)
    }

    #[cfg(feature = "cuda")]
    fn forward_queries_dynamic(
        &self,
        query_embeds: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
    ) -> Result<Tensor, Error> {
        let caches = self.caches.borrow();
        let cos_sin = if query_embeds.dtype() == DType::BF16 {
            Some(
                Tensor::cat(&[cos, sin], 0)
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: pack dynamic RoPE cos/sin", e))?,
            )
        } else {
            None
        };
        let mut hidden = query_embeds.clone();
        for (layer, cache) in self.layers.iter().zip(caches.iter()) {
            hidden = layer.forward_dynamic(
                &hidden,
                cos,
                sin,
                cos_sin.as_ref(),
                query_lengths,
                kv_lengths,
                cache,
            )?;
        }
        self.norm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "dynamic final norm", e))
    }

    fn proposals_from_hidden(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let num_spec = self.cfg.block_size.saturating_sub(1);
        let draft_rows = hidden
            .narrow(1, 1, num_spec)
            .and_then(|tensor| tensor.squeeze(0))
            .map_err(|e| tensor_err("HunyuanOCR DFlash: draft mask rows", e))?;
        let logits = draft_rows
            .matmul(&self.lm_head_weight_t)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: graph draft logits", e))?;
        #[cfg(feature = "cuda")]
        if logits.device().is_cuda() && logits.dtype() == DType::BF16 {
            return logits
                .apply_op1_no_bwd(&ArgmaxFirstBf16)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: fused draft argmax", e));
        }
        logits
            .argmax(D::Minus1)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: draft argmax", e))
    }

    #[cfg(feature = "cuda")]
    fn capture_cuda_graph(&self) -> Result<(), Error> {
        use candle_core::cuda_backend::cudarc::driver::sys::{
            CUgraphInstantiate_flags_enum, CUstreamCaptureMode_enum,
        };

        if self.decode_graph.borrow().is_some() {
            return Ok(());
        }
        let Device::Cuda(cuda) = &self.device else {
            return Ok(());
        };
        let query_len = self.cfg.block_size;
        let template = Tensor::zeros(
            (
                1,
                self.cfg.num_key_value_heads,
                query_len,
                self.cfg.head_dim,
            ),
            self.dtype,
            &self.device,
        )
        .map_err(|e| tensor_err("HunyuanOCR DFlash: dynamic cache template", e))?;
        for cache in self.caches.borrow_mut().iter_mut() {
            cache.initialize_storage(&template)?;
        }

        let query_input = Tensor::zeros(
            (1, query_len, self.cfg.hidden_size),
            self.dtype,
            &self.device,
        )
        .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph query input", e))?;
        let cos_input = Tensor::zeros(
            (1, 1, query_len, self.cfg.head_dim),
            self.dtype,
            &self.device,
        )
        .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph cos input", e))?;
        let sin_input = Tensor::zeros(
            (1, 1, query_len, self.cfg.head_dim),
            self.dtype,
            &self.device,
        )
        .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph sin input", e))?;
        let query_lengths = Tensor::new(&[0u32, query_len as u32], &self.device)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph query lengths", e))?;
        let kv_lengths = CudaGraphKvLengths::new(query_len, &self.device)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph KV lengths", e))?;
        let stream = cuda.cuda_stream();
        let _htod_cache = cuda.enable_cuda_graph_htod_cache();

        let warm = self.forward_queries_dynamic(
            &query_input,
            &cos_input,
            &sin_input,
            &query_lengths,
            kv_lengths.tensor(),
        )?;
        let warm_proposals = self.proposals_from_hidden(&warm)?;
        sync_graph_tensor(
            "HunyuanOCR DFlash",
            &warm_proposals,
            "warm full draft graph",
        )?;
        // Allocate the output buffers before capture so they belong to the
        // regular stream-ordered pool; a capture-time allocation lives in the
        // graph's private pool and can never be returned to the allocator
        // safely. Prime the copies so the captured run sees warm kernels.
        let hidden_output = Tensor::zeros_like(&warm)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph hidden output", e))?;
        let proposals_output = Tensor::zeros_like(&warm_proposals)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: full graph proposals output", e))?;
        hidden_output
            .slice_set(&warm, 0, 0)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: prime full hidden copy", e))?;
        proposals_output
            .slice_set(&warm_proposals, 0, 0)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: prime full proposals copy", e))?;

        stream
            .begin_capture(CUstreamCaptureMode_enum::CU_STREAM_CAPTURE_MODE_GLOBAL)
            .map_err(|e| {
                cuda_graph_error("HunyuanOCR DFlash", "begin full draft graph capture", e)
            })?;
        let captured_output: Result<(), Error> = (|| {
            let hidden = self.forward_queries_dynamic(
                &query_input,
                &cos_input,
                &sin_input,
                &query_lengths,
                kv_lengths.tensor(),
            )?;
            let proposals = self.proposals_from_hidden(&hidden)?;
            hidden_output
                .slice_set(&hidden, 0, 0)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: record full hidden copy", e))?;
            proposals_output
                .slice_set(&proposals, 0, 0)
                .map_err(|e| tensor_err("HunyuanOCR DFlash: record full proposals copy", e))
        })();
        if let Err(error) = captured_output {
            let _ = stream.end_capture(
                CUgraphInstantiate_flags_enum::CUDA_GRAPH_INSTANTIATE_FLAG_AUTO_FREE_ON_LAUNCH,
            );
            return Err(error);
        }
        let graph = stream
            .end_capture(
                CUgraphInstantiate_flags_enum::CUDA_GRAPH_INSTANTIATE_FLAG_AUTO_FREE_ON_LAUNCH,
            )
            .map_err(|e| cuda_graph_error("HunyuanOCR DFlash", "end full draft graph capture", e))?
            .ok_or_else(|| Error::Config {
                message: "HunyuanOCR DFlash full graph capture returned no graph".to_string(),
            })?;
        graph
            .launch()
            .map_err(|e| cuda_graph_error("HunyuanOCR DFlash", "warm full draft graph", e))?;
        sync_graph_tensor(
            "HunyuanOCR DFlash",
            &proposals_output,
            "sync full draft graph",
        )?;
        self.clear_context();
        *self.decode_graph.borrow_mut() = Some(DFlashCudaGraph {
            graph,
            query_input,
            cos_input,
            sin_input,
            _query_lengths: query_lengths,
            kv_lengths,
            hidden_output,
            proposals_output,
        });
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn replay_cuda_graph(
        &self,
        query_embeds: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        total_kv_len: usize,
    ) -> Result<Option<Tensor>, Error> {
        if total_kv_len > CONTEXT_KV_INITIAL_CAPACITY {
            self.invalidate_cuda_graph();
            return Ok(None);
        }
        let captured_ref = self.decode_graph.borrow();
        let Some(captured) = captured_ref.as_ref() else {
            return Ok(None);
        };
        if query_embeds.shape() != captured.query_input.shape()
            || cos.shape() != captured.cos_input.shape()
            || sin.shape() != captured.sin_input.shape()
        {
            return Ok(None);
        }
        captured
            .query_input
            .slice_set(query_embeds, 0, 0)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: copy full graph query", e))?;
        captured
            .cos_input
            .slice_set(cos, 0, 0)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: copy full graph cos", e))?;
        captured
            .sin_input
            .slice_set(sin, 0, 0)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: copy full graph sin", e))?;
        captured
            .kv_lengths
            .update(total_kv_len)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: update full graph KV lengths", e))?;
        captured
            .graph
            .launch()
            .map_err(|e| cuda_graph_error("HunyuanOCR DFlash", "launch full draft graph", e))?;
        Ok(Some(captured.proposals_output.clone()))
    }

    /// Run the bonus+mask query block and project all mask rows into draft
    /// proposals. CUDA graph replay includes both the shared LM head and the
    /// argmax so they do not incur per-round host dispatch.
    pub(crate) fn forward_proposals(&self, query_embeds: &Tensor) -> Result<Tensor, Error> {
        #[cfg(feature = "cuda")]
        let _cuda_htod_cache = match &self.device {
            Device::Cuda(device) => Some(device.enable_cuda_graph_htod_cache()),
            _ => None,
        };
        let query_len = query_embeds
            .dim(1)
            .map_err(|e| tensor_err("HunyuanOCR DFlash: query length", e))?;
        let context_len = self.context_len();
        let (cos, sin) = self.rope(context_len, query_len)?;
        #[cfg(feature = "cuda")]
        if let Some(output) =
            self.replay_cuda_graph(query_embeds, &cos, &sin, context_len + query_len)?
        {
            return Ok(output);
        }
        #[cfg(feature = "cuda")]
        let cos_sin = if query_embeds.device().is_cuda() && query_embeds.dtype() == DType::BF16 {
            Some(
                Tensor::cat(&[&cos, &sin], 0)
                    .map_err(|e| tensor_err("HunyuanOCR DFlash: pack query RoPE cos/sin", e))?,
            )
        } else {
            None
        };
        #[cfg(not(feature = "cuda"))]
        let cos_sin: Option<Tensor> = None;
        let mut caches = self.caches.borrow_mut();
        let mut hidden = query_embeds.clone();
        for (layer, cache) in self.layers.iter().zip(caches.iter_mut()) {
            hidden = layer.forward(&hidden, &cos, &sin, cos_sin.as_ref(), cache)?;
        }
        let hidden = self
            .norm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "final norm", e))?;
        self.proposals_from_hidden(&hidden)
    }

    pub(crate) fn clear_context(&self) {
        for cache in self.caches.borrow_mut().iter_mut() {
            cache.reset();
        }
    }
}

#[cfg(feature = "cuda")]
impl Drop for DFlashModel {
    fn drop(&mut self) {
        // A cached graph must go through dispose: plainly dropping it returns
        // graph-bound buffers to the allocator and poisons it.
        self.invalidate_cuda_graph();
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextKv, DFlashConfig};
    use candle_core::{DType, Device, Tensor};

    #[test]
    fn parses_hunyuan_dflash_config() {
        let cfg: DFlashConfig = serde_json::from_str(
            r#"{
                "block_size": 16,
                "hidden_size": 1024,
                "intermediate_size": 3584,
                "num_attention_heads": 16,
                "num_hidden_layers": 5,
                "num_key_value_heads": 8,
                "head_dim": 128,
                "vocab_size": 120818,
                "rms_norm_eps": 0.00001,
                "rope_theta": 10000.0,
                "dflash_config": {
                    "mask_token_id": 120817,
                    "target_layer_ids": [1, 8, 15, 22]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.block_size, 16);
        assert_eq!(cfg.dflash_config.target_layer_ids, [1, 8, 15, 22]);
        cfg.validate().unwrap();
    }

    #[test]
    fn incompatible_context_storage_starts_a_new_logical_cache() {
        let device = Device::Cpu;
        let mut cache = ContextKv::default();
        let old = Tensor::zeros((1, 1, 3, 4), DType::F32, &device).unwrap();
        cache.append(&old, &old).unwrap();

        // Changing the number of heads forces replacement storage.
        let new = Tensor::ones((1, 2, 2, 4), DType::F32, &device).unwrap();
        cache.append(&new, &new).unwrap();

        assert_eq!(cache.len, 2);
        let k = cache.k.as_ref().unwrap();
        let v = cache.v.as_ref().unwrap();
        assert_eq!(k.dims(), &[1, 2, 2, 4]);
        assert_eq!(v.dims(), &[1, 2, 2, 4]);
        assert!(
            k.flatten_all()
                .unwrap()
                .to_vec1::<f32>()
                .unwrap()
                .iter()
                .all(|&x| x == 1.0)
        );
    }
}
