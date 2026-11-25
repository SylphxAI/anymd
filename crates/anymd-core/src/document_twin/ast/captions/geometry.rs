//! Caption bounding-box validation and relative placement.

use super::super::DocumentAstNode;
use crate::text_index::TextBoundingBox;

pub(super) fn ast_caption_link_box(node: &DocumentAstNode) -> Option<TextBoundingBox> {
    let box_ = node.bounding_boxes.first()?;
    let parsed = TextBoundingBox {
        left: box_.get("left")?.as_f64()?,
        bottom: box_.get("bottom")?.as_f64()?,
        right: box_.get("right")?.as_f64()?,
        top: box_.get("top")?.as_f64()?,
    };
    ([parsed.left, parsed.bottom, parsed.right, parsed.top]
        .into_iter()
        .all(f64::is_finite)
        && parsed.right > parsed.left
        && parsed.top > parsed.bottom)
        .then_some(parsed)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AstCaptionGeometry {
    pub(super) relation: &'static str,
    pub(super) gap: f64,
    pub(super) max_gap: f64,
    pub(super) overlap_ratio: f64,
    pub(super) min_overlap_ratio: f64,
    pub(super) overlap_signal: &'static str,
}

fn ast_overlap_ratio(left_start: f64, left_end: f64, right_start: f64, right_end: f64) -> f64 {
    let overlap = left_end.min(right_end) - left_start.max(right_start);
    let denominator = (left_end - left_start).min(right_end - right_start);
    if overlap <= 0.0 || denominator <= 0.0 {
        0.0
    } else {
        overlap / denominator
    }
}

pub(super) fn ast_caption_geometry(
    caption: TextBoundingBox,
    target: TextBoundingBox,
) -> AstCaptionGeometry {
    if caption.top <= target.bottom {
        return AstCaptionGeometry {
            relation: "below",
            gap: target.bottom - caption.top,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.left,
                caption.right,
                target.left,
                target.right,
            ),
            min_overlap_ratio: 0.2,
            overlap_signal: "horizontal-overlap",
        };
    }
    if caption.bottom >= target.top {
        return AstCaptionGeometry {
            relation: "above",
            gap: caption.bottom - target.top,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.left,
                caption.right,
                target.left,
                target.right,
            ),
            min_overlap_ratio: 0.2,
            overlap_signal: "horizontal-overlap",
        };
    }
    if caption.right <= target.left {
        return AstCaptionGeometry {
            relation: "left",
            gap: target.left - caption.right,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.bottom,
                caption.top,
                target.bottom,
                target.top,
            ),
            min_overlap_ratio: 0.32,
            overlap_signal: "vertical-overlap",
        };
    }
    if caption.left >= target.right {
        return AstCaptionGeometry {
            relation: "right",
            gap: caption.left - target.right,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.bottom,
                caption.top,
                target.bottom,
                target.top,
            ),
            min_overlap_ratio: 0.32,
            overlap_signal: "vertical-overlap",
        };
    }
    let horizontal = ast_overlap_ratio(caption.left, caption.right, target.left, target.right);
    let vertical = ast_overlap_ratio(caption.bottom, caption.top, target.bottom, target.top);
    AstCaptionGeometry {
        relation: "overlapping",
        gap: 0.0,
        max_gap: 0.0,
        overlap_ratio: horizontal.max(vertical),
        min_overlap_ratio: 0.2,
        overlap_signal: if horizontal >= vertical {
            "horizontal-overlap"
        } else {
            "vertical-overlap"
        },
    }
}
