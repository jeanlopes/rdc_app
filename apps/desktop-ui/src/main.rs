mod introspection_bridge;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "RDC Desktop UI",
        native_options,
        Box::new(|cc| Ok(Box::new(introspection_bridge::RdcApp::new(cc)))),
    ).map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
