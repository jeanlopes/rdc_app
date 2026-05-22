//! Paint command capture: converts egui's `ClippedShape` stream to [`PaintCmd`].

use serde::{Deserialize, Serialize};
use crate::widget_node::SerializableRect;

/// A single render instruction captured from egui's paint pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintCmd {
    /// The clip rectangle that constrains this draw call.
    pub clip_rect: SerializableRect,
    /// The paint primitive.
    pub primitive: PaintPrimitive,
}

/// Simplified paint primitive types (abstracted from egui's 11-variant `Shape`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaintPrimitive {
    /// A filled/stroked rectangle.
    Rect {
        /// The rect bounds.
        rect: SerializableRect,
        /// Fill colour as `[r, g, b, a]`.
        fill: [u8; 4],
    },
    /// A text draw call.
    Text {
        /// Top-left position.
        pos: [f32; 2],
        /// The text string.
        text: String,
    },
    /// A circle.
    Circle {
        /// Centre position.
        center: [f32; 2],
        /// Radius in pixels.
        radius: f32,
    },
    /// A polyline / path.
    Line {
        /// Number of points.
        point_count: usize,
    },
    /// A tessellated mesh.
    Mesh {
        /// Number of vertices.
        vertex_count: u32,
        /// Number of indices.
        index_count: u32,
    },
    /// Any other shape variant not mapped above.
    Other,
}

/// Convert a slice of egui `ClippedShape`s to `PaintCmd`s.
pub fn from_clipped_shapes(shapes: &[egui::epaint::ClippedShape]) -> Vec<PaintCmd> {
    shapes.iter().map(|cs| {
        let clip_rect: SerializableRect = cs.clip_rect.into();
        let primitive = shape_to_primitive(&cs.shape);
        PaintCmd { clip_rect, primitive }
    }).collect()
}

fn shape_to_primitive(shape: &egui::Shape) -> PaintPrimitive {
    match shape {
        egui::Shape::Rect(r) => PaintPrimitive::Rect {
            rect: r.rect.into(),
            fill: r.fill.to_array(),
        },
        egui::Shape::Text(t) => PaintPrimitive::Text {
            pos: [t.pos.x, t.pos.y],
            text: t.galley.text().to_string(),
        },
        egui::Shape::Circle(c) => PaintPrimitive::Circle {
            center: [c.center.x, c.center.y],
            radius: c.radius,
        },
        egui::Shape::LineSegment { points, .. } => PaintPrimitive::Line {
            point_count: points.len(),
        },
        egui::Shape::Path(p) => PaintPrimitive::Line {
            point_count: p.points.len(),
        },
        egui::Shape::Mesh(m) => PaintPrimitive::Mesh {
            vertex_count: m.vertices.len() as u32,
            index_count: m.indices.len() as u32,
        },
        egui::Shape::Vec(shapes) => {
            // Flatten: report as Other for simplicity
            let _ = shapes;
            PaintPrimitive::Other
        }
        _ => PaintPrimitive::Other,
    }
}
