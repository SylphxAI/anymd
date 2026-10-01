//! Utility functions for VL models and document parsing.
//!
//! This module provides:
//! - Device configuration for Candle-based models
//! - Candle tensor utilities for model inference
//! - Markdown conversion for parsed documents
//! - OTSL to HTML table conversion
//! - Image processing helpers

pub mod image;
pub mod table;
pub mod text;

use crate::error::Error;
use crate::geometry::BoundingBox;
use crate::structure::{LayoutElement, LayoutElementType};
use ::image::{GrayImage, RgbImage};
use candle_core::{DType, Device, Tensor};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

/// Parses a device string and creates a Candle [`Device`].
///
/// # Supported formats
///
/// - `"cpu"` selects the CPU device
/// - `"cuda"` or `"gpu"` selects CUDA device 0
/// - `"cuda:N"` selects CUDA device N (e.g., `"cuda:1"`)
/// - `"metal"` selects Metal device 0 (Apple GPU)
/// - `"metal:N"` selects Metal device N (e.g., `"metal:1"` for Mac Pro with multiple GPUs)
///
/// # Errors
///
/// Returns an error if:
/// - The device string is invalid
/// - CUDA is requested but the `cuda` feature is not enabled
/// - Metal is requested but the `metal` feature is not enabled
/// - Device creation fails
///
/// # Examples
///
/// ```no_run
/// use oar_ocr_vl::utils::parse_device;
///
/// # #[allow(unused_variables)]
/// # fn main() -> Result<(), oar_ocr_vl::Error> {
/// // CPU is always available
/// let cpu = parse_device("cpu")?;
///
/// // CUDA examples (only when cuda feature is enabled)
/// # #[cfg(feature = "cuda")]
/// let cuda = parse_device("cuda")?;
/// # #[cfg(feature = "cuda")]
/// let cuda1 = parse_device("cuda:1")?;
///
/// // Metal examples (only when metal feature is enabled)
/// # #[cfg(all(feature = "metal", target_os = "macos"))]
/// let metal = parse_device("metal")?;
/// # #[cfg(all(feature = "metal", target_os = "macos"))]
/// let metal1 = parse_device("metal:1")?;
/// # Ok(())
/// # }
/// ```
#[cfg(not(feature = "cuda"))]
fn cuda_not_enabled() -> Error {
    Error::Config {
        message: "CUDA support not enabled. Compile with --features cuda".to_string(),
    }
}

#[cfg(not(all(feature = "metal", target_os = "macos")))]
fn metal_not_enabled() -> Error {
    Error::Config {
        message: "Metal support not enabled. Compile on macOS with --features metal".to_string(),
    }
}

/// Helper function to parse a device string with an ordinal (e.g., "cuda:1", "metal:0").
#[cfg(any(feature = "cuda", all(feature = "metal", target_os = "macos")))]
fn parse_device_with_ordinal(
    s: &str,
    prefix: &str,
    device_name: &str,
    creator: impl Fn(usize) -> candle_core::Result<Device>,
) -> Result<Device, Error> {
    let ordinal_str = s.strip_prefix(prefix).ok_or_else(|| Error::Config {
        message: format!("Invalid {} device string '{}'", device_name, s),
    })?;
    let ordinal: usize = ordinal_str.parse().map_err(|_| Error::Config {
        message: format!("Invalid {} device ordinal in '{}'", device_name, s),
    })?;
    creator(ordinal).map_err(|e| Error::Config {
        message: format!("Failed to create {} device {}: {}", device_name, ordinal, e),
    })
}

pub fn parse_device(device_str: &str) -> Result<Device, Error> {
    let device_str = device_str.to_lowercase();
    match device_str.as_str() {
        "cpu" => Ok(Device::Cpu),
        "cuda" | "gpu" => {
            #[cfg(feature = "cuda")]
            {
                Device::new_cuda(0).map_err(|e| Error::Config {
                    message: format!("Failed to create CUDA device: {}", e),
                })
            }
            #[cfg(not(feature = "cuda"))]
            {
                Err(cuda_not_enabled())
            }
        }
        "metal" => {
            #[cfg(all(feature = "metal", target_os = "macos"))]
            {
                Device::new_metal(0).map_err(|e| Error::Config {
                    message: format!("Failed to create Metal device: {}", e),
                })
            }
            #[cfg(not(all(feature = "metal", target_os = "macos")))]
            {
                Err(metal_not_enabled())
            }
        }
        s if s.starts_with("cuda:") => {
            #[cfg(feature = "cuda")]
            {
                parse_device_with_ordinal(s, "cuda:", "CUDA", Device::new_cuda)
            }
            #[cfg(not(feature = "cuda"))]
            {
                Err(cuda_not_enabled())
            }
        }
        s if s.starts_with("metal:") => {
            #[cfg(all(feature = "metal", target_os = "macos"))]
            {
                parse_device_with_ordinal(s, "metal:", "Metal", Device::new_metal)
            }
            #[cfg(not(all(feature = "metal", target_os = "macos")))]
            {
                Err(metal_not_enabled())
            }
        }
        _ => Err(Error::Config {
            message: format!(
                "Unknown device: '{}'. Use 'cpu', 'cuda', 'cuda:N', 'metal', or 'metal:N'",
                device_str
            ),
        }),
    }
}

/// Selects the weight/compute dtype for a VL model on `device`.
///
/// BF16 is preferred on accelerators, but Candle's CUDA BF16 kernels are only
/// compiled for compute capability >= 8.0 (Ampere and newer). On older GPUs
/// (GTX 10xx/16xx, RTX 20xx, ...) the first BF16 op fails at runtime, so this
/// probes a tiny BF16 computation on the device and falls back to F16 when
/// the probe fails. CPU always uses F32.
///
/// The `OAR_VL_DTYPE` environment variable (`bf16`, `f16`, or `f32`,
/// case-insensitive) overrides the automatic choice.
pub fn select_dtype(device: &Device) -> DType {
    if let Ok(v) = std::env::var("OAR_VL_DTYPE") {
        match dtype_from_str(&v) {
            Some(dtype) => {
                tracing::info!("using dtype {dtype:?} from OAR_VL_DTYPE={v}");
                return dtype;
            }
            None => {
                tracing::warn!(
                    "ignoring invalid OAR_VL_DTYPE value '{v}' (expected bf16, f16, or f32)"
                );
            }
        }
    }

    if !device.supports_bf16() {
        return DType::F32;
    }
    if bf16_works(device) {
        DType::BF16
    } else {
        tracing::warn!(
            "device does not support BF16 kernels (CUDA compute capability < 8.0?); \
             falling back to F16. Set OAR_VL_DTYPE=f32 if you see numerical issues."
        );
        DType::F16
    }
}

/// Parses a dtype name as accepted by the `OAR_VL_DTYPE` environment variable.
fn dtype_from_str(s: &str) -> Option<DType> {
    match s.to_lowercase().as_str() {
        "bf16" | "bfloat16" => Some(DType::BF16),
        "f16" | "fp16" | "float16" | "half" => Some(DType::F16),
        "f32" | "fp32" | "float32" => Some(DType::F32),
        _ => None,
    }
}

/// Runs a tiny BF16 computation on `device` covering the kernel families used
/// during inference (fill, matmul, affine, cast) and reads the result back,
/// so missing-kernel and unsupported-cuBLAS errors surface here instead of
/// mid-inference.
fn bf16_works(device: &Device) -> bool {
    let probe = || -> candle_core::Result<()> {
        let t = Tensor::ones((2, 2), DType::BF16, device)?;
        let t = t.matmul(&t)?;
        let t = (t * 2.0)?;
        t.to_dtype(DType::F32)?.sum_all()?.to_scalar::<f32>()?;
        Ok(())
    };
    match probe() {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!("BF16 probe failed on {device:?}: {e}");
            false
        }
    }
}

/// Convert Candle error to Error for inference operations.
pub fn candle_to_ocr_inference(
    model_name: &str,
    context: impl Into<String>,
    err: candle_core::Error,
) -> Error {
    Error::Inference {
        model_name: model_name.to_string(),
        context: context.into(),
        source: Box::new(err),
    }
}

/// Free device memory in bytes; `None` when it can't be queried
/// (CPU, or a CUDA `cuMemGetInfo` failure).
///
/// On Metal this is the headroom in the GPU's recommended working set —
/// exceeding it makes the driver page resources every dispatch. Reporting it
/// lets memory-aware fallbacks (chunked vision attention) engage there.
///
/// The Metal figure is conservative: candle's `current_allocated_size` counts
/// pooled buffers, which it only releases on `wait_until_completed`, so free
/// memory reads lower as the pool warms up. Callers should treat it as a lower
/// bound rather than a stable measure of what is available.
pub fn free_device_memory(device: &Device) -> Option<usize> {
    #[cfg(feature = "cuda")]
    if let Device::Cuda(dev) = device {
        return match dev.cuda_stream().context().mem_get_info() {
            Ok((free, _total)) => Some(free),
            Err(e) => {
                tracing::debug!("querying free CUDA memory failed: {e}");
                None
            }
        };
    }
    #[cfg(all(feature = "metal", target_os = "macos"))]
    if let Device::Metal(dev) = device {
        let raw = dev.metal_device();
        let budget = raw.recommended_max_working_set_size();
        let allocated = raw.current_allocated_size();
        // A zero budget means the driver declined to report one; unknown, not
        // "nothing free".
        return (budget > 0).then(|| budget.saturating_sub(allocated));
    }
    let _ = device;
    None
}

/// Formats an error with its full `source()` chain, so underlying candle /
/// CUDA failures (e.g. out-of-memory) aren't hidden by the top-level Display.
pub fn error_chain_message(prefix: &str, e: &(dyn std::error::Error + 'static)) -> String {
    use std::fmt::Write;
    let mut chain = format!("{prefix}: {e}");
    let mut cur = e.source();
    while let Some(s) = cur {
        let _ = write!(chain, "\n  caused by: {s}");
        cur = s.source();
    }
    chain
}

/// Convert Candle error to Error for processing operations.
pub fn candle_to_ocr_processing(
    kind: crate::error::ProcessingStage,
    context: impl Into<String>,
    err: candle_core::Error,
) -> Error {
    Error::Processing {
        kind,
        context: context.into(),
        source: Box::new(err),
    }
}

/// Resolve the safetensors weight files for a model directory.
///
/// Returns the single `model.safetensors` when present, otherwise the sorted
/// list of sharded `model-*.safetensors` files. The returned order is
/// deterministic so `VarBuilder::from_mmaped_safetensors` sees a stable layout.
///
/// `model_name` is only used to prefix the error message when no weights are
/// found.
pub fn collect_safetensors(
    model_dir: &std::path::Path,
    model_name: &str,
) -> Result<Vec<std::path::PathBuf>, Error> {
    let single = model_dir.join("model.safetensors");
    if single.exists() {
        return Ok(vec![single]);
    }

    let read_dir = std::fs::read_dir(model_dir).map_err(|e| Error::Config {
        message: format!(
            "{model_name}: cannot read model dir {}: {e}",
            model_dir.display()
        ),
    })?;
    let mut shards: Vec<std::path::PathBuf> = Vec::new();
    for entry in read_dir {
        // Propagate per-entry I/O errors instead of silently dropping them, so a
        // partially-failed directory scan can't masquerade as "no shards found".
        let path = entry
            .map_err(|e| Error::Config {
                message: format!(
                    "{model_name}: error reading entry in model dir {}: {e}",
                    model_dir.display()
                ),
            })?
            .path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("model-") && s.ends_with(".safetensors"))
        {
            shards.push(path);
        }
    }
    shards.sort();
    if shards.is_empty() {
        return Err(Error::Config {
            message: format!(
                "{model_name}: no model.safetensors or model-*.safetensors found in {}",
                model_dir.display()
            ),
        });
    }
    Ok(shards)
}

/// Read and deserialize a JSON config file (e.g. `config.json` or
/// `preprocessor_config.json`) into `T`.
///
/// `model_name` and `file_name` are only used to build the parse-error message,
/// matching the historical `"failed to parse {model} {file}: {err}"` format.
pub fn load_json_config<T: serde::de::DeserializeOwned>(
    path: impl AsRef<std::path::Path>,
    model_name: &str,
    file_name: &str,
) -> Result<T, Error> {
    let contents = std::fs::read_to_string(path)?;
    serde_json::from_str(&contents).map_err(|e| Error::Config {
        message: format!("failed to parse {model_name} {file_name}: {e}"),
    })
}

/// `serde` default helper: boolean fields that default to `true`.
pub fn default_true() -> bool {
    true
}

/// `serde` default helper: the standard `1/255` pixel rescale factor.
pub fn default_rescale_factor() -> f32 {
    1.0 / 255.0
}

/// Validate that image normalization `mean`/`std` vectors both have length 3
/// (one entry per RGB channel). Shared by every VL image-processor config.
pub fn validate_image_mean_std(
    model_name: &str,
    image_mean: &[f32],
    image_std: &[f32],
) -> Result<(), Error> {
    if image_mean.len() != 3 || image_std.len() != 3 {
        return Err(Error::Config {
            message: format!(
                "{model_name} image_mean/std must have length 3, got mean={} std={}",
                image_mean.len(),
                image_std.len()
            ),
        });
    }
    Ok(())
}

/// Validate that the patch / merge / temporal-patch sizes are all non-zero.
/// Shared by the VL image-processor configs that emit a single combined check.
pub fn validate_patch_merge_temporal(
    model_name: &str,
    patch_size: usize,
    merge_size: usize,
    temporal_patch_size: usize,
) -> Result<(), Error> {
    if patch_size == 0 || merge_size == 0 || temporal_patch_size == 0 {
        return Err(Error::Config {
            message: format!("{model_name} patch_size/merge_size/temporal_patch_size must be > 0"),
        });
    }
    Ok(())
}

/// Build the inverse-frequency vector for a vision rotary embedding:
/// `inv_freq[j] = 1 / theta^(2j / dim)` for `j in 0..dim/2`, as a `(dim/2,)`
/// F32 tensor. Computed in f64 then cast to f32 to match the reference. Shared
/// by the Qwen2-VL-style vision towers (MinerU, PaddleOCR-VL).
pub fn vision_inv_freq(
    dim: usize,
    theta: f64,
    model_name: &str,
    device: &Device,
) -> Result<Tensor, Error> {
    let mut inv_freq = Vec::with_capacity(dim / 2);
    for i in (0..dim).step_by(2) {
        let v = 1f64 / theta.powf(i as f64 / dim as f64);
        inv_freq.push(v as f32);
    }
    Tensor::from_vec(inv_freq, (dim / 2,), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            format!("{model_name}: vision inv_freq tensor failed"),
            e,
        )
    })
}

/// Rotate half of the last dimension for RoPE: `[x1, x2] -> [-x2, x1]`.
///
/// Operates on the final axis only, so it is rank-agnostic — both the 4D
/// text-attention tensors `(batch, heads, seq, head_dim)` and the 3D
/// vision-attention tensors `(seq, heads, head_dim)` share this path.
pub fn rotate_half(x: &Tensor) -> Result<Tensor, Error> {
    let d = x.dim(candle_core::D::Minus1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "rotate_half dim failed",
            e,
        )
    })?;
    let half = d / 2;
    let x1 = x.narrow(candle_core::D::Minus1, 0, half).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "rotate_half slice x1 failed",
            e,
        )
    })?;
    let x2 = x
        .narrow(candle_core::D::Minus1, half, d - half)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "rotate_half slice x2 failed",
                e,
            )
        })?;
    let nx2 = x2.neg().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "rotate_half neg failed",
            e,
        )
    })?;
    Tensor::cat(&[&nx2, &x1], candle_core::D::Minus1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "rotate_half cat failed",
            e,
        )
    })
}

/// Labels PP-StructureV3 leaves out of Markdown: running heads, page
/// furniture and marginalia.
///
/// `formula_number` is here because PaddleOCR-VL drops it too, unless it was
/// merged into the preceding formula.
pub const DEFAULT_MARKDOWN_IGNORE_LABELS: [&str; 8] = [
    "number",
    "footnote",
    "header",
    "header_image",
    "footer",
    "footer_image",
    "aside_text",
    "formula_number",
];

/// [`DEFAULT_MARKDOWN_IGNORE_LABELS`] as owned strings, for the `ignore_labels`
/// arguments below.
pub fn default_markdown_ignore_labels() -> Vec<String> {
    DEFAULT_MARKDOWN_IGNORE_LABELS
        .iter()
        .map(|label| (*label).to_string())
        .collect()
}

// --- OpenOCR / PaddleX markdown compatibility ---

// Matches PaddleX `compile_title_pattern()` in:
// paddlex/inference/pipelines/layout_parsing/result_v2.py
static OPENOCR_TITLE_RE_PATTERN: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| {
    // Note: Rust's `regex` is stricter about escapes inside character classes than Python `re`.
    // Keep the pattern semantically identical while avoiding unnecessary escapes.
    Regex::new(
        r"^\s*((?:[1-9][0-9]*(?:\.[1-9][0-9]*)*[.、]?|[(（](?:[1-9][0-9]*|[一二三四五六七八九十百千万亿零壹贰叁肆伍陆柒捌玖拾]+)[)）]|[一二三四五六七八九十百千万亿零壹贰叁肆伍陆柒捌玖拾]+[、.]?|(?:I|II|III|IV|V|VI|VII|VIII|IX|X)(?:\.|\s)))(\s*)(.*)$",
    )
});

fn openocr_format_title(text: &str) -> String {
    let mut title = text.to_string();
    if let Ok(re) = OPENOCR_TITLE_RE_PATTERN.as_ref()
        && let Some(caps) = re.captures(&title)
    {
        let numbering_raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let numbering = numbering_raw.trim();
        let title_content = caps.get(3).map(|m| m.as_str()).unwrap_or("").trim_start();
        if !numbering.is_empty() {
            title = format!("{numbering} {title_content}");
        }
    }

    title = title.trim_end_matches('.').to_string();
    let level = if title.contains('.') {
        title.chars().filter(|&c| c == '.').count() + 1
    } else {
        1
    };

    format!("{} {}", "#".repeat(level + 1), title)
        .replace("-\n", "")
        .replace('\n', " ")
}

fn openocr_format_centered_by_html(text: &str) -> String {
    let content = text.replace("-\n", "").replace('\n', " ");
    format!("<div style=\"text-align: center;\">{}</div>\n", content)
}

fn openocr_format_table_center_func(html: &str) -> String {
    let mut table_content = html.to_string();
    table_content = table_content.replace(
        "<table>",
        "<table border=1 style='margin: auto; word-wrap: break-word;'>",
    );
    table_content = table_content.replace(
        "<th>",
        "<th style='text-align: center; word-wrap: break-word;'>",
    );
    table_content = table_content.replace(
        "<td>",
        "<td style='text-align: center; word-wrap: break-word;'>",
    );
    table_content
}

fn openocr_format_text_block(text: &str) -> String {
    text.replace("\n\n", "\n").replace('\n', "\n\n")
}

fn openocr_format_content_block(text: &str) -> String {
    text.replace("-\n", "  \n").replace('\n', "  \n")
}

fn openocr_format_first_line(
    text: &str,
    templates_lower: &[&str],
    format_first_line: impl Fn(&str) -> String,
    splitter: &str,
) -> String {
    let mut parts: Vec<String> = text.split(splitter).map(|s| s.to_string()).collect();
    for part in parts.iter_mut() {
        if part.trim().is_empty() {
            continue;
        }
        let lower = part.to_lowercase();
        if templates_lower.iter().any(|t| *t == lower) {
            *part = format_first_line(part);
        }
        break;
    }
    parts.join(splitter)
}

/// Renders layout elements as Markdown.
///
/// Matches `PaddleOCRVLResult._to_markdown(pretty=...)` when labels come from
/// PP-DocLayoutV2/V3: headings take their level from the section numbering
/// (`1.2 Foo` becomes `### 1.2 Foo`), and `abstract`, `reference` and
/// `content` blocks get their own handling.
///
/// # Arguments
/// * `elements` - Layout elements with recognized text, in reading order
/// * `ignore_labels` - Labels to leave out, usually
///   [`default_markdown_ignore_labels`]
/// * `pretty` - Centre captions and tables with inline HTML, as the reference
///   does. With `false` they stay plain text and table markup is unwrapped.
pub fn to_markdown(elements: &[LayoutElement], ignore_labels: &[String], pretty: bool) -> String {
    let mut markdown = String::new();

    for element in elements {
        let label = element.label.as_deref().unwrap_or("");
        if ignore_labels.iter().any(|l| l == label) {
            continue;
        }

        let content = element.text.as_deref().unwrap_or("");

        let formatted = match label {
            // Titles
            "paragraph_title" | "abstract_title" | "reference_title" | "content_title" => {
                openocr_format_title(content)
            }
            "doc_title" => format!("# {}", content)
                .replace("-\n", "")
                .replace('\n', " "),

            // Captions (centered in pretty mode)
            "table_title" | "figure_title" | "chart_title" => {
                if pretty {
                    openocr_format_centered_by_html(content)
                } else {
                    content.to_string()
                }
            }

            // Text blocks
            "text" | "ocr" | "vertical_text" | "reference_content" => {
                openocr_format_text_block(content)
            }

            // Special sections
            "abstract" => openocr_format_first_line(
                content,
                &["摘要", "abstract"],
                |l| format!("## {l}\n"),
                " ",
            ),
            "reference" => openocr_format_first_line(
                content,
                &["参考文献", "references"],
                |l| format!("## {l}"),
                "\n",
            ),
            "content" => openocr_format_content_block(content),

            // Tables
            "table" => {
                if pretty {
                    format!("\n{}", openocr_format_table_center_func(content))
                } else {
                    // `simplify_table_func("\n" + block.content)`
                    format!("\n{}", content)
                        .replace("<html>", "")
                        .replace("</html>", "")
                        .replace("<body>", "")
                        .replace("</body>", "")
                }
            }

            // Formulas are already formatted with $$ in the pipeline.
            "formula" | "display_formula" | "inline_formula" => content.to_string(),

            // Algorithm blocks.
            "algorithm" => content.trim_matches('\n').to_string(),

            // Fallback: use existing heuristic mapping by element_type.
            _ => match element.element_type {
                LayoutElementType::ParagraphTitle => openocr_format_title(content),
                LayoutElementType::DocTitle => format!("# {}", content)
                    .replace("-\n", "")
                    .replace('\n', " "),
                LayoutElementType::Table => {
                    if pretty {
                        format!("\n{}", openocr_format_table_center_func(content))
                    } else {
                        content.to_string()
                    }
                }
                _ => content.to_string(),
            },
        };

        if markdown.is_empty() {
            markdown.push_str(&formatted);
        } else {
            markdown.push_str("\n\n");
            markdown.push_str(&formatted);
        }
    }

    markdown
}

pub use self::table::convert_otsl_to_html;
pub use self::text::truncate_repetitive_content;

/// Calculate the area of a bounding box.
#[inline]
pub fn calculate_bbox_area(bbox: &BoundingBox) -> f32 {
    (bbox.x_max() - bbox.x_min()).abs() * (bbox.y_max() - bbox.y_min()).abs()
}

/// Calculate overlap ratio between two bounding boxes.
pub fn calculate_overlap_ratio(bbox1: &BoundingBox, bbox2: &BoundingBox, mode: &str) -> f32 {
    let x_min_inter = bbox1.x_min().max(bbox2.x_min());
    let y_min_inter = bbox1.y_min().max(bbox2.y_min());
    let x_max_inter = bbox1.x_max().min(bbox2.x_max());
    let y_max_inter = bbox1.y_max().min(bbox2.y_max());

    let inter_width = (x_max_inter - x_min_inter).max(0.0);
    let inter_height = (y_max_inter - y_min_inter).max(0.0);
    let inter_area = inter_width * inter_height;

    let bbox1_area = calculate_bbox_area(bbox1);
    let bbox2_area = calculate_bbox_area(bbox2);

    let ref_area = match mode {
        "union" => bbox1_area + bbox2_area - inter_area,
        "small" => bbox1_area.min(bbox2_area),
        "large" => bbox1_area.max(bbox2_area),
        _ => bbox1_area + bbox2_area - inter_area,
    };

    if ref_area == 0.0 {
        0.0
    } else {
        inter_area / ref_area
    }
}

/// Calculate projection overlap ratio between two bounding boxes.
pub fn calculate_projection_overlap_ratio(
    bbox1: &BoundingBox,
    bbox2: &BoundingBox,
    direction: &str,
    mode: &str,
) -> f32 {
    let (start1, end1, start2, end2) = if direction == "horizontal" {
        (bbox1.x_min(), bbox1.x_max(), bbox2.x_min(), bbox2.x_max())
    } else {
        (bbox1.y_min(), bbox1.y_max(), bbox2.y_min(), bbox2.y_max())
    };

    let intersection_start = start1.max(start2);
    let intersection_end = end1.min(end2);
    let overlap = intersection_end - intersection_start;

    if overlap <= 0.0 {
        return 0.0;
    }

    let ref_width = match mode {
        "union" => end1.max(end2) - start1.min(start2),
        "small" => (end1 - start1).min(end2 - start2),
        "large" => (end1 - start1).max(end2 - start2),
        _ => end1.max(end2) - start1.min(start2),
    };

    if ref_width > 0.0 {
        overlap / ref_width
    } else {
        0.0
    }
}

/// Detected layout box with label and score.
#[derive(Debug, Clone)]
pub struct DetectedBox {
    pub bbox: BoundingBox,
    pub label: String,
    pub score: f32,
}

/// Filter overlapping boxes from layout detection results.
pub fn filter_overlap_boxes(boxes: Vec<DetectedBox>, overlap_threshold: f32) -> Vec<DetectedBox> {
    let boxes: Vec<DetectedBox> = boxes
        .into_iter()
        .filter(|b| b.label != "reference")
        .collect();

    let mut dropped_indexes: HashSet<usize> = HashSet::new();

    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            if dropped_indexes.contains(&i) || dropped_indexes.contains(&j) {
                continue;
            }

            let overlap_ratio = calculate_overlap_ratio(&boxes[i].bbox, &boxes[j].bbox, "small");

            if overlap_ratio > overlap_threshold {
                let area_i = calculate_bbox_area(&boxes[i].bbox);
                let area_j = calculate_bbox_area(&boxes[j].bbox);

                if (boxes[i].label == "image" || boxes[j].label == "image")
                    && boxes[i].label != boxes[j].label
                {
                    continue;
                }

                if area_i >= area_j {
                    dropped_indexes.insert(j);
                } else {
                    dropped_indexes.insert(i);
                }
            }
        }
    }

    boxes
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !dropped_indexes.contains(idx))
        .map(|(_, b)| b)
        .collect()
}

/// Crop white margins from a formula image.
pub fn crop_margin(img: &RgbImage) -> RgbImage {
    let gray: GrayImage = ::image::imageops::grayscale(img);
    let (min_val, max_val) = gray.pixels().fold((255u8, 0u8), |(min, max), p| {
        (min.min(p.0[0]), max.max(p.0[0]))
    });

    if max_val == min_val {
        return img.clone();
    }

    let threshold = 200u8;
    let mut x_min = img.width();
    let mut y_min = img.height();
    let mut x_max = 0;
    let mut y_max = 0;
    let mut found = false;

    for (x, y, pixel) in gray.enumerate_pixels() {
        let normalized = ((pixel.0[0] as f32 - min_val as f32) / (max_val as f32 - min_val as f32)
            * 255.0) as u8;
        if normalized < threshold {
            if x < x_min {
                x_min = x;
            }
            if x > x_max {
                x_max = x;
            }
            if y < y_min {
                y_min = y;
            }
            if y > y_max {
                y_max = y;
            }
            found = true;
        }
    }

    if !found {
        return img.clone();
    }

    let width = (x_max - x_min + 1).min(img.width() - x_min);
    let height = (y_max - y_min + 1).min(img.height() - y_min);

    if width == 0 || height == 0 {
        return img.clone();
    }

    ::image::imageops::crop_imm(img, x_min, y_min, width, height).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(label: &str, text: &str) -> LayoutElement {
        LayoutElement::new(
            BoundingBox::from_coords(0.0, 0.0, 10.0, 10.0),
            LayoutElementType::from_label(label),
            0.9,
        )
        .with_label(label)
        .with_text(text)
    }

    /// The single renderer keeps the reference behaviour that the removed
    /// lightweight one lacked: heading depth follows the section numbering
    /// rather than being a flat `##`.
    #[test]
    fn headings_take_their_level_from_the_numbering() {
        let elements = vec![
            titled("doc_title", "A Paper"),
            titled("paragraph_title", "1 Introduction"),
            titled("paragraph_title", "1.2 Background"),
            titled("paragraph_title", "1.2.3 Details"),
        ];
        let markdown = to_markdown(&elements, &default_markdown_ignore_labels(), true);
        let headings: Vec<&str> = markdown.lines().filter(|l| l.starts_with('#')).collect();
        assert_eq!(
            headings,
            vec![
                "# A Paper",
                "## 1 Introduction",
                "### 1.2 Background",
                "#### 1.2.3 Details",
            ]
        );
    }

    /// `pretty` is the only knob left; it decides whether captions are centred
    /// with inline HTML.
    #[test]
    fn pretty_controls_caption_centring() {
        let elements = vec![titled("figure_title", "Figure 1: a plot")];
        let ignore = default_markdown_ignore_labels();
        assert!(to_markdown(&elements, &ignore, true).contains("text-align: center"));
        assert!(!to_markdown(&elements, &ignore, false).contains("text-align: center"));
    }

    #[test]
    fn test_parse_device_cpu() {
        let device = parse_device("cpu").unwrap();
        assert!(matches!(device, Device::Cpu));
    }

    #[test]
    fn test_error_chain_message_includes_sources() {
        let root = std::io::Error::other("CUDA_ERROR_OUT_OF_MEMORY");
        let err = Error::Inference {
            model_name: "PaddleOCR-VL".to_string(),
            context: "vision attn softmax".to_string(),
            source: Box::new(root),
        };
        let msg = error_chain_message("generation failed", &err);
        assert_eq!(
            msg,
            "generation failed: inference failed in model 'PaddleOCR-VL': vision attn softmax\n  caused by: CUDA_ERROR_OUT_OF_MEMORY"
        );
    }

    #[test]
    fn test_dtype_from_str() {
        assert_eq!(dtype_from_str("bf16"), Some(DType::BF16));
        assert_eq!(dtype_from_str("BF16"), Some(DType::BF16));
        assert_eq!(dtype_from_str("bfloat16"), Some(DType::BF16));
        assert_eq!(dtype_from_str("f16"), Some(DType::F16));
        assert_eq!(dtype_from_str("fp16"), Some(DType::F16));
        assert_eq!(dtype_from_str("f32"), Some(DType::F32));
        assert_eq!(dtype_from_str("float32"), Some(DType::F32));
        assert_eq!(dtype_from_str("f64"), None);
        assert_eq!(dtype_from_str(""), None);
    }

    #[test]
    fn test_bf16_probe_detects_incomplete_backend() {
        // Candle's CPU backend lacks BF16 matmul, so the probe must report
        // false — the same signal a pre-Ampere CUDA device produces. (The CPU
        // path never reaches the probe in select_dtype; supports_bf16() short
        // circuits it to F32.)
        assert!(!bf16_works(&Device::Cpu));
    }

    #[test]
    fn test_select_dtype_cpu_is_f32() {
        assert_eq!(select_dtype(&Device::Cpu), DType::F32);
    }

    #[test]
    fn test_parse_device_invalid() {
        let result = parse_device("invalid");
        assert!(result.is_err());
        if let Err(Error::Config { message }) = result {
            assert!(message.contains("Unknown device"));
        }
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn test_parse_device_metal_with_ordinal() {
        // Test parsing "metal:0" - should always work on Apple devices
        let result = parse_device("metal:0");
        assert!(result.is_ok(), "metal:0 should succeed on Apple devices");

        // Note: metal:1 and higher ordinals may panic in Candle on single-GPU systems,
        // so we don't test them here. The parsing logic itself works correctly.
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn test_parse_device_metal_invalid_ordinal() {
        let result = parse_device("metal:abc");
        assert!(result.is_err());
        if let Err(Error::Config { message }) = result {
            assert!(message.contains("Invalid Metal device ordinal"));
        }
    }

    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    #[test]
    fn test_parse_device_metal_not_enabled() {
        let result = parse_device("metal");
        assert!(result.is_err());
        if let Err(Error::Config { message }) = result {
            assert!(message.contains("Metal support not enabled"));
        }

        let result = parse_device("metal:0");
        assert!(result.is_err());
        if let Err(Error::Config { message }) = result {
            assert!(message.contains("Metal support not enabled"));
        }
    }

    #[test]
    fn test_format_heading() {
        // Avoid false-positive roman numerals (e.g., "Impact" should not become "I mpact").
        assert_eq!(
            openocr_format_title("Impact of Class Labels"),
            "## Impact of Class Labels"
        );
        assert_eq!(
            openocr_format_title("Vision Transformers"),
            "## Vision Transformers"
        );
        assert_eq!(openocr_format_title("Learning Curve"), "## Learning Curve");
        assert_eq!(openocr_format_title("1.2.3 Section"), "#### 1.2.3 Section");
        assert_eq!(openocr_format_title("1.2Title"), "### 1.2 Title");
        assert_eq!(
            openocr_format_title("I. Introduction"),
            "### I. Introduction"
        );
    }
}
