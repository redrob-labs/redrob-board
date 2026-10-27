//! Redrob Board desktop entry point; `redrob-board <command>` is the command line (`cli`).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::dispatch(&args) {
        return code;
    }
    match window(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("redrob-board: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn window(args: &[String]) -> eframe::Result {
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("redrob-board {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]).with_title("Redrob Board").with_app_id("ai.redrob.board"),
        ..Default::default()
    };
    eframe::run_native("Redrob Board", options, Box::new(|_cc| Ok(Box::new(board_app::BoardApp::default()))))
}
