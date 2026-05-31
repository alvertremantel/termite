mod app;
mod model;

pub fn run() -> color_eyre::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Azide GUI",
        native_options,
        Box::new(|cc| Ok(Box::new(app::AzideGui::new(cc)))),
    )
    .map_err(|error| color_eyre::eyre::eyre!(error.to_string()))
}
