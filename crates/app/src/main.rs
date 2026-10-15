//! Redrob Board desktop entry point.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> eframe::Result {
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("redrob-board {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]).with_title("Redrob Board").with_app_id("ai.redrob.board"),
        ..Default::default()
    };
    eframe::run_native("Redrob Board", options, Box::new(|_cc| Ok(Box::new(board_app::BoardApp::default()))))
}
