//! `redrob-board pipeline | tool | examples` -- the whole flow from the terminal, exit code = verdict.
//!
//! ```text
//! redrob-board pipeline --design sensor-node --outline 30x25 --edge J1=left --edge SW1=right --out DIR
//! redrob-board tool '<json ToolCall>'          # one tool call, JSON result on stdout
//! redrob-board examples
//! ```
//! `pipeline` runs place -> route -> DRC -> export and exits 0 only when routing completed and
//! DRC is clean.

use std::path::PathBuf;
use std::process::ExitCode;

use board_agent::{drc_design, example_design, example_names, export_fab, place_design, route_design, run_tool_json};
use board_designs::library;
use board_drc::summary;
use board_place::Edge;

pub fn main(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("examples") => {
            for n in example_names() {
                println!("{n}");
            }
            ExitCode::SUCCESS
        }
        Some("tool") if args.len() == 2 => {
            println!("{}", run_tool_json(&args[1]));
            ExitCode::SUCCESS
        }
        Some("pipeline") => pipeline(&args[1..]),
        _ => {
            eprintln!(
                "usage: redrob-board examples | tool '<json>' | pipeline --design NAME --outline WxH [--edge REF=left|right|top|bottom]... [--seed N] --out DIR"
            );
            ExitCode::from(2)
        }
    }
}

fn pipeline(args: &[String]) -> ExitCode {
    let mut design = None;
    let mut outline = (30.0, 25.0);
    let mut edges = Vec::new();
    let mut seed = 1;
    let mut out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = |it: &mut std::slice::Iter<String>| it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--design" => design = Some(value(&mut it)),
            "--outline" => {
                let v = value(&mut it);
                let Some((w, h)) = v.split_once('x') else {
                    eprintln!("--outline wants WxH in mm, got {v}");
                    return ExitCode::from(2);
                };
                outline = (w.parse().unwrap_or(0.0), h.parse().unwrap_or(0.0));
            }
            "--edge" => {
                let v = value(&mut it);
                let Some((r, e)) = v.split_once('=') else {
                    eprintln!("--edge wants REF=side, got {v}");
                    return ExitCode::from(2);
                };
                let side = match e {
                    "left" => Edge::Left,
                    "right" => Edge::Right,
                    "top" => Edge::Top,
                    "bottom" => Edge::Bottom,
                    other => {
                        eprintln!("unknown side {other}");
                        return ExitCode::from(2);
                    }
                };
                edges.push((r.to_string(), side));
            }
            "--seed" => seed = value(&mut it).parse().unwrap_or(1),
            "--out" => out = Some(PathBuf::from(value(&mut it))),
            other => {
                eprintln!("unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(design) = design else {
        eprintln!("--design is required; one of {:?}", example_names());
        return ExitCode::from(2);
    };
    let Some(netlist) = example_design(&design) else {
        eprintln!("no design named {design}; one of {:?}", example_names());
        return ExitCode::from(2);
    };
    let mut board = match place_design(&netlist, &library(), outline.0, outline.1, &edges, seed) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("place: {e}");
            return ExitCode::from(1);
        }
    };
    println!("placed {} parts on {}x{} mm", board.placements.len(), outline.0, outline.1);
    let report = route_design(&mut board, &netlist);
    println!("routed: {} tracks, {} vias, {}", report.tracks, report.vias, if report.is_complete() { "complete" } else { "INCOMPLETE" });
    let violations = drc_design(&board, &netlist);
    let counts: Vec<String> = summary(&violations).iter().map(|(r, n)| format!("{r:?}={n}")).collect();
    println!("drc: {} violations {}", violations.len(), counts.join(" "));
    if let Some(dir) = out {
        match export_fab(&board, &netlist, &dir) {
            Ok(files) => println!("exported {} files to {}", files.len(), dir.display()),
            Err(e) => {
                eprintln!("export: {e}");
                return ExitCode::from(1);
            }
        }
    }
    if report.is_complete() && violations.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
