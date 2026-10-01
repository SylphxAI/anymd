//! Bounding-box geometry used by the layout and structure types.

use serde::{Deserialize, Serialize};

/// A 2D point.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Point {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
}

impl Point {
    /// Creates a point.
    #[inline]
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A polygon bounding box. Detectors here emit axis-aligned rectangles, but the
/// representation keeps arbitrary point lists so rotated regions stay exact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundingBox {
    /// Corner points, in order.
    pub points: Vec<Point>,
}

impl BoundingBox {
    /// Creates a box from its corner points.
    #[inline]
    pub fn new(points: Vec<Point>) -> Self {
        Self { points }
    }

    /// Creates an axis-aligned box from two opposite corners.
    pub fn from_coords(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self::new(vec![
            Point::new(x1, y1),
            Point::new(x2, y1),
            Point::new(x2, y2),
            Point::new(x1, y2),
        ])
    }

    /// Smallest x over all points, or `0.0` when empty.
    pub fn x_min(&self) -> f32 {
        self.fold(f32::INFINITY, f32::min, |p| p.x)
    }

    /// Largest x over all points, or `0.0` when empty.
    pub fn x_max(&self) -> f32 {
        self.fold(f32::NEG_INFINITY, f32::max, |p| p.x)
    }

    /// Smallest y over all points, or `0.0` when empty.
    pub fn y_min(&self) -> f32 {
        self.fold(f32::INFINITY, f32::min, |p| p.y)
    }

    /// Largest y over all points, or `0.0` when empty.
    pub fn y_max(&self) -> f32 {
        self.fold(f32::NEG_INFINITY, f32::max, |p| p.y)
    }

    /// Width of the axis-aligned extent.
    pub fn width(&self) -> f32 {
        self.x_max() - self.x_min()
    }

    /// Height of the axis-aligned extent.
    pub fn height(&self) -> f32 {
        self.y_max() - self.y_min()
    }

    /// Intersection over union of the two axis-aligned extents.
    pub fn iou(&self, other: &BoundingBox) -> f32 {
        let inter_width =
            (self.x_max().min(other.x_max()) - self.x_min().max(other.x_min())).max(0.0);
        let inter_height =
            (self.y_max().min(other.y_max()) - self.y_min().max(other.y_min())).max(0.0);
        let inter_area = inter_width * inter_height;
        if inter_area <= 0.0 {
            return 0.0;
        }
        let union = self.width() * self.height() + other.width() * other.height() - inter_area;
        if union > 0.0 { inter_area / union } else { 0.0 }
    }

    fn fold(&self, init: f32, op: fn(f32, f32) -> f32, get: fn(&Point) -> f32) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        self.points.iter().map(get).fold(init, op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_extent_of_a_rectangle() {
        let bbox = BoundingBox::from_coords(10.0, 20.0, 40.0, 60.0);
        assert_eq!(bbox.x_min(), 10.0);
        assert_eq!(bbox.y_min(), 20.0);
        assert_eq!(bbox.x_max(), 40.0);
        assert_eq!(bbox.y_max(), 60.0);
        assert_eq!(bbox.width(), 30.0);
        assert_eq!(bbox.height(), 40.0);
    }

    /// An empty box must not leak infinities into downstream arithmetic.
    #[test]
    fn reports_zero_for_an_empty_box() {
        let bbox = BoundingBox::new(Vec::new());
        assert_eq!(bbox.x_min(), 0.0);
        assert_eq!(bbox.y_max(), 0.0);
    }
}
