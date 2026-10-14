//! `board-fixtures <outdir>` writes every fixture as `<name>.board.json`, `<name>.netlist.json`
//! and `<name>.drc.json` (our DRC summary), for the oracle workflow to compare against KiCad.

use std::collections::BTreeMap;
use std::process::ExitCode;

use board_designs::fixtures::fixtures;
use board_drc::{Rules, check, summary};

fn main() -> ExitCode {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: board-fixtures <outdir>");
        return ExitCode::from(2);
    };
    let out = std::path::Path::new(&out);
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("{}: {e}", out.display());
        return ExitCode::from(2);
    }
    let rules = Rules::default();
    for f in fixtures() {
        let violations = check(&f.board, &f.netlist, &rules);
        let counts: BTreeMap<String, usize> = summary(&violations).into_iter().map(|(r, n)| (format!("{r:?}"), n)).collect();
        let write = |suffix: &str, text: String| std::fs::write(out.join(format!("{}.{suffix}", f.name)), text);
        let result = write("board.json", f.board.to_json())
            .and_then(|_| write("netlist.json", f.netlist.to_json()))
            .and_then(|_| write("drc.json", serde_json::to_string_pretty(&counts).expect("counts serialise")));
        if let Err(e) = result {
            eprintln!("{}: {e}", f.name);
            return ExitCode::from(2);
        }
        println!("{}: {} violations {:?}", f.name, violations.len(), counts);
    }
    std::fs::write(out.join("rules.json"), serde_json::to_string_pretty(&rules).expect("rules serialise")).ok();
    ExitCode::SUCCESS
}
