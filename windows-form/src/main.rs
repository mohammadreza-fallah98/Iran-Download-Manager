#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod model;
mod net;
mod server;
mod tray;
mod widgets;

fn main() -> eframe::Result<()> {
    if !tray::ensure_single_instance() {
        return Ok(());
    }
    let args: Vec<String> = std::env::args().collect();
    let screenshot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);

    let (server_tx, server_rx) = std::sync::mpsc::channel::<server::AddRequest>();
    if screenshot.is_none() {
        server::spawn(server_tx);
    }

    let icon = {
        use image::GenericImageView;
        let img = image::load_from_memory(include_bytes!("../assets/icon.png")).ok();
        img.map(|img| {
            let (w, h) = img.dimensions();
            egui::IconData { rgba: img.into_rgba8().into_raw(), width: w, height: h }
        })
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Iran Download Manager")
        .with_inner_size([1060.0, 640.0])
        .with_min_inner_size([700.0, 400.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Iran Download Manager", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, server_rx)))))
}
