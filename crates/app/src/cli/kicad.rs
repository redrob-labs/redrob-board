//! `redrob-board kicad-export <board.json> <netlist.json> <outdir> [rules.json]` writes
//! `<outdir>/board.kicad_pcb` and `<outdir>/board.kicad_pro` for the oracle run.

use std::process::ExitCode;

use board_drc::Rules;
use board_kicad::{export_pcb, export_pcb_with_fills, export_project};
use board_layout::Board;
use board_netlist::Netlist;

pub fn main(args: &[String]) -> ExitCode {
    if args.len() < 3 || args.len() > 4 {
        eprintln!("usage: redrob-board kicad-export <board.json> <netlist.json> <outdir> [rules.json]");
        return ExitCode::from(2);
    }
    let run = || -> Result<(), String> {
        let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
        let board = Board::from_json(&read(&args[0])?).map_err(|e| format!("{}: {e}", args[0]))?;
        let netlist = Netlist::from_json(&read(&args[1])?).map_err(|e| format!("{}: {e}", args[1]))?;
        let rules = match args.get(3) {
            Some(p) => serde_json::from_str::<Rules>(&read(p)?).map_err(|e| format!("{p}: {e}"))?,
            None => Rules::default(),
        };
        let out = std::path::Path::new(&args[2]);
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        let pcb = export_pcb(&board, &netlist).map_err(|e| e.to_string())?;
        let filled = export_pcb_with_fills(&board, &netlist).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(out.join("fills")).map_err(|e| e.to_string())?;
        std::fs::write(out.join("fills/board.kicad_pcb"), filled).map_err(|e| e.to_string())?;
        std::fs::write(out.join("board.kicad_pcb"), pcb).map_err(|e| e.to_string())?;
        std::fs::write(out.join("board.kicad_pro"), export_project(&rules, &board)).map_err(|e| e.to_string())?;
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("redrob-board kicad-export: {e}");
            ExitCode::from(2)
        }
    }
}
