//! Wires `IntrospectableUi` into the eframe App loop and manages `IntrospectionStore`.

use egui_introspection::{IntrospectionContext, IntrospectionStore};

/// The main egui application with introspection enabled.
pub struct RdcApp {
    ctx: IntrospectionContext,
}

impl RdcApp {
    /// Create a new app using a pre-existing store (shared with the MCP server).
    pub fn new_with_store(store: IntrospectionStore, _cc: &eframe::CreationContext<'_>) -> Self {
        Self { ctx: IntrospectionContext::new(store) }
    }
}

impl eframe::App for RdcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ctx.begin_frame();

        // Draw UI directly — eframe is already inside its own ctx.run() call.
        // IntrospectableUi captures widget responses as they are rendered.
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut iui = self.ctx.wrap(ui);
            iui.label("RDC Desktop UI — introspection active");
            iui.button("Example Button");
        });

        // eframe manages end_frame() internally; we publish the widget tree now.
        // PaintCmd capture is not available in eframe mode (eframe owns FullOutput).
        self.ctx.end_frame(ctx, vec![]);
    }
}
