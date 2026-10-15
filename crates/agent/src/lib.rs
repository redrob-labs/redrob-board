//! The agent's tool surface for Redrob Board.
//!
//! Every tool is a plain function over the data types the other crates already use, plus a JSON
//! envelope (`ToolCall` / `ToolResult`) so an engine can drive the same functions by message. The
//! functions are the product; the envelope is just how an agent reaches them. Nothing here is a
//! stub: each tool runs the real library and reports the real outcome.

#![forbid(unsafe_code)]

pub mod workspace;
pub mod zipfile;

use std::path::Path;

use board_bom::{bom_csv, cpl_csv};
use board_designs::{library, sensor_node};
use board_drc::{Rules, Violation, check, summary};
use board_geom::{Point, Rect};
use board_gerber::{export_drill, export_layer};
use board_layout::{Board, Layer};
use board_lib::Library;
use board_netlist::Netlist;
use board_place::{Constraints, Edge, PlaceError, place};
use board_route::{RouteReport, RouteRules, route};
use board_units::Nm;
use serde::{Deserialize, Serialize};

/// Designs the agent can start from by name.
pub fn example_design(name: &str) -> Option<Netlist> {
    match name {
        "sensor-node" | "sensor_node" => Some(sensor_node()),
        "divider" => Some(board_designs::fixtures::divider_netlist()),
        _ => None,
    }
}

pub fn example_names() -> &'static [&'static str] {
    &["sensor-node", "divider"]
}

/// Placement with the default library and an `width x height` mm outline, origin bottom-left.
pub fn place_design(
    netlist: &Netlist,
    lib: &Library,
    width_mm: f64,
    height_mm: f64,
    edge_parts: &[(String, Edge)],
    seed: u64,
) -> Result<Board, PlaceError> {
    let outline = Rect::from_points(Point::ORIGIN, Point::new(Nm::from_mm_f64(width_mm), Nm::from_mm_f64(height_mm)));
    let c = Constraints { edge_parts: edge_parts.to_vec(), seed, ..Constraints::default() };
    place(netlist, lib, outline, &c)
}

pub fn route_design(board: &mut Board, netlist: &Netlist) -> RouteReport {
    route(board, netlist, &RouteRules::default())
}

pub fn drc_design(board: &Board, netlist: &Netlist) -> Vec<Violation> {
    check(board, netlist, &drc_rules())
}

/// DRC limits consistent with what the router was asked to keep.
pub fn drc_rules() -> Rules {
    let r = RouteRules::default();
    Rules { clearance: r.clearance, min_track_width: r.track_width, edge_clearance: r.edge_clearance, ..Rules::default() }
}

/// The manufacturing files as `(name, text)`: Gerbers, drill, BOM, pick-and-place, plus the
/// design's own JSON.
pub fn fab_files(board: &Board, netlist: &Netlist) -> Vec<(&'static str, String)> {
    vec![
        ("board-F_Cu.gtl", export_layer(board, Layer::FrontCopper).write()),
        ("board-B_Cu.gbl", export_layer(board, Layer::BackCopper).write()),
        ("board-F_Mask.gts", export_layer(board, Layer::FrontMask).write()),
        ("board-B_Mask.gbs", export_layer(board, Layer::BackMask).write()),
        ("board-F_Paste.gtp", export_layer(board, Layer::FrontPaste).write()),
        ("board-B_Paste.gbp", export_layer(board, Layer::BackPaste).write()),
        ("board-F_Silkscreen.gto", export_layer(board, Layer::FrontSilk).write()),
        ("board-B_Silkscreen.gbo", export_layer(board, Layer::BackSilk).write()),
        ("board-Edge_Cuts.gm1", export_layer(board, Layer::EdgeCuts).write()),
        ("board.drl", export_drill(board)),
        ("bom.csv", bom_csv(netlist)),
        ("cpl.csv", cpl_csv(board)),
        ("board.json", board.to_json()),
        ("netlist.json", netlist.to_json()),
    ]
}

/// Every manufacturing file into `dir`. Returns the file names written.
pub fn export_fab(board: &Board, netlist: &Netlist, dir: &Path) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let mut names = Vec::new();
    for (name, text) in fab_files(board, netlist) {
        std::fs::write(dir.join(name), text)?;
        names.push(name.to_string());
    }
    Ok(names)
}

/// The manufacturing files as one ZIP archive (the upload a board house takes). Gerbers and drill
/// sit at the archive root, as the upload forms expect; the BOM, CPL and JSON beside them.
pub fn fab_zip(board: &Board, netlist: &Netlist) -> Vec<u8> {
    let files: Vec<(String, Vec<u8>)> = fab_files(board, netlist).into_iter().map(|(n, t)| (n.to_string(), t.into_bytes())).collect();
    let mut out = Vec::new();
    zipfile::write_zip(&mut out, &files).expect("writing to a Vec cannot fail");
    out
}

/// Write the ZIP archive to `path`; returns the number of files inside.
pub fn export_zip(board: &Board, netlist: &Netlist, path: &Path) -> std::io::Result<usize> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let n = fab_files(board, netlist).len();
    std::fs::write(path, fab_zip(board, netlist))?;
    Ok(n)
}

/// JSON envelope: what an engine sends.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    ListExamples,
    LoadExample {
        name: String,
    },
    Place {
        netlist: Netlist,
        width_mm: f64,
        height_mm: f64,
        #[serde(default)]
        edge_parts: Vec<(String, Edge)>,
        #[serde(default = "default_seed")]
        seed: u64,
    },
    Route {
        board: Board,
        netlist: Netlist,
    },
    Drc {
        board: Board,
        netlist: Netlist,
    },
    ExportFab {
        board: Board,
        netlist: Netlist,
        dir: String,
    },
}

fn default_seed() -> u64 {
    1
}

/// JSON envelope: what the engine gets back.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ToolResult {
    Examples { names: Vec<String> },
    Netlist { netlist: Netlist },
    Board { board: Board },
    Routed { board: Board, complete: bool, failed: Vec<(String, usize)>, tracks: usize, vias: usize },
    Drc { clean: bool, counts: Vec<(String, usize)>, violations: Vec<String> },
    Exported { files: Vec<String> },
    Error { message: String },
}

pub fn run_tool(call: ToolCall) -> ToolResult {
    match call {
        ToolCall::ListExamples => ToolResult::Examples { names: example_names().iter().map(|s| s.to_string()).collect() },
        ToolCall::LoadExample { name } => match example_design(&name) {
            Some(netlist) => ToolResult::Netlist { netlist },
            None => ToolResult::Error { message: format!("no example named {name}; try one of {:?}", example_names()) },
        },
        ToolCall::Place { netlist, width_mm, height_mm, edge_parts, seed } => {
            match place_design(&netlist, &library(), width_mm, height_mm, &edge_parts, seed) {
                Ok(board) => ToolResult::Board { board },
                Err(e) => ToolResult::Error { message: e.to_string() },
            }
        }
        ToolCall::Route { mut board, netlist } => {
            let r = route_design(&mut board, &netlist);
            ToolResult::Routed { complete: r.is_complete(), failed: r.failed.into_iter().collect(), tracks: r.tracks, vias: r.vias, board }
        }
        ToolCall::Drc { board, netlist } => {
            let v = drc_design(&board, &netlist);
            ToolResult::Drc {
                clean: v.is_empty(),
                counts: summary(&v).into_iter().map(|(r, n)| (format!("{r:?}"), n)).collect(),
                violations: v.iter().map(|v| v.to_string()).collect(),
            }
        }
        ToolCall::ExportFab { board, netlist, dir } => match export_fab(&board, &netlist, Path::new(&dir)) {
            Ok(files) => ToolResult::Exported { files },
            Err(e) => ToolResult::Error { message: e.to_string() },
        },
    }
}

/// `run_tool` over JSON text, for a process boundary.
pub fn run_tool_json(call: &str) -> String {
    let result = match serde_json::from_str::<ToolCall>(call) {
        Ok(c) => run_tool(c),
        Err(e) => ToolResult::Error { message: format!("bad tool call: {e}") },
    };
    serde_json::to_string(&result).expect("results serialise")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_pipeline_through_the_envelope() {
        let r = run_tool(ToolCall::LoadExample { name: "sensor-node".into() });
        let ToolResult::Netlist { netlist } = r else { panic!("{r:?}") };
        let r = run_tool(ToolCall::Place {
            netlist: netlist.clone(),
            width_mm: 30.0,
            height_mm: 25.0,
            edge_parts: vec![("J1".into(), Edge::Left), ("SW1".into(), Edge::Right)],
            seed: 1,
        });
        let ToolResult::Board { board } = r else { panic!("{r:?}") };
        let r = run_tool(ToolCall::Route { board, netlist: netlist.clone() });
        let ToolResult::Routed { board, complete, .. } = r else { panic!("{r:?}") };
        assert!(complete);
        let r = run_tool(ToolCall::Drc { board: board.clone(), netlist: netlist.clone() });
        let ToolResult::Drc { clean, violations, .. } = r else { panic!("{r:?}") };
        assert!(clean, "{violations:?}");
        let dir = std::env::temp_dir().join(format!("board-agent-{}", std::process::id()));
        let r = run_tool(ToolCall::ExportFab { board, netlist, dir: dir.display().to_string() });
        let ToolResult::Exported { files } = r else { panic!("{r:?}") };
        assert_eq!(files.len(), 14);
        assert!(dir.join("bom.csv").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn json_envelope_and_errors() {
        let out = run_tool_json(r#"{"tool":"list_examples"}"#);
        assert!(out.contains("sensor-node"), "{out}");
        let out = run_tool_json(r#"{"tool":"load_example","name":"nope"}"#);
        assert!(out.contains("\"result\":\"error\""), "{out}");
        let out = run_tool_json("{not json");
        assert!(out.contains("bad tool call"));
        let out = run_tool_json(r#"{"tool":"place","netlist":{"components":[],"nets":[]},"width_mm":1,"height_mm":1}"#);
        assert!(out.contains("\"result\":\"board\""), "{out}");
    }
}
