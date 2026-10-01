#include <cuda_bf16.h>
#include <cuda_fp16.h>
#include <math.h>
#include <stdint.h>

namespace {

__device__ __forceinline__ float ovis_gd_to_float(float value) { return value; }

__device__ __forceinline__ float ovis_gd_to_float(half value) {
  return __half2float(value);
}

__device__ __forceinline__ float ovis_gd_to_float(__nv_bfloat16 value) {
  return __bfloat162float(value);
}

// One block owns one (batch, head) recurrent state. The state is kept in the
// F32 tail of the packed output buffer so it is both the working buffer and the
// returned final state. Q/K/V may be BF16, F16, or F32; all recurrence math is
// deliberately F32, matching the Qwen3.5 reference fallback.
template <typename T>
__device__ __forceinline__ void ovis_gated_delta_rule_body(
    const T* qkv,
    const float* gate_beta,
    const float* initial_state,
    float* packed_output,
    uint32_t batch_size,
    uint32_t sequence_length,
    uint32_t num_heads,
    uint32_t head_dim,
    float* q_reduction,
    float* k_reduction,
    float* delta) {
  const uint32_t batch_head = blockIdx.x;
  const uint32_t batch = batch_head / num_heads;
  const uint32_t head = batch_head - batch * num_heads;
  if (batch >= batch_size) {
    return;
  }

  const uint32_t lane = threadIdx.x;
  const uint64_t state_size = static_cast<uint64_t>(head_dim) * head_dim;
  const uint64_t core_count =
      static_cast<uint64_t>(batch_size) * sequence_length * num_heads * head_dim;
  const uint64_t state_offset = static_cast<uint64_t>(batch_head) * state_size;
  float* state = packed_output + core_count + state_offset;
  for (uint64_t index = lane; index < state_size; index += blockDim.x) {
    state[index] = initial_state[state_offset + index];
  }
  __syncthreads();

  const float q_scale = rsqrtf(static_cast<float>(head_dim));
  for (uint32_t token = 0; token < sequence_length; ++token) {
    const uint64_t token_head =
        (static_cast<uint64_t>(batch) * sequence_length + token) * num_heads + head;
    const uint64_t qkv_offset = token_head * (3u * head_dim);
    float q_sum = 0.0f;
    float k_sum = 0.0f;
    for (uint32_t dim = lane; dim < head_dim; dim += blockDim.x) {
      const float q_value = ovis_gd_to_float(qkv[qkv_offset + dim]);
      const float k_value = ovis_gd_to_float(qkv[qkv_offset + head_dim + dim]);
      q_sum += q_value * q_value;
      k_sum += k_value * k_value;
    }
    q_reduction[lane] = q_sum;
    k_reduction[lane] = k_sum;
    for (uint32_t stride = blockDim.x >> 1; stride > 0; stride >>= 1) {
      __syncthreads();
      if (lane < stride) {
        q_reduction[lane] += q_reduction[lane + stride];
        k_reduction[lane] += k_reduction[lane + stride];
      }
    }
    __syncthreads();
    const float q_inv_norm = rsqrtf(q_reduction[0] + 1.0e-6f) * q_scale;
    const float k_inv_norm = rsqrtf(k_reduction[0] + 1.0e-6f);
    const uint64_t gate_offset = token_head * 2u;
    const float decay = expf(gate_beta[gate_offset]);
    const float beta = gate_beta[gate_offset + 1u];

    for (uint64_t index = lane; index < state_size; index += blockDim.x) {
      state[index] *= decay;
    }
    __syncthreads();

    for (uint32_t value_dim = lane; value_dim < head_dim;
         value_dim += blockDim.x) {
      float memory = 0.0f;
      for (uint32_t key_dim = 0; key_dim < head_dim; ++key_dim) {
        const float key =
            ovis_gd_to_float(qkv[qkv_offset + head_dim + key_dim]) * k_inv_norm;
        memory += state[static_cast<uint64_t>(key_dim) * head_dim + value_dim] * key;
      }
      const float value =
          ovis_gd_to_float(qkv[qkv_offset + 2u * head_dim + value_dim]);
      delta[value_dim] = (value - memory) * beta;
    }
    __syncthreads();

    // Traverse the state matrix directly instead of recovering its row and
    // column with integer division for every element.
    for (uint32_t key_dim = 0; key_dim < head_dim; ++key_dim) {
      const float key =
          ovis_gd_to_float(qkv[qkv_offset + head_dim + key_dim]) * k_inv_norm;
      for (uint32_t value_dim = lane; value_dim < head_dim;
           value_dim += blockDim.x) {
        const uint64_t index = static_cast<uint64_t>(key_dim) * head_dim + value_dim;
        state[index] += key * delta[value_dim];
      }
    }
    __syncthreads();

    const uint64_t output_offset = token_head * head_dim;
    for (uint32_t value_dim = lane; value_dim < head_dim;
         value_dim += blockDim.x) {
      float output = 0.0f;
      for (uint32_t key_dim = 0; key_dim < head_dim; ++key_dim) {
        const float query = ovis_gd_to_float(qkv[qkv_offset + key_dim]) * q_inv_norm;
        output += state[static_cast<uint64_t>(key_dim) * head_dim + value_dim] * query;
      }
      packed_output[output_offset + value_dim] = output;
    }
    __syncthreads();
  }
}

}  // namespace

#define OVIS_DEFINE_GATED_DELTA_KERNEL(name, input_type)                         \
  extern "C" __global__ void name(                                               \
      const input_type* qkv, const float* gate_beta,                             \
      const float* initial_state, float* packed_output, uint32_t batch_size,      \
      uint32_t sequence_length, uint32_t num_heads, uint32_t head_dim) {          \
    __shared__ float q_reduction[256];                                             \
    __shared__ float k_reduction[256];                                             \
    __shared__ float delta[256];                                                   \
    ovis_gated_delta_rule_body(qkv, gate_beta, initial_state, packed_output,       \
                               batch_size, sequence_length, num_heads, head_dim,   \
                               q_reduction, k_reduction, delta);                   \
  }

OVIS_DEFINE_GATED_DELTA_KERNEL(gated_delta_rule_bf16, __nv_bfloat16)
OVIS_DEFINE_GATED_DELTA_KERNEL(gated_delta_rule_f16, half)
OVIS_DEFINE_GATED_DELTA_KERNEL(gated_delta_rule_f32, float)

#undef OVIS_DEFINE_GATED_DELTA_KERNEL
