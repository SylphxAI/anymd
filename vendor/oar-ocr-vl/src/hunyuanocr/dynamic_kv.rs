use crate::cuda_kernels::CUDA_KERNEL_MODULE;
use candle_core::backend::BackendStorage;
use candle_core::{
    CpuStorage, CustomOp2, CustomOp3, InplaceOp2, InplaceOp3, Layout, Result, Shape,
};

const PTX: &str = include_str!(concat!(env!("OUT_DIR"), "/oar_vl_kernels.ptx"));
const PARTITIONS_PER_ROW: usize = 8;

pub(crate) struct DynamicKvAppend {
    pub query_len: usize,
    pub cache_len: usize,
}

pub(super) struct DynamicPagedKvAppend {
    pub query_len: usize,
    pub cache_len: usize,
}

pub(super) struct FusedXdRope {
    pub projection_width: usize,
    pub projection_offset: usize,
    pub num_heads: usize,
    pub query_len: usize,
    pub head_dim: usize,
}

pub(super) struct FusedXdRopeRmsNormF16 {
    pub projection_width: usize,
    pub projection_offset: usize,
    pub num_heads: usize,
    pub query_len: usize,
    pub head_dim: usize,
    pub eps: f32,
    pub include_v: bool,
}

pub(super) struct FusedRmsNormRopeBf16 {
    pub projection_width: usize,
    pub projection_offset: usize,
    pub num_heads: usize,
    pub query_len: usize,
    pub head_dim: usize,
    pub eps: f32,
    pub include_v: bool,
}

pub(super) struct FusedRopeBf16;

pub(super) struct FusedAddRmsNormBf16 {
    pub eps: f32,
}

pub(super) struct FusedSiluMulBf16;

pub(super) struct RepetitionPenaltyF32 {
    pub penalty: f32,
}

pub(super) struct MarkRepetitionHistoryU8;

pub(super) struct DFlashRepetitionArgmaxBf16 {
    pub penalty: f32,
}

pub(super) struct RepetitionArgmaxBf16 {
    pub penalty: f32,
}

impl InplaceOp2 for MarkRepetitionHistoryU8 {
    fn name(&self) -> &'static str {
        "hunyuan-mark-repetition-history-u8"
    }

    fn cpu_fwd(
        &self,
        _history: &mut CpuStorage,
        _history_layout: &Layout,
        _token_ids: &CpuStorage,
        _token_ids_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("repetition-history marking is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        history: &mut candle_core::CudaStorage,
        history_layout: &Layout,
        token_ids: &candle_core::CudaStorage,
        token_ids_layout: &Layout,
    ) -> Result<()> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !history_layout.is_contiguous()
            || !token_ids_layout.is_contiguous()
            || history.dtype() != candle_core::DType::U8
            || token_ids.dtype() != candle_core::DType::U32
        {
            candle_core::bail!(
                "repetition-history marking requires contiguous U8 history and U32 ids"
            )
        }
        let vocab_size = history_layout.shape().elem_count();
        let token_count = token_ids_layout.shape().dims1()?;
        if token_count == 0 {
            return Ok(());
        }
        let device = history.device().clone();
        let history = history.as_cuda_slice_mut::<u8>()?;
        let token_ids = token_ids.as_cuda_slice::<u32>()?;
        let mut history = history.slice_mut(history_layout.start_offset()..);
        let token_ids = token_ids.slice(token_ids_layout.start_offset()..);
        let function = device.get_or_load_custom_func(
            "mark_repetition_history_u8",
            CUDA_KERNEL_MODULE,
            PTX,
        )?;
        let mut builder = function.builder();
        builder.arg(&mut history);
        builder.arg(&token_ids);
        candle_core::builder_arg!(builder, token_count as u32, vocab_size as u32);
        unsafe { builder.launch(LaunchConfig::for_num_elems(token_count as u32)) }.w()?;
        Ok(())
    }
}

impl CustomOp2 for RepetitionArgmaxBf16 {
    fn name(&self) -> &'static str {
        "hunyuan-repetition-argmax-bf16"
    }

    fn cpu_fwd(
        &self,
        _logits: &CpuStorage,
        _logits_layout: &Layout,
        _history: &CpuStorage,
        _history_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("fused repetition argmax is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        logits: &candle_core::CudaStorage,
        logits_layout: &Layout,
        history: &candle_core::CudaStorage,
        history_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !logits_layout.is_contiguous() || !history_layout.is_contiguous() {
            candle_core::bail!("fused repetition argmax requires contiguous tensors")
        }
        let (rows, vocab_size) = logits_layout.shape().dims2()?;
        if history_layout.shape().dims() != [rows, vocab_size]
            || logits.dtype() != candle_core::DType::BF16
            || history.dtype() != candle_core::DType::U8
            || !self.penalty.is_finite()
            || self.penalty < 1.0
        {
            candle_core::bail!(
                "fused repetition argmax mismatch logits={:?}/{:?} history={:?}/{:?} penalty={}",
                logits_layout.shape(),
                logits.dtype(),
                history_layout.shape(),
                history.dtype(),
                self.penalty
            )
        }

        let device = logits.device().clone();
        let logits = logits.as_cuda_slice::<half::bf16>()?;
        let history = history.as_cuda_slice::<u8>()?;
        let logits = logits.slice(logits_layout.start_offset()..);
        let history = history.slice(history_layout.start_offset()..);
        let partial_count = rows * PARTITIONS_PER_ROW;
        let partial_values = unsafe { device.alloc::<f32>(partial_count) }?;
        let partial_indices = unsafe { device.alloc::<u32>(partial_count) }?;
        let output = unsafe { device.alloc::<u32>(rows) }?;
        let stage1 = device.get_or_load_custom_func(
            "repetition_argmax_bf16_stage1",
            CUDA_KERNEL_MODULE,
            PTX,
        )?;
        let mut builder = stage1.builder();
        builder.arg(&logits);
        builder.arg(&history);
        builder.arg(&partial_values);
        builder.arg(&partial_indices);
        candle_core::builder_arg!(
            builder,
            rows as u32,
            vocab_size as u32,
            PARTITIONS_PER_ROW as u32,
            self.penalty
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

impl CustomOp3 for DFlashRepetitionArgmaxBf16 {
    fn name(&self) -> &'static str {
        "hunyuan-dflash-repetition-argmax-bf16"
    }

    fn cpu_fwd(
        &self,
        _logits: &CpuStorage,
        _logits_layout: &Layout,
        _history: &CpuStorage,
        _history_layout: &Layout,
        _proposals: &CpuStorage,
        _proposals_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("fused DFlash repetition argmax is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        logits: &candle_core::CudaStorage,
        logits_layout: &Layout,
        history: &candle_core::CudaStorage,
        history_layout: &Layout,
        proposals: &candle_core::CudaStorage,
        proposals_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !logits_layout.is_contiguous()
            || !history_layout.is_contiguous()
            || !proposals_layout.is_contiguous()
        {
            candle_core::bail!("fused DFlash repetition argmax requires contiguous tensors")
        }
        let (rows, vocab_size) = logits_layout.shape().dims2()?;
        let history_size = history_layout.shape().dims1()?;
        let proposal_count = proposals_layout.shape().dims1()?;
        if rows != proposal_count + 1
            || history_size != vocab_size
            || logits.dtype() != candle_core::DType::BF16
            || history.dtype() != candle_core::DType::U8
            || proposals.dtype() != candle_core::DType::U32
            || !self.penalty.is_finite()
            || self.penalty < 1.0
        {
            candle_core::bail!(
                "fused DFlash repetition argmax mismatch logits={:?}/{:?} history={:?}/{:?} proposals={:?}/{:?} penalty={}",
                logits_layout.shape(),
                logits.dtype(),
                history_layout.shape(),
                history.dtype(),
                proposals_layout.shape(),
                proposals.dtype(),
                self.penalty
            )
        }

        let device = logits.device().clone();
        let logits = logits.as_cuda_slice::<half::bf16>()?;
        let history = history.as_cuda_slice::<u8>()?;
        let proposals = proposals.as_cuda_slice::<u32>()?;
        let logits = logits.slice(logits_layout.start_offset()..);
        let history = history.slice(history_layout.start_offset()..);
        let proposals = proposals.slice(proposals_layout.start_offset()..);
        let partial_count = rows * PARTITIONS_PER_ROW;
        let partial_values = unsafe { device.alloc::<f32>(partial_count) }?;
        let partial_indices = unsafe { device.alloc::<u32>(partial_count) }?;
        let output = unsafe { device.alloc::<u32>(rows) }?;
        let stage1 = device.get_or_load_custom_func(
            "dflash_repetition_argmax_bf16_stage1",
            CUDA_KERNEL_MODULE,
            PTX,
        )?;
        let mut builder = stage1.builder();
        builder.arg(&logits);
        builder.arg(&history);
        builder.arg(&proposals);
        builder.arg(&partial_values);
        builder.arg(&partial_indices);
        candle_core::builder_arg!(
            builder,
            proposal_count as u32,
            rows as u32,
            vocab_size as u32,
            PARTITIONS_PER_ROW as u32,
            self.penalty
        );
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: (partial_count as u32, 1, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: (proposal_count * std::mem::size_of::<u32>()) as u32,
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

impl InplaceOp2 for RepetitionPenaltyF32 {
    fn name(&self) -> &'static str {
        "hunyuan-repetition-penalty-f32"
    }

    fn cpu_fwd(
        &self,
        _logits: &mut CpuStorage,
        _logits_layout: &Layout,
        _token_ids: &CpuStorage,
        _token_ids_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("fused repetition penalty is CUDA-only")
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

        if !logits_layout.is_contiguous() || !token_ids_layout.is_contiguous() {
            candle_core::bail!("repetition penalty requires contiguous tensors")
        }
        let (rows, vocab_size) = logits_layout.shape().dims2()?;
        let (token_rows, token_stride) = token_ids_layout.shape().dims2()?;
        if !matches!(token_rows, 1) && rows != token_rows
            || logits.dtype() != candle_core::DType::F32
            || token_ids.dtype() != candle_core::DType::U32
            || !self.penalty.is_finite()
            || self.penalty < 1.0
        {
            candle_core::bail!(
                "repetition penalty shape/dtype mismatch logits={:?}/{:?} ids={:?}/{:?} penalty={}",
                logits_layout.shape(),
                logits.dtype(),
                token_ids_layout.shape(),
                token_ids.dtype(),
                self.penalty
            )
        }
        if token_stride == 0 {
            return Ok(());
        }

        let device = logits.device().clone();
        let logits = logits.as_cuda_slice_mut::<f32>()?;
        let token_ids = token_ids.as_cuda_slice::<u32>()?;
        let mut logits = logits.slice_mut(logits_layout.start_offset()..);
        let token_ids = token_ids.slice(token_ids_layout.start_offset()..);
        let count = rows * token_stride;
        let function =
            device.get_or_load_custom_func("repetition_penalty_f32", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&mut logits);
        builder.arg(&token_ids);
        candle_core::builder_arg!(
            builder,
            token_stride as u32,
            token_rows as u32,
            rows as u32,
            vocab_size as u32,
            self.penalty
        );
        unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
        Ok(())
    }
}

impl CustomOp2 for FusedSiluMulBf16 {
    fn name(&self) -> &'static str {
        "hunyuan-fused-silu-mul-bf16"
    }

    fn cpu_fwd(
        &self,
        _gate: &CpuStorage,
        _gate_layout: &Layout,
        _up: &CpuStorage,
        _up_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("fused SiLU*up is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        gate: &candle_core::CudaStorage,
        gate_layout: &Layout,
        up: &candle_core::CudaStorage,
        up_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if gate_layout.shape() != up_layout.shape()
            || gate.dtype() != candle_core::DType::BF16
            || up.dtype() != candle_core::DType::BF16
        {
            candle_core::bail!("fused SiLU*up requires matching BF16 tensors")
        }
        let dims = gate_layout.shape().dims();
        let Some(&ncols) = dims.last() else {
            candle_core::bail!("fused SiLU*up does not support scalar tensors")
        };
        if ncols == 0 {
            candle_core::bail!("fused SiLU*up does not support empty rows")
        }
        // A last-dimension narrow of fused [gate, up] projections has unit
        // column stride but a row stride of 2*ncols. Accept that layout
        // directly so the fusion does not need two materializing copies.
        let flat_row_stride = |layout: &Layout| -> Option<usize> {
            let dims = layout.shape().dims();
            let strides = layout.stride();
            if strides.last().copied()? != 1 {
                return None;
            }
            if dims.len() == 1 {
                return Some(dims[0]);
            }
            let row_dim = dims.len() - 2;
            let row_stride = strides[row_dim];
            let mut expected = row_stride.checked_mul(dims[row_dim])?;
            for dim in (0..row_dim).rev() {
                if dims[dim] > 1 && strides[dim] != expected {
                    return None;
                }
                expected = expected.checked_mul(dims[dim])?;
            }
            Some(row_stride)
        };
        let gate_row_stride = flat_row_stride(gate_layout).ok_or_else(|| {
            candle_core::Error::Msg(format!(
                "unsupported fused SiLU gate layout {gate_layout:?}"
            ))
        })?;
        let up_row_stride = flat_row_stride(up_layout).ok_or_else(|| {
            candle_core::Error::Msg(format!("unsupported fused SiLU up layout {up_layout:?}"))
        })?;
        let device = gate.device().clone();
        let gate = gate.as_cuda_slice::<half::bf16>()?;
        let up = up.as_cuda_slice::<half::bf16>()?;
        let gate = gate.slice(gate_layout.start_offset()..);
        let up = up.slice(up_layout.start_offset()..);
        let count = gate_layout.shape().elem_count();
        let output = unsafe { device.alloc::<half::bf16>(count) }?;
        let function = device.get_or_load_custom_func("silu_mul_bf16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&gate);
        builder.arg(&up);
        builder.arg(&output);
        candle_core::builder_arg!(
            builder,
            count as u32,
            ncols as u32,
            gate_row_stride as u32,
            up_row_stride as u32
        );
        unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            gate_layout.shape().clone(),
        ))
    }
}

impl CustomOp3 for FusedXdRopeRmsNormF16 {
    fn name(&self) -> &'static str {
        "hunyuan-fused-xdrope-rmsnorm-f16"
    }

    fn cpu_fwd(
        &self,
        _qkv: &CpuStorage,
        _qkv_layout: &Layout,
        _cos_sin: &CpuStorage,
        _cos_sin_layout: &Layout,
        _weight: &CpuStorage,
        _weight_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("fused XDRoPE+RMSNorm is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        qkv: &candle_core::CudaStorage,
        qkv_layout: &Layout,
        cos_sin: &candle_core::CudaStorage,
        cos_sin_layout: &Layout,
        weight: &candle_core::CudaStorage,
        weight_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !qkv_layout.is_contiguous()
            || !cos_sin_layout.is_contiguous()
            || !weight_layout.is_contiguous()
        {
            candle_core::bail!("fused XDRoPE+RMSNorm requires contiguous tensors")
        }
        let (batch, qkv_len, projection_width) = qkv_layout.shape().dims3()?;
        if batch != 1
            || qkv_len != self.query_len
            || projection_width != self.projection_width
            || self.head_dim != 128
            || self.projection_offset + self.num_heads * self.head_dim > projection_width
            || cos_sin_layout.shape().elem_count() != 2 * self.query_len * self.head_dim
            || weight_layout.shape().dims1()? != self.head_dim
            || (self.include_v
                && self.projection_offset + 2 * self.num_heads * self.head_dim > projection_width)
        {
            candle_core::bail!(
                "fused XDRoPE+RMSNorm shape mismatch qkv={:?} cos_sin={:?} weight={:?}",
                qkv_layout.shape(),
                cos_sin_layout.shape(),
                weight_layout.shape()
            )
        }
        let device = qkv.device().clone();
        let qkv = qkv.as_cuda_slice::<half::bf16>()?;
        let cos_sin = cos_sin.as_cuda_slice::<f32>()?;
        let weight = weight.as_cuda_slice::<half::bf16>()?;
        let qkv = qkv.slice(qkv_layout.start_offset()..);
        let cos_sin = cos_sin.slice(cos_sin_layout.start_offset()..);
        let weight = weight.slice(weight_layout.start_offset()..);
        let count = self.num_heads * self.query_len * self.head_dim;
        let output_count = if self.include_v { 2 * count } else { count };
        let output = unsafe { device.alloc::<half::f16>(output_count) }?;
        let function =
            device.get_or_load_custom_func("xdrope_rmsnorm_f16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&qkv);
        builder.arg(&cos_sin);
        builder.arg(&weight);
        builder.arg(&output);
        candle_core::builder_arg!(
            builder,
            self.projection_width as u32,
            self.projection_offset as u32,
            self.num_heads as u32,
            self.query_len as u32,
            self.head_dim as u32,
            self.eps,
            u32::from(self.include_v)
        );
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: ((self.num_heads * self.query_len) as u32, 1, 1),
                block_dim: (32, 1, 1),
                shared_mem_bytes: 0,
            })
        }
        .w()?;
        let shape = if self.include_v {
            Shape::from_dims(&[2, 1, self.num_heads, self.query_len, self.head_dim])
        } else {
            Shape::from_dims(&[1, self.num_heads, self.query_len, self.head_dim])
        };
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            shape,
        ))
    }
}

impl CustomOp3 for FusedRmsNormRopeBf16 {
    fn name(&self) -> &'static str {
        "hunyuan-fused-rmsnorm-rope-bf16"
    }

    fn cpu_fwd(
        &self,
        _qkv: &CpuStorage,
        _qkv_layout: &Layout,
        _cos_sin: &CpuStorage,
        _cos_sin_layout: &Layout,
        _weight: &CpuStorage,
        _weight_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("fused RMSNorm+RoPE is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        qkv: &candle_core::CudaStorage,
        qkv_layout: &Layout,
        cos_sin: &candle_core::CudaStorage,
        cos_sin_layout: &Layout,
        weight: &candle_core::CudaStorage,
        weight_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !qkv_layout.is_contiguous()
            || !cos_sin_layout.is_contiguous()
            || !weight_layout.is_contiguous()
        {
            candle_core::bail!("fused RMSNorm+RoPE requires contiguous tensors")
        }
        let (batch, qkv_len, projection_width) = qkv_layout.shape().dims3()?;
        if batch != 1
            || qkv_len != self.query_len
            || projection_width != self.projection_width
            || self.head_dim != 128
            || self.projection_offset + self.num_heads * self.head_dim > projection_width
            || cos_sin_layout.shape().elem_count() != 2 * self.query_len * self.head_dim
            || weight_layout.shape().dims1()? != self.head_dim
            || (self.include_v
                && self.projection_offset + 2 * self.num_heads * self.head_dim > projection_width)
        {
            candle_core::bail!(
                "fused RMSNorm+RoPE shape mismatch qkv={:?} cos_sin={:?} weight={:?}",
                qkv_layout.shape(),
                cos_sin_layout.shape(),
                weight_layout.shape()
            )
        }
        let device = qkv.device().clone();
        let qkv = qkv.as_cuda_slice::<half::bf16>()?;
        let cos_sin = cos_sin.as_cuda_slice::<half::bf16>()?;
        let weight = weight.as_cuda_slice::<half::bf16>()?;
        let qkv = qkv.slice(qkv_layout.start_offset()..);
        let cos_sin = cos_sin.slice(cos_sin_layout.start_offset()..);
        let weight = weight.slice(weight_layout.start_offset()..);
        let count = self.num_heads * self.query_len * self.head_dim;
        let output_count = if self.include_v { 2 * count } else { count };
        let output = unsafe { device.alloc::<half::bf16>(output_count) }?;
        let function =
            device.get_or_load_custom_func("rmsnorm_rope_bf16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&qkv);
        builder.arg(&cos_sin);
        builder.arg(&weight);
        builder.arg(&output);
        candle_core::builder_arg!(
            builder,
            self.projection_width as u32,
            self.projection_offset as u32,
            self.num_heads as u32,
            self.query_len as u32,
            self.head_dim as u32,
            self.eps,
            u32::from(self.include_v)
        );
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: ((self.num_heads * self.query_len) as u32, 1, 1),
                block_dim: (32, 1, 1),
                shared_mem_bytes: 0,
            })
        }
        .w()?;
        let shape = if self.include_v {
            Shape::from_dims(&[2, 1, self.num_heads, self.query_len, self.head_dim])
        } else {
            Shape::from_dims(&[1, self.num_heads, self.query_len, self.head_dim])
        };
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            shape,
        ))
    }
}

impl CustomOp3 for FusedAddRmsNormBf16 {
    fn name(&self) -> &'static str {
        "hunyuan-fused-add-rmsnorm-bf16"
    }

    fn cpu_fwd(
        &self,
        _input: &CpuStorage,
        _input_layout: &Layout,
        _delta: &CpuStorage,
        _delta_layout: &Layout,
        _weight: &CpuStorage,
        _weight_layout: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        candle_core::bail!("fused add+RMSNorm is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        input: &candle_core::CudaStorage,
        input_layout: &Layout,
        delta: &candle_core::CudaStorage,
        delta_layout: &Layout,
        weight: &candle_core::CudaStorage,
        weight_layout: &Layout,
    ) -> Result<(candle_core::CudaStorage, Shape)> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !input_layout.is_contiguous()
            || !delta_layout.is_contiguous()
            || !weight_layout.is_contiguous()
            || input_layout.shape() != delta_layout.shape()
        {
            candle_core::bail!("fused add+RMSNorm requires matching contiguous tensors")
        }
        let dims = input_layout.shape().dims();
        let ncols = *dims
            .last()
            .ok_or_else(|| candle_core::Error::Msg("RMSNorm input is scalar".into()))?;
        if ncols != 1024 || weight_layout.shape().dims1()? != ncols {
            candle_core::bail!(
                "fused add+RMSNorm requires hidden width 1024, got input={:?} weight={:?}",
                input_layout.shape(),
                weight_layout.shape()
            )
        }
        let element_count = input_layout.shape().elem_count();
        let rows = element_count / ncols;
        let device = input.device().clone();
        let input = input.as_cuda_slice::<half::bf16>()?;
        let delta = delta.as_cuda_slice::<half::bf16>()?;
        let weight = weight.as_cuda_slice::<half::bf16>()?;
        let input = input.slice(input_layout.start_offset()..);
        let delta = delta.slice(delta_layout.start_offset()..);
        let weight = weight.slice(weight_layout.start_offset()..);
        let output = unsafe { device.alloc::<half::bf16>(2 * element_count) }?;
        let function =
            device.get_or_load_custom_func("add_rmsnorm_bf16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&input);
        builder.arg(&delta);
        builder.arg(&weight);
        builder.arg(&output);
        candle_core::builder_arg!(builder, ncols as u32, self.eps);
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: (rows as u32, 1, 1),
                block_dim: (ncols as u32, 1, 1),
                shared_mem_bytes: 0,
            })
        }
        .w()?;
        let mut output_shape = Vec::with_capacity(dims.len() + 1);
        output_shape.push(2);
        output_shape.extend_from_slice(dims);
        Ok((
            candle_core::CudaStorage::wrap_cuda_slice(output, device),
            Shape::from_dims(&output_shape),
        ))
    }
}

impl InplaceOp3 for FusedRopeBf16 {
    fn name(&self) -> &'static str {
        "hunyuan-fused-rope-bf16"
    }

    fn cpu_fwd(
        &self,
        _output: &mut CpuStorage,
        _output_layout: &Layout,
        _input: &CpuStorage,
        _input_layout: &Layout,
        _cos_sin: &CpuStorage,
        _cos_sin_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("fused BF16 RoPE is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        output: &mut candle_core::CudaStorage,
        output_layout: &Layout,
        input: &candle_core::CudaStorage,
        input_layout: &Layout,
        cos_sin: &candle_core::CudaStorage,
        cos_sin_layout: &Layout,
    ) -> Result<()> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !output_layout.is_contiguous()
            || !input_layout.is_contiguous()
            || !cos_sin_layout.is_contiguous()
            || output_layout.shape() != input_layout.shape()
        {
            candle_core::bail!("fused BF16 RoPE requires matching contiguous tensors")
        }
        let (_, heads, query_len, head_dim) = output_layout.shape().dims4()?;
        if !head_dim.is_multiple_of(2)
            || cos_sin_layout.shape().elem_count() != 2 * query_len * head_dim
        {
            candle_core::bail!(
                "fused BF16 RoPE shape mismatch input={:?} cos_sin={:?}",
                input_layout.shape(),
                cos_sin_layout.shape()
            )
        }
        let device = output.device().clone();
        let output = output.as_cuda_slice_mut::<half::bf16>()?;
        let input = input.as_cuda_slice::<half::bf16>()?;
        let cos_sin = cos_sin.as_cuda_slice::<half::bf16>()?;
        let mut output = output.slice_mut(output_layout.start_offset()..);
        let input = input.slice(input_layout.start_offset()..);
        let cos_sin = cos_sin.slice(cos_sin_layout.start_offset()..);
        let count = heads * query_len * head_dim;
        let function = device.get_or_load_custom_func("rope_bf16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&mut output);
        builder.arg(&input);
        builder.arg(&cos_sin);
        candle_core::builder_arg!(builder, heads as u32, query_len as u32, head_dim as u32);
        unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
        Ok(())
    }
}

impl InplaceOp3 for FusedXdRope {
    fn name(&self) -> &'static str {
        "hunyuan-fused-xdrope"
    }

    fn cpu_fwd(
        &self,
        _output: &mut CpuStorage,
        _output_layout: &Layout,
        _qkv: &CpuStorage,
        _qkv_layout: &Layout,
        _cos_sin: &CpuStorage,
        _cos_sin_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("fused XDRoPE is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        output: &mut candle_core::CudaStorage,
        output_layout: &Layout,
        qkv: &candle_core::CudaStorage,
        qkv_layout: &Layout,
        cos_sin: &candle_core::CudaStorage,
        cos_sin_layout: &Layout,
    ) -> Result<()> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        if !output_layout.is_contiguous()
            || !qkv_layout.is_contiguous()
            || !cos_sin_layout.is_contiguous()
        {
            candle_core::bail!("fused XDRoPE requires contiguous tensors")
        }
        let (_, heads, query_len, head_dim) = output_layout.shape().dims4()?;
        let (_, qkv_len, projection_width) = qkv_layout.shape().dims3()?;
        if heads != self.num_heads
            || query_len != self.query_len
            || qkv_len != query_len
            || head_dim != self.head_dim
            || projection_width != self.projection_width
            || self.projection_offset + heads * head_dim > projection_width
            || cos_sin_layout.shape().elem_count() != 2 * query_len * head_dim
        {
            candle_core::bail!(
                "fused XDRoPE shape mismatch output={:?} qkv={:?} cos_sin={:?}",
                output_layout.shape(),
                qkv_layout.shape(),
                cos_sin_layout.shape()
            )
        }
        let device = output.device().clone();
        let output = output.as_cuda_slice_mut::<half::bf16>()?;
        let qkv = qkv.as_cuda_slice::<half::bf16>()?;
        let cos_sin = cos_sin.as_cuda_slice::<f32>()?;
        let mut output = output.slice_mut(output_layout.start_offset()..);
        let qkv = qkv.slice(qkv_layout.start_offset()..);
        let cos_sin = cos_sin.slice(cos_sin_layout.start_offset()..);
        let count = heads * query_len * head_dim;
        let function = device.get_or_load_custom_func("xdrope_bf16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&mut output);
        builder.arg(&qkv);
        builder.arg(&cos_sin);
        candle_core::builder_arg!(
            builder,
            projection_width as u32,
            self.projection_offset as u32,
            heads as u32,
            query_len as u32,
            head_dim as u32
        );
        unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
        Ok(())
    }
}

impl InplaceOp3 for DynamicKvAppend {
    fn name(&self) -> &'static str {
        "hunyuan-dynamic-kv-append"
    }

    fn cpu_fwd(
        &self,
        _cache: &mut CpuStorage,
        _cache_layout: &Layout,
        _source: &CpuStorage,
        _source_layout: &Layout,
        _lengths: &CpuStorage,
        _lengths_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("dynamic KV append is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        cache: &mut candle_core::CudaStorage,
        cache_layout: &Layout,
        source: &candle_core::CudaStorage,
        source_layout: &Layout,
        lengths: &candle_core::CudaStorage,
        lengths_layout: &Layout,
    ) -> Result<()> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        let (_, num_heads, cache_len, head_dim) = cache_layout.shape().dims4()?;
        let (_, source_heads, query_len, source_head_dim) = source_layout.shape().dims4()?;
        if num_heads != source_heads
            || head_dim != source_head_dim
            || query_len != self.query_len
            || cache_len != self.cache_len
        {
            candle_core::bail!(
                "dynamic KV shape mismatch cache={:?} source={:?}",
                cache_layout.shape(),
                source_layout.shape()
            )
        }
        if lengths_layout.shape().dims1()? != 2 {
            candle_core::bail!("dynamic KV cumulative lengths must have shape [2]")
        }
        let device = cache.device().clone();
        let lengths = lengths.as_cuda_slice::<u32>()?;
        let lengths = lengths.slice(lengths_layout.start_offset()..);
        let count = num_heads * query_len * head_dim;
        macro_rules! launch {
            ($ty:ty, $function:literal) => {{
                let cache = cache.as_cuda_slice_mut::<$ty>()?;
                let source = source.as_cuda_slice::<$ty>()?;
                let mut cache = cache.slice_mut(cache_layout.start_offset()..);
                let source = source.slice(source_layout.start_offset()..);
                let function =
                    device.get_or_load_custom_func($function, CUDA_KERNEL_MODULE, PTX)?;
                let mut builder = function.builder();
                builder.arg(&mut cache);
                builder.arg(&source);
                builder.arg(&lengths);
                candle_core::builder_arg!(
                    builder,
                    query_len as u32,
                    num_heads as u32,
                    head_dim as u32,
                    cache_len as u32
                );
                unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
            }};
        }
        match cache.dtype() {
            candle_core::DType::F16 => launch!(half::f16, "append_kv_f16"),
            candle_core::DType::BF16 => launch!(half::bf16, "append_kv_bf16"),
            dtype => candle_core::bail!("dynamic KV append does not support {dtype:?}"),
        }
        Ok(())
    }
}

impl InplaceOp3 for DynamicPagedKvAppend {
    fn name(&self) -> &'static str {
        "hunyuan-dynamic-paged-kv-append"
    }

    fn cpu_fwd(
        &self,
        _cache: &mut CpuStorage,
        _cache_layout: &Layout,
        _source: &CpuStorage,
        _source_layout: &Layout,
        _lengths: &CpuStorage,
        _lengths_layout: &Layout,
    ) -> Result<()> {
        candle_core::bail!("dynamic paged KV append is CUDA-only")
    }

    fn cuda_fwd(
        &self,
        cache: &mut candle_core::CudaStorage,
        cache_layout: &Layout,
        source: &candle_core::CudaStorage,
        source_layout: &Layout,
        lengths: &candle_core::CudaStorage,
        lengths_layout: &Layout,
    ) -> Result<()> {
        use candle_core::cuda_backend::WrapErr;
        use candle_core::cuda_backend::cudarc::driver::{LaunchConfig, PushKernelArg};

        let (blocks, page_size, num_heads, head_dim) = cache_layout.shape().dims4()?;
        let (_, source_heads, query_len, source_head_dim) = source_layout.shape().dims4()?;
        if num_heads != source_heads
            || head_dim != source_head_dim
            || query_len != self.query_len
            || blocks * page_size != self.cache_len
        {
            candle_core::bail!(
                "dynamic paged KV shape mismatch cache={:?} source={:?}",
                cache_layout.shape(),
                source_layout.shape()
            )
        }
        if lengths_layout.shape().dims1()? != 2 {
            candle_core::bail!("dynamic paged KV cumulative lengths must have shape [2]")
        }
        if cache.dtype() != candle_core::DType::BF16 || source.dtype() != candle_core::DType::BF16 {
            candle_core::bail!("dynamic paged KV append requires BF16")
        }
        let device = cache.device().clone();
        let cache = cache.as_cuda_slice_mut::<half::bf16>()?;
        let source = source.as_cuda_slice::<half::bf16>()?;
        let lengths = lengths.as_cuda_slice::<u32>()?;
        let mut cache = cache.slice_mut(cache_layout.start_offset()..);
        let source = source.slice(source_layout.start_offset()..);
        let lengths = lengths.slice(lengths_layout.start_offset()..);
        let count = num_heads * query_len * head_dim;
        let function =
            device.get_or_load_custom_func("append_paged_kv_bf16", CUDA_KERNEL_MODULE, PTX)?;
        let mut builder = function.builder();
        builder.arg(&mut cache);
        builder.arg(&source);
        builder.arg(&lengths);
        candle_core::builder_arg!(
            builder,
            query_len as u32,
            num_heads as u32,
            head_dim as u32,
            self.cache_len as u32
        );
        unsafe { builder.launch(LaunchConfig::for_num_elems(count as u32)) }.w()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cuda_kernels::ArgmaxFirstBf16;
    use candle_core::{DType, Device, Tensor};

    #[cfg(feature = "cuda")]
    #[test]
    fn deterministic_bf16_argmax_keeps_first_index() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let mut values = vec![0.0f32; 2 * 1030];
        values[1] = 10.0;
        values[1024] = 10.0;
        values[1030 + 2] = 7.0;
        values[1030 + 1025] = 7.0;
        let logits = Tensor::from_vec(values, (2, 1030), &device)?.to_dtype(DType::BF16)?;
        let tokens = logits
            .apply_op1_no_bwd(&ArgmaxFirstBf16)?
            .to_vec1::<u32>()?;
        assert_eq!(tokens, vec![1, 2]);
        Ok(())
    }

    #[test]
    fn fused_silu_mul_supports_strided_projection_halves() -> Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let gate_up = Tensor::from_vec(
            vec![
                -2.0f32, -1.0, 0.5, 2.0, 3.0, -4.0, // row 0: gate, up
                1.5, -0.5, 4.0, -3.0, 2.0, 0.25, // row 1: gate, up
            ],
            (1, 2, 6),
            &device,
        )?
        .to_dtype(DType::BF16)?;
        let gate = gate_up.narrow(2, 0, 3)?;
        let up = gate_up.narrow(2, 3, 3)?;
        assert!(!gate.is_contiguous());
        assert!(!up.is_contiguous());

        let actual = gate.apply_op2_no_bwd(&up, &FusedSiluMulBf16)?;
        let expected = (candle_nn::ops::silu(&gate)? * up)?;
        assert_eq!(
            actual.to_vec3::<half::bf16>()?,
            expected.to_vec3::<half::bf16>()?
        );
        Ok(())
    }
}
