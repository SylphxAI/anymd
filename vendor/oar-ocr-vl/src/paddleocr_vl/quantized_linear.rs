//! CPU-only GGML weight-only quantization for the Ernie decoder. Quantize
//! directly from pinned safetensors at load; do not dequantize on every token.
use candle_core::quantized::{GgmlDType, QMatMul, QTensor};
use candle_core::{Module, Result, Tensor};

#[derive(Debug, Clone)]
pub(crate) enum Linear {
    Dense(candle_nn::Linear),
    Quantized {
        weight: QMatMul,
        bias: Option<Tensor>,
    },
}

impl Linear {
    /// Only used by the upstream CUDA graph path. Quantization is CPU-only,
    /// so a graph can never own the quantized variant.
    #[cfg(feature = "cuda")]
    pub fn weight(&self) -> &Tensor {
        match self {
            Self::Dense(linear) => linear.weight(),
            Self::Quantized { .. } => unreachable!("CPU quantization cannot enter CUDA graphs"),
        }
    }
}

impl Module for Linear {
    fn forward(&self, input: &Tensor) -> Result<Tensor> {
        match self {
            Self::Dense(linear) => linear.forward(input),
            Self::Quantized { weight, bias } => {
                let output = weight.forward(input)?;
                match bias {
                    Some(bias) => output.broadcast_add(bias),
                    None => Ok(output),
                }
            }
        }
    }
}

pub(crate) fn linear_b(
    input: usize,
    output: usize,
    bias: bool,
    vb: candle_nn::VarBuilder,
) -> Result<Linear> {
    let mode = std::env::var("ANYMD_OCR_QUANTIZATION").unwrap_or_else(|_| "none".into());
    let dtype = match mode.as_str() {
        "none" => return candle_nn::linear_b(input, output, bias, vb).map(Linear::Dense),
        "q8" => GgmlDType::Q8_0,
        "q4" => GgmlDType::Q4_0,
        _ => candle_core::bail!("ANYMD_OCR_QUANTIZATION must be none, q8 or q4"),
    };
    if !vb.device().is_cpu() {
        candle_core::bail!("OCR quantization currently requires CPU");
    }
    let tensor = vb.get((output, input), "weight")?;
    let weight = QMatMul::from_qtensor(QTensor::quantize(&tensor, dtype)?)?;
    let bias = if bias {
        Some(vb.get(output, "bias")?)
    } else {
        None
    };
    Ok(Linear::Quantized { weight, bias })
}

pub(crate) fn linear_no_bias(
    input: usize,
    output: usize,
    vb: candle_nn::VarBuilder,
) -> Result<Linear> {
    linear_b(input, output, false, vb)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ggml_q8_and_q4_forward_preserve_shape_and_values() -> Result<()> {
        let device = candle_core::Device::Cpu;
        let values: Vec<f32> = (0..1024).map(|n| (n % 13) as f32 / 13.0 - 0.5).collect();
        let weights = Tensor::from_vec(values, (32, 32), &device)?;
        let input = Tensor::ones((1, 32), candle_core::DType::F32, &device)?;
        let dense = candle_nn::Linear::new(weights.clone(), None).forward(&input)?;
        for (dtype, tolerance) in [(GgmlDType::Q8_0, 0.1f32), (GgmlDType::Q4_0, 0.6f32)] {
            let linear = Linear::Quantized {
                weight: QMatMul::from_qtensor(QTensor::quantize(&weights, dtype)?)?,
                bias: None,
            };
            let output = linear.forward(&input)?;
            assert_eq!(output.dims(), &[1, 32]);
            for (a, b) in dense
                .flatten_all()?
                .to_vec1::<f32>()?
                .into_iter()
                .zip(output.flatten_all()?.to_vec1::<f32>()?)
            {
                assert!((a - b).abs() < tolerance);
            }
        }
        Ok(())
    }
}
