use super::config::HpdParsingConfig;
use super::processing::{HpdImageInputs, preprocess_image};
use super::vision::HpdVisionModel;
#[cfg(feature = "cuda")]
use crate::cuda_kernels::{ArgmaxFirstBf16, ArgmaxFirstF32};
use crate::error::Error;
use crate::mineru_diffusion::text::{SdarKvCache, SdarModel};
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, RmsNorm, VarBuilder, linear_no_bias, rms_norm};
use image::RgbImage;
use std::collections::VecDeque;
use std::path::Path;
use tokenizers::Tokenizer;

const MODEL_NAME: &str = "HPD-Parsing";
pub const DEFAULT_PROMPT: &str = "document parsing with fork.";
pub const DEFAULT_MAX_NEW_TOKENS: usize = 8_000;
pub const DEFAULT_SPECULATIVE_TOKENS: usize = 6;
const IMAGE_CONTEXT_TOKEN: &str = "<IMG_CONTEXT>";

#[derive(Debug, Clone)]
pub struct HpdGenerationConfig {
    pub max_new_tokens: usize,
    pub use_mtp: bool,
    pub num_speculative_tokens: usize,
    /// Maximum number of parent/content requests advanced in one continuous
    /// decode batch. Newly forked children have admission priority.
    pub max_active_branches: usize,
}

impl Default for HpdGenerationConfig {
    fn default() -> Self {
        Self {
            max_new_tokens: DEFAULT_MAX_NEW_TOKENS,
            use_mtp: true,
            num_speculative_tokens: DEFAULT_SPECULATIVE_TOKENS,
            max_active_branches: 64,
        }
    }
}

impl HpdGenerationConfig {
    fn validate(&self) -> Result<(), Error> {
        if self.use_mtp && self.num_speculative_tokens == 0 {
            return Err(Error::Config {
                message:
                    "HPD-Parsing num_speculative_tokens must be non-zero when P-MTP is enabled"
                        .to_string(),
            });
        }
        if self.max_active_branches == 0 {
            return Err(Error::Config {
                message: "HPD-Parsing max_active_branches must be non-zero".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct HpdOutput {
    pub text: String,
    pub token_ids: Vec<u32>,
    pub parent_token_count: usize,
    pub child_token_counts: Vec<usize>,
    pub runtime: HpdRuntimeStats,
}

#[derive(Debug, Clone, Default)]
pub struct HpdRuntimeStats {
    pub scheduler_rounds: usize,
    pub peak_active_branches: usize,
    pub forked_branches: usize,
    /// Sum of logical prefix lengths attached to children. These are Tensor
    /// views, not duplicated persistent K/V allocations.
    pub shared_prefix_tokens: usize,
    pub mtp_drafted_tokens: usize,
    pub mtp_accepted_tokens: usize,
}

#[derive(Debug)]
struct MtpHead {
    fc: Linear,
    pre_fc_norm_hidden: RmsNorm,
    pre_fc_norm_embedding: RmsNorm,
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
    norm: RmsNorm,
}

impl MtpHead {
    fn load(cfg: &crate::mineru_diffusion::SdarConfig, vb: VarBuilder) -> Result<Self, Error> {
        let load_linear = |input, output, vb, name| {
            linear_no_bias(input, output, vb)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, name, e))
        };
        Ok(Self {
            fc: load_linear(
                cfg.hidden_size * 2,
                cfg.hidden_size,
                vb.pp("fc"),
                "load P-MTP fusion",
            )?,
            pre_fc_norm_hidden: rms_norm(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("pre_fc_norm_hidden"),
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load P-MTP hidden norm", e))?,
            pre_fc_norm_embedding: rms_norm(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("pre_fc_norm_embedding"),
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load P-MTP embedding norm", e))?,
            gate_proj: load_linear(
                cfg.hidden_size,
                cfg.intermediate_size,
                vb.pp("layers.0.mlp.gate_proj"),
                "load P-MTP gate",
            )?,
            up_proj: load_linear(
                cfg.hidden_size,
                cfg.intermediate_size,
                vb.pp("layers.0.mlp.up_proj"),
                "load P-MTP up projection",
            )?,
            down_proj: load_linear(
                cfg.intermediate_size,
                cfg.hidden_size,
                vb.pp("layers.0.mlp.down_proj"),
                "load P-MTP down projection",
            )?,
            norm: rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load P-MTP norm", e))?,
        })
    }

    fn step(&self, hidden: &Tensor, previous_embedding: &Tensor) -> Result<Tensor, Error> {
        let hidden = self
            .pre_fc_norm_hidden
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP hidden norm", e))?;
        let embedding = self
            .pre_fc_norm_embedding
            .forward(previous_embedding)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP embedding norm", e))?;
        let fused = Tensor::cat(&[&hidden, &embedding], D::Minus1)
            .and_then(|x| self.fc.forward(&x))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP fusion", e))?;
        let gate = self
            .gate_proj
            .forward(&fused)
            .and_then(|x| candle_nn::ops::silu(&x))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP gate", e))?;
        let up = self
            .up_proj
            .forward(&fused)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP up projection", e))?;
        let mlp = (gate * up)
            .and_then(|x| self.down_proj.forward(&x))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP MLP", e))?;
        self.norm
            .forward(&(fused + mlp).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "HPD P-MTP residual",
                    e,
                )
            })?)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "P-MTP final norm", e))
    }
}

#[derive(Debug, Clone, Copy)]
enum BranchKind {
    Parent,
    Child(usize),
}

struct BranchState {
    kind: BranchKind,
    cache: SdarKvCache,
    producer_hidden: Tensor,
    pending_token: u32,
    tokens: Vec<u32>,
    max_new_tokens: usize,
    allow_fork: bool,
    finished: bool,
}

#[derive(Debug, Clone, Copy)]
struct ForkEvent {
    branch_index: usize,
    prefix_len: usize,
}

struct SchedulerOutput {
    parent_tokens: Vec<u32>,
    children: Vec<Vec<u32>>,
    runtime: HpdRuntimeStats,
}

/// Native Candle implementation of PaddlePaddle/HPD-Parsing.
pub struct HpdParsing {
    device: Device,
    dtype: DType,
    cfg: HpdParsingConfig,
    tokenizer: Tokenizer,
    vision: HpdVisionModel,
    text: SdarModel,
    mtp: MtpHead,
    image_context_token_id: u32,
    fork_token_id: u32,
    child_token_id: u32,
    stop_token_ids: Vec<u32>,
}

impl HpdParsing {
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = HpdParsingConfig::from_path(model_dir.join("config.json"))?;
        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("HPD-Parsing failed to load tokenizer.json: {e}"),
            })?;
        let image_context_token_id = require_token(&tokenizer, IMAGE_CONTEXT_TOKEN, Some(151671))?;
        require_token(&tokenizer, "<img>", Some(151669))?;
        require_token(&tokenizer, "</img>", Some(151670))?;
        require_token(&tokenizer, "<|im_start|>", Some(151644))?;
        let im_end = require_token(&tokenizer, "<|im_end|>", Some(cfg.eos_token_id))?;
        require_token(&tokenizer, "<FORK>", Some(cfg.fork_token_id))?;
        require_token(&tokenizer, "<CHILD>", Some(cfg.child_token_id))?;

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, MODEL_NAME)?;
        // SAFETY: the checkpoint files must not be mutated while this model is alive.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load safetensors", e))?
        };
        let vision = HpdVisionModel::load(&cfg, vb.clone())?;
        let text = SdarModel::load(&cfg.llm_config, vb.pp("language_model"), dtype)?;
        let mtp = MtpHead::load(&cfg.llm_config, vb.pp("language_model.mtp"))?;
        let mut stop_token_ids = vec![im_end, cfg.eos_token_id];
        stop_token_ids.sort_unstable();
        stop_token_ids.dedup();
        let fork_token_id = cfg.fork_token_id;
        let child_token_id = cfg.child_token_id;
        Ok(Self {
            device,
            dtype,
            cfg,
            tokenizer,
            vision,
            text,
            mtp,
            image_context_token_id,
            fork_token_id,
            child_token_id,
            stop_token_ids,
        })
    }

    /// Parse complete pages with the official fork-enabled prompt.
    pub fn parse(
        &self,
        images: &[RgbImage],
        generation: &HpdGenerationConfig,
    ) -> crate::error::BatchResult<String> {
        Ok(images
            .iter()
            .map(|image| {
                self.generate_one(image, DEFAULT_PROMPT, generation)
                    .map(|x| x.text)
            })
            .collect())
    }

    /// Generate with one caller-provided instruction per image.
    pub fn generate(
        &self,
        images: &[RgbImage],
        prompts: &[impl AsRef<str>],
        generation: &HpdGenerationConfig,
    ) -> crate::error::BatchResult<String> {
        if images.len() != prompts.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "HPD-Parsing image count {} != prompt count {}",
                    images.len(),
                    prompts.len()
                ),
            });
        }
        Ok(images
            .iter()
            .zip(prompts)
            .map(|(image, prompt)| {
                self.generate_one(image, prompt.as_ref(), generation)
                    .map(|x| x.text)
            })
            .collect())
    }

    /// Generate a single page and retain raw IDs plus parent/child statistics.
    pub fn generate_one(
        &self,
        image: &RgbImage,
        instruction: &str,
        generation: &HpdGenerationConfig,
    ) -> Result<HpdOutput, Error> {
        generation.validate()?;
        if generation.max_new_tokens == 0 {
            return Ok(HpdOutput {
                text: String::new(),
                token_ids: Vec::new(),
                parent_token_count: 0,
                child_token_counts: Vec::new(),
                runtime: HpdRuntimeStats::default(),
            });
        }
        let image_inputs = preprocess_image(image, &self.cfg, &self.device, self.dtype)?;
        let image_tokens = self.cfg.image_tokens_per_tile()? * image_inputs.num_tiles;
        let prompt = build_prompt(instruction, image_tokens);
        let encoding = self
            .tokenizer
            .encode(prompt, false)
            .map_err(|e| Error::InvalidInput {
                message: format!("HPD-Parsing tokenizer encode failed: {e}"),
            })?;
        let prompt_ids = encoding.get_ids();
        validate_generation_length(
            prompt_ids.len(),
            generation.max_new_tokens,
            self.cfg.llm_config.max_position_embeddings,
        )?;
        let prompt_embeddings = self.prepare_inputs(prompt_ids, &image_inputs)?;
        let parent_cache = SdarKvCache::with_capacity(
            self.text.num_layers(),
            prompt_ids.len() + generation.max_new_tokens,
        );
        let parent = self.start_branch(
            parent_cache,
            &prompt_embeddings,
            generation.max_new_tokens,
            BranchKind::Parent,
            true,
        )?;
        let scheduled = self.run_scheduler(parent, generation)?;
        let parent_tokens = scheduled.parent_tokens;
        let children = scheduled.children;
        let runtime = scheduled.runtime;

        let parent_token_count = parent_tokens.len();
        let child_token_counts = children.iter().map(Vec::len).collect::<Vec<_>>();
        let mut final_ids =
            Vec::with_capacity(parent_token_count + child_token_counts.iter().sum::<usize>());
        let mut child_index = 0;
        for token in parent_tokens {
            if token == self.fork_token_id {
                final_ids.push(self.child_token_id);
                if let Some(child) = children.get(child_index) {
                    final_ids.extend(child);
                    child_index += 1;
                }
            } else {
                final_ids.push(token);
            }
        }
        let text = self.decode_tokens(&final_ids)?;
        Ok(HpdOutput {
            text,
            token_ids: final_ids,
            parent_token_count,
            child_token_counts,
            runtime,
        })
    }

    fn prepare_inputs(
        &self,
        input_ids: &[u32],
        image_inputs: &HpdImageInputs,
    ) -> Result<Tensor, Error> {
        let token_tensor = Tensor::new(input_ids, &self.device)
            .and_then(|x| x.unsqueeze(0))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create prompt IDs", e))?;
        let token_embeddings = self.text.embed(&token_tensor)?;
        let image_embeddings = self
            .vision
            .forward(&image_inputs.pixel_patches)?
            .to_dtype(self.dtype)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast image embeddings", e))?;
        let positions = input_ids
            .iter()
            .enumerate()
            .filter_map(|(index, &token)| (token == self.image_context_token_id).then_some(index))
            .collect::<Vec<_>>();
        let image_len = image_embeddings
            .dim(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read image token count", e))?;
        if positions.len() != image_len || positions.is_empty() {
            return Err(Error::InvalidInput {
                message: format!(
                    "HPD-Parsing image placeholder count {} != image embedding count {image_len}",
                    positions.len()
                ),
            });
        }
        let start = positions[0];
        if positions
            .iter()
            .enumerate()
            .any(|(offset, &position)| position != start + offset)
        {
            return Err(Error::InvalidInput {
                message: "HPD-Parsing image placeholders must be contiguous".to_string(),
            });
        }
        let end = start + image_len;
        let prefix = token_embeddings
            .narrow(1, 0, start)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select prompt prefix", e))?;
        let image_embeddings = image_embeddings
            .unsqueeze(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "batch image embeddings", e))?;
        let suffix = token_embeddings
            .narrow(1, end, input_ids.len() - end)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select prompt suffix", e))?;
        Tensor::cat(&[&prefix, &image_embeddings, &suffix], 1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "merge multimodal embeddings", e))
    }

    fn start_branch(
        &self,
        mut cache: SdarKvCache,
        first_embeddings: &Tensor,
        max_new_tokens: usize,
        kind: BranchKind,
        allow_fork: bool,
    ) -> Result<BranchState, Error> {
        let hidden = self.lm_forward(first_embeddings, &mut cache)?;
        let producer_hidden = last_hidden(&hidden)?;
        let pending_token = select_last_greedy(&self.text.lm_logits(&hidden)?)?;
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(max_new_tokens)
            .map_err(|e| Error::InvalidInput {
                message: format!("HPD-Parsing cannot reserve generated tokens: {e}"),
            })?;
        Ok(BranchState {
            kind,
            cache,
            producer_hidden,
            pending_token,
            tokens,
            max_new_tokens,
            allow_fork,
            finished: false,
        })
    }

    fn run_scheduler(
        &self,
        parent: BranchState,
        generation: &HpdGenerationConfig,
    ) -> Result<SchedulerOutput, Error> {
        let mut active = vec![parent];
        let mut waiting = VecDeque::new();
        let mut parent_tokens = None;
        let mut children = Vec::<Vec<u32>>::new();
        let mut stats = HpdRuntimeStats::default();
        let child_ids = Tensor::new(&[[self.child_token_id]], &self.device)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create child token", e))?;
        let child_embedding = self.text.embed(&child_ids)?;

        while !active.is_empty() || !waiting.is_empty() {
            while active.len() < generation.max_active_branches {
                let Some(branch) = waiting.pop_front() else {
                    break;
                };
                active.push(branch);
            }
            stats.scheduler_rounds += 1;
            stats.peak_active_branches = stats.peak_active_branches.max(active.len());
            let events = if generation.use_mtp {
                self.advance_mtp_batch(&mut active, generation, &mut stats)?
            } else {
                self.advance_greedy_batch(&mut active)?
            };

            // Fork immediately from the post-verification cache at the exact
            // boundary before <FORK>. The parent stays live and continues in
            // the same scheduler; children are admitted with priority.
            let mut spawned = Vec::with_capacity(events.len());
            for event in events {
                let parent_branch = &active[event.branch_index];
                let child_cache = parent_branch.cache.fork_at(event.prefix_len)?;
                let remaining = self
                    .cfg
                    .llm_config
                    .max_position_embeddings
                    .saturating_sub(event.prefix_len + 1);
                if remaining == 0 {
                    return Err(Error::InvalidInput {
                        message: format!(
                            "HPD-Parsing child at KV position {} has no context capacity",
                            event.prefix_len
                        ),
                    });
                }
                let child_index = children.len();
                children.push(Vec::new());
                stats.forked_branches += 1;
                stats.shared_prefix_tokens += child_cache.shared_prefix_len();
                spawned.push(self.start_branch(
                    child_cache,
                    &child_embedding,
                    generation.max_new_tokens.min(remaining),
                    BranchKind::Child(child_index),
                    false,
                )?);
            }

            let mut unfinished = VecDeque::new();
            for branch in active.drain(..) {
                if branch.finished {
                    match branch.kind {
                        BranchKind::Parent => parent_tokens = Some(branch.tokens),
                        BranchKind::Child(index) => children[index] = branch.tokens,
                    }
                } else {
                    unfinished.push_back(branch);
                }
            }

            // Children bypass the regular FCFS admission gate, matching HPD's
            // scheduler. If the active pool is full, unfinished older requests
            // are preempted into the waiting queue and retain their KV views.
            active.extend(spawned);
            while active.len() < generation.max_active_branches {
                let Some(branch) = unfinished.pop_front().or_else(|| waiting.pop_front()) else {
                    break;
                };
                active.push(branch);
            }
            waiting.extend(unfinished);
            if active.len() > generation.max_active_branches {
                let preempted = active.split_off(generation.max_active_branches);
                waiting.extend(preempted);
            }
        }

        let parent_tokens = parent_tokens.ok_or_else(|| Error::InvalidInput {
            message: "HPD scheduler terminated without a parent result".to_string(),
        })?;
        Ok(SchedulerOutput {
            parent_tokens,
            children,
            runtime: stats,
        })
    }

    fn advance_greedy_batch(&self, branches: &mut [BranchState]) -> Result<Vec<ForkEvent>, Error> {
        let ids = branches.iter().map(|b| b.pending_token).collect::<Vec<_>>();
        let input = Tensor::from_vec(ids, (branches.len(), 1), &self.device)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create greedy batch", e))?;
        let embeddings = self.text.embed(&input)?;
        let starts = branches
            .iter()
            .map(|b| b.cache.seq_len())
            .collect::<Vec<_>>();
        let positions = starts
            .iter()
            .map(|&start| vec![start as i64])
            .collect::<Vec<_>>();
        let mut caches = branches
            .iter_mut()
            .map(|b| &mut b.cache)
            .collect::<Vec<_>>();
        let hidden = self
            .text
            .forward_causal_batch(&embeddings, &positions, &mut caches)?;
        drop(caches);
        let next = greedy_batch_last(&self.text.lm_logits(&hidden)?)?;
        let mut events = Vec::new();
        for (index, branch) in branches.iter_mut().enumerate() {
            branch.producer_hidden = hidden
                .narrow(0, index, 1)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select greedy row", e))?;
            let emitted = branch.pending_token;
            branch.pending_token = next[index];
            self.emit(branch, emitted, starts[index], index, &mut events);
        }
        Ok(events)
    }

    fn advance_mtp_batch(
        &self,
        branches: &mut [BranchState],
        generation: &HpdGenerationConfig,
        stats: &mut HpdRuntimeStats,
    ) -> Result<Vec<ForkEvent>, Error> {
        let batch = branches.len();
        let k = branches
            .iter()
            .fold(generation.num_speculative_tokens, |k, branch| {
                let output_room = branch
                    .max_new_tokens
                    .saturating_sub(branch.tokens.len())
                    .saturating_sub(1);
                let context_room = self
                    .cfg
                    .llm_config
                    .max_position_embeddings
                    .saturating_sub(branch.cache.seq_len())
                    .saturating_sub(1);
                k.min(output_room).min(context_room)
            });
        let hidden_rows = branches
            .iter()
            .map(|b| &b.producer_hidden)
            .collect::<Vec<_>>();
        let mut draft_hidden = Tensor::cat(&hidden_rows, 0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "batch P-MTP hidden", e))?;
        let mut previous = branches.iter().map(|b| b.pending_token).collect::<Vec<_>>();
        let mut drafts = vec![Vec::with_capacity(k); batch];
        for _ in 0..k {
            let ids = Tensor::from_vec(previous, (batch, 1), &self.device)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "batch P-MTP input", e))?;
            let embedding = self.text.embed(&ids)?;
            draft_hidden = self.mtp.step(&draft_hidden, &embedding)?;
            previous = greedy_batch_last(&self.text.lm_logits(&draft_hidden)?)?;
            for (row, &token) in previous.iter().enumerate() {
                drafts[row].push(token);
            }
        }
        stats.mtp_drafted_tokens += batch * k;

        let mut verify = Vec::with_capacity(batch * (k + 1));
        for (branch, row) in branches.iter().zip(&drafts) {
            verify.push(branch.pending_token);
            verify.extend_from_slice(row);
        }
        let verify_ids = Tensor::from_vec(verify, (batch, k + 1), &self.device)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "batch verification IDs", e))?;
        let embeddings = self.text.embed(&verify_ids)?;
        let starts = branches
            .iter()
            .map(|b| b.cache.seq_len())
            .collect::<Vec<_>>();
        let positions = starts
            .iter()
            .map(|&start| (start..start + k + 1).map(|p| p as i64).collect())
            .collect::<Vec<Vec<_>>>();
        let mut caches = branches
            .iter_mut()
            .map(|b| &mut b.cache)
            .collect::<Vec<_>>();
        let hidden = self
            .text
            .forward_causal_batch(&embeddings, &positions, &mut caches)?;
        drop(caches);
        let targets = greedy_batch_rows(&self.text.lm_logits(&hidden)?)?;
        let mut events = Vec::new();
        for index in 0..batch {
            let mut matched = 0;
            while matched < k && drafts[index][matched] == targets[index][matched] {
                matched += 1;
            }
            stats.mtp_accepted_tokens += matched;
            let branch = &mut branches[index];
            branch.cache.trim_to(starts[index] + 1 + matched)?;
            branch.producer_hidden = hidden
                .narrow(0, index, 1)
                .and_then(|x| x.narrow(1, matched, 1))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select P-MTP row", e))?;
            let pending = branch.pending_token;
            branch.pending_token = targets[index][matched];
            self.emit(branch, pending, starts[index], index, &mut events);
            for (offset, &confirmed) in targets[index].iter().take(matched).enumerate() {
                if branch.finished {
                    break;
                }
                self.emit(
                    branch,
                    confirmed,
                    starts[index] + 1 + offset,
                    index,
                    &mut events,
                );
            }
        }
        Ok(events)
    }

    fn emit(
        &self,
        branch: &mut BranchState,
        token: u32,
        prefix_len: usize,
        branch_index: usize,
        events: &mut Vec<ForkEvent>,
    ) {
        if branch.finished {
            return;
        }
        branch.tokens.push(token);
        if branch.allow_fork && token == self.fork_token_id {
            events.push(ForkEvent {
                branch_index,
                prefix_len,
            });
        }
        branch.finished =
            self.stop_token_ids.contains(&token) || branch.tokens.len() >= branch.max_new_tokens;
    }

    fn lm_forward(&self, embeddings: &Tensor, cache: &mut SdarKvCache) -> Result<Tensor, Error> {
        let seq_len = embeddings
            .dim(1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read decoder input length", e))?;
        let start = cache.seq_len() as i64;
        let positions = (start..start + seq_len as i64).collect::<Vec<_>>();
        self.text
            .forward_causal(embeddings, &positions, cache, true)
    }

    pub fn decode_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        decode_protocol_tokens(&self.tokenizer, tokens, &self.stop_token_ids)
            .map(|text| text.trim().to_string())
            .map_err(|e| Error::InvalidInput {
                message: format!("HPD-Parsing tokenizer decode failed: {e}"),
            })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    pub fn config(&self) -> &HpdParsingConfig {
        &self.cfg
    }
}

fn build_prompt(instruction: &str, image_tokens: usize) -> String {
    const SYSTEM: &str = "你是书生·万象，英文名是InternVL，是由上海人工智能实验室、清华大学及多家合作单位联合开发的多模态大语言模型。";
    let mut prompt = String::with_capacity(instruction.len() + image_tokens * 13 + 256);
    prompt.push_str("<|im_start|>system\n");
    prompt.push_str(SYSTEM);
    prompt.push_str("<|im_end|>\n<|im_start|>user\n<img>");
    for _ in 0..image_tokens {
        prompt.push_str(IMAGE_CONTEXT_TOKEN);
    }
    prompt.push_str("</img>\n");
    prompt.push_str(instruction);
    prompt.push_str("<|im_end|>\n<|im_start|>assistant\n");
    prompt
}

fn require_token(tokenizer: &Tokenizer, token: &str, expected: Option<u32>) -> Result<u32, Error> {
    let id = tokenizer.token_to_id(token).ok_or_else(|| Error::Config {
        message: format!("HPD-Parsing tokenizer is missing {token:?}"),
    })?;
    if let Some(expected) = expected
        && id != expected
    {
        return Err(Error::Config {
            message: format!(
                "HPD-Parsing token {token:?} id mismatch: tokenizer {id} != config {expected}"
            ),
        });
    }
    Ok(id)
}

fn validate_generation_length(
    prompt_len: usize,
    max_new_tokens: usize,
    context_limit: usize,
) -> Result<(), Error> {
    let requested = prompt_len
        .checked_add(max_new_tokens)
        .ok_or_else(|| Error::InvalidInput {
            message: "HPD-Parsing requested sequence length overflows usize".to_string(),
        })?;
    if requested > context_limit {
        return Err(Error::InvalidInput {
            message: format!(
                "HPD-Parsing prompt ({prompt_len}) plus max_new_tokens ({max_new_tokens}) exceeds context limit {context_limit}"
            ),
        });
    }
    Ok(())
}

fn last_hidden(hidden: &Tensor) -> Result<Tensor, Error> {
    let len = hidden
        .dim(1)
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read hidden length", e))?;
    hidden
        .narrow(1, len - 1, 1)
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select last hidden", e))
}

fn greedy_batch_rows(logits: &Tensor) -> Result<Vec<Vec<u32>>, Error> {
    let (batch, seq_len, vocab) = logits
        .dims3()
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read batched logits shape", e))?;
    #[cfg(feature = "cuda")]
    if logits.device().is_cuda() && matches!(logits.dtype(), DType::BF16 | DType::F32) {
        let rows = batch
            .checked_mul(seq_len)
            .ok_or_else(|| Error::InvalidInput {
                message: "HPD batched logits row count overflows usize".to_string(),
            })?;
        let logits = logits
            .reshape((rows, vocab))
            .and_then(|x| x.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape batched GPU logits", e))?;
        let ids = match logits.dtype() {
            DType::BF16 => logits.apply_op1_no_bwd(&ArgmaxFirstBf16),
            DType::F32 => logits.apply_op1_no_bwd(&ArgmaxFirstF32),
            _ => unreachable!(),
        }
        .and_then(|ids| ids.to_vec1::<u32>())
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "batched GPU argmax", e))?;
        return Ok(ids.chunks_exact(seq_len).map(<[u32]>::to_vec).collect());
    }
    let _ = (batch, seq_len, vocab);
    logits
        .argmax(D::Minus1)
        .and_then(|ids| ids.to_vec2::<u32>())
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HPD batched verification argmax",
                e,
            )
        })
}

fn decode_protocol_tokens(
    tokenizer: &Tokenizer,
    tokens: &[u32],
    stop_token_ids: &[u32],
) -> tokenizers::Result<String> {
    let visible = tokens
        .iter()
        .copied()
        .filter(|token| !stop_token_ids.contains(token))
        .collect::<Vec<_>>();
    // HPD's <BLOCK>/<FORK>/<CHILD> markers are output protocol, not chat
    // framing. Decode all remaining IDs explicitly so checkpoint metadata
    // cannot accidentally hide structural markers.
    tokenizer.decode(&visible, false)
}

fn greedy_batch_last(logits: &Tensor) -> Result<Vec<u32>, Error> {
    let rows = greedy_batch_rows(logits)?;
    rows.into_iter()
        .map(|row| {
            row.last().copied().ok_or_else(|| Error::InvalidInput {
                message: "HPD received an empty logits row".to_string(),
            })
        })
        .collect()
}

fn select_last_greedy(logits: &Tensor) -> Result<u32, Error> {
    let (_, seq_len, vocab) = logits
        .dims3()
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read logits shape", e))?;
    let logits = logits
        .i((0, seq_len - 1, ..))
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select last logits", e))?;
    #[cfg(feature = "cuda")]
    if logits.device().is_cuda() && matches!(logits.dtype(), DType::BF16 | DType::F32) {
        let logits = logits
            .reshape((1, vocab))
            .and_then(|x| x.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape GPU logits", e))?;
        let ids = match logits.dtype() {
            DType::BF16 => logits.apply_op1_no_bwd(&ArgmaxFirstBf16),
            DType::F32 => logits.apply_op1_no_bwd(&ArgmaxFirstF32),
            _ => unreachable!(),
        }
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GPU greedy argmax", e))?;
        return ids
            .i(0)
            .and_then(|id| id.to_scalar::<u32>())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "copy greedy token", e));
    }
    let _ = vocab;
    logits
        .argmax(D::Minus1)
        .and_then(|id| id.to_scalar::<u32>())
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HPD greedy argmax",
                e,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokenizers::{AddedToken, models::wordlevel::WordLevel};

    #[test]
    fn prompt_matches_internvl_2_5_template() {
        assert_eq!(
            build_prompt("document parsing with fork.", 2),
            "<|im_start|>system\n你是书生·万象，英文名是InternVL，是由上海人工智能实验室、清华大学及多家合作单位联合开发的多模态大语言模型。<|im_end|>\n<|im_start|>user\n<img><IMG_CONTEXT><IMG_CONTEXT></img>\ndocument parsing with fork.<|im_end|>\n<|im_start|>assistant\n"
        );
    }

    #[test]
    fn default_enables_official_hpd_acceleration() {
        let cfg = HpdGenerationConfig::default();
        assert!(cfg.use_mtp);
        assert_eq!(cfg.num_speculative_tokens, 6);
        assert_eq!(cfg.max_active_branches, 64);
    }

    #[test]
    fn scheduler_rejects_an_empty_active_pool() {
        let cfg = HpdGenerationConfig {
            max_active_branches: 0,
            ..HpdGenerationConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn protocol_decode_keeps_structure_and_removes_terminators() {
        let mut tokenizer = Tokenizer::new(WordLevel::default());
        tokenizer
            .add_special_tokens([
                AddedToken::from("<BLOCK>", true),
                AddedToken::from("<CHILD>", true),
                AddedToken::from("<|im_end|>", true),
            ])
            .unwrap();
        let block = tokenizer.token_to_id("<BLOCK>").unwrap();
        let child = tokenizer.token_to_id("<CHILD>").unwrap();
        let end = tokenizer.token_to_id("<|im_end|>").unwrap();

        let decoded = decode_protocol_tokens(&tokenizer, &[block, child, end], &[end]).unwrap();
        assert!(decoded.contains("<BLOCK>"));
        assert!(decoded.contains("<CHILD>"));
        assert!(!decoded.contains("<|im_end|>"));
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn batched_cuda_argmax_keeps_first_tie_for_every_sequence_row() {
        let Ok(device) = Device::new_cuda(0) else {
            return;
        };
        let logits = Tensor::from_vec(
            vec![
                0.0f32, 5.0, 5.0, 1.0, 7.0, 7.0, 2.0, 0.0, 3.0, 1.0, 3.0, 0.0, 4.0, 2.0, 4.0, 1.0,
            ],
            (2, 2, 4),
            &device,
        )
        .unwrap()
        .to_dtype(DType::BF16)
        .unwrap();

        assert_eq!(
            greedy_batch_rows(&logits).unwrap(),
            vec![vec![1, 0], vec![0, 0]]
        );
    }
}
