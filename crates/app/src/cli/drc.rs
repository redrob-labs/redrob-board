//! `redrob-board drc <board.json> <netlist.json> [rules.json]` -- exit 0 when clean, 1 when any rule is
//! violated, 2 on a usage or read error. The exit code is the gate; the text is for humans.

use std::process::ExitCode;

use board_drc::{Rules, check, summary};
use board_layout::Board;
use board_netlist::Netlist;

pub fn main(args: &[String]) -> ExitCode {
    if args.len() < 2 || args.len() > 3 {
        eprintln!("usage: redrob-board drc <board.json> <netlist.json> [rules.json]");
        return ExitCode::from(2);
    }
    let read = |path: &str| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    let run = || -> Result<Vec<board_drc::Violation>, String> {
        let board = Board::from_json(&read(&args[0])?).map_err(|e| format!("{}: {e}", args[0]))?;
        let netlist = Netlist::from_json(&read(&args[1])?).map_err(|e| format!("{}: {e}", args[1]))?;
        let rules = match args.get(2) {
            Some(p) => serde_json::from_str::<Rules>(&read(p)?).map_err(|e| format!("{p}: {e}"))?,
            None => Rules::default(),
        };
        Ok(check(&board, &netlist, &rules))
    };
    match run() {
        Err(e) => {
            eprintln!("redrob-board drc: {e}");
            ExitCode::from(2)
        }
        Ok(violations) if violations.is_empty() => {
            println!("DRC: 0 violations");
            ExitCode::SUCCESS
        }
        Ok(violations) => {
            for v in &violations {
                println!("{v}");
            }
            let counts: Vec<String> = summary(&violations).iter().map(|(r, n)| format!("{r:?}={n}")).collect();
            println!("DRC: {} violations ({})", violations.len(), counts.join(", "));
            ExitCode::from(1)
        }
    }
}
