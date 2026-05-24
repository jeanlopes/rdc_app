//! Shared in-process state bus for the visual debugger.
//!
//! Provides [`DebugUIState`], [`ToolbarAction`], and [`DebugSessionView`] — the
//! same `Arc<RwLock> + watch` pattern used by `egui-introspection`.
//!
//! # Example
//!
//! ```rust,no_run
//! use debug_session_view::{DebugSessionView, ToolbarAction};
//!
//! # async fn example() {
//! let view = DebugSessionView::new();
//! view.publish_action(ToolbarAction::StepOver).await;
//! let state = view.latest().await;
//! assert!(state.is_pressed(ToolbarAction::StepOver));
//! # }
//! ```

#![warn(missing_docs)]

pub mod action;
pub mod state;
pub mod view;

pub use action::ToolbarAction;
pub use state::{BreakpointEntry, DebugSessionState, DebugUIState, ThreadInfo};
pub use view::DebugSessionView;
