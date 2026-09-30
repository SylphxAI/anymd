//! HGNetV2 backbone shared by PP-DocLayoutV2 and PP-DocLayoutV3.
//!
//! Ported from `transformers/models/hgnet_v2/modeling_hgnet_v2.py`. Both
//! checkpoints use the `L` architecture with `use_learnable_affine_block =
//! false`, so the learnable-affine path is intentionally absent here.

use super::err::{MODEL_NAME, infer_err};
use crate::error::Error;
use candle_core::{D, Tensor};
use candle_nn::{BatchNorm, BatchNormConfig, Conv2d, Conv2dConfig, Module, ModuleT, VarBuilder};

/// Stage layout of the `L` architecture, the only one these checkpoints use.
const STEM_CHANNELS: [usize; 3] = [3, 32, 48];
const STEM_STRIDES: [usize; 5] = [2, 1, 1, 2, 1];
const STAGE_IN_CHANNELS: [usize; 4] = [48, 128, 512, 1024];
const STAGE_MID_CHANNELS: [usize; 4] = [48, 96, 192, 384];
const STAGE_OUT_CHANNELS: [usize; 4] = [128, 512, 1024, 2048];
const STAGE_NUM_BLOCKS: [usize; 4] = [1, 1, 3, 1];
const STAGE_DOWNSAMPLE: [bool; 4] = [false, true, true, true];
const STAGE_DOWNSAMPLE_STRIDES: [usize; 4] = [2, 2, 2, 2];
const STAGE_LIGHT_BLOCK: [bool; 4] = [false, false, true, true];
const STAGE_KERNEL_SIZE: [usize; 4] = [3, 3, 5, 5];
const STAGE_NUM_LAYERS: [usize; 4] = [6, 6, 6, 6];

/// Convolution + batch norm, optionally followed by ReLU.
#[derive(Debug)]
struct ConvLayer {
    conv: Conv2d,
    norm: BatchNorm,
    activation: bool,
}

impl ConvLayer {
    #[allow(clippy::too_many_arguments)]
    fn load(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        groups: usize,
        activation: bool,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let cfg = Conv2dConfig {
            padding: kernel_size.saturating_sub(1) / 2,
            stride,
            dilation: 1,
            groups,
            ..Default::default()
        };
        let conv = candle_nn::conv2d_no_bias(
            in_channels,
            out_channels,
            kernel_size,
            cfg,
            vb.pp("convolution"),
        )
        .map_err(|e| infer_err("load backbone convolution", e))?;
        let norm = candle_nn::batch_norm(
            out_channels,
            BatchNormConfig {
                eps: batch_norm_eps,
                ..Default::default()
            },
            vb.pp("normalization"),
        )
        .map_err(|e| infer_err("load backbone normalization", e))?;
        Ok(Self {
            conv,
            norm,
            activation,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let x = self
            .conv
            .forward(x)
            .map_err(|e| infer_err("backbone convolution", e))?;
        let x = self
            .norm
            .forward_t(&x, false)
            .map_err(|e| infer_err("backbone normalization", e))?;
        if self.activation {
            x.relu().map_err(|e| infer_err("backbone relu", e))
        } else {
            Ok(x)
        }
    }
}

/// Pointwise convolution followed by a depthwise one, used by the light stages.
#[derive(Debug)]
struct ConvLayerLight {
    conv1: ConvLayer,
    conv2: ConvLayer,
}

impl ConvLayerLight {
    fn load(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        Ok(Self {
            conv1: ConvLayer::load(
                in_channels,
                out_channels,
                1,
                1,
                1,
                false,
                batch_norm_eps,
                vb.pp("conv1"),
            )?,
            conv2: ConvLayer::load(
                out_channels,
                out_channels,
                kernel_size,
                1,
                out_channels,
                true,
                batch_norm_eps,
                vb.pp("conv2"),
            )?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        self.conv2.forward(&self.conv1.forward(x)?)
    }
}

#[derive(Debug)]
enum BlockLayer {
    Basic(ConvLayer),
    Light(ConvLayerLight),
}

impl BlockLayer {
    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        match self {
            Self::Basic(layer) => layer.forward(x),
            Self::Light(layer) => layer.forward(x),
        }
    }
}

/// One HGNetV2 block: `layer_num` convolutions whose outputs are concatenated
/// with the input and squeezed back down by the two aggregation convolutions.
#[derive(Debug)]
struct BasicLayer {
    layers: Vec<BlockLayer>,
    aggregation: Vec<ConvLayer>,
    residual: bool,
}

impl BasicLayer {
    #[allow(clippy::too_many_arguments)]
    fn load(
        in_channels: usize,
        middle_channels: usize,
        out_channels: usize,
        layer_num: usize,
        kernel_size: usize,
        residual: bool,
        light_block: bool,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let layers_vb = vb.pp("layers");
        let mut layers = Vec::with_capacity(layer_num);
        for i in 0..layer_num {
            let layer_in = if i == 0 { in_channels } else { middle_channels };
            let vb_i = layers_vb.pp(i);
            layers.push(if light_block {
                BlockLayer::Light(ConvLayerLight::load(
                    layer_in,
                    middle_channels,
                    kernel_size,
                    batch_norm_eps,
                    vb_i,
                )?)
            } else {
                BlockLayer::Basic(ConvLayer::load(
                    layer_in,
                    middle_channels,
                    kernel_size,
                    1,
                    1,
                    true,
                    batch_norm_eps,
                    vb_i,
                )?)
            });
        }

        let total_channels = in_channels + layer_num * middle_channels;
        let aggregation_vb = vb.pp("aggregation");
        let aggregation = vec![
            ConvLayer::load(
                total_channels,
                out_channels / 2,
                1,
                1,
                1,
                true,
                batch_norm_eps,
                aggregation_vb.pp(0),
            )?,
            ConvLayer::load(
                out_channels / 2,
                out_channels,
                1,
                1,
                1,
                true,
                batch_norm_eps,
                aggregation_vb.pp(1),
            )?,
        ];

        Ok(Self {
            layers,
            aggregation,
            residual,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let mut outputs = Vec::with_capacity(self.layers.len() + 1);
        outputs.push(x.clone());
        let mut hidden = x.clone();
        for layer in &self.layers {
            hidden = layer.forward(&hidden)?;
            outputs.push(hidden.clone());
        }
        let mut hidden =
            Tensor::cat(&outputs, 1).map_err(|e| infer_err("backbone block concat", e))?;
        for conv in &self.aggregation {
            hidden = conv.forward(&hidden)?;
        }
        if self.residual {
            hidden = (hidden + x).map_err(|e| infer_err("backbone block residual", e))?;
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct Stage {
    downsample: Option<ConvLayer>,
    blocks: Vec<BasicLayer>,
}

impl Stage {
    fn load(index: usize, batch_norm_eps: f64, vb: VarBuilder) -> Result<Self, Error> {
        let in_channels = STAGE_IN_CHANNELS[index];
        let mid_channels = STAGE_MID_CHANNELS[index];
        let out_channels = STAGE_OUT_CHANNELS[index];
        let num_layers = STAGE_NUM_LAYERS[index];
        let kernel_size = STAGE_KERNEL_SIZE[index];
        let light_block = STAGE_LIGHT_BLOCK[index];

        let downsample = if STAGE_DOWNSAMPLE[index] {
            Some(ConvLayer::load(
                in_channels,
                in_channels,
                3,
                STAGE_DOWNSAMPLE_STRIDES[index],
                in_channels,
                false,
                batch_norm_eps,
                vb.pp("downsample"),
            )?)
        } else {
            None
        };

        let blocks_vb = vb.pp("blocks");
        let mut blocks = Vec::with_capacity(STAGE_NUM_BLOCKS[index]);
        for i in 0..STAGE_NUM_BLOCKS[index] {
            blocks.push(BasicLayer::load(
                if i == 0 { in_channels } else { out_channels },
                mid_channels,
                out_channels,
                num_layers,
                kernel_size,
                i != 0,
                light_block,
                batch_norm_eps,
                blocks_vb.pp(i),
            )?);
        }

        Ok(Self { downsample, blocks })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let mut hidden = match &self.downsample {
            Some(conv) => conv.forward(x)?,
            None => x.clone(),
        };
        for block in &self.blocks {
            hidden = block.forward(&hidden)?;
        }
        Ok(hidden)
    }
}

/// Stem: two parallel paths (max-pooled and doubly-convolved) concatenated,
/// then squeezed by `stem3`/`stem4`.
#[derive(Debug)]
struct Embeddings {
    stem1: ConvLayer,
    stem2a: ConvLayer,
    stem2b: ConvLayer,
    stem3: ConvLayer,
    stem4: ConvLayer,
}

impl Embeddings {
    fn load(batch_norm_eps: f64, vb: VarBuilder) -> Result<Self, Error> {
        let c = STEM_CHANNELS;
        Ok(Self {
            stem1: ConvLayer::load(
                c[0],
                c[1],
                3,
                STEM_STRIDES[0],
                1,
                true,
                batch_norm_eps,
                vb.pp("stem1"),
            )?,
            stem2a: ConvLayer::load(
                c[1],
                c[1] / 2,
                2,
                STEM_STRIDES[1],
                1,
                true,
                batch_norm_eps,
                vb.pp("stem2a"),
            )?,
            stem2b: ConvLayer::load(
                c[1] / 2,
                c[1],
                2,
                STEM_STRIDES[2],
                1,
                true,
                batch_norm_eps,
                vb.pp("stem2b"),
            )?,
            stem3: ConvLayer::load(
                c[1] * 2,
                c[1],
                3,
                STEM_STRIDES[3],
                1,
                true,
                batch_norm_eps,
                vb.pp("stem3"),
            )?,
            stem4: ConvLayer::load(
                c[1],
                c[2],
                1,
                STEM_STRIDES[4],
                1,
                true,
                batch_norm_eps,
                vb.pp("stem4"),
            )?,
        })
    }

    fn forward(&self, pixel_values: &Tensor) -> Result<Tensor, Error> {
        let embedding = self.stem1.forward(pixel_values)?;
        // Torch pads right/bottom by one before each 2x2 stride-1 convolution,
        // and max-pools the padded tensor so both branches keep the same size.
        let padded = pad_right_bottom(&embedding)?;
        let branch = self.stem2a.forward(&padded)?;
        let branch = self.stem2b.forward(&pad_right_bottom(&branch)?)?;
        let pooled = padded
            .max_pool2d_with_stride(2, 1)
            .map_err(|e| infer_err("backbone stem pooling", e))?;
        let embedding = Tensor::cat(&[&pooled, &branch], 1)
            .map_err(|e| infer_err("backbone stem concat", e))?;
        let embedding = self.stem3.forward(&embedding)?;
        self.stem4.forward(&embedding)
    }
}

/// Replicates `torch.nn.functional.pad(x, (0, 1, 0, 1))`: one zero column on
/// the right and one zero row at the bottom.
fn pad_right_bottom(x: &Tensor) -> Result<Tensor, Error> {
    let x = x
        .pad_with_zeros(D::Minus1, 0, 1)
        .map_err(|e| infer_err("backbone stem pad width", e))?;
    x.pad_with_zeros(D::Minus2, 0, 1)
        .map_err(|e| infer_err("backbone stem pad height", e))
}

/// HGNetV2-L backbone returning the requested stage feature maps.
#[derive(Debug)]
pub(super) struct HGNetV2 {
    embedder: Embeddings,
    stages: Vec<Stage>,
    return_idx: Vec<usize>,
}

impl HGNetV2 {
    /// `return_idx` selects which stages to return, e.g. `[1, 2, 3]` for
    /// stage2/stage3/stage4.
    pub(super) fn load(
        return_idx: &[usize],
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let vb = vb.pp("model");
        let embedder = Embeddings::load(batch_norm_eps, vb.pp("embedder"))?;
        let stages_vb = vb.pp("encoder").pp("stages");
        let mut stages = Vec::with_capacity(STAGE_IN_CHANNELS.len());
        for index in 0..STAGE_IN_CHANNELS.len() {
            stages.push(Stage::load(index, batch_norm_eps, stages_vb.pp(index))?);
        }
        if let Some(&bad) = return_idx.iter().find(|&&i| i >= stages.len()) {
            return Err(Error::Config {
                message: format!("{MODEL_NAME}: backbone return_idx {bad} is out of range"),
            });
        }
        Ok(Self {
            embedder,
            stages,
            return_idx: return_idx.to_vec(),
        })
    }

    /// Runs the backbone, returning one feature map per entry of `return_idx`.
    pub(super) fn forward(&self, pixel_values: &Tensor) -> Result<Vec<Tensor>, Error> {
        let mut hidden = self.embedder.forward(pixel_values)?;
        let mut outputs = Vec::with_capacity(self.return_idx.len());
        for (index, stage) in self.stages.iter().enumerate() {
            hidden = stage.forward(&hidden)?;
            if self.return_idx.contains(&index) {
                outputs.push(hidden.clone());
            }
        }
        Ok(outputs)
    }
}
