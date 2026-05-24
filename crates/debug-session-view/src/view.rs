//! Shared in-process bus between the egui render loop and the debug/AI layer.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, RwLock};
use tracing::instrument;

use crate::action::ToolbarAction;
use crate::state::DebugUIState;

/// Shared in-process bus. Same pattern as `IntrospectionStore`.
#[derive(Debug, Clone)]
pub struct DebugSessionView {
    /// Shared mutable state.
    pub state: Arc<RwLock<DebugUIState>>,
    /// Watch channel sender used to notify subscribers of state changes.
    pub notifier: watch::Sender<u64>,
}

impl DebugSessionView {
    /// Create a new view with default state.
    pub fn new() -> Self {
        let (notifier, _) = watch::channel(0);
        Self {
            state: Arc::new(RwLock::new(DebugUIState::default())),
            notifier,
        }
    }

    /// Publish the current state to all subscribers by incrementing the frame counter.
    #[instrument(skip(self))]
    pub async fn publish(&self) {
        let next = *self.notifier.borrow() + 1;
        let _ = self.notifier.send(next);
    }

    /// Read the current state (blocking, for sync contexts).
    pub fn latest_blocking(&self) -> DebugUIState {
        let rt = tokio::runtime::Handle::try_current();
        match rt {
            Ok(handle) => handle.block_on(async { self.state.read().await.clone() }),
            Err(_) => {
                // Outside of a runtime, use a temporary single-threaded runtime.
                // This path is mainly for tests without a surrounding runtime.
                tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(async { self.state.read().await.clone() })
            }
        }
    }

    /// Read the current state (async).
    pub async fn latest(&self) -> DebugUIState {
        self.state.read().await.clone()
    }

    /// Subscribe to state changes.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.notifier.subscribe()
    }

    /// Write an action into `recent_actions` and publish.
    #[instrument(skip(self))]
    pub async fn publish_action(&self, action: ToolbarAction) {
        {
            let mut guard = self.state.write().await;
            guard.press_action(action);
        }
        let next = *self.notifier.borrow() + 1;
        let _ = self.notifier.send(next);
    }

    /// Wait for a notification with timeout (useful in tests).
    pub async fn wait_for_change(&self, timeout: Duration) -> Option<u64> {
        let mut rx = self.notifier.subscribe();
        let current = *rx.borrow();
        tokio::time::timeout(timeout, async {
            loop {
                if rx.changed().await.is_err() {
                    return None;
                }
                let v = *rx.borrow();
                if v != current {
                    return Some(v);
                }
            }
        })
        .await
        .ok()
        .flatten()
    }
}

impl Default for DebugSessionView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn view_publish_notifies_watch() {
        let view = DebugSessionView::new();
        let mut rx = view.subscribe();
        view.publish().await;
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), 1);
    }

    #[tokio::test]
    async fn view_read_after_publish() {
        let view = DebugSessionView::new();
        view.publish_action(ToolbarAction::StepOver).await;
        let state = view.latest().await;
        assert!(state.is_pressed(ToolbarAction::StepOver));
    }

    #[test]
    fn view_clone_shares_arc() {
        let view = DebugSessionView::new();
        let clone = view.clone();
        // Use a temporary runtime for the async write.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            view.publish_action(ToolbarAction::Continue).await;
        });
        let state = rt.block_on(async { clone.latest().await });
        assert!(state.is_pressed(ToolbarAction::Continue));
    }
}
