use super::config::{HunyuanOcrVersion, HunyuanOcrVisionConfig};
use crate::attention::flash_attention;
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{Conv2d, Conv2dConfig, LayerNorm, LayerNormConfig, Linear, Module};
use rayon::prelude::*;
use std::sync::Arc;

/// Late vit layers are the cross-implementation drift hotspot: BF16 Q·K
/// accumulation drift redirects attention "sink" positions enough to swap the
/// dominant attention head by layer 26 (cosine to upstream drops from
/// ~0.999 at layer 11 to ~0.95). Running attention in F32 from this layer
/// onwards is the empirically stable compromise. See `VisionAttention::forward`.
const VIT_LATE_F32_THRESHOLD: usize = 20;

#[derive(Debug, Clone)]
struct VisionEmbeddings {
    patch_embedding: Conv2d,
    patch_position_base: Arc<[f32]>,
    position_grid: usize,
    position_dim: usize,
    version: HunyuanOcrVersion,
}

impl VisionEmbeddings {
    fn load(
        cfg: &HunyuanOcrVisionConfig,
        version: HunyuanOcrVersion,
        vb: candle_nn::VarBuilder,
    ) -> Result<Self, Error> {
        let conv_cfg = Conv2dConfig {
            stride: cfg.patch_size,
            padding: 0,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };

        let patch_embedding = if cfg.add_patchemb_bias {
            candle_nn::conv2d(
                cfg.num_channels,
                cfg.hidden_size,
                cfg.patch_size,
                conv_cfg,
                vb.pp("patch_embedding"),
            )
        } else {
            candle_nn::conv2d_no_bias(
                cfg.num_channels,
                cfg.hidden_size,
                cfg.patch_size,
                conv_cfg,
                vb.pp("patch_embedding"),
            )
        }
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit patch_embedding", e))?;

        // V1.5 raises max_vit_seq_len to 65,536 for 4K inputs, but retains
        // the 2048px learned positional base (16,384 patches + cls slot).
        // The input grid is interpolated from this base, matching upstream.
        if cfg.patch_size == 0 || !cfg.max_image_size.is_multiple_of(cfg.patch_size) {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: max_image_size {} must be divisible by patch_size {}",
                    cfg.max_image_size, cfg.patch_size
                ),
            });
        }
        let position_edge = cfg.max_image_size / cfg.patch_size;
        let num_positions = position_edge
            .checked_mul(position_edge)
            .and_then(|positions| positions.checked_add(cfg.cat_extra_token))
            .ok_or_else(|| Error::Config {
                message: "HunyuanOCR: position embedding size overflow".to_string(),
            })?;
        let position_embedding =
            candle_nn::embedding(num_positions, cfg.hidden_size, vb.pp("position_embedding"))
                .map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "load vit position_embedding", e)
                })?;
        let pos_w = position_embedding.embeddings();
        let (loaded_positions, position_dim) = pos_w.dims2().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: vit position_embedding dims2 failed",
                e,
            )
        })?;
        if loaded_positions < 2 {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: vit position_embedding is too small: {loaded_positions}"
                ),
            });
        }
        let num_patch_positions = loaded_positions - 1;
        let position_grid = (num_patch_positions as f64).sqrt() as usize;
        if position_grid * position_grid != num_patch_positions {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: vit patch position grid is not square: num={num_patch_positions}"
                ),
            });
        }
        // Position weights never change. Copy the BF16->F32 base once while
        // loading instead of transferring the same 75.5 MB GPU tensor back to
        // the CPU for every page before interpolation.
        let patch_position_base: Arc<[f32]> = pos_w
            .i((1..loaded_positions, ..))
            .and_then(|x| x.to_dtype(DType::F32))
            .and_then(|x| x.flatten_all())
            .and_then(|x| x.to_vec1::<f32>())
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: cache vit patch position embedding failed",
                    e,
                )
            })?
            .into();

        Ok(Self {
            patch_embedding,
            patch_position_base,
            position_grid,
            position_dim,
            version,
        })
    }

    fn interpolate_patch_pos(
        &self,
        height: usize,
        width: usize,
        device: &Device,
        dtype: DType,
    ) -> Result<Tensor, Error> {
        if height == 0 || width == 0 {
            return Err(Error::InvalidInput {
                message: format!(
                    "HunyuanOCR: vit interpolate_patch_pos requires height/width > 0, got {height}x{width}"
                ),
            });
        }

        let out = interpolate_bilinear_align_corners_false(
            &self.patch_position_base,
            self.position_grid,
            self.position_grid,
            height,
            width,
            self.position_dim,
            self.version,
        );
        Tensor::from_vec(out, (height * width, self.position_dim), device)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: vit interpolated patch pos tensor failed",
                    e,
                )
            })?
            .to_dtype(dtype)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: vit interpolated patch pos cast failed",
                    e,
                )
            })
    }
}

#[derive(Debug, Clone)]
struct VisionAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scaling: f64,
    /// Precomputed `layer_idx >= VIT_LATE_F32_THRESHOLD` — true when this
    /// layer's attention runs in F32 (see [`VIT_LATE_F32_THRESHOLD`]).
    use_f32: bool,
}

impl VisionAttention {
    fn load(
        cfg: &HunyuanOcrVisionConfig,
        layer_idx: usize,
        vb: candle_nn::VarBuilder,
    ) -> Result<Self, Error> {
        if !cfg.hidden_size.is_multiple_of(cfg.num_attention_heads) {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: vit hidden_size {} not divisible by num_attention_heads {}",
                    cfg.hidden_size, cfg.num_attention_heads
                ),
            });
        }
        let q_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("q_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit q_proj", e))?;
        let k_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("k_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit k_proj", e))?;
        let v_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("v_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit v_proj", e))?;
        let o_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("o_proj"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit o_proj", e))?;

        let head_dim = cfg.hidden_size / cfg.num_attention_heads;
        Ok(Self {
            q_proj,
            k_proj,
            v_proj,
            o_proj,
            num_heads: cfg.num_attention_heads,
            head_dim,
            scaling: (head_dim as f64).powf(-0.5),
            use_f32: layer_idx >= VIT_LATE_F32_THRESHOLD,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let (b, seq_len, _) = hidden_states.dims3().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: vit attn hidden_states dims3 failed",
                e,
            )
        })?;

        let q_proj = self
            .q_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn q_proj", e))?;
        let k_proj = self
            .k_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn k_proj", e))?;
        let v_proj = self
            .v_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn v_proj", e))?;

        let q = q_proj
            .reshape((b, seq_len, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn q reshape", e))?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn q transpose", e))?;

        let k = k_proj
            .reshape((b, seq_len, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn k reshape", e))?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn k transpose", e))?;

        let v = v_proj
            .reshape((b, seq_len, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn v reshape", e))?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn v transpose", e))?;

        let q = q
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn q contiguous", e))?;
        let k = k
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn k contiguous", e))?;
        let v = v
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn v contiguous", e))?;

        // The official CUDA path uses fused full attention for the vision
        // encoder. Besides avoiding the quadratic attention buffer, the
        // kernel accumulates the BF16 dot products stably without the late
        // F32 eager fallback below.
        if let Some(attn_output) = flash_attention(&q, &k, &v, self.scaling, false)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit flash attention", e))?
        {
            let attn_output = attn_output
                .transpose(1, 2)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit flash transpose", e))?
                .reshape((b, seq_len, self.num_heads * self.head_dim))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit flash reshape", e))?;
            return self
                .o_proj
                .forward(&attn_output)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn o_proj", e));
        }

        // Chunked attention over the query dimension. Without chunking the
        // (B, H, N, N) attention matrix at N=4320 (the vit's full patch
        // sequence) needs ~4 GB just for the BF16 buffer, OOM'ing the 4090.
        //
        // Late layers (idx >= VIT_LATE_F32_THRESHOLD) run attention in F32
        // because BF16 Q·K accumulation drift redirects attention to different
        // sink tokens and gets amplified by the final MLPs — see the constant
        // for the cross-implementation cosine numbers.
        const VIT_ATTN_QUERY_CHUNK: usize = 1024;
        let use_f32 = self.use_f32;
        let v_dtype = v.dtype();
        let kt = k
            .transpose(2, 3)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn k t23", e))?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn k t23 contiguous", e))?;
        let (kt_attn, v_attn) = if use_f32 {
            let kt_f32 = kt
                .to_dtype(DType::F32)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn kt to f32", e))?;
            let v_f32 = v
                .to_dtype(DType::F32)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn v to f32", e))?;
            (kt_f32, v_f32)
        } else {
            (kt, v.clone())
        };

        let attend_chunk = |q_in: &Tensor| -> Result<Tensor, Error> {
            let q_use = if use_f32 {
                q_in.to_dtype(DType::F32)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn q to f32", e))?
            } else {
                q_in.clone()
            };
            let attn = q_use
                .matmul(&kt_attn)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn qk matmul", e))?
                .affine(self.scaling, 0.0)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn scaling", e))?;
            let attn = if use_f32 {
                candle_nn::ops::softmax_last_dim(&attn)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn softmax", e))?
            } else {
                let attn_f32 = attn
                    .to_dtype(DType::F32)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn cast f32", e))?;
                let attn_f32 = candle_nn::ops::softmax_last_dim(&attn_f32)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn softmax", e))?;
                attn_f32
                    .to_dtype(v_dtype)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn cast back", e))?
            };
            let out = attn
                .matmul(&v_attn)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn av matmul", e))?;
            if use_f32 {
                out.to_dtype(v_dtype)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn out to bf16", e))
            } else {
                Ok(out)
            }
        };

        let attn_output = if seq_len <= VIT_ATTN_QUERY_CHUNK {
            attend_chunk(&q)?
        } else {
            let mut chunks: Vec<Tensor> =
                Vec::with_capacity(seq_len.div_ceil(VIT_ATTN_QUERY_CHUNK));
            let mut start = 0;
            while start < seq_len {
                let len = (seq_len - start).min(VIT_ATTN_QUERY_CHUNK);
                let q_chunk = q.narrow(2, start, len).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "vit attn chunked q narrow", e)
                })?;
                chunks.push(attend_chunk(&q_chunk)?);
                start += len;
            }
            let chunk_refs: Vec<&Tensor> = chunks.iter().collect();
            Tensor::cat(&chunk_refs, 2)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn chunked cat", e))?
        };

        let attn_output = attn_output
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn out transpose", e))?
            .reshape((b, seq_len, self.num_heads * self.head_dim))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn out reshape", e))?;

        self.o_proj
            .forward(&attn_output)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attn o_proj", e))
    }
}

#[derive(Debug, Clone)]
struct VisionMlp {
    fc1: Linear,
    fc2: Linear,
}

impl VisionMlp {
    fn load(cfg: &HunyuanOcrVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let fc1 = candle_nn::linear(
            cfg.hidden_size,
            cfg.intermediate_size,
            vb.pp("dense_h_to_4h"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit mlp fc1", e))?;
        let fc2 = candle_nn::linear(
            cfg.intermediate_size,
            cfg.hidden_size,
            vb.pp("dense_4h_to_h"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load vit mlp fc2", e))?;
        Ok(Self { fc1, fc2 })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let hidden = self
            .fc1
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit mlp fc1", e))?;
        // Match PyTorch `nn.GELU()` (exact erf formula). candle's `.gelu()`
        // uses the tanh approximation
        // (`0.5 * v * (1 + tanh(sqrt(2/π)*(v + 0.044715*v³)))`), which
        // diverges by up to ~0.001 per element from the erf formula. Across
        // 27 vit MLPs that drift compounds enough to swap which positions
        // become attention sinks in late layers (max-abs delta jumped from
        // 1.4 at layer 1 to 13419 at layer 26).
        let hidden = hidden
            .gelu_erf()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit mlp gelu_erf", e))?;
        self.fc2
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit mlp fc2", e))
    }
}

#[derive(Debug, Clone)]
struct VisionEncoderLayer {
    self_attn: VisionAttention,
    mlp: VisionMlp,
    input_layernorm: LayerNorm,
    post_attention_layernorm: LayerNorm,
}

impl VisionEncoderLayer {
    fn load(
        cfg: &HunyuanOcrVisionConfig,
        layer_idx: usize,
        vb: candle_nn::VarBuilder,
    ) -> Result<Self, Error> {
        let self_attn = VisionAttention::load(cfg, layer_idx, vb.pp("self_attn"))?;
        let mlp = VisionMlp::load(cfg, vb.pp("mlp"))?;

        let ln_cfg = LayerNormConfig {
            eps: cfg.rms_norm_eps,
            remove_mean: true,
            affine: true,
        };
        let input_layernorm =
            candle_nn::layer_norm(cfg.hidden_size, ln_cfg, vb.pp("input_layernorm")).map_err(
                |e| candle_to_ocr_inference("HunyuanOCR", "load vit input_layernorm", e),
            )?;
        let post_attention_layernorm =
            candle_nn::layer_norm(cfg.hidden_size, ln_cfg, vb.pp("post_attention_layernorm"))
                .map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "load vit post_attention_layernorm", e)
                })?;

        Ok(Self {
            self_attn,
            mlp,
            input_layernorm,
            post_attention_layernorm,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let residual = xs;
        let hidden = self
            .input_layernorm
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit input_layernorm forward", e))?;
        let attn_out = self.self_attn.forward(&hidden)?;
        let hidden = (residual + &attn_out)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit attention residual add", e))?;

        let residual = hidden.clone();
        let hidden = self
            .post_attention_layernorm
            .forward(&hidden)
            .map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "vit post_attention_layernorm forward", e)
            })?;
        let mlp_out = self.mlp.forward(&hidden)?;
        (&residual + &mlp_out)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit mlp residual add", e))
    }
}

#[derive(Debug, Clone)]
struct VisionPerceive {
    hidden_size: usize,
    before_rms: candle_nn::RmsNorm,
    proj_0: Conv2d,
    proj_2: Conv2d,
    mlp: Linear,
    after_rms: candle_nn::RmsNorm,
    image_begin: Tensor,
    image_end: Tensor,
    image_newline: Tensor,
}

impl VisionPerceive {
    fn load(cfg: &HunyuanOcrVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let before_rms =
            candle_nn::rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("before_rms")).map_err(
                |e| candle_to_ocr_inference("HunyuanOCR", "load perceive before_rms", e),
            )?;

        let conv_cfg_0 = Conv2dConfig {
            stride: cfg.spatial_merge_size,
            padding: 0,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };
        let proj_0 = candle_nn::conv2d(
            cfg.hidden_size,
            2304usize,
            cfg.spatial_merge_size,
            conv_cfg_0,
            vb.pp("proj.0"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive proj.0", e))?;

        let conv_cfg_2 = Conv2dConfig {
            stride: 1,
            padding: 0,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };
        let proj_2 =
            candle_nn::conv2d(2304usize, 4608usize, 1usize, conv_cfg_2, vb.pp("proj.2"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive proj.2", e))?;

        let mlp = candle_nn::linear(4608usize, 1024usize, vb.pp("mlp"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive mlp", e))?;

        let after_rms = candle_nn::rms_norm(1024usize, cfg.rms_norm_eps, vb.pp("after_rms"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive after_rms", e))?;

        let image_begin = vb
            .get(1024usize, "image_begin")
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive image_begin", e))?;
        let image_end = vb
            .get(1024usize, "image_end")
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive image_end", e))?;
        // `image_sep` exists in the trained weights but is *never* used by
        // upstream's `HunYuanVisionPatchMerger.forward` — see
        // the upstream Transformers Hunyuan-VL implementation, lines 189-206. We
        // skip loading it.
        let image_newline = vb
            .get(4608usize, "image_newline")
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load perceive image_newline", e))?;

        Ok(Self {
            hidden_size: cfg.hidden_size,
            before_rms,
            proj_0,
            proj_2,
            mlp,
            after_rms,
            image_begin,
            image_end,
            image_newline,
        })
    }

    fn forward(&self, patch_tokens: &Tensor, h: usize, w: usize) -> Result<Tensor, Error> {
        let patch_tokens = self
            .before_rms
            .forward(patch_tokens)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive before_rms forward", e))?;

        let d = patch_tokens.dim(D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: perceive patch_tokens dim failed",
                e,
            )
        })?;
        if d != self.hidden_size {
            return Err(Error::InvalidInput {
                message: format!(
                    "HunyuanOCR: unexpected vit hidden dim {d}, expected {}",
                    self.hidden_size
                ),
            });
        }

        let feat_map = patch_tokens
            .reshape((h, w, d))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: perceive reshape hwd failed",
                    e,
                )
            })?
            .permute((2, 0, 1))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: perceive permute to chw failed",
                    e,
                )
            })?
            .unsqueeze(0)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: perceive add batch dim failed",
                    e,
                )
            })?;

        let feat = self
            .proj_0
            .forward(&feat_map)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive proj.0 forward", e))?;
        // Match PyTorch `nn.GELU()` exact erf formula here too — see
        // `VisionMlp::forward` for the rationale.
        let feat = feat
            .gelu_erf()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive proj.0 gelu_erf", e))?;
        let feat = self
            .proj_2
            .forward(&feat)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive proj.2 forward", e))?;

        let (_b, c, h2, w2) = feat.dims4().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: perceive feat dims4 failed",
                e,
            )
        })?;
        if c != 4608 {
            return Err(Error::InvalidInput {
                message: format!("HunyuanOCR: unexpected perceive channel dim {c}, expected 4608"),
            });
        }

        // Append an extra newline token per row (extra column).
        let newline = self
            .image_newline
            .reshape((1usize, 4608usize, 1usize, 1usize))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: reshape image_newline failed",
                    e,
                )
            })?
            .expand((1usize, 4608usize, h2, 1usize))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: expand image_newline column failed",
                    e,
                )
            })?;
        let feat = Tensor::cat(&[&feat, &newline], 3).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: concat newline column failed",
                e,
            )
        })?;

        // Flatten as row-major: (H2, W2+1) tokens, each with 4608 dims.
        let tokens = feat
            .permute((0, 2, 3, 1))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: permute perceive tokens failed",
                    e,
                )
            })?
            .reshape((h2 * (w2 + 1), 4608usize))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: reshape perceive tokens failed",
                    e,
                )
            })?;

        let tokens = self
            .mlp
            .forward(&tokens)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive mlp forward", e))?;

        // Match upstream HF (`HunYuanVisionPatchMerger.forward` in
        // modeling_hunyuan_vl.py:189-206) exactly:
        //   1. mlp(x)
        //   2. cat([image_begin, mlp_out, image_end])
        //   3. after_rms(cat)
        //
        // `after_rms` must run over the full begin+tokens+end sequence: it
        // lifts the image_begin / image_end embeddings from their stored norm
        // (~0.9) to the post-RMSNorm scale (~22). Normalizing before the cat
        // leaves the markers as near-zero vectors and the prefill logits
        // diverge into hallucinated text. The `image_sep` parameter exists in
        // the upstream weights but is never used in the forward path.
        let begin = self.image_begin.reshape((1usize, 1024usize)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: reshape image_begin failed",
                e,
            )
        })?;
        let end = self.image_end.reshape((1usize, 1024usize)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: reshape image_end failed",
                e,
            )
        })?;
        let begin = begin.to_dtype(tokens.dtype()).map_err(|e| {
            candle_to_ocr_inference("HunyuanOCR", "perceive image_begin to dtype", e)
        })?;
        let end = end
            .to_dtype(tokens.dtype())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive image_end to dtype", e))?;
        let cat = Tensor::cat(&[&begin, &tokens, &end], 0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: concat begin/tokens/end failed",
                e,
            )
        })?;
        self.after_rms
            .forward(&cat)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "perceive after_rms forward", e))
    }
}

#[derive(Debug, Clone)]
pub struct HunyuanVisionModel {
    cfg: HunyuanOcrVisionConfig,
    embeddings: VisionEmbeddings,
    layers: Vec<VisionEncoderLayer>,
    perceive: VisionPerceive,
}

impl HunyuanVisionModel {
    pub fn load(
        cfg: &HunyuanOcrVisionConfig,
        version: HunyuanOcrVersion,
        vb: candle_nn::VarBuilder,
    ) -> Result<Self, Error> {
        let embeddings = VisionEmbeddings::load(cfg, version, vb.pp("embeddings"))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            layers.push(VisionEncoderLayer::load(
                cfg,
                i,
                vb.pp(format!("layers.{i}")),
            )?);
        }
        let perceive = VisionPerceive::load(cfg, vb.pp("perceive"))?;
        Ok(Self {
            cfg: cfg.clone(),
            embeddings,
            layers,
            perceive,
        })
    }

    /// Produce image token embeddings suitable for replacing the image-token span in the LLM input.
    ///
    /// Returned shape: (1 + Hm*(Wm+1) + 1, out_hidden=1024) where Hm/Wm are merged-grid sizes.
    pub fn forward(&self, pixel_values: &Tensor) -> Result<(Tensor, (usize, usize)), Error> {
        let (_b, _c, rh, rw) = pixel_values.dims4().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: pixel_values dims4 failed",
                e,
            )
        })?;
        if rh % self.cfg.patch_size != 0 || rw % self.cfg.patch_size != 0 {
            return Err(Error::InvalidInput {
                message: format!(
                    "HunyuanOCR: pixel_values {rw}x{rh} not divisible by patch_size={}",
                    self.cfg.patch_size
                ),
            });
        }
        let h = rh / self.cfg.patch_size;
        let w = rw / self.cfg.patch_size;

        let patches = self
            .embeddings
            .patch_embedding
            .forward(pixel_values)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "vit patch_embedding forward", e))?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "vit patch_embedding contiguous", e)
            })?;
        let (_b2, _c2, ph, pw) = patches.dims4().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: vit patches dims4 failed",
                e,
            )
        })?;
        if ph != h || pw != w {
            return Err(Error::InvalidInput {
                message: format!(
                    "HunyuanOCR: vit patch grid mismatch: expected {h}x{w} got {ph}x{pw}"
                ),
            });
        }

        let patches = patches.squeeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: squeeze batch from vit patch embeddings failed",
                e,
            )
        })?;

        let patches = patches
            .permute((1, 2, 0))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: permute patches to hwd failed",
                    e,
                )
            })?
            .reshape((h * w, self.cfg.hidden_size))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HunyuanOCR: reshape patches to seq failed",
                    e,
                )
            })?;

        let patch_pos =
            self.embeddings
                .interpolate_patch_pos(h, w, pixel_values.device(), patches.dtype())?;
        let patch_tokens = patches.broadcast_add(&patch_pos).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: add vit patch pos embedding failed",
                e,
            )
        })?;

        // No extra/cls token: upstream HunYuanVisionPatchEmbed declares
        // `num_positions = max_num_patches + 1` but uses
        // `position_embedding.weight[1:, :]` and feeds only patch tokens —
        // slot 0 is a vestigial cls token in the trained weights, never
        // propagated. Prepending one adds attention noise across all layers.
        let hidden = patch_tokens.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: add batch dim to vit tokens failed",
                e,
            )
        })?;

        let mut hidden_states = hidden;
        for (i, layer) in self.layers.iter().enumerate() {
            hidden_states = layer
                .forward(&hidden_states)
                .map_err(|e| Error::Inference {
                    model_name: "HunyuanOCR".to_string(),
                    context: format!("vit encoder layer {i} forward failed"),
                    source: Box::new(e),
                })?;
        }

        // No extra token to drop now (see above). Just unbatch.
        let patch_out = hidden_states.squeeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: squeeze vit patch outputs failed",
                e,
            )
        })?;

        if !h.is_multiple_of(self.cfg.spatial_merge_size)
            || !w.is_multiple_of(self.cfg.spatial_merge_size)
        {
            return Err(Error::InvalidInput {
                message: format!(
                    "HunyuanOCR: vit grid {h}x{w} not divisible by spatial_merge_size={}",
                    self.cfg.spatial_merge_size
                ),
            });
        }

        let image_embeds = self.perceive.forward(&patch_out, h, w)?;
        let merged_hw = (
            h / self.cfg.spatial_merge_size,
            w / self.cfg.spatial_merge_size,
        );
        Ok((image_embeds, merged_hw))
    }
}

// A small, dependency-free bilinear interpolation that matches PyTorch's
// align_corners=False semantics for position embeddings.
fn interpolate_bilinear_align_corners_false(
    base: &[f32],
    in_h: usize,
    in_w: usize,
    out_h: usize,
    out_w: usize,
    dim: usize,
    version: HunyuanOcrVersion,
) -> Vec<f32> {
    let mut out = vec![0f32; out_h * out_w * dim];
    if in_h == 0 || in_w == 0 || out_h == 0 || out_w == 0 {
        return out;
    }

    let (inv_scale_y, inv_scale_x) = match version {
        HunyuanOcrVersion::V1 => (
            in_h as f32 / (out_h as f32 + 0.1),
            in_w as f32 / (out_w as f32 + 0.1),
        ),
        HunyuanOcrVersion::V1_5 => (in_h as f32 / out_h as f32, in_w as f32 / out_w as f32),
    };

    let row = |oy: usize, out_row: &mut [f32]| {
        let fy = ((oy as f32 + 0.5) * inv_scale_y - 0.5).max(0.0);
        let y0 = fy.floor().max(0.0) as usize;
        let y1 = (y0 + 1).min(in_h - 1);
        let wy = fy - y0 as f32;
        for ox in 0..out_w {
            let fx = ((ox as f32 + 0.5) * inv_scale_x - 0.5).max(0.0);
            let x0 = fx.floor().max(0.0) as usize;
            let x1 = (x0 + 1).min(in_w - 1);
            let wx = fx - x0 as f32;
            let out_base = ox * dim;
            let i00 = (y0 * in_w + x0) * dim;
            let i01 = (y0 * in_w + x1) * dim;
            let i10 = (y1 * in_w + x0) * dim;
            let i11 = (y1 * in_w + x1) * dim;
            for c in 0..dim {
                let v00 = base[i00 + c];
                let v01 = base[i01 + c];
                let v10 = base[i10 + c];
                let v11 = base[i11 + c];
                let v0 = v00 + (v01 - v00) * wx;
                let v1 = v10 + (v11 - v10) * wx;
                out_row[out_base + c] = v0 + (v1 - v0) * wy;
            }
        }
    };

    // Below this row count, Rayon's scheduling overhead outweighs the gain
    // from parallelizing; run sequentially instead.
    const PAR_ROW_THRESHOLD: usize = 16;
    if out_h < PAR_ROW_THRESHOLD {
        for (oy, out_row) in out.chunks_mut(out_w * dim).enumerate() {
            row(oy, out_row);
        }
    } else {
        out.par_chunks_mut(out_w * dim)
            .enumerate()
            .for_each(|(oy, out_row)| row(oy, out_row));
    }
    out
}
