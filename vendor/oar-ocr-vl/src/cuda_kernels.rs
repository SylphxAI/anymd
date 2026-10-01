use candle_core::backend::BackendStorage;
use candle_core::{CpuStorage, CustomOp1, CustomOp2, InplaceOp2, Layout, Result, Shape};

const PTX: &str = include_str!(concat!(env!("OUT_DIR"), "/oar_vl_kernels.ptx"));
pub(crate) const CUDA_KERNEL_MODULE: &str = "oar_vl_kernels";
const PARTITIONS_PER_ROW: usize = 8;

/// Stable row-wise argmax for F32 logits. Equal maxima keep the lowest
/// vocabulary id, matching the scalar CPU generation loops.
pub(crate) struct ArgmaxFirstF32;

/// Stable row-wise argmax for BF16 logits. Values are compared after the same
/// exact BF16-to-F32 conversion used by the CPU fallback.
pub(crate) struct ArgmaxFirstBf16;

/// Set a sparse list of vocabulary positions to negative infinity before
/// greedy selection. The same list is applied to every logits row.
pub(crate) struct MaskTokenIds;

/// GPU row-wise multinomial/greedy selection with the selected token's
/// softmax probability. The result is F32 `[rows, 2]`: token id, confidence.
pub(crate) struct SampleWithConfidence {
    pub(crate) temperature: f32,
    pub(crate) greedy: bool,
}

impl CustomOp1 for ArgmaxFirstF32 {
    fn name(&self) -> &'static str {
        "stable-argmax-first-f32"
    }

    fn cpu_fwd(&self, _storage: &CpuStorage, _layout: &Layout) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("stable argmax is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        storage: &candle_core::CudaStorage,
        layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !layout.is_contiguous() || storage.dtype() != candle_core::DType::F32 {
            candle_core::bail!("stable argmax requires contiguous F32 logits")
        }
        let (rows, vocab_size) = layout.shape().dims2()?;
        if vocab_size == 0 {
            candle_core::bail!("stable argmax does not support an empty vocabulary")
        }
        let device = storage.device().clone();
        let logits = storage.as_cuda_slice::<f32>()?;
        let logits = logits.slice(layout.start_offset()..);
        let output = unsafe { device.alloc::<u32>(rows) }?;
        let function =
            device.get_or_load_custom_func("argmax_first_f32", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&logits);
        builder.arg(&output);
        candle_core::builder_arg!(builder, rows as u32, vocab_size as u32);
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: (rows as u32, 1, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: 0,
            })
        }
        .w()?;
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            Shape::from_dims(&[rows]),
        ))
    }
}

impl CustomOp1 for ArgmaxFirstBf16 {
    fn name(&self) -> &'static str {
        "stable-argmax-first-bf16"
    }

    fn cpu_fwd(&self, _storage: &CpuStorage, _layout: &Layout) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("stable BF16 argmax is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        storage: &candle_core::CudaStorage,
        layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !layout.is_contiguous() || storage.dtype() != candle_core::DType::BF16 {
            candle_core::bail!("stable BF16 argmax requires contiguous logits")
        }
        let (rows, vocab_size) = layout.shape().dims2()?;
        if vocab_size == 0 {
            candle_core::bail!("stable BF16 argmax does not support an empty vocabulary")
        }
        let device = storage.device().clone();
        let logits = storage.as_cuda_slice::<half::bf16>()?;
        let logits = logits.slice(layout.start_offset()..);
        let partial_count = rows * PARTITIONS_PER_ROW;
        let partial_values = unsafe { device.alloc::<f32>(partial_count) }?;
        let partial_indices = unsafe { device.alloc::<u32>(partial_count) }?;
        let output = unsafe { device.alloc::<u32>(rows) }?;
        let stage1 =
            device.get_or_load_custom_func("argmax_first_bf16_stage1", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = stage1.builder();
        builder.arg(&logits);
        builder.arg(&partial_values);
        builder.arg(&partial_indices);
        candle_core::builder_arg!(
            builder,
            rows as u32,
            vocab_size as u32,
            PARTITIONS_PER_ROW as u32
        );
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: (partial_count as u32, 1, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: 0,
            })
        }
        .w()?;

        let stage2 = device.get_or_load_custom_func(
            "dflash_repetition_argmax_stage2",
            CUDA_KERNEL_MODULE,
            PTX,
        )?;
        let mut builder = stage2.builder();
        builder.arg(&partial_values);
        builder.arg(&partial_indices);
        builder.arg(&output);
        candle_core::builder_arg!(builder, PARTITIONS_PER_ROW as u32, rows as u32);
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: (rows as u32, 1, 1),
                block_dim: (32, 1, 1),
                shared_mem_bytes: 0,
            })
        }
        .w()?;
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            Shape::from_dims(&[rows]),
        ))
    }
}

impl InplaceOp2 for MaskTokenIds {
    fn name(&self) -> &'static str {
        "mask-token-ids"
    }

    fn cpu_fwd(
        &self,
        _logits: &mut CpuStorage,
        _logits_layout: &Layout,
        _token_ids: &CpuStorage,
        _token_ids_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("sparse token masking is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        logits: &mut candle_core::CudaStorage,
        logits_layout: &Layout,
        token_ids: &candle_core::CudaStorage,
        token_ids_layout: &Layout,
    ) -> Result<()> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !logits_layout.is_contiguous()
            || !token_ids_layout.is_contiguous()
            || token_ids.dtype() != candle_core::DType::U32
        {
            candle_core::bail!(
                "sparse token masking requires contiguous logits and contiguous U32 ids"
            )
        }
        let (rows, vocab_size) = logits_layout.shape().dims2()?;
        let token_count = token_ids_layout.shape().dims1()?;
        if token_count == 0 {
            return Ok(());
        }

        let device = logits.device().clone();
        let token_ids = token_ids.as_cuda_slice::<u32>()?;
        let token_ids = token_ids.slice(token_ids_layout.start_offset()..);
        let count = rows * token_count;
        match logits.dtype() {
            candle_core::DType::BF16 => {
                let logits = logits.as_cuda_slice_mut::<half::bf16>()?;
                let mut logits = logits.slice_mut(logits_layout.start_offset()..);
                let function = device.get_or_load_custom_func(
                    "mask_token_ids_bf16",
                    CUDA_KERNEL_MODULE,
                    PTX,
                )?;
                let mut builder = function.builder();
                builder.arg(&mut logits);
                builder.arg(&token_ids);
                candle_core::builder_arg!(
                    builder,
                    rows as u32,
                    vocab_size as u32,
                    token_count as u32
                );
                unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
            }
            candle_core::DType::F32 => {
                let logits = logits.as_cuda_slice_mut::<f32>()?;
                let mut logits = logits.slice_mut(logits_layout.start_offset()..);
                let function = device.get_or_load_custom_func(
                    "mask_token_ids_f32",
                    CUDA_KERNEL_MODULE,
                    PTX,
                )?;
                let mut builder = function.builder();
                builder.arg(&mut logits);
                builder.arg(&token_ids);
                candle_core::builder_arg!(
                    builder,
                    rows as u32,
                    vocab_size as u32,
                    token_count as u32
                );
                unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
            }
            dtype => candle_core::bail!(
                "sparse token masking requires BF16 or F32 logits, got {dtype:?}"
            ),
        }
        Ok(())
    }
}

impl CustomOp2 for SampleWithConfidence {
    fn name(&self) -> &'static str {
        "sample-with-confidence"
    }

    fn cpu_fwd(
        &self,
        _logits: &CpuStorage,
        _logits_layout: &Layout,
        _uniforms: &CpuStorage,
        _uniforms_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("sample-with-confidence is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        logits: &candle_core::CudaStorage,
        logits_layout: &Layout,
        uniforms: &candle_core::CudaStorage,
        uniforms_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !logits_layout.is_contiguous()
            || !uniforms_layout.is_contiguous()
            || uniforms.dtype() != candle_core::DType::F32
        {
            candle_core::bail!("sample-with-confidence requires contiguous logits and F32 uniforms")
        }
        let (rows, vocab_size) = logits_layout.shape().dims2()?;
        if rows == 0 || vocab_size == 0 || uniforms_layout.shape().dims1()? != rows {
            candle_core::bail!(
                "sample-with-confidence shape mismatch: logits={:?}, uniforms={:?}",
                logits_layout.shape(),
                uniforms_layout.shape()
            )
        }
        if vocab_size >= (1 << 24) {
            candle_core::bail!(
                "sample-with-confidence requires vocabulary size < 2^24, got {vocab_size}"
            )
        }
        if !self.greedy && (!self.temperature.is_finite() || self.temperature <= 0.0) {
            candle_core::bail!(
                "sample-with-confidence requires a positive finite sampling temperature"
            )
        }

        let device = logits.device().clone();
        let uniforms = uniforms.as_cuda_slice::<f32>()?;
        let uniforms = uniforms.slice(uniforms_layout.start_offset()..);
        let output = unsafe { device.alloc::<f32>(rows * 2) }?;
        let function_name = match logits.dtype() {
            candle_core::DType::BF16 => "sample_with_confidence_bf16",
            candle_core::DType::F32 => "sample_with_confidence_f32",
            dtype => candle_core::bail!(
                "sample-with-confidence requires BF16 or F32 logits, got {dtype:?}"
            ),
        };
        let function = device.get_or_load_custom_func(function_name, CUDA_KERNEL_MODULE, PTX)?;
        let inv_temperature = if self.greedy {
            1.0
        } else {
            self.temperature.recip()
        };
        let greedy = u32::from(self.greedy);
        match logits.dtype() {
            candle_core::DType::BF16 => {
                let logits = logits.as_cuda_slice::<half::bf16>()?;
                let logits = logits.slice(logits_layout.start_offset()..);
                let mut builder = function.builder();
                builder.arg(&logits);
                builder.arg(&uniforms);
                builder.arg(&output);
                candle_core::builder_arg!(
                    builder,
                    rows as u32,
                    vocab_size as u32,
                    inv_temperature,
                    greedy
                );
                unsafe {
                    builder.launch(LaunchConfig {
                        grid_dim: (rows as u32, 1, 1),
                        block_dim: (256, 1, 1),
                        shared_mem_bytes: 0,
                    })
                }
                .w()?;
            }
            candle_core::DType::F32 => {
                let logits = logits.as_cuda_slice::<f32>()?;
                let logits = logits.slice(logits_layout.start_offset()..);
                let mut builder = function.builder();
                builder.arg(&logits);
                builder.arg(&uniforms);
                builder.arg(&output);
                candle_core::builder_arg!(
                    builder,
                    rows as u32,
                    vocab_size as u32,
                    inv_temperature,
                    greedy
                );
                unsafe {
                    builder.launch(LaunchConfig {
                        grid_dim: (rows as u32, 1, 1),
                        block_dim: (256, 1, 1),
                        shared_mem_bytes: 0,
                    })
                }
                .w()?;
            }
            _ => unreachable!(),
        }
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            Shape::from_dims(&[rows, 2]),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::{DType, Device, Tensor};

    #[cfg(feature = "cuda")]
    #[test]
    fn stable_bf16_argmax_masks_ids_and_keeps_first_tie() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let logits = Tensor::from_vec(vec![0.0f32, 10.0, 3.0, 10.0, 9.0, 1.0], (1, 6), &device)?
            .to_dtype(DType::BF16)?;
        let banned = Tensor::new(&[1u32, u32::MAX], &device)?;
        logits.inplace_op2(&banned, &MaskTokenIds)?;
        let token = logits
            .apply_op1_no_bwd(&ArgmaxFirstBf16)?
            .to_vec1::<u32>()?;
        assert_eq!(token, vec![3]);
        Ok(())
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn stable_f32_argmax_masks_ids_for_every_row() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let logits = Tensor::from_vec(vec![1.0f32, 8.0, 7.0, 0.0, 2.0, 9.0], (2, 3), &device)?;
        let banned = Tensor::new(&[1u32], &device)?;
        logits.inplace_op2(&banned, &MaskTokenIds)?;
        let tokens = logits.apply_op1_no_bwd(&ArgmaxFirstF32)?.to_vec1::<u32>()?;
        assert_eq!(tokens, vec![2, 2]);
        Ok(())
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn gpu_sampling_returns_stable_greedy_and_selected_probability() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let logits = Tensor::from_vec(vec![2.0f32, 1.0, 0.0, 2.0, 2.0, 0.0], (2, 3), &device)?
            .to_dtype(DType::BF16)?;
        let uniforms = Tensor::zeros(2, DType::F32, &device)?;
        let sampled = logits
            .apply_op2_no_bwd(
                &uniforms,
                &SampleWithConfidence {
                    temperature: 1.0,
                    greedy: true,
                },
            )?
            .to_vec2::<f32>()?;
        assert_eq!(sampled[0][0] as u32, 0);
        // Equal maxima keep the first vocabulary id.
        assert_eq!(sampled[1][0] as u32, 0);
        assert!((sampled[0][1] - 0.665240).abs() < 1e-4);
        assert!((sampled[1][1] - 0.468311).abs() < 1e-4);
        Ok(())
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn gpu_sampling_uses_cdf_uniforms() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let logits = Tensor::from_vec(vec![0.0f32, 0.0, 0.0, 0.0], (2, 2), &device)?;
        let uniforms = Tensor::new(&[0.1f32, 0.9], &device)?;
        let sampled = logits
            .apply_op2_no_bwd(
                &uniforms,
                &SampleWithConfidence {
                    temperature: 1.0,
                    greedy: false,
                },
            )?
            .to_vec2::<f32>()?;
        assert_eq!(sampled[0][0] as u32, 0);
        assert_eq!(sampled[1][0] as u32, 1);
        assert!((sampled[0][1] - 0.5).abs() < 1e-6);
        assert!((sampled[1][1] - 0.5).abs() < 1e-6);
        Ok(())
    }
}
