//! Redrob Board desktop entry point.

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]).with_title("Redrob Board"),
        ..Default::default()
    };
    eframe::run_native("Redrob Board", options, Box::new(|_cc| Ok(Box::new(board_app::BoardApp::default()))))
}
