//! `board-fixtures <outdir>` writes every fixture as `<name>.board.json`, `<name>.netlist.json`
//! and `<name>.drc.json` (our DRC summary), for the oracle workflow to compare against KiCad.

use std::collections::BTreeMap;
use std::process::ExitCode;

use board_designs::fixtures::{
    Fixture, divider_netlist, divider_placed, fixtures, sensor_node_four_layer, sensor_node_hand, sensor_node_power_class,
};
use board_designs::sensor_node;
use board_drc::{Rules, check, summary};
use board_gerber::{export_drill, export_layer};
use board_layout::Layer;
use board_route::{RouteRules, route};

/// Fixtures that need the router: the placed boards, routed to completion. KiCad must find them
/// clean too.
fn routed_fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();
    for (name, netlist, mut board) in [
        ("divider_autorouted", divider_netlist(), divider_placed()),
        ("sensor_node_routed", sensor_node(), sensor_node_hand()),
        ("sensor_node_power_class", sensor_node(), sensor_node_power_class()),
        ("sensor_node_four_layer", sensor_node(), sensor_node_four_layer()),
    ] {
        let report = route(&mut board, &netlist, &RouteRules::default());
        assert!(report.is_complete(), "{name}: {report:?}");
        out.push(Fixture { name, netlist, board });
    }
    out
}

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
    let mut all = fixtures();
    all.extend(routed_fixtures());
    for f in all {
        let violations = check(&f.board, &f.netlist, &rules);
        let counts: BTreeMap<String, usize> = summary(&violations).into_iter().map(|(r, n)| (format!("{r:?}"), n)).collect();
        let write = |suffix: &str, text: String| std::fs::write(out.join(format!("{}.{suffix}", f.name)), text);
        // Fabrication outputs, named the way the oracle names its own so the script can pair them.
        let fab = out.join(format!("{}.fab", f.name));
        std::fs::create_dir_all(&fab).ok();
        let fab_write = |name: &str, text: String| std::fs::write(fab.join(name), text);
        let result = write("board.json", f.board.to_json())
            .and_then(|_| write("netlist.json", f.netlist.to_json()))
            .and_then(|_| write("drc.json", serde_json::to_string_pretty(&counts).expect("counts serialise")))
            .and_then(|_| fab_write("board-F_Cu.gtl", export_layer(&f.board, Layer::FrontCopper).write()))
            .and_then(|_| fab_write("board-B_Cu.gbl", export_layer(&f.board, Layer::BackCopper).write()))
            .and_then(|_| {
                for l in &f.board.copper_layers {
                    if let Layer::InnerCopper(n) = l {
                        fab_write(&format!("board-In{n}_Cu.g{n}"), export_layer(&f.board, *l).write())?;
                    }
                }
                Ok(())
            })
            .and_then(|_| fab_write("board-F_Mask.gts", export_layer(&f.board, Layer::FrontMask).write()))
            .and_then(|_| fab_write("board-B_Mask.gbs", export_layer(&f.board, Layer::BackMask).write()))
            .and_then(|_| fab_write("board-F_Paste.gtp", export_layer(&f.board, Layer::FrontPaste).write()))
            .and_then(|_| fab_write("board-B_Paste.gbp", export_layer(&f.board, Layer::BackPaste).write()))
            .and_then(|_| fab_write("board-F_Silkscreen.gto", export_layer(&f.board, Layer::FrontSilk).write()))
            .and_then(|_| fab_write("board-B_Silkscreen.gbo", export_layer(&f.board, Layer::BackSilk).write()))
            .and_then(|_| fab_write("board-Edge_Cuts.gm1", export_layer(&f.board, Layer::EdgeCuts).write()))
            .and_then(|_| fab_write("board.drl", export_drill(&f.board)));
        if let Err(e) = result {
            eprintln!("{}: {e}", f.name);
            return ExitCode::from(2);
        }
        println!("{}: {} violations {:?}", f.name, violations.len(), counts);
    }
    std::fs::write(out.join("rules.json"), serde_json::to_string_pretty(&rules).expect("rules serialise")).ok();
    ExitCode::SUCCESS
}
