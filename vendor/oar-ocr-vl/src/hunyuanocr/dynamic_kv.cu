#include <cuda_fp16.h>
#include <cuda_bf16.h>
#include <math_constants.h>
#include <stdint.h>

// Copy one fixed-size verification block into a preallocated KV cache.  The
// destination offset is read from a device-side cumulative-length tensor, so
// the same kernel node can be replayed inside a CUDA graph for every decode
// round without patching graph parameters on the host.
extern "C" __global__ void append_kv_f16(
    half* cache,
    const half* source,
    const uint32_t* cumulative_lengths,
    uint32_t query_len,
    uint32_t num_heads,
    uint32_t head_dim,
    uint32_t cache_len) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = num_heads * query_len * head_dim;
  if (index >= count) {
    return;
  }

  const uint32_t head_stride = query_len * head_dim;
  const uint32_t head = index / head_stride;
  const uint32_t within_head = index - head * head_stride;
  const uint32_t token = within_head / head_dim;
  const uint32_t lane = within_head - token * head_dim;
  const uint32_t end = cumulative_lengths[1];
  const uint32_t start = end - query_len;
  cache[(head * cache_len + start + token) * head_dim + lane] = source[index];
}

extern "C" __global__ void append_kv_bf16(
    __nv_bfloat16* cache,
    const __nv_bfloat16* source,
    const uint32_t* cumulative_lengths,
    uint32_t query_len,
    uint32_t num_heads,
    uint32_t head_dim,
    uint32_t cache_len) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = num_heads * query_len * head_dim;
  if (index >= count) {
    return;
  }

  const uint32_t head_stride = query_len * head_dim;
  const uint32_t head = index / head_stride;
  const uint32_t within_head = index - head * head_stride;
  const uint32_t token = within_head / head_dim;
  const uint32_t lane = within_head - token * head_dim;
  const uint32_t end = cumulative_lengths[1];
  const uint32_t start = end - query_len;
  cache[(head * cache_len + start + token) * head_dim + lane] = source[index];
}

// Write head-major [head, query, lane] data into the token-major physical
// layout consumed by FlashAttention's paged split-KV kernel. Blocks are stored
// in identity order, so the flattened destination is [token, head, lane].
extern "C" __global__ void append_paged_kv_bf16(
    __nv_bfloat16* cache,
    const __nv_bfloat16* source,
    const uint32_t* cumulative_lengths,
    uint32_t query_len,
    uint32_t num_heads,
    uint32_t head_dim,
    uint32_t cache_len) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = num_heads * query_len * head_dim;
  if (index >= count) {
    return;
  }

  const uint32_t head_stride = query_len * head_dim;
  const uint32_t head = index / head_stride;
  const uint32_t within_head = index - head * head_stride;
  const uint32_t token = within_head / head_dim;
  const uint32_t lane = within_head - token * head_dim;
  const uint32_t end = cumulative_lengths[1];
  const uint32_t start = end - query_len;
  const uint32_t destination_token = start + token;
  if (destination_token < cache_len) {
    cache[(destination_token * num_heads + head) * head_dim + lane] = source[index];
  }
}

// Match Candle's separate BF16 SiLU and multiply kernels, including the BF16
// rounding after every SiLU arithmetic operation and before multiplication.
extern "C" __global__ void silu_mul_bf16(
    const __nv_bfloat16* gate,
    const __nv_bfloat16* up,
    __nv_bfloat16* output,
    uint32_t count,
    uint32_t ncols,
    uint32_t gate_row_stride,
    uint32_t up_row_stride) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  if (index >= count) {
    return;
  }
  const uint32_t row = index / ncols;
  const uint32_t col = index - row * ncols;
  const uint32_t gate_index = row * gate_row_stride + col;
  const uint32_t up_index = row * up_row_stride + col;
  const __nv_bfloat16 one = __float2bfloat16_rn(1.0f);
  const __nv_bfloat16 gate_value = gate[gate_index];
  const __nv_bfloat16 negative = -gate_value;
  const __nv_bfloat16 exponential = hexp(negative);
  const __nv_bfloat16 denominator = one + exponential;
  const __nv_bfloat16 activated = gate_value / denominator;
  output[index] = activated * up[up_index];
}

// Apply HuggingFace's repetition-penalty rule sparsely. token_ids contains one
// deduplicated row per logits row; UINT32_MAX pads rows to a common width. The
// logits are F32 so this has the same rounding points as the previous CPU
// implementation while avoiding a full-vocabulary device-to-host copy.
extern "C" __global__ void repetition_penalty_f32(
    float* logits,
    const uint32_t* token_ids,
    uint32_t token_stride,
    uint32_t token_rows,
    uint32_t rows,
    uint32_t vocab_size,
    float penalty) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = rows * token_stride;
  if (index >= count) {
    return;
  }
  const uint32_t row = index / token_stride;
  const uint32_t col = index - row * token_stride;
  const uint32_t token_row = token_rows == 1 ? 0 : row;
  const uint32_t token = token_ids[token_row * token_stride + col];
  if (token >= vocab_size) {
    return;
  }
  const uint32_t offset = row * vocab_size + token;
  const float value = logits[offset];
  logits[offset] = value > 0.0f ? __fdiv_rn(value, penalty)
                                : __fmul_rn(value, penalty);
}

// Maintain a device-resident presence map for generated tokens. Repetition
// penalty is set-based, so duplicate ids may race while writing the same byte
// value without changing the result.
extern "C" __global__ void mark_repetition_history_u8(
    uint8_t* history,
    const uint32_t* token_ids,
    uint32_t token_count,
    uint32_t vocab_size) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  if (index >= token_count) {
    return;
  }
  const uint32_t token = token_ids[index];
  if (token < vocab_size) {
    history[token] = 1;
  }
}

// Apply a common sparse banned-token list to every logits row. This is used
// by exact greedy processors such as no-repeat-ngram without copying the full
// vocabulary to the host.
extern "C" __global__ void mask_token_ids_bf16(
    __nv_bfloat16* logits,
    const uint32_t* token_ids,
    uint32_t rows,
    uint32_t vocab_size,
    uint32_t token_count) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = rows * token_count;
  if (index >= count) {
    return;
  }
  const uint32_t row = index / token_count;
  const uint32_t token = token_ids[index - row * token_count];
  if (token < vocab_size) {
    logits[row * vocab_size + token] = __float2bfloat16(-CUDART_INF_F);
  }
}

extern "C" __global__ void mask_token_ids_f32(
    float* logits,
    const uint32_t* token_ids,
    uint32_t rows,
    uint32_t vocab_size,
    uint32_t token_count) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = rows * token_count;
  if (index >= count) {
    return;
  }
  const uint32_t row = index / token_count;
  const uint32_t token = token_ids[index - row * token_count];
  if (token < vocab_size) {
    logits[row * vocab_size + token] = -CUDART_INF_F;
  }
}

extern "C" __global__ void argmax_first_bf16_stage1(
    const __nv_bfloat16* logits,
    float* partial_values,
    uint32_t* partial_indices,
    uint32_t rows,
    uint32_t vocab_size,
    uint32_t partitions_per_row) {
  const uint32_t row = blockIdx.x / partitions_per_row;
  const uint32_t partition = blockIdx.x - row * partitions_per_row;
  const uint32_t lane = threadIdx.x;
  if (row >= rows) {
    return;
  }

  float best_value = -CUDART_INF_F;
  uint32_t best_index = UINT32_MAX;
  const uint32_t global_lane = partition * blockDim.x + lane;
  const uint32_t global_stride = partitions_per_row * blockDim.x;
  for (uint32_t col = global_lane; col < vocab_size; col += global_stride) {
    const float value = __bfloat162float(logits[row * vocab_size + col]);
    if (value > best_value || (value == best_value && col < best_index)) {
      best_value = value;
      best_index = col;
    }
  }

  __shared__ float values[256];
  __shared__ uint32_t indices[256];
  values[lane] = best_value;
  indices[lane] = best_index;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      const float other_value = values[lane + width];
      const uint32_t other_index = indices[lane + width];
      if (other_value > values[lane] ||
          (other_value == values[lane] && other_index < indices[lane])) {
        values[lane] = other_value;
        indices[lane] = other_index;
      }
    }
  }
  if (lane == 0) {
    partial_values[blockIdx.x] = values[0];
    partial_indices[blockIdx.x] = indices[0];
  }
}

// Fuse BF16-to-F32 conversion, set-based repetition penalty, and stable
// argmax for autoregressive sampling.
extern "C" __global__ void repetition_argmax_bf16_stage1(
    const __nv_bfloat16* logits,
    const uint8_t* history,
    float* partial_values,
    uint32_t* partial_indices,
    uint32_t rows,
    uint32_t vocab_size,
    uint32_t partitions_per_row,
    float penalty) {
  const uint32_t row = blockIdx.x / partitions_per_row;
  const uint32_t partition = blockIdx.x - row * partitions_per_row;
  const uint32_t lane = threadIdx.x;
  if (row >= rows) {
    return;
  }

  float best_value = -CUDART_INF_F;
  uint32_t best_index = UINT32_MAX;
  const uint32_t global_lane = partition * blockDim.x + lane;
  const uint32_t global_stride = partitions_per_row * blockDim.x;
  for (uint32_t col = global_lane; col < vocab_size; col += global_stride) {
    float value = __bfloat162float(logits[row * vocab_size + col]);
    if (history[row * vocab_size + col] != 0) {
      value = value > 0.0f ? __fdiv_rn(value, penalty)
                           : __fmul_rn(value, penalty);
    }
    if (value > best_value || (value == best_value && col < best_index)) {
      best_value = value;
      best_index = col;
    }
  }

  __shared__ float values[256];
  __shared__ uint32_t indices[256];
  values[lane] = best_value;
  indices[lane] = best_index;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      const float other_value = values[lane + width];
      const uint32_t other_index = indices[lane + width];
      if (other_value > values[lane] ||
          (other_value == values[lane] && other_index < indices[lane])) {
        values[lane] = other_value;
        indices[lane] = other_index;
      }
    }
  }
  if (lane == 0) {
    partial_values[blockIdx.x] = values[0];
    partial_indices[blockIdx.x] = indices[0];
  }
}

// DFlash row r sees the common generated history plus proposals [0, r);
// equal maxima keep the lowest vocabulary id.
extern "C" __global__ void dflash_repetition_argmax_bf16_stage1(
    const __nv_bfloat16* logits,
    const uint8_t* history,
    const uint32_t* proposals,
    float* partial_values,
    uint32_t* partial_indices,
    uint32_t proposal_count,
    uint32_t rows,
    uint32_t vocab_size,
    uint32_t partitions_per_row,
    float penalty) {
  const uint32_t row = blockIdx.x / partitions_per_row;
  const uint32_t partition = blockIdx.x - row * partitions_per_row;
  const uint32_t lane = threadIdx.x;
  if (row >= rows) {
    return;
  }

  extern __shared__ uint32_t proposal_ids[];
  for (uint32_t i = lane; i < proposal_count; i += blockDim.x) {
    proposal_ids[i] = proposals[i];
  }
  __syncthreads();

  const uint32_t prefix_len = row < proposal_count ? row : proposal_count;
  float best_value = -CUDART_INF_F;
  uint32_t best_index = UINT32_MAX;
  const uint32_t global_lane = partition * blockDim.x + lane;
  const uint32_t global_stride = partitions_per_row * blockDim.x;
  for (uint32_t col = global_lane; col < vocab_size; col += global_stride) {
    bool seen = history[col] != 0;
    for (uint32_t i = 0; !seen && i < prefix_len; ++i) {
      seen = proposal_ids[i] == col;
    }
    float value = __bfloat162float(logits[row * vocab_size + col]);
    if (seen) {
      value = value > 0.0f ? __fdiv_rn(value, penalty)
                           : __fmul_rn(value, penalty);
    }
    if (value > best_value || (value == best_value && col < best_index)) {
      best_value = value;
      best_index = col;
    }
  }

  __shared__ float values[256];
  __shared__ uint32_t indices[256];
  values[lane] = best_value;
  indices[lane] = best_index;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      const float other_value = values[lane + width];
      const uint32_t other_index = indices[lane + width];
      if (other_value > values[lane] ||
          (other_value == values[lane] && other_index < indices[lane])) {
        values[lane] = other_value;
        indices[lane] = other_index;
      }
    }
  }
  if (lane == 0) {
    partial_values[blockIdx.x] = values[0];
    partial_indices[blockIdx.x] = indices[0];
  }
}

extern "C" __global__ void dflash_repetition_argmax_stage2(
    const float* partial_values,
    const uint32_t* partial_indices,
    uint32_t* output,
    uint32_t partitions_per_row,
    uint32_t rows) {
  const uint32_t row = blockIdx.x;
  const uint32_t lane = threadIdx.x;
  if (row >= rows) {
    return;
  }
  float best_value = -CUDART_INF_F;
  uint32_t best_index = UINT32_MAX;
  for (uint32_t partition = lane; partition < partitions_per_row;
       partition += blockDim.x) {
    const float value =
        partial_values[row * partitions_per_row + partition];
    const uint32_t index =
        partial_indices[row * partitions_per_row + partition];
    if (value > best_value ||
        (value == best_value && index < best_index)) {
      best_value = value;
      best_index = index;
    }
  }
  __shared__ float values[32];
  __shared__ uint32_t indices[32];
  values[lane] = best_value;
  indices[lane] = best_index;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      const float other_value = values[lane + width];
      const uint32_t other_index = indices[lane + width];
      if (other_value > values[lane] ||
          (other_value == values[lane] && other_index < indices[lane])) {
        values[lane] = other_value;
        indices[lane] = other_index;
      }
    }
  }
  if (lane == 0) {
    output[row] = indices[0] == UINT32_MAX ? 0 : indices[0];
  }
}

// Candle's parallel argmax does not preserve the lowest vocabulary index when
// equal maxima land in different reduction lanes. BF16 logits produce real
// ties, while the previous CPU loop deliberately kept the first index. Reduce
// (value, index) pairs so GPU repetition penalty remains token-exact.
extern "C" __global__ void argmax_first_f32(
    const float* logits,
    uint32_t* output,
    uint32_t rows,
    uint32_t vocab_size) {
  const uint32_t row = blockIdx.x;
  const uint32_t lane = threadIdx.x;
  if (row >= rows) {
    return;
  }

  float best_value = -CUDART_INF_F;
  uint32_t best_index = UINT32_MAX;
  for (uint32_t col = lane; col < vocab_size; col += blockDim.x) {
    const float value = logits[row * vocab_size + col];
    if (value > best_value || (value == best_value && col < best_index)) {
      best_value = value;
      best_index = col;
    }
  }

  __shared__ float values[256];
  __shared__ uint32_t indices[256];
  values[lane] = best_value;
  indices[lane] = best_index;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      const float other_value = values[lane + width];
      const uint32_t other_index = indices[lane + width];
      if (other_value > values[lane] ||
          (other_value == values[lane] && other_index < indices[lane])) {
        values[lane] = other_value;
        indices[lane] = other_index;
      }
    }
  }
  if (lane == 0) {
    output[row] = indices[0] == UINT32_MAX ? 0 : indices[0];
  }
}

// Projected QKV is laid out [token, q+k+v]. Produce the contiguous
// [head, token, lane] Q or K tensor while applying XDRoPE with the exact
// upstream rounding points: BF16 -> F32, two RN multiplies, one RN add, then
// F32 -> BF16. This replaces the generic cast/mul/neg/cat/add/cast sequence.
extern "C" __global__ void xdrope_bf16(
    __nv_bfloat16* output,
    const __nv_bfloat16* qkv,
    const float* cos_sin,
    uint32_t projection_width,
    uint32_t projection_offset,
    uint32_t num_heads,
    uint32_t query_len,
    uint32_t head_dim) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = num_heads * query_len * head_dim;
  if (index >= count) {
    return;
  }

  const uint32_t head_stride = query_len * head_dim;
  const uint32_t head = index / head_stride;
  const uint32_t within_head = index - head * head_stride;
  const uint32_t token = within_head / head_dim;
  const uint32_t lane = within_head - token * head_dim;
  const uint32_t half_dim = head_dim / 2;
  const uint32_t partner_lane =
      lane < half_dim ? lane + half_dim : lane - half_dim;
  const uint32_t source_base =
      token * projection_width + projection_offset + head * head_dim;
  const float x = __bfloat162float(qkv[source_base + lane]);
  float rotated = __bfloat162float(qkv[source_base + partner_lane]);
  if (lane < half_dim) {
    rotated = -rotated;
  }
  const uint32_t rope_index = token * head_dim + lane;
  const uint32_t rope_elements = query_len * head_dim;
  const float direct = __fmul_rn(x, cos_sin[rope_index]);
  const float cross =
      __fmul_rn(rotated, cos_sin[rope_elements + rope_index]);
  output[index] = __float2bfloat16_rn(__fadd_rn(direct, cross));
}

static __device__ __forceinline__ float warp_sum(float value) {
#pragma unroll
  for (int mask = 16; mask > 0; mask >>= 1) {
    value += __shfl_xor_sync(0xffffffff, value, mask, 32);
  }
  return value;
}

// Fuse the target model's XDRoPE, per-head RMSNorm, and BF16-to-F16
// conversion. Candle uses one 32-thread warp for a 128-wide RMSNorm, with
// each lane accumulating four columns; duplicate that reduction order so the
// verification logits remain bit-for-bit unchanged. For K, optionally emit V
// beside it to also remove the generic slice/transpose/copy/cast chain.
extern "C" __global__ void xdrope_rmsnorm_f16(
    const __nv_bfloat16* qkv,
    const float* cos_sin,
    const __nv_bfloat16* weight,
    half* output,
    uint32_t projection_width,
    uint32_t projection_offset,
    uint32_t num_heads,
    uint32_t query_len,
    uint32_t head_dim,
    float eps,
    uint32_t include_v) {
  const uint32_t row = blockIdx.x;
  const uint32_t lane_id = threadIdx.x;
  const uint32_t head = row / query_len;
  const uint32_t token = row - head * query_len;
  const uint32_t half_dim = head_dim / 2;
  const uint32_t source_base =
      token * projection_width + projection_offset + head * head_dim;
  const uint32_t rope_elements = query_len * head_dim;
  __nv_bfloat16 values[4];
  float sum = 0.0f;

#pragma unroll
  for (uint32_t item = 0; item < 4; ++item) {
    const uint32_t col = lane_id + item * 32;
    const uint32_t partner_col =
        col < half_dim ? col + half_dim : col - half_dim;
    const float x = __bfloat162float(qkv[source_base + col]);
    float partner = __bfloat162float(qkv[source_base + partner_col]);
    if (col < half_dim) {
      partner = -partner;
    }
    const uint32_t rope_index = token * head_dim + col;
    const float direct = __fmul_rn(x, cos_sin[rope_index]);
    const float cross =
        __fmul_rn(partner, cos_sin[rope_elements + rope_index]);
    values[item] = __float2bfloat16_rn(__fadd_rn(direct, cross));
    const float value = __bfloat162float(values[item]);
    sum += value * value;
  }
  sum = warp_sum(sum);
  const float scale = rsqrtf(sum / static_cast<float>(head_dim) + eps);
  const uint32_t output_base = row * head_dim;

#pragma unroll
  for (uint32_t item = 0; item < 4; ++item) {
    const uint32_t col = lane_id + item * 32;
    const float normalized = scale * __bfloat162float(values[item]) *
        __bfloat162float(weight[col]);
    const __nv_bfloat16 rounded = __float2bfloat16_rn(normalized);
    output[output_base + col] = __float2half_rn(__bfloat162float(rounded));
    if (include_v) {
      const uint32_t count = num_heads * query_len * head_dim;
      const uint32_t v_offset = projection_offset + num_heads * head_dim;
      output[count + output_base + col] = __float2half_rn(__bfloat162float(
          qkv[token * projection_width + v_offset + head * head_dim + col]));
    }
  }
}

// Draft attention normalizes Q/K before RoPE. Fuse that ordering while
// matching Candle's 32-thread/128-column RMSNorm and the three BF16 rounding
// points in the existing RoPE path. K can emit V in the same transposed
// [head, token, lane] layout.
extern "C" __global__ void rmsnorm_rope_bf16(
    const __nv_bfloat16* qkv,
    const __nv_bfloat16* cos_sin,
    const __nv_bfloat16* weight,
    __nv_bfloat16* output,
    uint32_t projection_width,
    uint32_t projection_offset,
    uint32_t num_heads,
    uint32_t query_len,
    uint32_t head_dim,
    float eps,
    uint32_t include_v) {
  const uint32_t row = blockIdx.x;
  const uint32_t lane_id = threadIdx.x;
  const uint32_t head = row / query_len;
  const uint32_t token = row - head * query_len;
  const uint32_t source_base =
      token * projection_width + projection_offset + head * head_dim;
  __nv_bfloat16 normalized[4];
  float sum = 0.0f;

#pragma unroll
  for (uint32_t item = 0; item < 4; ++item) {
    const uint32_t col = lane_id + item * 32;
    const float value = __bfloat162float(qkv[source_base + col]);
    sum += value * value;
  }
  sum = warp_sum(sum);
  const float scale = rsqrtf(sum / static_cast<float>(head_dim) + eps);

#pragma unroll
  for (uint32_t item = 0; item < 4; ++item) {
    const uint32_t col = lane_id + item * 32;
    normalized[item] = __float2bfloat16_rn(
        scale * __bfloat162float(qkv[source_base + col]) *
        __bfloat162float(weight[col]));
  }

  const uint32_t output_base = row * head_dim;
  const uint32_t rope_elements = query_len * head_dim;
#pragma unroll
  for (uint32_t item = 0; item < 4; ++item) {
    const uint32_t col = lane_id + item * 32;
    const uint32_t partner_item = col < head_dim / 2 ? item + 2 : item - 2;
    float partner = __bfloat162float(normalized[partner_item]);
    if (col < head_dim / 2) {
      partner = -partner;
    }
    const uint32_t rope_index = token * head_dim + col;
    const __nv_bfloat16 direct = __float2bfloat16_rn(
        __bfloat162float(normalized[item]) *
        __bfloat162float(cos_sin[rope_index]));
    const __nv_bfloat16 cross = __float2bfloat16_rn(
        partner * __bfloat162float(cos_sin[rope_elements + rope_index]));
    output[output_base + col] = __float2bfloat16_rn(
        __bfloat162float(direct) + __bfloat162float(cross));
    if (include_v) {
      const uint32_t count = num_heads * query_len * head_dim;
      const uint32_t v_offset = projection_offset + num_heads * head_dim;
      output[count + output_base + col] =
          qkv[token * projection_width + v_offset + head * head_dim + col];
    }
  }
}

// DFlash applies RoPE directly in BF16. Preserve all three BF16 rounding
// points (both products and their sum) while replacing the generic operation
// chain with one kernel.
extern "C" __global__ void rope_bf16(
    __nv_bfloat16* output,
    const __nv_bfloat16* input,
    const __nv_bfloat16* cos_sin,
    uint32_t num_heads,
    uint32_t query_len,
    uint32_t head_dim) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  const uint32_t count = num_heads * query_len * head_dim;
  if (index >= count) {
    return;
  }

  const uint32_t head_stride = query_len * head_dim;
  const uint32_t within_head = index % head_stride;
  const uint32_t token = within_head / head_dim;
  const uint32_t lane = within_head - token * head_dim;
  const uint32_t half_dim = head_dim / 2;
  const uint32_t partner =
      lane < half_dim ? index + half_dim : index - half_dim;
  float rotated = __bfloat162float(input[partner]);
  if (lane < half_dim) {
    rotated = -rotated;
  }
  const uint32_t rope_index = token * head_dim + lane;
  const uint32_t rope_elements = query_len * head_dim;
  const float x = __bfloat162float(input[index]);
  const __nv_bfloat16 direct = __float2bfloat16_rn(
      __fmul_rn(x, __bfloat162float(cos_sin[rope_index])));
  const __nv_bfloat16 cross = __float2bfloat16_rn(__fmul_rn(
      rotated, __bfloat162float(cos_sin[rope_elements + rope_index])));
  output[index] = __float2bfloat16_rn(__fadd_rn(
      __bfloat162float(direct), __bfloat162float(cross)));
}

// Match Candle's BF16 badd followed by its F32-accumulating RMSNorm, but write
// both the rounded residual and normalized value in one pass. Output is
// [2, rows, cols]: residual first, normalized second.
extern "C" __global__ void add_rmsnorm_bf16(
    const __nv_bfloat16* input,
    const __nv_bfloat16* delta,
    const __nv_bfloat16* weight,
    __nv_bfloat16* output,
    uint32_t ncols,
    float eps) {
  const uint32_t row = blockIdx.x;
  const uint32_t col = threadIdx.x;
  const uint32_t row_offset = row * ncols;
  const uint32_t element_count = gridDim.x * ncols;

  const __nv_bfloat16 residual = __float2bfloat16_rn(__fadd_rn(
      __bfloat162float(input[row_offset + col]),
      __bfloat162float(delta[row_offset + col])));
  output[row_offset + col] = residual;
  const float value = __bfloat162float(residual);
  float sum = value * value;
  sum = warp_sum(sum);

  __shared__ float warp_sums[32];
  const uint32_t warp = col / 32;
  const uint32_t lane = col % 32;
  if (lane == 0) {
    warp_sums[warp] = sum;
  }
  __syncthreads();
  sum = warp_sums[lane];
  sum = warp_sum(sum);
  const float scale = rsqrtf(sum / static_cast<float>(ncols) + eps);
  output[element_count + row_offset + col] = __float2bfloat16_rn(
      scale * value * __bfloat162float(weight[col]));
}

template <typename T>
static __device__ __forceinline__ float sampling_logit(const T* logits,
                                                        uint32_t index);

template <>
__device__ __forceinline__ float sampling_logit<float>(const float* logits,
                                                        uint32_t index) {
  return logits[index];
}

template <>
__device__ __forceinline__ float sampling_logit<__nv_bfloat16>(
    const __nv_bfloat16* logits,
    uint32_t index) {
  return __bfloat162float(logits[index]);
}

// Select one token per logits row and return both the token id and its softmax
// probability. The output is F32 [rows, 2]; vocabulary ids below 2^24 are
// exactly representable, and MinerU's vocabulary is much smaller than that.
//
// Sampling uses a caller-provided U[0, 1) variate. This keeps RNG ownership on
// the host (and therefore preserves seeded/reproducible generation) while the
// O(rows*vocab) softmax/CDF work stays on the GPU. The scan is tiled so all 256
// lanes evaluate exponentials in parallel instead of making one lane walk the
// full vocabulary.
template <typename T>
static __device__ void sample_with_confidence_impl(
    const T* logits,
    const float* uniforms,
    float* output,
    uint32_t rows,
    uint32_t vocab_size,
    float inv_temperature,
    uint32_t greedy,
    float* scratch_values,
    uint32_t* scratch_indices) {
  const uint32_t row = blockIdx.x;
  const uint32_t lane = threadIdx.x;
  if (row >= rows) {
    return;
  }

  const uint32_t row_offset = row * vocab_size;
  float best_value = -CUDART_INF_F;
  uint32_t best_index = UINT32_MAX;
  for (uint32_t col = lane; col < vocab_size; col += blockDim.x) {
    const float value = sampling_logit(logits, row_offset + col) *
                        inv_temperature;
    if (value > best_value || (value == best_value && col < best_index)) {
      best_value = value;
      best_index = col;
    }
  }

  scratch_values[lane] = best_value;
  scratch_indices[lane] = best_index;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      const float other_value = scratch_values[lane + width];
      const uint32_t other_index = scratch_indices[lane + width];
      if (other_value > scratch_values[lane] ||
          (other_value == scratch_values[lane] &&
           other_index < scratch_indices[lane])) {
        scratch_values[lane] = other_value;
        scratch_indices[lane] = other_index;
      }
    }
  }
  __syncthreads();
  const float max_value = scratch_values[0];
  const uint32_t argmax =
      scratch_indices[0] == UINT32_MAX ? 0 : scratch_indices[0];

  float local_sum = 0.0f;
  for (uint32_t col = lane; col < vocab_size; col += blockDim.x) {
    const float value = sampling_logit(logits, row_offset + col) *
                        inv_temperature;
    local_sum += expf(value - max_value);
  }
  scratch_values[lane] = local_sum;
  for (uint32_t width = blockDim.x / 2; width > 0; width >>= 1) {
    __syncthreads();
    if (lane < width) {
      scratch_values[lane] += scratch_values[lane + width];
    }
  }
  __syncthreads();
  const float sum = scratch_values[0];

  uint32_t selected = argmax;
  if (!greedy && isfinite(sum) && sum > 0.0f) {
    const float threshold = uniforms[row] * sum;
    float cumulative = 0.0f;
    for (uint32_t base = 0; base < vocab_size; base += blockDim.x) {
      const uint32_t col = base + lane;
      const float weight = col < vocab_size
          ? expf(sampling_logit(logits, row_offset + col) * inv_temperature -
                 max_value)
          : 0.0f;
      scratch_values[lane] = weight;
      __syncthreads();

      // Inclusive scan of this 256-token tile.
      for (uint32_t offset = 1; offset < blockDim.x; offset <<= 1) {
        const float add = lane >= offset ? scratch_values[lane - offset] : 0.0f;
        __syncthreads();
        if (lane >= offset) {
          scratch_values[lane] += add;
        }
        __syncthreads();
      }

      if (lane == 0) {
        scratch_indices[0] = UINT32_MAX;
      }
      __syncthreads();
      if (col < vocab_size && cumulative + scratch_values[lane] > threshold) {
        atomicMin(scratch_indices, col);
      }
      __syncthreads();
      const uint32_t crossing = scratch_indices[0];
      const float tile_sum = scratch_values[blockDim.x - 1];
      if (crossing != UINT32_MAX) {
        selected = crossing;
        break;
      }
      cumulative += tile_sum;
      __syncthreads();
    }
  }

  if (lane == 0) {
    const float selected_value =
        sampling_logit(logits, row_offset + selected) * inv_temperature;
    const float confidence = isfinite(sum) && sum > 0.0f
        ? expf(selected_value - max_value) / sum
        : 1.0f;
    output[row * 2] = static_cast<float>(selected);
    output[row * 2 + 1] = confidence;
  }
}

extern "C" __global__ void sample_with_confidence_f32(
    const float* logits,
    const float* uniforms,
    float* output,
    uint32_t rows,
    uint32_t vocab_size,
    float inv_temperature,
    uint32_t greedy) {
  __shared__ float scratch_values[256];
  __shared__ uint32_t scratch_indices[256];
  sample_with_confidence_impl(logits, uniforms, output, rows, vocab_size,
                              inv_temperature, greedy, scratch_values,
                              scratch_indices);
}

extern "C" __global__ void sample_with_confidence_bf16(
    const __nv_bfloat16* logits,
    const float* uniforms,
    float* output,
    uint32_t rows,
    uint32_t vocab_size,
    float inv_temperature,
    uint32_t greedy) {
  __shared__ float scratch_values[256];
  __shared__ uint32_t scratch_indices[256];
  sample_with_confidence_impl(logits, uniforms, output, rows, vocab_size,
                              inv_temperature, greedy, scratch_values,
                              scratch_indices);
}
