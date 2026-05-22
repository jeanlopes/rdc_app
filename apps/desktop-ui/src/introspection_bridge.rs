//! Wires `IntrospectableUi` into the eframe App loop and manages `IntrospectionStore`.

use egui_introspection::{IntrospectionContext, IntrospectionStore};

/// The main egui application with introspection enabled.
pub struct RdcApp {
    store: IntrospectionStore,
    ctx: IntrospectionContext,
}

impl RdcApp {
    /// Create a new app. The `IntrospectionStore` is shared with the MCP server.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let store = IntrospectionStore::new();
        let ctx = IntrospectionContext::new(store.clone());
        Self { store, ctx }
    }

    /// Expose the store for injection into the MCP server.
    pub fn store(&self) -> IntrospectionStore {
        self.store.clone()
    }
}

impl eframe::App for RdcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ctx.begin_frame();

        let shapes = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut iui = self.ctx.wrap(ui);
                iui.label("RDC Desktop UI — introspection active");
                iui.button("Example Button");
            });
        }).shapes;

        self.ctx.end_frame(ctx, shapes);
    }
}
