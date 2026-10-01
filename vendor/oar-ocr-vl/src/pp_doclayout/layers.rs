//! Small building blocks shared by the PP-DocLayout encoder and decoder.

use super::err::infer_err;
use crate::error::Error;
use candle_core::Tensor;
use candle_nn::{BatchNorm, BatchNormConfig, Conv2d, Conv2dConfig, Module, ModuleT, VarBuilder};

/// The activations these checkpoints use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Activation {
    Identity,
    Relu,
    Silu,
    Gelu,
}

impl Activation {
    /// Resolves an activation name from `config.json`.
    pub(super) fn parse(name: &str) -> Result<Self, Error> {
        match name {
            "relu" => Ok(Self::Relu),
            "silu" | "swish" => Ok(Self::Silu),
            "gelu" => Ok(Self::Gelu),
            "identity" | "none" => Ok(Self::Identity),
            other => Err(Error::Config {
                message: format!("PP-DocLayout: unsupported activation '{other}'"),
            }),
        }
    }

    pub(super) fn apply(&self, x: &Tensor) -> Result<Tensor, Error> {
        match self {
            Self::Identity => Ok(x.clone()),
            Self::Relu => x.relu().map_err(|e| infer_err("relu", e)),
            Self::Silu => x.silu().map_err(|e| infer_err("silu", e)),
            Self::Gelu => x.gelu_erf().map_err(|e| infer_err("gelu", e)),
        }
    }
}

/// Convolution, batch norm and an optional activation, the RT-DETR
/// `ConvNormLayer`.
#[derive(Debug)]
pub(super) struct ConvNormLayer {
    conv: Conv2d,
    norm: BatchNorm,
    activation: Activation,
}

impl ConvNormLayer {
    /// `padding` defaults to `(kernel_size - 1) / 2` when `None`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn load(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: Option<usize>,
        activation: Activation,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let cfg = Conv2dConfig {
            padding: padding.unwrap_or(kernel_size.saturating_sub(1) / 2),
            stride,
            dilation: 1,
            groups: 1,
            ..Default::default()
        };
        Ok(Self {
            conv: candle_nn::conv2d_no_bias(
                in_channels,
                out_channels,
                kernel_size,
                cfg,
                vb.pp("conv"),
            )
            .map_err(|e| infer_err("load conv-norm convolution", e))?,
            norm: candle_nn::batch_norm(
                out_channels,
                BatchNormConfig {
                    eps: batch_norm_eps,
                    ..Default::default()
                },
                vb.pp("norm"),
            )
            .map_err(|e| infer_err("load conv-norm normalization", e))?,
            activation,
        })
    }

    pub(super) fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let x = self
            .conv
            .forward(x)
            .map_err(|e| infer_err("conv-norm convolution", e))?;
        let x = self
            .norm
            .forward_t(&x, false)
            .map_err(|e| infer_err("conv-norm normalization", e))?;
        self.activation.apply(&x)
    }
}

/// `nn.Sequential(Conv2d(bias=False), BatchNorm2d)`, stored with numeric child
/// names in the checkpoints.
#[derive(Debug)]
pub(super) struct SequentialConvNorm {
    conv: Conv2d,
    norm: BatchNorm,
}

impl SequentialConvNorm {
    pub(super) fn load(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let cfg = Conv2dConfig {
            padding: kernel_size.saturating_sub(1) / 2,
            stride,
            dilation: 1,
            groups: 1,
            ..Default::default()
        };
        Ok(Self {
            conv: candle_nn::conv2d_no_bias(in_channels, out_channels, kernel_size, cfg, vb.pp(0))
                .map_err(|e| infer_err("load input projection convolution", e))?,
            norm: candle_nn::batch_norm(
                out_channels,
                BatchNormConfig {
                    eps: batch_norm_eps,
                    ..Default::default()
                },
                vb.pp(1),
            )
            .map_err(|e| infer_err("load input projection normalization", e))?,
        })
    }

    pub(super) fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let x = self
            .conv
            .forward(x)
            .map_err(|e| infer_err("input projection convolution", e))?;
        self.norm
            .forward_t(&x, false)
            .map_err(|e| infer_err("input projection normalization", e))
    }
}

/// Convolution, batch norm and activation stored under the `convolution` /
/// `normalization` names (`ResNetConvLayer` upstream).
#[derive(Debug)]
pub(super) struct NamedConvNormLayer {
    conv: Conv2d,
    norm: BatchNorm,
    activation: Activation,
}

impl NamedConvNormLayer {
    pub(super) fn load(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        activation: Activation,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let cfg = Conv2dConfig {
            padding: kernel_size.saturating_sub(1) / 2,
            stride,
            dilation: 1,
            groups: 1,
            ..Default::default()
        };
        Ok(Self {
            conv: candle_nn::conv2d_no_bias(
                in_channels,
                out_channels,
                kernel_size,
                cfg,
                vb.pp("convolution"),
            )
            .map_err(|e| infer_err("load mask convolution", e))?,
            norm: candle_nn::batch_norm(
                out_channels,
                BatchNormConfig {
                    eps: batch_norm_eps,
                    ..Default::default()
                },
                vb.pp("normalization"),
            )
            .map_err(|e| infer_err("load mask normalization", e))?,
            activation,
        })
    }

    pub(super) fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let x = self
            .conv
            .forward(x)
            .map_err(|e| infer_err("mask convolution", e))?;
        let x = self
            .norm
            .forward_t(&x, false)
            .map_err(|e| infer_err("mask normalization", e))?;
        self.activation.apply(&x)
    }
}
