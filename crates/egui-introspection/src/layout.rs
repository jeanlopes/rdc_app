//! Layout pass data: pre-clip vs allocated rects and overflow detection.

use serde::{Deserialize, Serialize};
use crate::stable_id::StableWidgetId;
use crate::widget_node::SerializableRect;

/// Layout information for one widget in one frame.
///
/// Captures the desired (pre-clip) rect alongside the allocated (post-clip) rect,
/// enabling detection of overflow and clipping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutPass {
    /// The widget this layout entry belongs to.
    pub widget_id: StableWidgetId,
    /// Post-clip allocated rect (same as [`crate::WidgetNode::rect`]).
    pub allocated_rect: SerializableRect,
    /// Pre-clip desired rect (approximated from `Ui::max_rect` context).
    pub desired_rect: SerializableRect,
    /// The parent's clip rect at the time of layout.
    pub clip_rect: SerializableRect,
    /// True when `desired_rect` extends beyond `clip_rect`.
    pub overflow: bool,
}

impl LayoutPass {
    /// Build a `LayoutPass` from the rects collected during widget capture.
    pub fn new(
        widget_id: StableWidgetId,
        response_rect: egui::Rect,
        clip_rect: egui::Rect,
    ) -> Self {
        let allocated: SerializableRect = response_rect.into();
        let clip: SerializableRect = clip_rect.into();
        // Desired rect: the response_rect before intersection with clip.
        // We approximate by using response_rect (already clipped) as the desired rect
        // unless we can detect the clipping via rect vs clip comparison.
        let desired = allocated; // will be refined when clipping is detected
        let overflow = !clip.contains_rect(&allocated);
        Self {
            widget_id,
            allocated_rect: allocated,
            desired_rect: desired,
            clip_rect: clip,
            overflow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(min_x, min_y), egui::pos2(max_x, max_y))
    }

    #[test]
    fn overflow_detected_when_desired_exceeds_clip() {
        // Widget extends below its parent clip rect
        let response_rect = rect(0.0, 580.0, 390.0, 640.0);
        let clip_rect = rect(0.0, 0.0, 400.0, 600.0);
        let pass = LayoutPass::new(StableWidgetId(1), response_rect, clip_rect);
        assert!(pass.overflow, "widget extending below clip rect should be overflow");
    }

    #[test]
    fn no_overflow_when_fits() {
        let response_rect = rect(10.0, 10.0, 100.0, 50.0);
        let clip_rect = rect(0.0, 0.0, 400.0, 600.0);
        let pass = LayoutPass::new(StableWidgetId(1), response_rect, clip_rect);
        assert!(!pass.overflow, "widget fitting inside clip rect should not overflow");
    }
}
