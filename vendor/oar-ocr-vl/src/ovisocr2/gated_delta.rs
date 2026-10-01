//! Recurrent Gated Delta Rule used by Qwen3.5 linear-attention layers.
//!
//! The custom-op ABI packs the two logical outputs into one flat F32 buffer
//! because Candle custom ops return a single tensor. [`gated_delta_rule`]
//! immediately unpacks that buffer into the public crate-internal layout:
//! `[B, S, H, D]` core output (cast back to the QKV dtype) and
//! `[B, H, D, D]` final recurrent state in F32.

#[cfg(feature = "cuda")]
use crate::cuda_kernels::CUDA_KERNEL_MODULE;
use candle_core::backend::BackendStorage;
use candle_core::{CpuStorage, CustomOp3, D, DType, Layout, Result, Shape, Tensor};

#[cfg(feature = "cuda")]
const PTX: &str = include_str!(concat!(env!("OUT_DIR"), "/oar_vl_kernels.ptx"));

const QK_NORM_EPS: f32 = 1e-6;
#[cfg(feature = "cuda")]
const MAX_CUDA_HEAD_DIM: usize = 256;

#[derive(Debug, Clone, Copy)]
struct GatedDeltaRule {
    batch: usize,
    sequence: usize,
    heads: usize,
    head_dim: usize,
}

impl GatedDeltaRule {
    fn core_len(self) -> usize {
        self.batch * self.sequence * self.heads * self.head_dim
    }

    fn state_len(self) -> usize {
        self.batch * self.heads * self.head_dim * self.head_dim
    }

    fn packed_len(self) -> usize {
        self.core_len() + self.state_len()
    }

    fn validate_layouts(
        self,
        qkv_layout: &Layout,
        gate_beta_layout: &Layout,
        state_layout: &Layout,
    ) -> Result<()> {
        if !qkv_layout.is_contiguous()
            || !gate_beta_layout.is_contiguous()
            || !state_layout.is_contiguous()
        {
            candle_core::bail!("gated delta rule requires contiguous tensors")
        }

        let expected_qkv = [self.batch, self.sequence, self.heads, self.head_dim * 3];
        let expected_gate_beta = [self.batch, self.sequence, self.heads, 2];
        let expected_state = [self.batch, self.heads, self.head_dim, self.head_dim];
        if qkv_layout.shape().dims() != expected_qkv
            || gate_beta_layout.shape().dims() != expected_gate_beta
            || state_layout.shape().dims() != expected_state
        {
            candle_core::bail!(
                "gated delta rule shape mismatch: qkv={:?} (expected {:?}), gate_beta={:?} (expected {:?}), state={:?} (expected {:?})",
                qkv_layout.shape(),
                expected_qkv,
                gate_beta_layout.shape(),
                expected_gate_beta,
                state_layout.shape(),
                expected_state,
            )
        }
        Ok(())
    }
}

trait IntoF32: Copy {
    fn into_f32(self) -> f32;
}

impl IntoF32 for f32 {
    fn into_f32(self) -> f32 {
        self
    }
}

impl IntoF32 for half::bf16 {
    fn into_f32(self) -> f32 {
        f32::from(self)
    }
}

impl IntoF32 for half::f16 {
    fn into_f32(self) -> f32 {
        f32::from(self)
    }
}

fn contiguous_range(layout: &Layout, name: &str) -> Result<(usize, usize)> {
    layout
        .contiguous_offsets()
        .ok_or_else(|| candle_core::Error::Msg(format!("{name} must be contiguous")))
}

fn cpu_recurrent<T: IntoF32>(
    op: GatedDeltaRule,
    qkv: &[T],
    gate_beta: &[f32],
    initial_state: &[f32],
) -> Vec<f32> {
    let GatedDeltaRule {
        batch,
        sequence,
        heads,
        head_dim,
    } = op;
    let core_len = op.core_len();
    let mut packed = vec![0f32; op.packed_len()];
    let (core, final_states) = packed.split_at_mut(core_len);
    let q_scale = (head_dim as f32).sqrt().recip();

    for batch_idx in 0..batch {
        for head_idx in 0..heads {
            let state_offset = (batch_idx * heads + head_idx) * head_dim * head_dim;
            let state = &mut final_states[state_offset..state_offset + head_dim * head_dim];
            state.copy_from_slice(&initial_state[state_offset..state_offset + head_dim * head_dim]);

            let mut q = vec![0f32; head_dim];
            let mut k = vec![0f32; head_dim];
            let mut delta = vec![0f32; head_dim];
            for token_idx in 0..sequence {
                let token_offset =
                    ((batch_idx * sequence + token_idx) * heads + head_idx) * head_dim * 3;
                let mut q_norm_sq = 0f32;
                let mut k_norm_sq = 0f32;
                for lane in 0..head_dim {
                    let q_value = qkv[token_offset + lane].into_f32();
                    let k_value = qkv[token_offset + head_dim + lane].into_f32();
                    q[lane] = q_value;
                    k[lane] = k_value;
                    q_norm_sq += q_value * q_value;
                    k_norm_sq += k_value * k_value;
                }
                let q_inv_norm = (q_norm_sq + QK_NORM_EPS).sqrt().recip() * q_scale;
                let k_inv_norm = (k_norm_sq + QK_NORM_EPS).sqrt().recip();
                for lane in 0..head_dim {
                    q[lane] *= q_inv_norm;
                    k[lane] *= k_inv_norm;
                }

                let gate_offset = ((batch_idx * sequence + token_idx) * heads + head_idx) * 2;
                let decay = gate_beta[gate_offset].exp();
                let beta = gate_beta[gate_offset + 1];
                for value in state.iter_mut() {
                    *value *= decay;
                }

                for value_lane in 0..head_dim {
                    let mut memory = 0f32;
                    for key_lane in 0..head_dim {
                        memory += state[key_lane * head_dim + value_lane] * k[key_lane];
                    }
                    let value = qkv[token_offset + head_dim * 2 + value_lane].into_f32();
                    delta[value_lane] = (value - memory) * beta;
                }

                for key_lane in 0..head_dim {
                    for value_lane in 0..head_dim {
                        state[key_lane * head_dim + value_lane] += k[key_lane] * delta[value_lane];
                    }
                }

                let output_offset =
                    ((batch_idx * sequence + token_idx) * heads + head_idx) * head_dim;
                for value_lane in 0..head_dim {
                    let mut value = 0f32;
                    for key_lane in 0..head_dim {
                        value += state[key_lane * head_dim + value_lane] * q[key_lane];
                    }
                    core[output_offset + value_lane] = value;
                }
            }
        }
    }
    packed
}

impl CustomOp3 for GatedDeltaRule {
    fn name(&self) -> &'static str {
        "qwen3.5-gated-delta-rule"
    }

    fn cpu_fwd(
        &self,
        qkv: &CpuStorage,
        qkv_layout: &Layout,
        gate_beta: &CpuStorage,
        gate_beta_layout: &Layout,
        initial_state: &CpuStorage,
        state_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        self.validate_layouts(qkv_layout, gate_beta_layout, state_layout)?;
        if gate_beta.dtype() != DType::F32 || initial_state.dtype() != DType::F32 {
            candle_core::bail!(
                "gated delta rule requires F32 gate_beta and initial_state, got {:?} and {:?}",
                gate_beta.dtype(),
                initial_state.dtype()
            )
        }
        let (gate_start, gate_end) = contiguous_range(gate_beta_layout, "gate_beta")?;
        let (state_start, state_end) = contiguous_range(state_layout, "initial_state")?;
        let gates = &gate_beta.as_slice::<f32>()?[gate_start..gate_end];
        let state = &initial_state.as_slice::<f32>()?[state_start..state_end];
        let (qkv_start, qkv_end) = contiguous_range(qkv_layout, "qkv")?;
        let packed = match qkv {
            CpuStorage::BF16(values) => {
                cpu_recurrent(*self, &values[qkv_start..qkv_end], gates, state)
            }
            CpuStorage::F16(values) => {
                cpu_recurrent(*self, &values[qkv_start..qkv_end], gates, state)
            }
            CpuStorage::F32(values) => {
                cpu_recurrent(*self, &values[qkv_start..qkv_end], gates, state)
            }
            _ => candle_core::bail!(
                "gated delta rule supports BF16, F16, or F32 QKV, got {:?}",
                qkv.dtype()
            ),
        };
        Ok((
            CpuStorage::F32(packed),
            Shape::from_dims(&[self.packed_len()]),
        ))
    }

    #[cfg(feature = "cuda")]
    fn cuda_fwd(
        &self,
        qkv: &candle_core::CudaStorage,
        qkv_layout: &Layout,
        gate_beta: &candle_core::CudaStorage,
        gate_beta_layout: &Layout,
        initial_state: &candle_core::CudaStorage,
        state_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        self.validate_layouts(qkv_layout, gate_beta_layout, state_layout)?;
        if gate_beta.dtype() != DType::F32 || initial_state.dtype() != DType::F32 {
            candle_core::bail!(
                "gated delta rule requires F32 gate_beta and initial_state, got {:?} and {:?}",
                gate_beta.dtype(),
                initial_state.dtype()
            )
        }
        if self.head_dim > MAX_CUDA_HEAD_DIM {
            candle_core::bail!(
                "gated delta rule CUDA kernel supports head_dim <= {MAX_CUDA_HEAD_DIM}, got {}",
                self.head_dim
            )
        }

        let device = qkv.device().clone();
        let gate_beta = gate_beta.as_cuda_slice::<f32>()?;
        let gate_beta = gate_beta.slice(gate_beta_layout.start_offset()..);
        let initial_state = initial_state.as_cuda_slice::<f32>()?;
        let initial_state = initial_state.slice(state_layout.start_offset()..);
        let mut output = unsafe { device.alloc::<f32>(self.packed_len()) }?;
        let function_name = match qkv.dtype() {
            DType::BF16 => "gated_delta_rule_bf16",
            DType::F16 => "gated_delta_rule_f16",
            DType::F32 => "gated_delta_rule_f32",
            dtype => {
                candle_core::bail!("gated delta rule supports BF16, F16, or F32 QKV, got {dtype:?}")
            }
        };
        let function = device.get_or_load_custom_func(function_name, CUDA_KERNEL_MODULE, PTX)?;
        let threads = self.head_dim.next_power_of_two().min(MAX_CUDA_HEAD_DIM) as u32;

        macro_rules! launch {
            ($ty:ty) => {{
                let qkv = qkv.as_cuda_slice::<$ty>()?;
                let qkv = qkv.slice(qkv_layout.start_offset()..);
                let mut builder = function.builder();
                builder.arg(&qkv);
                builder.arg(&gate_beta);
                builder.arg(&initial_state);
                builder.arg(&mut output);
                candle_core::builder_arg!(
                    builder,
                    self.batch as u32,
                    self.sequence as u32,
                    self.heads as u32,
                    self.head_dim as u32
                );
                unsafe {
                    builder.launch(LaunchConfig {
                        grid_dim: ((self.batch * self.heads) as u32, 1, 1),
                        block_dim: (threads, 1, 1),
                        shared_mem_bytes: 0,
                    })
                }
                .w()?;
            }};
        }

        match qkv.dtype() {
            DType::BF16 => launch!(half::bf16),
            DType::F16 => launch!(half::f16),
            DType::F32 => launch!(f32),
            _ => unreachable!(),
        }
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            Shape::from_dims(&[self.packed_len()]),
        ))
    }
}

fn validate_inputs(
    qkv: &Tensor,
    gate_beta: &Tensor,
    initial_state: &Tensor,
) -> Result<GatedDeltaRule> {
    let (batch, sequence, heads, packed_dim) = qkv.dims4()?;
    if batch == 0 || sequence == 0 || heads == 0 || packed_dim == 0 || packed_dim % 3 != 0 {
        candle_core::bail!(
            "gated delta rule expects non-empty qkv [B,S,H,3D], got {:?}",
            qkv.shape()
        )
    }
    let head_dim = packed_dim / 3;
    if gate_beta.dims4()? != (batch, sequence, heads, 2) {
        candle_core::bail!(
            "gated delta rule expects gate_beta [B,S,H,2], got {:?} for qkv {:?}",
            gate_beta.shape(),
            qkv.shape()
        )
    }
    if initial_state.dims4()? != (batch, heads, head_dim, head_dim) {
        candle_core::bail!(
            "gated delta rule expects initial_state [B,H,D,D], got {:?} for qkv {:?}",
            initial_state.shape(),
            qkv.shape()
        )
    }
    if !qkv.device().same_device(gate_beta.device())
        || !qkv.device().same_device(initial_state.device())
    {
        candle_core::bail!(
            "gated delta rule inputs must be on the same device, got qkv={:?}, gate_beta={:?}, state={:?}",
            qkv.device(),
            gate_beta.device(),
            initial_state.device()
        )
    }
    if !matches!(qkv.dtype(), DType::BF16 | DType::F16 | DType::F32) {
        candle_core::bail!(
            "gated delta rule supports BF16, F16, or F32 QKV, got {:?}",
            qkv.dtype()
        )
    }
    if gate_beta.dtype() != DType::F32 || initial_state.dtype() != DType::F32 {
        candle_core::bail!(
            "gated delta rule requires F32 gate_beta and initial_state, got {:?} and {:?}",
            gate_beta.dtype(),
            initial_state.dtype()
        )
    }
    Ok(GatedDeltaRule {
        batch,
        sequence,
        heads,
        head_dim,
    })
}

/// Apply Qwen3.5's recurrent Gated Delta Rule.
///
/// Input layouts are:
///
/// - `qkv`: contiguous logical `[B, S, H, 3D]`, packed as Q then K then V.
/// - `gate_beta`: F32 `[B, S, H, 2]`, with log-decay `g` in lane 0 and
///   delta interpolation `beta` in lane 1.
/// - `initial_state`: F32 `[B, H, D, D]`, key dimension before value dimension.
///
/// The returned core output is `[B, S, H, D]` in the original QKV dtype. The
/// returned final state is `[B, H, D, D]` in F32. Q and K are L2-normalized
/// with epsilon `1e-6`; Q additionally receives the `1/sqrt(D)` attention
/// scale. CPU and CUDA use the packed-output [`CustomOp3`] above. Metal uses a
/// portable Candle tensor recurrence because Candle custom ops have no Metal
/// implementation here.
pub(crate) fn gated_delta_rule(
    qkv: &Tensor,
    gate_beta: &Tensor,
    initial_state: &Tensor,
) -> Result<(Tensor, Tensor)> {
    let op = validate_inputs(qkv, gate_beta, initial_state)?;
    if qkv.device().is_metal() {
        return tensor_recurrent(qkv, gate_beta, initial_state, op);
    }

    let qkv = qkv.contiguous()?;
    let gate_beta = gate_beta.contiguous()?;
    let initial_state = initial_state.contiguous()?;
    let packed = qkv.apply_op3_no_bwd(&gate_beta, &initial_state, &op)?;
    let core = packed
        .narrow(0, 0, op.core_len())?
        .reshape((op.batch, op.sequence, op.heads, op.head_dim))?
        .to_dtype(qkv.dtype())?;
    let final_state = packed.narrow(0, op.core_len(), op.state_len())?.reshape((
        op.batch,
        op.heads,
        op.head_dim,
        op.head_dim,
    ))?;
    Ok((core, final_state))
}

fn tensor_recurrent(
    qkv: &Tensor,
    gate_beta: &Tensor,
    initial_state: &Tensor,
    op: GatedDeltaRule,
) -> Result<(Tensor, Tensor)> {
    let source_dtype = qkv.dtype();
    let qkv = qkv.to_dtype(DType::F32)?;
    let q = qkv.narrow(3, 0, op.head_dim)?;
    let k = qkv.narrow(3, op.head_dim, op.head_dim)?;
    let v = qkv.narrow(3, op.head_dim * 2, op.head_dim)?;
    let q_norm = q
        .sqr()?
        .sum_keepdim(D::Minus1)?
        .affine(1.0, QK_NORM_EPS as f64)?
        .sqrt()?;
    let k_norm = k
        .sqr()?
        .sum_keepdim(D::Minus1)?
        .affine(1.0, QK_NORM_EPS as f64)?
        .sqrt()?;
    let q = q
        .broadcast_div(&q_norm)?
        .affine((op.head_dim as f64).sqrt().recip(), 0.0)?;
    let k = k.broadcast_div(&k_norm)?;
    let g = gate_beta.narrow(3, 0, 1)?.squeeze(3)?;
    let beta = gate_beta.narrow(3, 1, 1)?.squeeze(3)?;

    let mut state = initial_state.clone();
    let mut outputs = Vec::with_capacity(op.sequence);
    for token_idx in 0..op.sequence {
        let q_t = q.narrow(1, token_idx, 1)?.squeeze(1)?;
        let k_t = k.narrow(1, token_idx, 1)?.squeeze(1)?;
        let v_t = v.narrow(1, token_idx, 1)?.squeeze(1)?;
        let decay = g
            .narrow(1, token_idx, 1)?
            .squeeze(1)?
            .exp()?
            .unsqueeze(2)?
            .unsqueeze(3)?;
        state = state.broadcast_mul(&decay)?;
        let memory = state.broadcast_mul(&k_t.unsqueeze(3)?)?.sum(2)?;
        let beta_t = beta.narrow(1, token_idx, 1)?.squeeze(1)?.unsqueeze(2)?;
        let delta = v_t.broadcast_sub(&memory)?.broadcast_mul(&beta_t)?;
        state = state.broadcast_add(&k_t.unsqueeze(3)?.broadcast_mul(&delta.unsqueeze(2)?)?)?;
        let output = state
            .broadcast_mul(&q_t.unsqueeze(3)?)?
            .sum(2)?
            .unsqueeze(1)?;
        outputs.push(output);
    }
    let output_refs: Vec<&Tensor> = outputs.iter().collect();
    let core = Tensor::cat(&output_refs, 1)?.to_dtype(source_dtype)?;
    Ok((core, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Device;

    const B: usize = 1;
    const S: usize = 3;
    const H: usize = 1;
    const D_HEAD: usize = 2;

    fn inputs(device: &Device, dtype: DType) -> Result<(Tensor, Tensor, Tensor)> {
        let qkv = [
            3.0f32, 4.0, 0.0, 2.0, 1.0, -1.0, // token 0
            -1.0, 2.0, 2.0, 1.0, 0.5, 3.0, // token 1
            0.5, -0.25, -1.0, 0.75, 2.0, 0.25, // token 2
        ];
        let gate_beta = [0.5f32.ln(), 0.25, 0.8f32.ln(), 0.75, 0.9f32.ln(), 0.4];
        let state = [0.1f32, -0.2, 0.3, 0.4];
        Ok((
            Tensor::from_slice(&qkv, (B, S, H, D_HEAD * 3), device)?.to_dtype(dtype)?,
            Tensor::from_slice(&gate_beta, (B, S, H, 2), device)?,
            Tensor::from_slice(&state, (B, H, D_HEAD, D_HEAD), device)?,
        ))
    }

    fn assert_close(actual: &Tensor, expected: &Tensor, tolerance: f32) -> Result<()> {
        let actual = actual
            .to_dtype(DType::F32)?
            .flatten_all()?
            .to_vec1::<f32>()?;
        let expected = expected
            .to_dtype(DType::F32)?
            .flatten_all()?
            .to_vec1::<f32>()?;
        assert_eq!(actual.len(), expected.len());
        for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
            assert!(
                (actual - expected).abs() <= tolerance,
                "mismatch at {index}: actual={actual}, expected={expected}, tolerance={tolerance}"
            );
        }
        Ok(())
    }

    #[test]
    fn cpu_custom_op_matches_tensor_recurrence() -> Result<()> {
        let (qkv, gate_beta, state) = inputs(&Device::Cpu, DType::F32)?;
        let op = validate_inputs(&qkv, &gate_beta, &state)?;
        let expected = tensor_recurrent(&qkv, &gate_beta, &state, op)?;
        let actual = gated_delta_rule(&qkv, &gate_beta, &state)?;
        assert_eq!(actual.0.dims(), &[B, S, H, D_HEAD]);
        assert_eq!(actual.1.dims(), &[B, H, D_HEAD, D_HEAD]);
        assert_eq!(actual.0.dtype(), DType::F32);
        assert_eq!(actual.1.dtype(), DType::F32);
        assert_close(&actual.0, &expected.0, 2e-6)?;
        assert_close(&actual.1, &expected.1, 2e-6)?;

        // Independently calculated from the scalar recurrence in the
        // Transformers Qwen3.5 fallback. This guards against both
        // implementations agreeing on the same layout mistake.
        let expected_core = Tensor::from_slice(
            &[
                0.226_274_16f32,
                -0.098_994_926,
                0.170_762_97,
                -0.025_298_197,
                -0.513_062_3,
                0.540_510_9,
            ],
            (B, S, H, D_HEAD),
            &Device::Cpu,
        )?;
        let expected_state = Tensor::from_slice(
            &[-0.393_449_7f32, 1.428_460_7, 0.835_548_7, 1.147_673_1],
            (B, H, D_HEAD, D_HEAD),
            &Device::Cpu,
        )?;
        assert_close(&actual.0, &expected_core, 3e-6)?;
        assert_close(&actual.1, &expected_state, 3e-6)
    }

    #[test]
    fn bf16_core_preserves_dtype_and_is_numerically_close() -> Result<()> {
        let (qkv, gate_beta, state) = inputs(&Device::Cpu, DType::BF16)?;
        let op = validate_inputs(&qkv, &gate_beta, &state)?;
        let expected = tensor_recurrent(&qkv, &gate_beta, &state, op)?;
        let actual = gated_delta_rule(&qkv, &gate_beta, &state)?;
        assert_eq!(actual.0.dtype(), DType::BF16);
        assert_eq!(actual.1.dtype(), DType::F32);
        assert_close(&actual.0, &expected.0, 2e-2)?;
        assert_close(&actual.1, &expected.1, 2e-5)
    }

    #[test]
    fn f16_core_preserves_dtype_and_is_numerically_close() -> Result<()> {
        let (qkv, gate_beta, state) = inputs(&Device::Cpu, DType::F16)?;
        let op = validate_inputs(&qkv, &gate_beta, &state)?;
        let expected = tensor_recurrent(&qkv, &gate_beta, &state, op)?;
        let actual = gated_delta_rule(&qkv, &gate_beta, &state)?;
        assert_eq!(actual.0.dtype(), DType::F16);
        assert_eq!(actual.1.dtype(), DType::F32);
        assert_close(&actual.0, &expected.0, 3e-3)?;
        assert_close(&actual.1, &expected.1, 3e-6)
    }

    #[test]
    fn recurrent_state_makes_split_sequence_match_full_sequence() -> Result<()> {
        let (qkv, gate_beta, state) = inputs(&Device::Cpu, DType::F32)?;
        let (full_output, full_state) = gated_delta_rule(&qkv, &gate_beta, &state)?;
        let (first_output, first_state) =
            gated_delta_rule(&qkv.narrow(1, 0, 1)?, &gate_beta.narrow(1, 0, 1)?, &state)?;
        let (rest_output, rest_state) = gated_delta_rule(
            &qkv.narrow(1, 1, S - 1)?,
            &gate_beta.narrow(1, 1, S - 1)?,
            &first_state,
        )?;
        let split_output = Tensor::cat(&[&first_output, &rest_output], 1)?;
        assert_close(&split_output, &full_output, 2e-6)?;
        assert_close(&rest_state, &full_state, 2e-6)
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_supported_dtypes_match_cpu_reference_when_available() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        for (dtype, core_tolerance, state_tolerance) in [
            (DType::BF16, 3e-2, 3e-4),
            (DType::F16, 3e-3, 3e-5),
            (DType::F32, 3e-6, 3e-6),
        ] {
            let (cpu_qkv, cpu_gate_beta, cpu_state) = inputs(&Device::Cpu, dtype)?;
            let expected = gated_delta_rule(&cpu_qkv, &cpu_gate_beta, &cpu_state)?;
            let (qkv, gate_beta, state) = inputs(&device, dtype)?;
            let actual = gated_delta_rule(&qkv, &gate_beta, &state)?;
            assert_eq!(actual.0.dtype(), dtype);
            assert_close(
                &actual.0.to_device(&Device::Cpu)?,
                &expected.0,
                core_tolerance,
            )?;
            assert_close(
                &actual.1.to_device(&Device::Cpu)?,
                &expected.1,
                state_tolerance,
            )?;
        }
        Ok(())
    }
}
