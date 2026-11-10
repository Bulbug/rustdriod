// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod devices;
mod download;
mod images;
mod java;
mod launcher;
mod paths;
mod runner;
mod sdk;
mod virt;

/// One error type for everything that can fail (keeps the code simple).
pub type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([820.0, 640.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Android Emulator Manager",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
