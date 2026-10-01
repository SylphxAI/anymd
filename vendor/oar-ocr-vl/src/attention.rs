//! Unified attention and rotary embedding shared by the VLM backends
//! (PaddleOCR-VL, HunyuanOCR, GLM-OCR, MinerU2.5/Pro, and MinerU-Diffusion),
//! for consistent mask handling, KV-cache logic, and multi-axis RoPE
//! (MRoPE, XDRoPE).
//!
//! ## Usage
//!
//! ```ignore
//! use oar_ocr_vl::attention::{scaled_dot_product_attention, create_causal_mask, RotaryEmbedding};
//!
//! // Standard attention
//! let output = scaled_dot_product_attention(&q, &k, &v, mask, scale, is_causal)?;
//!
//! // Create causal mask for autoregressive decoding
//! let mask = create_causal_mask(seq_len, kv_len, dtype, device)?;
//!
//! // Multi-axis RoPE (for PaddleOCR-VL, HunyuanOCR)
//! let rope = RotaryEmbedding::new_multi_axis(head_dim, rope_theta, num_dims, device)?;
//! let (cos, sin) = rope.forward_multi_axis(&position_ids, dtype)?;
//! ```

use crate::error::Error;
use crate::utils::candle_to_ocr_processing;
use candle_core::{D, DType, Device, IndexOp, Result, Tensor};
use std::sync::OnceLock;

fn grouped_query_attention_disabled() -> bool {
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var_os("OAR_VL_DISABLE_GQA").is_some())
}

/// Native half-precision eager softmax is opt-in on Metal. Fused SDPA has its
/// own softmax implementation and is unaffected by these switches.
fn metal_native_softmax_enabled(dtype: DType) -> bool {
    static FORCE_NATIVE: OnceLock<bool> = OnceLock::new();
    static FORCE_F32: OnceLock<bool> = OnceLock::new();
    let force_native =
        *FORCE_NATIVE.get_or_init(|| std::env::var_os("OAR_VL_METAL_NATIVE_SOFTMAX").is_some());
    let force_f32 =
        *FORCE_F32.get_or_init(|| std::env::var_os("OAR_VL_METAL_F32_SOFTMAX").is_some());
    !force_f32 && force_native && matches!(dtype, DType::F16 | DType::BF16)
}

fn metal_sdpa_disabled() -> bool {
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var_os("OAR_VL_DISABLE_METAL_SDPA").is_some())
}

/// Fused Metal SDPA attempt; `Ok(None)` when unavailable or declined, so
/// callers keep their own eager fallback.
pub(crate) fn try_fused_sdpa(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    mask: Option<&Tensor>,
    scale: f64,
    is_causal: bool,
) -> Result<Option<Tensor>> {
    try_metal_sdpa(q, k, v, mask, scale, is_causal, false)
}

fn try_metal_sdpa(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    mask: Option<&Tensor>,
    scale: f64,
    is_causal: bool,
    allow_vector_kernel: bool,
) -> Result<Option<Tensor>> {
    if metal_sdpa_disabled() || !q.device().is_metal() {
        return Ok(None);
    }
    let Ok((batch, num_heads, query_len, head_dim)) = q.dims4() else {
        return Ok(None);
    };
    let Ok((k_batch, num_kv_heads, kv_len, k_head_dim)) = k.dims4() else {
        return Ok(None);
    };
    let Ok((v_batch, v_heads, v_len, v_head_dim)) = v.dims4() else {
        return Ok(None);
    };
    // Mirror the shape consistency scaled_dot_product_attention_gqa enforces
    // before it ever reaches this function, so the non-GQA caller (which has
    // no equivalent upstream check) can't hand a fused Metal kernel batch,
    // head, length, or head_dim mismatches it isn't guaranteed to validate.
    // `allow_vector_kernel` is only ever `true` for the GQA caller, which has
    // already validated `num_heads == num_kv_heads * num_kv_groups`; the
    // plain (non-GQA) caller documents equal head counts, so require an exact
    // match there instead of merely a multiple, or a head-count mismatch
    // would silently get grouped-query semantics on Metal only.
    let heads_match = if allow_vector_kernel {
        num_heads.is_multiple_of(num_kv_heads)
    } else {
        num_heads == num_kv_heads
    };
    if num_kv_heads == 0
        || batch != k_batch
        || batch != v_batch
        || num_kv_heads != v_heads
        || kv_len != v_len
        || head_dim != k_head_dim
        || head_dim != v_head_dim
        || !heads_match
    {
        return Ok(None);
    }
    // Candle's q_len == 1 vector kernel ignores both explicit masks and their
    // strides. Keep masked decode on the eager path. Standard attention also
    // keeps decode eager by design.
    //
    // Candle's causal kernel derives its diagonal offset as `kv_len - query_len`
    // in unsigned arithmetic, so query_len > kv_len underflows and masks every
    // row. `create_causal_mask` rejects that shape, so staying eager makes Metal
    // report the same error instead of returning NaNs.
    if query_len == 0
        || (query_len == 1 && (!allow_vector_kernel || mask.is_some()))
        || (is_causal && mask.is_none() && query_len > kv_len)
    {
        return Ok(None);
    }

    let expanded_mask = match mask {
        Some(mask) if mask.dims() == [batch, num_heads, query_len, kv_len] => Some(mask.clone()),
        Some(mask) => Some(mask.broadcast_as((batch, num_heads, query_len, kv_len))?),
        None => None,
    };
    // The eager implementation treats an explicit mask as authoritative and
    // only synthesizes a causal mask when no mask was supplied.
    let do_causal = is_causal && expanded_mask.is_none();
    let fused = candle_nn::ops::sdpa(
        q,
        k,
        v,
        expanded_mask.as_ref(),
        do_causal,
        scale as f32,
        1.0,
    );
    // Candle's Metal support matrix is private and may change between patch
    // releases. An unsupported fused shape is an optimization miss, not an
    // inference failure, so let the caller continue through the eager path.
    // Log it: the eager fallback needs more memory, so a rejection that is
    // really an allocation failure would otherwise vanish.
    if let Err(ref err) = fused {
        tracing::debug!(
            "Metal fused SDPA declined for q {:?}, k {:?}, v {:?}; using eager attention: {err}",
            q.dims(),
            k.dims(),
            v.dims(),
        );
    }
    Ok(fused.ok())
}

#[cfg(feature = "cuda")]
fn flash_attention_disabled() -> bool {
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var_os("OAR_VL_DISABLE_FLASH_ATTN").is_some())
}

/// Helper function to handle Metal device computation.
///
/// Metal backend doesn't support certain operations (arange, broadcast_*, etc.).
/// This helper executes operations on CPU for Metal devices, then transfers the
/// result back to Metal.
///
/// # Arguments
/// * `device` - Target device
/// * `f` - Closure that creates the tensor on the compute device
///
/// # Returns
/// Tensor on the target device
pub(crate) fn on_compute_device<F>(device: &Device, f: F) -> Result<Tensor>
where
    F: FnOnce(&Device) -> Result<Tensor>,
{
    if device.is_metal() {
        // Operations unsupported on Metal are run on the CPU...
        let cpu_device = Device::Cpu;
        let tensor_on_cpu = f(&cpu_device)?;
        // ...and the result is moved back to the Metal device.
        tensor_on_cpu.to_device(device)
    } else {
        // For other devices, run directly.
        f(device)
    }
}

/// Scaled dot-product attention.
///
/// Computes attention as: softmax(Q @ K^T * scale) @ V
///
/// # Arguments
/// * `q` - Query tensor: (batch, heads, seq_q, head_dim)
/// * `k` - Key tensor: (batch, heads, seq_kv, head_dim)
/// * `v` - Value tensor: (batch, heads, seq_kv, head_dim)
/// * `mask` - Optional attention mask to add before softmax
/// * `scale` - Scaling factor (typically 1/sqrt(head_dim))
/// * `is_causal` - If true and mask is None, creates a causal mask
///
/// # Returns
/// Output tensor: (batch, heads, seq_q, head_dim)
pub fn scaled_dot_product_attention(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    mask: Option<&Tensor>,
    scale: f64,
    is_causal: bool,
) -> Result<Tensor> {
    if let Some(output) = try_metal_sdpa(q, k, v, mask, scale, is_causal, false)? {
        return Ok(output);
    }

    // Q @ K^T
    let attn_weights = q.matmul(&k.transpose(2, 3)?)?;

    // Scale
    let attn_weights = (attn_weights * scale)?;

    // Apply mask
    let attn_weights = match mask {
        Some(m) => attn_weights.broadcast_add(m)?,
        None if is_causal => {
            let seq_len = attn_weights.dim(2)?;
            let kv_len = attn_weights.dim(3)?;
            let causal_mask =
                create_causal_mask(seq_len, kv_len, attn_weights.dtype(), q.device())?;
            attn_weights.broadcast_add(&causal_mask)?
        }
        None => attn_weights,
    };

    // Softmax: cast to F32 for numerical stability, then cast back. Metal can
    // opt into its native half-precision kernel to avoid two conversions.
    let input_dtype = attn_weights.dtype();
    let use_native_metal_softmax = q.device().is_metal()
        && input_dtype != DType::F32
        && metal_native_softmax_enabled(input_dtype);
    let attn_weights = if use_native_metal_softmax {
        candle_nn::ops::softmax_last_dim(&attn_weights)?
    } else {
        let attn_weights = attn_weights.to_dtype(DType::F32)?;
        let attn_weights = candle_nn::ops::softmax_last_dim(&attn_weights)?;
        attn_weights.to_dtype(input_dtype)?
    };

    // Attention @ V
    attn_weights.matmul(v)
}

/// Sequence length above which vision backends use query-chunked attention to
/// cap the size of the temporary attention-score matrix.
pub(crate) const VISION_CHUNKED_ATTN_SEQ_THRESHOLD: usize = 1024;
/// Query chunk size used by the shared eager vision-attention fallback.
pub(crate) const VISION_CHUNKED_ATTN_CHUNK_SIZE: usize = 256;

/// Non-causal scaled dot-product attention computed in query chunks. This has
/// the same result as one eager attention call while reducing peak memory.
pub(crate) fn chunked_vision_attention(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    scale: f64,
    chunk_size: usize,
) -> Result<Tensor> {
    if chunk_size == 0 {
        candle_core::bail!("vision attention chunk size must be non-zero")
    }
    let seq_len = q.dim(2)?;
    if seq_len == 0 {
        return scaled_dot_product_attention(q, k, v, None, scale, false);
    }
    let mut chunks = Vec::with_capacity(seq_len.div_ceil(chunk_size));
    let mut start = 0usize;
    while start < seq_len {
        let len = (seq_len - start).min(chunk_size);
        let q_chunk = q.narrow(2, start, len)?;
        chunks.push(scaled_dot_product_attention(
            &q_chunk, k, v, None, scale, false,
        )?);
        start += len;
    }
    let chunks = chunks.iter().collect::<Vec<_>>();
    Tensor::cat(&chunks, 2)
}

/// Scaled dot-product attention for grouped-query attention without expanding
/// K/V heads. Query heads that share one KV head are folded into the matrix row
/// dimension, preserving the usual head order in the returned tensor.
pub fn scaled_dot_product_attention_gqa(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    mask: Option<&Tensor>,
    scale: f64,
    is_causal: bool,
    num_kv_groups: usize,
) -> Result<Tensor> {
    if num_kv_groups == 1 {
        return scaled_dot_product_attention(q, k, v, mask, scale, is_causal);
    }
    if grouped_query_attention_disabled() {
        let k = repeat_kv(k, num_kv_groups)?;
        let v = repeat_kv(v, num_kv_groups)?;
        return scaled_dot_product_attention(q, &k, &v, mask, scale, is_causal);
    }

    let (batch, num_heads, query_len, head_dim) = q.dims4()?;
    let (k_batch, num_kv_heads, kv_len, k_head_dim) = k.dims4()?;
    let (v_batch, v_heads, v_len, v_head_dim) = v.dims4()?;
    if batch != k_batch
        || batch != v_batch
        || num_heads != num_kv_heads * num_kv_groups
        || num_kv_heads != v_heads
        || kv_len != v_len
        || head_dim != k_head_dim
        || head_dim != v_head_dim
    {
        candle_core::bail!(
            "invalid GQA shapes q={:?}, k={:?}, v={:?}, groups={num_kv_groups}",
            q.dims(),
            k.dims(),
            v.dims()
        )
    }

    if let Some(output) = try_metal_sdpa(q, k, v, mask, scale, is_causal, true)? {
        return Ok(output);
    }

    let grouped_batch = batch * num_kv_heads;
    let grouped_queries = num_kv_groups * query_len;
    let grouped_q = q.reshape((grouped_batch, grouped_queries, head_dim))?;
    let grouped_k = k
        .reshape((grouped_batch, kv_len, head_dim))?
        .transpose(1, 2)?;
    let mut weights =
        (grouped_q.matmul(&grouped_k)? * scale)?.reshape((batch, num_heads, query_len, kv_len))?;

    weights = match mask {
        Some(mask) => weights.broadcast_add(mask)?,
        None if is_causal => {
            let causal = create_causal_mask(query_len, kv_len, weights.dtype(), q.device())?;
            weights.broadcast_add(&causal)?
        }
        None => weights,
    };

    let weight_dtype = weights.dtype();
    let use_native_metal_softmax = q.device().is_metal()
        && weight_dtype != DType::F32
        && metal_native_softmax_enabled(weight_dtype);
    let weights = if use_native_metal_softmax {
        candle_nn::ops::softmax_last_dim(&weights)?
    } else {
        candle_nn::ops::softmax_last_dim(&weights.to_dtype(DType::F32)?)?.to_dtype(weight_dtype)?
    }
    .reshape((grouped_batch, grouped_queries, kv_len))?;
    let grouped_v = v.reshape((grouped_batch, kv_len, head_dim))?;
    weights
        .matmul(&grouped_v)?
        .reshape((batch, num_heads, query_len, head_dim))
}

/// Grouped-query attention over multiple logical KV segments.
///
/// This is the eager equivalent of paged attention for forked requests: the
/// shared prefix and the branch-private tail remain separate Tensor views. We
/// concatenate only the score matrix, never the (much larger) K/V tensors.
/// It is exact and keeps forked prefixes zero-copy. When `is_causal` is true,
/// queries must represent the final `query_len` positions of the concatenated
/// segments (right-aligned causal masking). A supplied mask's last dimension
/// must cover every segment, including both the shared prefix and private tail.
pub fn segmented_scaled_dot_product_attention_gqa(
    q: &Tensor,
    segments: &[(&Tensor, &Tensor)],
    mask: Option<&Tensor>,
    scale: f64,
    is_causal: bool,
    num_kv_groups: usize,
) -> Result<Tensor> {
    if segments.is_empty() {
        candle_core::bail!("segmented attention requires at least one KV segment")
    }
    if segments.len() == 1 {
        return scaled_dot_product_attention_gqa(
            q,
            segments[0].0,
            segments[0].1,
            mask,
            scale,
            is_causal,
            num_kv_groups,
        );
    }

    let (batch, num_heads, query_len, head_dim) = q.dims4()?;
    let (_, num_kv_heads, _, _) = segments[0].0.dims4()?;
    if num_heads != num_kv_heads * num_kv_groups {
        candle_core::bail!(
            "invalid segmented GQA heads q={} kv={} groups={num_kv_groups}",
            num_heads,
            num_kv_heads
        )
    }
    let grouped_batch = batch * num_kv_heads;
    let grouped_queries = num_kv_groups * query_len;
    let grouped_q = q.reshape((grouped_batch, grouped_queries, head_dim))?;
    let mut score_segments = Vec::with_capacity(segments.len());
    let mut lengths = Vec::with_capacity(segments.len());
    for &(k, v) in segments {
        let (k_batch, k_heads, len, k_dim) = k.dims4()?;
        let (v_batch, v_heads, v_len, v_dim) = v.dims4()?;
        if k_batch != batch
            || v_batch != batch
            || k_heads != num_kv_heads
            || v_heads != num_kv_heads
            || v_len != len
            || k_dim != head_dim
            || v_dim != head_dim
        {
            candle_core::bail!(
                "invalid segmented GQA shapes q={:?}, k={:?}, v={:?}",
                q.dims(),
                k.dims(),
                v.dims()
            )
        }
        let grouped_k = k.reshape((grouped_batch, len, head_dim))?.transpose(1, 2)?;
        score_segments.push(
            (grouped_q.matmul(&grouped_k)? * scale)?.reshape((batch, num_heads, query_len, len))?,
        );
        lengths.push(len);
    }
    let score_refs = score_segments.iter().collect::<Vec<_>>();
    let mut weights = Tensor::cat(&score_refs, 3)?;
    let kv_len = lengths.iter().sum();
    if let Some(mask) = mask {
        let mask_kv_len = mask.dim(D::Minus1)?;
        if mask_kv_len != kv_len {
            candle_core::bail!(
                "segmented GQA mask covers {mask_kv_len} KV positions, expected {kv_len} across all segments"
            )
        }
    }
    weights = match mask {
        Some(mask) => weights.broadcast_add(mask)?,
        None if is_causal => {
            let causal = create_causal_mask(query_len, kv_len, weights.dtype(), q.device())?;
            weights.broadcast_add(&causal)?
        }
        None => weights,
    };
    let weight_dtype = weights.dtype();
    let weights =
        candle_nn::ops::softmax_last_dim(&weights.to_dtype(DType::F32)?)?.to_dtype(weight_dtype)?;

    let mut offset = 0;
    let mut output: Option<Tensor> = None;
    for ((_, v), len) in segments.iter().zip(lengths) {
        let segment_weights =
            weights
                .narrow(3, offset, len)?
                .reshape((grouped_batch, grouped_queries, len))?;
        let grouped_v = v.reshape((grouped_batch, len, head_dim))?;
        let segment_output = segment_weights.matmul(&grouped_v)?;
        output = Some(match output {
            Some(acc) => (acc + segment_output)?,
            None => segment_output,
        });
        offset += len;
    }
    output
        .expect("segments are non-empty")
        .reshape((batch, num_heads, query_len, head_dim))
}

#[cfg(any(feature = "cuda", test))]
fn flash_attention_dtype_supported(dtype: DType) -> bool {
    matches!(dtype, DType::F16 | DType::BF16)
}

/// Run CUDA FlashAttention v2 for Q/K/V tensors in `(batch, heads, seq,
/// head_dim)` layout. Returns `None` on non-CUDA devices or for dtypes not
/// supported by the CUDA kernel so callers can retain their portable eager
/// fallback.
pub fn flash_attention(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    scale: f64,
    causal: bool,
) -> Result<Option<Tensor>> {
    #[cfg(feature = "cuda")]
    if !flash_attention_disabled()
        && q.device().is_cuda()
        && flash_attention_dtype_supported(q.dtype())
        && k.dtype() == q.dtype()
        && v.dtype() == q.dtype()
    {
        // The CUDA kernel consumes (batch, seq, heads, head_dim) and natively
        // supports GQA when K/V have fewer heads than Q.
        let q = q.transpose(1, 2)?;
        let k = k.transpose(1, 2)?;
        let v = v.transpose(1, 2)?;
        let output = candle_flash_attn::flash_attn(&q, &k, &v, scale as f32, causal)?;
        return Ok(Some(output.transpose(1, 2)?));
    }

    let _ = (q, k, v, scale, causal);
    Ok(None)
}

/// Create a causal (lower-triangular) attention mask.
///
/// Queries are right-aligned with the KV sequence: query row `i` represents KV
/// position `kv_len - seq_len + i` and can attend through that position. This
/// requires `kv_len >= seq_len`.
/// The mask contains 0 for allowed positions and -inf for masked positions.
///
/// # Arguments
/// * `seq_len` - Query sequence length
/// * `kv_len` - Key/Value sequence length
/// * `dtype` - Data type for the mask tensor
/// * `device` - Device for the mask tensor
///
/// # Returns
/// Mask tensor of shape (1, 1, seq_len, kv_len)
pub fn create_causal_mask(
    seq_len: usize,
    kv_len: usize,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    if kv_len < seq_len {
        candle_core::bail!(
            "causal mask requires right-aligned queries with kv_len ({kv_len}) >= seq_len ({seq_len})"
        )
    }
    on_compute_device(device, |compute_device| {
        let row_idx =
            Tensor::arange(0u32, seq_len as u32, compute_device)?.reshape((seq_len, 1))?;
        let col_idx = Tensor::arange(0u32, kv_len as u32, compute_device)?.reshape((1, kv_len))?;

        let offset = (kv_len - seq_len) as u32;
        // Condition: col <= row + offset
        // Keep this comparison in integer space. BF16 cannot distinguish
        // adjacent absolute positions once a document context grows beyond
        // 256 tokens, which would let verification queries see future draft
        // tokens and invalidate speculative decoding.
        let row_limit = row_idx.broadcast_add(&Tensor::new(offset, compute_device)?)?;
        let mask_cond = col_idx.broadcast_le(&row_limit)?;

        let zero = Tensor::new(0f32, compute_device)?
            .to_dtype(dtype)?
            .broadcast_as(mask_cond.shape())?;
        let neg_inf = Tensor::new(f32::NEG_INFINITY, compute_device)?
            .to_dtype(dtype)?
            .broadcast_as(mask_cond.shape())?;

        mask_cond
            .where_cond(&zero, &neg_inf)?
            .reshape((1, 1, seq_len, kv_len))
    })
}

/// Create a padding mask from sequence lengths.
///
/// # Arguments
/// * `seq_lens` - Sequence lengths for each batch item
/// * `max_len` - Maximum sequence length
/// * `dtype` - Data type for the mask tensor
/// * `device` - Device for the mask tensor
///
/// # Returns
/// Mask tensor of shape (batch, 1, 1, max_len)
pub fn create_padding_mask(
    seq_lens: &[usize],
    max_len: usize,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    let batch_size = seq_lens.len();

    on_compute_device(device, |compute_device| {
        // (B, 1, 1, 1)
        let lens_tensor = Tensor::from_vec(
            seq_lens.iter().map(|&x| x as u32).collect::<Vec<_>>(),
            (batch_size, 1, 1, 1),
            compute_device,
        )?
        .to_dtype(dtype)?;

        // (1, 1, 1, max_len)
        let pos_tensor = Tensor::arange(0u32, max_len as u32, compute_device)?
            .reshape((1, 1, 1, max_len))?
            .to_dtype(dtype)?;

        // Mask: pos < len -> 0, else -inf
        let mask_cond = pos_tensor.broadcast_lt(&lens_tensor)?;

        let zero = Tensor::new(0f32, compute_device)?
            .to_dtype(dtype)?
            .broadcast_as(mask_cond.shape())?;
        let neg_inf = Tensor::new(f32::NEG_INFINITY, compute_device)?
            .to_dtype(dtype)?
            .broadcast_as(mask_cond.shape())?;

        mask_cond.where_cond(&zero, &neg_inf)
    })
}

/// Combine causal and padding masks.
///
/// # Arguments
/// * `causal_mask` - Causal mask (1, 1, seq_len, kv_len)
/// * `padding_mask` - Padding mask (batch, 1, 1, kv_len)
///
/// # Returns
/// Combined mask (batch, 1, seq_len, kv_len)
pub fn combine_masks(causal_mask: &Tensor, padding_mask: &Tensor) -> Result<Tensor> {
    causal_mask.broadcast_add(padding_mask)
}

/// Flattens per-row decode positions into the buffer a `(axes, batch, 1)` MRoPE
/// position tensor expects.
///
/// The buffer is axis-major: every row's position for axis 0, then axis 1, and
/// so on. Writing it batch-major (`[p0, p0, p0, p1, ...]`) transposes the tensor
/// and hands each row another row's positions. The two layouts coincide at
/// `batch_size == 1`, so the mistake stays invisible until a real batch.
pub(crate) fn decode_position_buffer(positions: &[i64], axes: usize) -> Vec<i64> {
    (0..axes).flat_map(|_| positions.iter().copied()).collect()
}

/// Additive "cannot attend" fill: very negative, but finite in `dtype` with room
/// left to absorb attention logits before the softmax.
///
/// F16 tops out at 65504, so the value used for wider dtypes saturates to -inf
/// there — which is exactly the all-masked-row NaN a finite fill exists to
/// avoid. Staying well inside the range also keeps `score + fill` finite.
fn masked_score(dtype: DType) -> f64 {
    match dtype {
        DType::F16 => -1e4,
        _ => -1e9,
    }
}

/// Create a left-padding mask for batched sequences (right-aligned, standard for
/// autoregressive generation).
///
/// Returns a `(batch, 1, 1, max_len)` mask where left-padded positions
/// (`j < max_len - seq_len`) are strongly negative and valid positions are `0`.
/// With `seq_lens = [3, 5]` and `max_len = 5`, item 0 is `[m, m, 0, 0, 0]`.
pub fn create_left_padding_mask(
    seq_lens: &[usize],
    max_len: usize,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    let batch_size = seq_lens.len();

    on_compute_device(device, |compute_device| {
        // pad_len = max_len - len
        // lens: (B, 1, 1, 1)
        let lens_tensor = Tensor::from_vec(
            seq_lens.iter().map(|&x| x as u32).collect::<Vec<_>>(),
            (batch_size, 1, 1, 1),
            compute_device,
        )?
        .to_dtype(dtype)?;

        let max_len_t = Tensor::new(max_len as u32, compute_device)?.to_dtype(dtype)?;
        let pad_len = max_len_t.broadcast_sub(&lens_tensor)?; // (B, 1, 1, 1)

        // pos: (1, 1, 1, max_len)
        let pos_tensor = Tensor::arange(0u32, max_len as u32, compute_device)?
            .reshape((1, 1, 1, max_len))?
            .to_dtype(dtype)?;

        // Mask: pos < pad_len -> suppressed, else 0
        let mask_cond = pos_tensor.broadcast_lt(&pad_len)?;

        let zero = Tensor::new(0f32, compute_device)?
            .to_dtype(dtype)?
            .broadcast_as(mask_cond.shape())?;
        // Finite, not -inf: with the causal mask a query inside the padding
        // region reaches only padding keys, so -inf makes the row all -inf and
        // its softmax NaN, which `0 * NaN` then spreads everywhere.
        let masked = Tensor::new(masked_score(dtype) as f32, compute_device)?
            .to_dtype(dtype)?
            .broadcast_as(mask_cond.shape())?;

        // if pos < pad_len (padded region), suppress
        mask_cond.where_cond(&masked, &zero)
    })
}

/// Builds the per-step decode attention mask for a left-padded batch.
///
/// Masks out the leading `pad_lens[i]` padding positions of each row so the new
/// token never attends to padding KV (which would corrupt unequal-length
/// batches). Returns a `(batch, 1, 1, kv_len)` additive mask (`0` attendable, a
/// large negative for padding); a no-op when there is no padding.
pub fn create_generation_mask(
    pad_lens: &[usize],
    kv_len: usize,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    let batch_size = pad_lens.len();

    on_compute_device(device, |compute_device| {
        // pad_lens as tensor: (batch, 1, 1, 1)
        let pad_lens_tensor = Tensor::from_vec(
            pad_lens.iter().map(|&x| x as u32).collect::<Vec<_>>(),
            (batch_size, 1, 1, 1),
            compute_device,
        )?
        .to_dtype(dtype)?;

        // Position indices: (1, 1, 1, kv_len)
        let pos_tensor = Tensor::arange(0u32, kv_len as u32, compute_device)?
            .reshape((1, 1, 1, kv_len))?
            .to_dtype(dtype)?;

        // Mask condition: pos < pad_len -> masked (large negative value)
        let mask_cond = pos_tensor.broadcast_lt(&pad_lens_tensor)?;

        let zero = Tensor::zeros(mask_cond.shape(), dtype, compute_device)?;
        // Large negative rather than -inf, and finite in every dtype.
        let mask_value = Tensor::full(
            masked_score(dtype) as f32,
            mask_cond.shape(),
            compute_device,
        )?
        .to_dtype(dtype)?;

        mask_cond.where_cond(&mask_value, &zero)
    })
}

/// Avoid materializing an all-zero decode mask when every prompt in a batch
/// has the same length. Besides saving an allocation, `None` lets Metal select
/// its fused single-token GQA kernel.
pub(crate) fn create_generation_mask_if_needed(
    pad_lens: &[usize],
    kv_len: usize,
    dtype: DType,
    device: &Device,
) -> Result<Option<Tensor>> {
    if pad_lens.iter().all(|&len| len == 0) {
        Ok(None)
    } else {
        create_generation_mask(pad_lens, kv_len, dtype, device).map(Some)
    }
}

// Rotary Positional Embedding (RoPE)

/// Unified Rotary Positional Embedding supporting single-axis RoPE, MRoPE
/// (3-axis text/height/width, PaddleOCR-VL), and XDRoPE (configurable `num_dims`,
/// HunyuanOCR). All variants share one `Dynamic` representation parameterized by
/// `num_dims`; constructors pick the right value per model family.
#[derive(Debug, Clone)]
pub enum RotaryEmbedding {
    /// Dynamic computation from inverse frequencies (used by PaddleOCR-VL, HunyuanOCR).
    /// Supports multi-axis position encoding.
    Dynamic {
        inv_freq: Tensor,
        /// Number of position dimensions (1 for standard, 3 for MRoPE/XDRoPE)
        num_dims: usize,
    },
}

impl RotaryEmbedding {
    /// Create a dynamic single-axis RoPE computed on-the-fly from position IDs
    /// (suitable for variable-length sequences).
    ///
    /// `head_dim` must be even; `rope_theta` is the base frequency (typically 10000.0).
    pub fn new_dynamic(
        head_dim: usize,
        rope_theta: f64,
        device: &Device,
    ) -> std::result::Result<Self, Error> {
        Self::new_multi_axis(head_dim, rope_theta, 1, device)
    }

    /// Create a multi-axis RoPE (MRoPE/XDRoPE) with `num_dims` position
    /// dimensions (1 = standard, 3 = MRoPE text/height/width).
    ///
    /// `head_dim` must be even; `rope_theta` is the base frequency (typically 10000.0).
    pub fn new_multi_axis(
        head_dim: usize,
        rope_theta: f64,
        num_dims: usize,
        device: &Device,
    ) -> std::result::Result<Self, Error> {
        if !head_dim.is_multiple_of(2) {
            return Err(Error::Config {
                message: format!("RotaryEmbedding: head_dim must be even, got {head_dim}"),
            });
        }
        let half = head_dim / 2;
        let mut inv_freq = Vec::with_capacity(half);
        for i in (0..head_dim).step_by(2) {
            let v = 1f64 / rope_theta.powf(i as f64 / head_dim as f64);
            inv_freq.push(v as f32);
        }
        let inv_freq = Tensor::from_vec(inv_freq, (half,), device).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "RotaryEmbedding: failed to create inv_freq tensor",
                e,
            )
        })?;
        Ok(Self::Dynamic { inv_freq, num_dims })
    }

    /// Forward pass for multi-axis RoPE.
    ///
    /// Computes cos/sin from position IDs dynamically. Supports multi-dimensional
    /// position encoding.
    ///
    /// # Arguments
    /// * `position_ids` - Position tensor, shape: (num_dims, batch, seq)
    /// * `dtype` - Target data type for output
    ///
    /// # Returns
    /// Tuple of (cos, sin) tensors, shape: (num_dims, batch, seq, head_dim)
    pub fn forward_multi_axis(
        &self,
        position_ids: &Tensor,
        dtype: DType,
    ) -> std::result::Result<(Tensor, Tensor), Error> {
        match self {
            Self::Dynamic { inv_freq, num_dims } => {
                let dims = position_ids.dims();
                if dims.len() != 3 || dims[0] != *num_dims {
                    return Err(Error::InvalidInput {
                        message: format!(
                            "RotaryEmbedding: expected position_ids shape ({}, B, S), got {:?}",
                            num_dims, dims
                        ),
                    });
                }

                let position_ids = position_ids.to_dtype(DType::F32).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        format!("RotaryEmbedding: position_ids cast to f32 failed (dims {dims:?})"),
                        e,
                    )
                })?;

                let inv_len = inv_freq.dims1().map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "RotaryEmbedding: inv_freq dims1 failed",
                        e,
                    )
                })?;
                let inv = inv_freq
                    .reshape((1usize, 1usize, 1usize, inv_len))
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: inv_freq reshape failed",
                            e,
                        )
                    })?;

                let freqs = position_ids
                    .unsqueeze(3)
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: position_ids unsqueeze failed",
                            e,
                        )
                    })?
                    .broadcast_mul(&inv)
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: rotary freqs multiply failed",
                            e,
                        )
                    })?;

                let emb = Tensor::cat(&[&freqs, &freqs], D::Minus1).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "RotaryEmbedding: rotary emb cat failed",
                        e,
                    )
                })?;

                let cos = emb
                    .cos()
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: rotary cos failed",
                            e,
                        )
                    })?
                    .to_dtype(dtype)
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: rotary cos cast failed",
                            e,
                        )
                    })?;

                let sin = emb
                    .sin()
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: rotary sin failed",
                            e,
                        )
                    })?
                    .to_dtype(dtype)
                    .map_err(|e| {
                        candle_to_ocr_processing(
                            crate::error::ProcessingStage::TensorOperation,
                            "RotaryEmbedding: rotary sin cast failed",
                            e,
                        )
                    })?;
                Ok((cos, sin))
            }
        }
    }
}

/// Repeat KV heads for Grouped Query Attention (GQA).
///
/// When num_heads > num_kv_heads, the KV heads need to be repeated
/// to match the number of query heads.
///
/// # Arguments
/// * `x` - Input tensor: (batch, num_kv_heads, seq, head_dim)
/// * `n_rep` - Number of times to repeat each KV head
///
/// # Returns
/// Output tensor: (batch, num_kv_heads * n_rep, seq, head_dim)
pub fn repeat_kv(x: &Tensor, n_rep: usize) -> Result<Tensor> {
    if n_rep == 1 {
        return Ok(x.clone());
    }
    let (batch, num_kv_heads, seq_len, head_dim) = x.dims4()?;
    x.unsqueeze(2)?
        .expand((batch, num_kv_heads, n_rep, seq_len, head_dim))?
        .reshape((batch, num_kv_heads * n_rep, seq_len, head_dim))
}

/// Select and combine RoPE sections for multi-axis encoding (MRoPE 3-axis,
/// XDRoPE 4-axis): different `head_dim` sections take different position
/// dimensions, encoding spatial (height, width) and temporal positions separately.
///
/// `cos_or_sin` is `(num_dims, batch, seq, head_dim)`, `rope_section` sums to
/// `head_dim/2`; returns `(batch, 1, seq, head_dim)`.
///
/// # Example
/// MRoPE with `rope_section=[16, 24, 24]`, `head_dim=128`: sections double to
/// `[16, 24, 24, 16, 24, 24]`, each picking dim `i % 3`, concatenated along `head_dim`.
pub fn select_rope_sections(
    cos_or_sin: &Tensor,
    rope_section: &[usize],
    num_dims: usize,
) -> std::result::Result<Tensor, Error> {
    if rope_section.is_empty() {
        return Err(Error::Config {
            message: "rope_section is empty".to_string(),
        });
    }

    let dims = cos_or_sin.dims();
    let head_dim = dims.get(3).copied().unwrap_or(0);
    let section_sum: usize = rope_section.iter().sum();
    if section_sum * 2 != head_dim {
        return Err(Error::Config {
            message: format!(
                "rope_section sum ({}) * 2 != head_dim ({})",
                section_sum, head_dim
            ),
        });
    }

    let actual_dims = dims.first().copied().unwrap_or(0);
    if actual_dims != num_dims {
        return Err(Error::InvalidInput {
            message: format!(
                "rope tensor has {} dims, expected {}",
                actual_dims, num_dims
            ),
        });
    }

    // Double the sections: [a, b, c] -> [a, b, c, a, b, c]
    let doubled_sections: Vec<usize> = rope_section
        .iter()
        .chain(rope_section.iter())
        .copied()
        .collect();

    let mut offset = 0usize;
    let mut chunks: Vec<Tensor> = Vec::with_capacity(doubled_sections.len());
    for (i, &sec) in doubled_sections.iter().enumerate() {
        let next = offset + sec;
        let seg = cos_or_sin.i((.., .., .., offset..next)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                format!(
                    "rope slice failed at chunk {} (offset {}..{})",
                    i, offset, next
                ),
                e,
            )
        })?;
        let picked = seg.i((i % num_dims, .., .., ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                format!("rope pick failed at chunk {} (dim {})", i, i % num_dims),
                e,
            )
        })?;
        chunks.push(picked);
        offset = next;
    }

    let refs: Vec<&Tensor> = chunks.iter().collect();
    let cat = Tensor::cat(&refs, D::Minus1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "rope cat failed",
            e,
        )
    })?;
    cat.unsqueeze(1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "rope unsqueeze failed",
            e,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_attention_rejects_unsupported_dtypes() {
        assert!(flash_attention_dtype_supported(DType::F16));
        assert!(flash_attention_dtype_supported(DType::BF16));
        assert!(!flash_attention_dtype_supported(DType::F32));
        assert!(!flash_attention_dtype_supported(DType::F64));
    }

    #[test]
    fn test_scaled_dot_product_attention() -> Result<()> {
        let device = Device::Cpu;

        // Create simple Q, K, V tensors
        let q = Tensor::randn(0f32, 1., (1, 4, 8, 64), &device)?;
        let k = Tensor::randn(0f32, 1., (1, 4, 8, 64), &device)?;
        let v = Tensor::randn(0f32, 1., (1, 4, 8, 64), &device)?;

        let scale = 1.0 / (64f64).sqrt();

        // Without mask
        let out = scaled_dot_product_attention(&q, &k, &v, None, scale, false)?;
        assert_eq!(out.dims(), &[1, 4, 8, 64]);

        // With causal mask
        let out = scaled_dot_product_attention(&q, &k, &v, None, scale, true)?;
        assert_eq!(out.dims(), &[1, 4, 8, 64]);

        Ok(())
    }

    #[test]
    fn test_chunked_vision_attention_matches_eager() -> Result<()> {
        let device = Device::Cpu;
        let q = Tensor::randn(0f32, 1., (1, 2, 7, 8), &device)?;
        let k = Tensor::randn(0f32, 1., (1, 2, 5, 8), &device)?;
        let v = Tensor::randn(0f32, 1., (1, 2, 5, 8), &device)?;
        let scale = 1.0 / 8f64.sqrt();
        let eager = scaled_dot_product_attention(&q, &k, &v, None, scale, false)?;
        let chunked = chunked_vision_attention(&q, &k, &v, scale, 3)?;
        let eager = eager.flatten_all()?.to_vec1::<f32>()?;
        let chunked = chunked.flatten_all()?.to_vec1::<f32>()?;
        assert!(
            eager
                .iter()
                .zip(chunked)
                .all(|(left, right)| (left - right).abs() < 1e-6)
        );
        Ok(())
    }

    #[test]
    fn test_grouped_query_attention_matches_repeated_kv() -> Result<()> {
        let device = Device::Cpu;
        let q = Tensor::randn(0f32, 1., (1, 4, 3, 8), &device)?;
        let k = Tensor::randn(0f32, 1., (1, 2, 5, 8), &device)?;
        let v = Tensor::randn(0f32, 1., (1, 2, 5, 8), &device)?;
        let mask = create_causal_mask(3, 5, DType::F32, &device)?;
        let scale = 1.0 / (8f64).sqrt();

        let repeated = scaled_dot_product_attention(
            &q,
            &repeat_kv(&k, 2)?,
            &repeat_kv(&v, 2)?,
            Some(&mask),
            scale,
            false,
        )?;
        let grouped = scaled_dot_product_attention_gqa(&q, &k, &v, Some(&mask), scale, false, 2)?;
        let repeated = repeated.flatten_all()?.to_vec1::<f32>()?;
        let grouped = grouped.flatten_all()?.to_vec1::<f32>()?;
        assert!(
            repeated
                .iter()
                .zip(grouped)
                .all(|(left, right)| (left - right).abs() < 1e-5)
        );
        Ok(())
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn test_metal_fused_gqa_matches_repeated_kv() -> Result<()> {
        let Ok(device) = Device::new_metal(0) else {
            return Ok(());
        };
        let q = Tensor::randn(0f32, 1f32, (1, 4, 16, 64), &device)?.to_dtype(DType::F16)?;
        let k = Tensor::randn(0f32, 1f32, (1, 2, 16, 64), &device)?.to_dtype(DType::F16)?;
        let v = Tensor::randn(0f32, 1f32, (1, 2, 16, 64), &device)?.to_dtype(DType::F16)?;
        let mask = create_causal_mask(16, 16, DType::F16, &device)?;
        let fused = scaled_dot_product_attention_gqa(&q, &k, &v, Some(&mask), 0.125, false, 2)?;
        let repeated = scaled_dot_product_attention(
            &q,
            &repeat_kv(&k, 2)?.contiguous()?,
            &repeat_kv(&v, 2)?.contiguous()?,
            Some(&mask),
            0.125,
            false,
        )?;
        let max_prefill_error = (fused.to_dtype(DType::F32)? - repeated.to_dtype(DType::F32)?)?
            .abs()?
            .max_all()?
            .to_scalar::<f32>()?;
        assert!(
            max_prefill_error <= 0.01,
            "prefill error {max_prefill_error}"
        );

        let q = Tensor::randn(0f32, 1f32, (1, 4, 1, 64), &device)?.to_dtype(DType::F16)?;
        let k = Tensor::randn(0f32, 1f32, (1, 2, 32, 64), &device)?.to_dtype(DType::F16)?;
        let v = Tensor::randn(0f32, 1f32, (1, 2, 32, 64), &device)?.to_dtype(DType::F16)?;
        let fused = scaled_dot_product_attention_gqa(&q, &k, &v, None, 0.125, false, 2)?;
        let repeated = scaled_dot_product_attention(
            &q,
            &repeat_kv(&k, 2)?.contiguous()?,
            &repeat_kv(&v, 2)?.contiguous()?,
            None,
            0.125,
            false,
        )?;
        let max_decode_error = (fused.to_dtype(DType::F32)? - repeated.to_dtype(DType::F32)?)?
            .abs()?
            .max_all()?
            .to_scalar::<f32>()?;
        assert!(max_decode_error <= 0.01, "decode error {max_decode_error}");

        let q_batched = Tensor::randn(0f32, 1f32, (2, 4, 1, 64), &device)?.to_dtype(DType::F16)?;
        let k_batched = Tensor::randn(0f32, 1f32, (2, 2, 32, 64), &device)?.to_dtype(DType::F16)?;
        let v_batched = Tensor::randn(0f32, 1f32, (2, 2, 32, 64), &device)?.to_dtype(DType::F16)?;
        assert!(
            try_metal_sdpa(&q_batched, &k_batched, &v_batched, None, 0.125, false, true,)?
                .is_some(),
            "maskless batched decode should use Metal's fused vector kernel"
        );
        let fused_batched = scaled_dot_product_attention_gqa(
            &q_batched, &k_batched, &v_batched, None, 0.125, false, 2,
        )?;
        let repeated_batched = scaled_dot_product_attention(
            &q_batched,
            &repeat_kv(&k_batched, 2)?.contiguous()?,
            &repeat_kv(&v_batched, 2)?.contiguous()?,
            None,
            0.125,
            false,
        )?;
        let max_batched_decode_error = (fused_batched.to_dtype(DType::F32)?
            - repeated_batched.to_dtype(DType::F32)?)?
        .abs()?
        .max_all()?
        .to_scalar::<f32>()?;
        assert!(
            max_batched_decode_error <= 0.01,
            "batched decode error {max_batched_decode_error}"
        );

        let q_masked = Tensor::randn(0f32, 1f32, (2, 4, 1, 64), &device)?.to_dtype(DType::F16)?;
        let k_masked = Tensor::randn(0f32, 1f32, (2, 2, 32, 64), &device)?.to_dtype(DType::F16)?;
        let v_masked = Tensor::randn(0f32, 1f32, (2, 2, 32, 64), &device)?.to_dtype(DType::F16)?;
        let mut mask_values = vec![0f32; 64];
        mask_values[..16].fill(f32::NEG_INFINITY);
        mask_values[32..40].fill(f32::NEG_INFINITY);
        let decode_mask =
            Tensor::from_vec(mask_values, (2, 1, 1, 32), &device)?.to_dtype(DType::F16)?;
        assert!(
            try_metal_sdpa(
                &q_masked,
                &k_masked,
                &v_masked,
                Some(&decode_mask),
                0.125,
                false,
                true,
            )?
            .is_none(),
            "masked batched decode must not use Candle's mask-blind vector kernel"
        );
        let masked = scaled_dot_product_attention_gqa(
            &q_masked,
            &k_masked,
            &v_masked,
            Some(&decode_mask),
            0.125,
            false,
            2,
        )?;
        let repeated_masked = scaled_dot_product_attention(
            &q_masked,
            &repeat_kv(&k_masked, 2)?.contiguous()?,
            &repeat_kv(&v_masked, 2)?.contiguous()?,
            Some(&decode_mask),
            0.125,
            false,
        )?;
        let max_masked_error = (masked.to_dtype(DType::F32)?
            - repeated_masked.to_dtype(DType::F32)?)?
        .abs()?
        .max_all()?
        .to_scalar::<f32>()?;
        assert!(
            max_masked_error <= 0.01,
            "masked decode error {max_masked_error}"
        );

        let q = Tensor::randn(0f32, 1f32, (1, 4, 3, 48), &device)?.to_dtype(DType::F16)?;
        let k = Tensor::randn(0f32, 1f32, (1, 2, 3, 48), &device)?.to_dtype(DType::F16)?;
        let v = Tensor::randn(0f32, 1f32, (1, 2, 3, 48), &device)?.to_dtype(DType::F16)?;
        assert!(
            try_metal_sdpa(&q, &k, &v, None, 0.125, false, true)?.is_none(),
            "unsupported fused shapes must fall back instead of failing"
        );
        scaled_dot_product_attention_gqa(&q, &k, &v, None, 0.125, false, 2)?;
        Ok(())
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn test_metal_plain_sdpa_rejects_head_count_mismatch() -> Result<()> {
        let Ok(device) = Device::new_metal(0) else {
            return Ok(());
        };
        // scaled_dot_product_attention is the non-GQA entry point; unlike the
        // GQA wrapper it never validates num_heads against num_kv_heads
        // upstream, so try_metal_sdpa must reject a multiple-but-unequal head
        // count itself instead of silently getting GQA semantics from
        // Candle's fused kernel.
        let q = Tensor::randn(0f32, 1f32, (1, 4, 8, 64), &device)?.to_dtype(DType::F16)?;
        let k = Tensor::randn(0f32, 1f32, (1, 2, 8, 64), &device)?.to_dtype(DType::F16)?;
        let v = Tensor::randn(0f32, 1f32, (1, 2, 8, 64), &device)?.to_dtype(DType::F16)?;
        assert!(
            try_metal_sdpa(&q, &k, &v, None, 0.125, false, false)?.is_none(),
            "plain scaled_dot_product_attention must not dispatch mismatched head counts to the fused kernel"
        );
        assert!(scaled_dot_product_attention(&q, &k, &v, None, 0.125, false).is_err());
        Ok(())
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn test_metal_causal_query_longer_than_kv_matches_eager() -> Result<()> {
        let Ok(device) = Device::new_metal(0) else {
            return Ok(());
        };
        // query_len > kv_len with is_causal=true and no explicit mask: Candle's
        // Metal causal kernel underflows its diagonal offset for this shape, so
        // try_metal_sdpa must fall back to the eager path instead of dispatching.
        let q = Tensor::randn(0f32, 1f32, (1, 2, 8, 64), &device)?.to_dtype(DType::F16)?;
        let k = Tensor::randn(0f32, 1f32, (1, 2, 3, 64), &device)?.to_dtype(DType::F16)?;
        let v = Tensor::randn(0f32, 1f32, (1, 2, 3, 64), &device)?.to_dtype(DType::F16)?;
        assert!(
            try_metal_sdpa(&q, &k, &v, None, 0.125, true, false)?.is_none(),
            "causal query_len > kv_len must fall back to the eager path on Metal"
        );
        // The eager path then rejects the shape (`create_causal_mask` requires
        // right-aligned queries), so Metal refuses just like CPU.
        let err = scaled_dot_product_attention(&q, &k, &v, None, 0.125, true)
            .expect_err("causal query_len > kv_len is not a supported shape");
        assert!(
            err.to_string().contains("right-aligned queries"),
            "expected the causal-mask shape error, got: {err}"
        );
        Ok(())
    }

    #[test]
    fn segmented_gqa_matches_contiguous_cache() -> Result<()> {
        let device = Device::Cpu;
        let q = Tensor::randn(0f32, 1., (1, 4, 3, 8), &device)?;
        let prefix_k = Tensor::randn(0f32, 1., (1, 2, 5, 8), &device)?;
        let prefix_v = Tensor::randn(0f32, 1., (1, 2, 5, 8), &device)?;
        let tail_k = Tensor::randn(0f32, 1., (1, 2, 3, 8), &device)?;
        let tail_v = Tensor::randn(0f32, 1., (1, 2, 3, 8), &device)?;
        let full_k = Tensor::cat(&[&prefix_k, &tail_k], 2)?;
        let full_v = Tensor::cat(&[&prefix_v, &tail_v], 2)?;
        let scale = 1.0 / 8f64.sqrt();
        let contiguous =
            scaled_dot_product_attention_gqa(&q, &full_k, &full_v, None, scale, true, 2)?;
        let segmented = segmented_scaled_dot_product_attention_gqa(
            &q,
            &[(&prefix_k, &prefix_v), (&tail_k, &tail_v)],
            None,
            scale,
            true,
            2,
        )?;
        let contiguous = contiguous.flatten_all()?.to_vec1::<f32>()?;
        let segmented = segmented.flatten_all()?.to_vec1::<f32>()?;
        assert!(
            contiguous
                .iter()
                .zip(segmented)
                .all(|(left, right)| (left - right).abs() < 1e-5)
        );
        Ok(())
    }

    #[test]
    fn segmented_gqa_rejects_mask_that_omits_a_segment() -> Result<()> {
        let device = Device::Cpu;
        let q = Tensor::zeros((1, 4, 3, 8), DType::F32, &device)?;
        let prefix = Tensor::zeros((1, 2, 5, 8), DType::F32, &device)?;
        let tail = Tensor::zeros((1, 2, 3, 8), DType::F32, &device)?;
        let incomplete_mask = Tensor::zeros((1, 1, 3, 3), DType::F32, &device)?;
        let error = segmented_scaled_dot_product_attention_gqa(
            &q,
            &[(&prefix, &prefix), (&tail, &tail)],
            Some(&incomplete_mask),
            1.0 / 8f64.sqrt(),
            false,
            2,
        )
        .unwrap_err();
        assert!(error.to_string().contains("expected 8 across all segments"));
        Ok(())
    }

    #[test]
    fn test_causal_mask() -> Result<()> {
        let device = Device::Cpu;
        let mask = create_causal_mask(4, 4, DType::F32, &device)?;

        assert_eq!(mask.dims(), &[1, 1, 4, 4]);

        // Check mask values
        let mask_data: Vec<f32> = mask.flatten_all()?.to_vec1()?;

        // Row 0: can only attend to position 0
        assert_eq!(mask_data[0], 0.0);
        assert!(mask_data[1].is_infinite() && mask_data[1] < 0.0);

        // Row 3: can attend to all positions
        assert_eq!(mask_data[12], 0.0);
        assert_eq!(mask_data[13], 0.0);
        assert_eq!(mask_data[14], 0.0);
        assert_eq!(mask_data[15], 0.0);

        Ok(())
    }

    #[test]
    fn test_causal_mask_with_kv_cache() -> Result<()> {
        let device = Device::Cpu;

        // Simulating decode step: seq_len=1, kv_len=5 (4 cached + 1 new)
        let mask = create_causal_mask(1, 5, DType::F32, &device)?;
        assert_eq!(mask.dims(), &[1, 1, 1, 5]);

        // Should be able to attend to all positions (including cached)
        let mask_data: Vec<f32> = mask.flatten_all()?.to_vec1()?;
        for &v in &mask_data {
            assert_eq!(v, 0.0);
        }

        Ok(())
    }

    #[test]
    fn causal_mask_rejects_queries_longer_than_kv() {
        let error = create_causal_mask(3, 2, DType::F32, &Device::Cpu).unwrap_err();
        assert!(error.to_string().contains("kv_len (2) >= seq_len (3)"));
    }

    #[test]
    fn test_bf16_causal_mask_preserves_adjacent_positions_in_long_context() -> Result<()> {
        let device = Device::Cpu;
        let query_len = 16;
        let kv_len = 2048;
        let context_len = kv_len - query_len;
        let mask = create_causal_mask(query_len, kv_len, DType::BF16, &device)?
            .to_dtype(DType::F32)?
            .flatten_all()?
            .to_vec1::<f32>()?;

        for row in 0..query_len {
            let start = row * kv_len;
            let last_visible = context_len + row;
            assert_eq!(mask[start + last_visible], 0.0);
            if last_visible + 1 < kv_len {
                assert!(mask[start + last_visible + 1].is_infinite());
                assert!(mask[start + last_visible + 1].is_sign_negative());
            }
        }
        Ok(())
    }

    #[test]
    fn test_repeat_kv() -> Result<()> {
        let device = Device::Cpu;
        let x = Tensor::randn(0f32, 1., (1, 4, 8, 64), &device)?;

        // n_rep = 1, should return same tensor
        let out = repeat_kv(&x, 1)?;
        assert_eq!(out.dims(), &[1, 4, 8, 64]);

        // n_rep = 2, should double heads
        let out = repeat_kv(&x, 2)?;
        assert_eq!(out.dims(), &[1, 8, 8, 64]);

        Ok(())
    }

    #[test]
    fn test_padding_mask() -> Result<()> {
        let device = Device::Cpu;
        let seq_lens = vec![3, 5, 2];
        let max_len = 5;

        let mask = create_padding_mask(&seq_lens, max_len, DType::F32, &device)?;
        assert_eq!(mask.dims(), &[3, 1, 1, 5]);

        let mask_data: Vec<f32> = mask.flatten_all()?.to_vec1()?;

        // Batch 0: len=3, positions 0-2 valid, 3-4 masked
        assert_eq!(mask_data[0], 0.0);
        assert_eq!(mask_data[1], 0.0);
        assert_eq!(mask_data[2], 0.0);
        assert!(mask_data[3].is_infinite());
        assert!(mask_data[4].is_infinite());

        // Batch 1: len=5, all valid
        assert_eq!(mask_data[5], 0.0);
        assert_eq!(mask_data[6], 0.0);
        assert_eq!(mask_data[7], 0.0);
        assert_eq!(mask_data[8], 0.0);
        assert_eq!(mask_data[9], 0.0);

        Ok(())
    }

    #[test]
    fn test_left_padding_mask() -> Result<()> {
        let device = Device::Cpu;
        let seq_lens = vec![3, 5, 2];
        let max_len = 5;

        let mask = create_left_padding_mask(&seq_lens, max_len, DType::F32, &device)?;
        assert_eq!(mask.dims(), &[3, 1, 1, 5]);

        let mask_data: Vec<f32> = mask.flatten_all()?.to_vec1()?;

        // Batch 0: len=3, left-padded by 2. Masked entries stay finite; see
        // `padded_row_prefill_stays_finite`.
        assert!(mask_data[0] < -1e8 && mask_data[0].is_finite());
        assert!(mask_data[1] < -1e8 && mask_data[1].is_finite());
        assert_eq!(mask_data[2], 0.0);
        assert_eq!(mask_data[3], 0.0);
        assert_eq!(mask_data[4], 0.0);

        // Batch 1: len=5, no padding, all valid
        assert_eq!(mask_data[5], 0.0);
        assert_eq!(mask_data[6], 0.0);
        assert_eq!(mask_data[7], 0.0);
        assert_eq!(mask_data[8], 0.0);
        assert_eq!(mask_data[9], 0.0);

        // Batch 2: len=2, left-padded by 3 -> positions 0-2 masked, 3-4 valid
        assert!(mask_data[10] < -1e8 && mask_data[10].is_finite());
        assert!(mask_data[11] < -1e8 && mask_data[11].is_finite());
        assert!(mask_data[12] < -1e8 && mask_data[12].is_finite());
        assert_eq!(mask_data[13], 0.0);
        assert_eq!(mask_data[14], 0.0);

        Ok(())
    }

    #[test]
    fn generation_mask_is_omitted_for_equal_length_batch() -> Result<()> {
        let device = Device::Cpu;
        assert!(create_generation_mask_if_needed(&[0, 0, 0], 32, DType::F32, &device)?.is_none());

        let mask = create_generation_mask_if_needed(&[3, 0, 1], 32, DType::F32, &device)?
            .expect("unequal prompts need a padding mask");
        assert_eq!(mask.dims(), &[3, 1, 1, 32]);
        Ok(())
    }

    /// `decode_position_buffer` must lay positions out axis-major.
    ///
    /// Batch-major transposes the `(axes, batch, 1)` tensor and gives each row
    /// another row's positions — invisible at batch_size 1, where the two
    /// layouts coincide, and wrong for every larger batch. Covers the 3-axis
    /// (PaddleOCR-VL, MinerU2.5) and 4-axis (HunyuanOCR) users.
    #[test]
    fn decode_positions_are_axis_major() -> Result<()> {
        let positions: Vec<i64> = vec![10, 20, 30];
        let batch = positions.len();

        for axes in [3usize, 4] {
            let pos = Tensor::new(decode_position_buffer(&positions, axes), &Device::Cpu)?
                .reshape((axes, batch, 1))?;
            for axis in 0..axes {
                let row: Vec<i64> = pos.i(axis)?.flatten_all()?.to_vec1()?;
                assert_eq!(
                    row, positions,
                    "axes={axes}: axis {axis} has wrong positions"
                );
            }

            // The batch-major layout this replaced is a different tensor.
            let wrong: Vec<i64> = positions
                .iter()
                .flat_map(|&p| std::iter::repeat_n(p, axes))
                .collect();
            let wrong = Tensor::new(wrong, &Device::Cpu)?.reshape((axes, batch, 1))?;
            assert_ne!(
                wrong.i(0)?.flatten_all()?.to_vec1::<i64>()?,
                positions,
                "axes={axes}: batch-major should not accidentally be correct"
            );
        }
        Ok(())
    }

    /// The padding fill must stay finite in every dtype a model may run in.
    ///
    /// F16 caps at 65504, so a fill chosen for wider dtypes saturates to -inf
    /// there and reintroduces the all-masked-row NaN. F16 is reachable via
    /// `OAR_VL_DTYPE=f16` and via `select_dtype`'s fallback on CUDA devices
    /// without BF16 kernels.
    #[test]
    fn padding_mask_is_finite_in_every_dtype() -> Result<()> {
        let device = Device::Cpu;
        for dtype in [DType::F32, DType::BF16, DType::F16] {
            let causal = create_causal_mask(6, 6, dtype, &device)?;
            let padding = create_left_padding_mask(&[6, 4], 6, dtype, &device)?;
            let combined = combine_masks(&causal, &padding)?;

            // Row 1 is padded by 2, so its query 0 can only reach padding keys.
            // That row must keep at least one finite entry or its softmax is NaN.
            let row: Vec<f32> = combined.i((1, 0, 0))?.to_dtype(DType::F32)?.to_vec1()?;
            assert!(
                row.iter().any(|value| value.is_finite()),
                "{dtype:?}: padding-region row is entirely non-finite: {row:?}"
            );

            let probs = candle_nn::ops::softmax_last_dim(&combined.to_dtype(DType::F32)?)?;
            let values: Vec<f32> = probs.flatten_all()?.to_vec1()?;
            assert!(
                values.iter().all(|value| value.is_finite()),
                "{dtype:?}: softmax over the combined mask produced non-finite values"
            );
        }
        Ok(())
    }

    /// A left-padded row must survive prefill with finite values: an -inf
    /// padding fill makes padding-region queries all -inf, their softmax NaN,
    /// and `0 * NaN` smears it across the row. That made batched recognition
    /// decode garbage and never emit EOS.
    #[test]
    fn padded_row_prefill_stays_finite() -> Result<()> {
        let device = Device::Cpu;
        let (seq_lens, max_len) = (vec![6usize, 4], 6usize);
        let (batch, heads, head_dim) = (2usize, 2usize, 8usize);

        let causal = create_causal_mask(max_len, max_len, DType::F32, &device)?;
        let padding = create_left_padding_mask(&seq_lens, max_len, DType::F32, &device)?;
        let mask = combine_masks(&causal, &padding)?;

        let q = Tensor::randn(0f32, 1f32, (batch, heads, max_len, head_dim), &device)?;
        let k = Tensor::randn(0f32, 1f32, (batch, heads, max_len, head_dim), &device)?;
        let v = Tensor::randn(0f32, 1f32, (batch, heads, max_len, head_dim), &device)?;

        let out = scaled_dot_product_attention(&q, &k, &v, Some(&mask), 0.35, false)?;
        let values: Vec<f32> = out.flatten_all()?.to_vec1()?;
        let non_finite = values.iter().filter(|value| !value.is_finite()).count();
        assert_eq!(
            non_finite,
            0,
            "{non_finite}/{} attention outputs were non-finite for a left-padded batch",
            values.len()
        );
        Ok(())
    }

    // RoPE Tests

    #[test]
    fn test_rotary_embedding_dynamic_single_axis() -> std::result::Result<(), Error> {
        let device = Device::Cpu;
        let rope = RotaryEmbedding::new_dynamic(64, 10000.0, &device)?;

        // Create position IDs: (1, batch, seq)
        let position_ids = Tensor::arange(0u32, 8u32, &device)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "Failed to create position_ids",
                    e,
                )
            })?
            .reshape((1, 1, 8))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "Failed to reshape position_ids",
                    e,
                )
            })?;

        let (cos, sin) = rope.forward_multi_axis(&position_ids, DType::F32)?;
        assert_eq!(cos.dims(), &[1, 1, 8, 64]); // (num_dims, batch, seq, head_dim)
        assert_eq!(sin.dims(), &[1, 1, 8, 64]);

        Ok(())
    }

    #[test]
    fn test_rotary_embedding_multi_axis() -> std::result::Result<(), Error> {
        let device = Device::Cpu;
        let rope = RotaryEmbedding::new_multi_axis(128, 10000.0, 3, &device)?;

        // Create 3-axis position IDs: (3, batch, seq)
        let position_ids = Tensor::zeros((3, 2, 16), DType::U32, &device).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "Failed to create position_ids",
                e,
            )
        })?;

        let (cos, sin) = rope.forward_multi_axis(&position_ids, DType::F16)?;
        assert_eq!(cos.dims(), &[3, 2, 16, 128]); // (num_dims, batch, seq, head_dim)
        assert_eq!(sin.dims(), &[3, 2, 16, 128]);
        assert_eq!(cos.dtype(), DType::F16);

        Ok(())
    }

    #[test]
    fn test_rotary_embedding_invalid_head_dim() {
        let device = Device::Cpu;
        // Odd head_dim should fail
        let result = RotaryEmbedding::new_multi_axis(63, 10000.0, 1, &device);
        assert!(result.is_err());
        if let Err(Error::Config { message }) = result {
            assert!(message.contains("must be even"));
        } else {
            panic!("Expected ConfigError");
        }
    }

    #[test]
    fn test_rotary_embedding_wrong_position_ids_shape() -> std::result::Result<(), Error> {
        let device = Device::Cpu;
        let rope = RotaryEmbedding::new_multi_axis(64, 10000.0, 3, &device)?;

        // Wrong shape: (2, batch, seq) instead of (3, batch, seq)
        let position_ids = Tensor::zeros((2, 2, 16), DType::U32, &device).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "Failed to create position_ids",
                e,
            )
        })?;

        let result = rope.forward_multi_axis(&position_ids, DType::F32);
        assert!(result.is_err());
        if let Err(Error::InvalidInput { message }) = result {
            assert!(message.contains("expected position_ids shape (3"));
        } else {
            panic!("Expected InvalidInput error");
        }

        Ok(())
    }
}
