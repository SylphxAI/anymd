//! Validation and geometry operations for visual regions.

use serde_json::Value;

use super::VisualBoundingBox;

pub(super) fn parse_box(value: &Value) -> Option<VisualBoundingBox> {
    let box_ = VisualBoundingBox {
        left: value.get("left")?.as_f64()?,
        bottom: value.get("bottom")?.as_f64()?,
        right: value.get("right")?.as_f64()?,
        top: value.get("top")?.as_f64()?,
    };
    valid_box(box_).then_some(box_)
}

pub(super) fn valid_box(box_: VisualBoundingBox) -> bool {
    [box_.left, box_.bottom, box_.right, box_.top]
        .iter()
        .all(|value| value.is_finite())
        && box_.right > box_.left
        && box_.top > box_.bottom
}

pub(super) fn union_two(left: VisualBoundingBox, right: VisualBoundingBox) -> VisualBoundingBox {
    VisualBoundingBox {
        left: left.left.min(right.left),
        bottom: left.bottom.min(right.bottom),
        right: left.right.max(right.right),
        top: left.top.max(right.top),
    }
}

pub(super) fn horizontal_overlap_ratio(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    let overlap = left.right.min(right.right) - left.left.max(right.left);
    if overlap <= 0.0 {
        return 0.0;
    }
    let denominator = (left.right - left.left).min(right.right - right.left);
    if denominator > 0.0 {
        overlap / denominator
    } else {
        0.0
    }
}

pub(super) fn vertical_overlap_ratio(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    let overlap = left.top.min(right.top) - left.bottom.max(right.bottom);
    if overlap <= 0.0 {
        return 0.0;
    }
    let denominator = (left.top - left.bottom).min(right.top - right.bottom);
    if denominator > 0.0 {
        overlap / denominator
    } else {
        0.0
    }
}

pub(super) fn vertical_gap(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    if left.top < right.bottom {
        right.bottom - left.top
    } else if right.top < left.bottom {
        left.bottom - right.top
    } else {
        0.0
    }
}

pub(super) fn horizontal_gap(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    if left.right < right.left {
        right.left - left.right
    } else if right.right < left.left {
        left.left - right.right
    } else {
        0.0
    }
}
