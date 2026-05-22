//! Semantic UI tree capture for egui.
//!
//! Instruments an egui application to produce a [`UiSnapshot`] after every
//! rendered frame. The snapshot exposes every widget's identity, kind, label,
//! position, interaction state, and parent-child relationships. An AI agent
//! queries this data via six MCP tools without needing access to source code
//! or visual screenshots.
//!
//! # Quick start
//!
//! ```no_run
//! # async fn example() {
//! use egui_introspection::{IntrospectionContext, IntrospectionStore};
//!
//! let store = IntrospectionStore::new();
//! let mut ctx = IntrospectionContext::new(store.clone());
//!
//! // Inside your egui App::update:
//! // ctx.begin_frame();
//! // let mut iui = ctx.wrap(ui);
//! // iui.button("Sort");   ← response captured automatically
//! // ctx.end_frame(full_output);
//! # }
//! ```

#![warn(missing_docs)]

pub mod context;
pub mod diff;
pub mod layout;
pub mod paint;
pub mod snapshot;
pub mod stable_id;
pub mod ui;
pub mod widget_node;

pub use context::IntrospectionContext;
pub use diff::{SnapshotDiff, WidgetStateDelta};
pub use layout::LayoutPass;
pub use paint::{PaintCmd, PaintPrimitive};
pub use snapshot::{IntrospectionStore, UiSnapshot};
pub use stable_id::{StableIdRegistry, StableWidgetId};
pub use ui::IntrospectableUi;
pub use widget_node::{InputState, WidgetKind, WidgetNode};
