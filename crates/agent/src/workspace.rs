//! The engine's view of one open design: a small set of named tools over a `Workspace`.
//!
//! The JSON envelope in `lib.rs` moves whole boards through a process boundary. A language model
//! driving the app should not: it names a tool and a few scalars, and the state (netlist, board,
//! verdicts) stays here. These are the tools the chat palette declares on `/v1/chat/completions`
//! (caller-owned, so the engine emits `tool_calls` and the app executes them).

use std::path::Path;

use board_designs::library;
use board_drc::{Violation, summary};
use board_layout::Board;
use board_netlist::Netlist;
use board_place::Edge;
use serde_json::{Value, json};

use crate::{drc_design, example_design, example_names, export_fab, place_design, route_design};

/// The design the user and the engine both work on.
#[derive(Clone, Debug, Default)]
pub struct Workspace {
    pub design: String,
    pub netlist: Option<Netlist>,
    pub board: Option<Board>,
    pub violations: Vec<Violation>,
    pub drc_run: bool,
    pub route_complete: Option<bool>,
}

/// One tool the engine may call: name, what it does, JSON schema of its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

impl ToolSpec {
    /// The OpenAI `tools[]` entry.
    pub fn to_openai(&self) -> Value {
        json!({"type": "function", "function": {"name": self.name, "description": self.description, "parameters": self.parameters}})
    }
}

fn no_args() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

/// Every tool, in the order the engine sees them.
pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "describe",
            description: "Current state of the open design: name, parts, nets, whether it is placed, routed and checked, and the DRC verdict.",
            parameters: no_args(),
        },
        ToolSpec { name: "list_examples", description: "Names of the built-in example designs.", parameters: no_args() },
        ToolSpec {
            name: "load_example",
            description: "Open one of the built-in example designs as the current netlist (drops any board).",
            parameters: json!({"type": "object", "required": ["name"], "properties": {"name": {"type": "string", "description": "An example name from list_examples, e.g. sensor-node"}}, "additionalProperties": false}),
        },
        ToolSpec {
            name: "place",
            description: "Place every part of the current netlist on a rectangular board. Loads the design first if needed.",
            parameters: json!({"type": "object", "required": ["width_mm", "height_mm"], "properties": {
                "width_mm": {"type": "number", "minimum": 5, "maximum": 300},
                "height_mm": {"type": "number", "minimum": 5, "maximum": 300},
                "edge_parts": {"type": "array", "description": "Parts pinned to a board edge, e.g. a connector on the left", "items": {"type": "object", "required": ["refdes", "edge"], "properties": {"refdes": {"type": "string"}, "edge": {"type": "string", "enum": ["left", "right", "top", "bottom"]}}, "additionalProperties": false}},
                "seed": {"type": "integer", "minimum": 0, "description": "Placement seed; a different seed gives a different arrangement"}
            }, "additionalProperties": false}),
        },
        ToolSpec {
            name: "route",
            description: "Route every net of the placed board on two copper layers. Places first if needed.",
            parameters: no_args(),
        },
        ToolSpec { name: "drc", description: "Run the design rule check on the board and return the verdict and violations.", parameters: no_args() },
        ToolSpec {
            name: "export_fab",
            description: "Write the fabrication files (Gerbers, Excellon drill, BOM, pick-and-place) for the board into a directory.",
            parameters: json!({"type": "object", "required": ["dir"], "properties": {"dir": {"type": "string", "description": "Output directory, created if missing"}}, "additionalProperties": false}),
        },
    ]
}

fn edge(s: &str) -> Option<Edge> {
    match s {
        "left" => Some(Edge::Left),
        "right" => Some(Edge::Right),
        "top" => Some(Edge::Top),
        "bottom" => Some(Edge::Bottom),
        _ => None,
    }
}

impl Workspace {
    pub fn describe(&self) -> Value {
        let mut v = json!({"design": self.design, "loaded": self.netlist.is_some(), "placed": self.board.is_some()});
        if let Some(n) = &self.netlist {
            v["parts"] = json!(n.components.len());
            v["nets"] = json!(n.nets.len());
            v["refdes"] = json!(n.components.iter().map(|c| format!("{} {}", c.refdes, c.value)).collect::<Vec<_>>());
        }
        if let Some(b) = &self.board {
            v["tracks"] = json!(b.tracks.len());
            v["vias"] = json!(b.vias.len());
            if let Some(bb) = b.outline.bbox() {
                v["outline_mm"] = json!([bb.width().to_mm_f64(), bb.height().to_mm_f64()]);
            }
        }
        if let Some(c) = self.route_complete {
            v["routing"] = json!(if c { "complete" } else { "incomplete" });
        }
        if self.drc_run {
            v["drc"] = json!({"clean": self.violations.is_empty(), "violations": self.violations.len()});
        }
        v
    }

    pub fn load_example(&mut self, name: &str) -> Result<Value, String> {
        let n = example_design(name).ok_or_else(|| format!("no example named {name}; try one of {:?}", example_names()))?;
        self.design = name.to_string();
        let out = json!({"loaded": name, "parts": n.components.len(), "nets": n.nets.len()});
        self.netlist = Some(n);
        self.board = None;
        self.violations.clear();
        self.drc_run = false;
        self.route_complete = None;
        Ok(out)
    }

    pub fn place(&mut self, width_mm: f64, height_mm: f64, edge_parts: &[(String, Edge)], seed: u64) -> Result<Value, String> {
        if self.netlist.is_none() {
            let name = if self.design.is_empty() { "sensor-node".to_string() } else { self.design.clone() };
            self.load_example(&name)?;
        }
        let netlist = self.netlist.as_ref().ok_or("no netlist")?;
        let b = place_design(netlist, &library(), width_mm, height_mm, edge_parts, seed).map_err(|e| e.to_string())?;
        let out = json!({"placed": b.placements.len(), "outline_mm": [width_mm, height_mm]});
        self.board = Some(b);
        self.violations.clear();
        self.drc_run = false;
        self.route_complete = None;
        Ok(out)
    }

    pub fn route(&mut self) -> Result<Value, String> {
        if self.board.is_none() {
            self.place(30.0, 25.0, &[], 1)?;
        }
        let (Some(board), Some(netlist)) = (&mut self.board, &self.netlist) else { return Err("no board".into()) };
        let r = route_design(board, netlist);
        self.route_complete = Some(r.is_complete());
        self.drc_run = false;
        Ok(json!({"complete": r.is_complete(), "tracks": r.tracks, "vias": r.vias, "failed": r.failed}))
    }

    pub fn drc(&mut self) -> Result<Value, String> {
        let (Some(board), Some(netlist)) = (&self.board, &self.netlist) else { return Err("nothing to check: place first".into()) };
        self.violations = drc_design(board, netlist);
        self.drc_run = true;
        let counts: Vec<(String, usize)> = summary(&self.violations).into_iter().map(|(r, n)| (format!("{r:?}"), n)).collect();
        let shown: Vec<String> = self.violations.iter().take(20).map(|v| v.to_string()).collect();
        Ok(json!({"clean": self.violations.is_empty(), "counts": counts, "violations": shown, "total": self.violations.len()}))
    }

    pub fn export_fab(&mut self, dir: &str) -> Result<Value, String> {
        let (Some(board), Some(netlist)) = (&self.board, &self.netlist) else { return Err("nothing to export: place first".into()) };
        let files = export_fab(board, netlist, Path::new(dir)).map_err(|e| e.to_string())?;
        Ok(json!({"dir": dir, "files": files}))
    }

    /// Run a tool by name with JSON arguments; the result is what goes back as the `tool` message.
    pub fn run(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let str_arg =
            |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("{name}: missing string argument `{k}`"));
        let num_arg = |k: &str| args.get(k).and_then(Value::as_f64).ok_or_else(|| format!("{name}: missing number argument `{k}`"));
        match name {
            "describe" => Ok(self.describe()),
            "list_examples" => Ok(json!({"examples": example_names()})),
            "load_example" => self.load_example(&str_arg("name")?),
            "place" => {
                let edges = args
                    .get("edge_parts")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|e| {
                                let r = e.get("refdes").and_then(Value::as_str).ok_or("edge_parts[].refdes must be a string")?;
                                let side =
                                    e.get("edge").and_then(Value::as_str).and_then(edge).ok_or("edge_parts[].edge must be left|right|top|bottom")?;
                                Ok::<_, String>((r.to_string(), side))
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let seed = args.get("seed").and_then(Value::as_u64).unwrap_or(1);
                self.place(num_arg("width_mm")?, num_arg("height_mm")?, &edges, seed)
            }
            "route" => self.route(),
            "drc" => self.drc(),
            "export_fab" => self.export_fab(&str_arg("dir")?),
            other => Err(format!("unknown tool `{other}`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_valid_openai_tools() {
        let specs = tool_specs();
        assert_eq!(specs.len(), 7);
        for s in &specs {
            let v = s.to_openai();
            assert_eq!(v["type"], "function");
            assert_eq!(v["function"]["parameters"]["type"], "object");
        }
    }

    #[test]
    fn tools_drive_the_pipeline_by_name() {
        let mut w = Workspace::default();
        assert_eq!(w.run("describe", &json!({})).unwrap()["loaded"], false);
        let r = w.run("load_example", &json!({"name": "sensor-node"})).unwrap();
        assert_eq!(r["parts"], 14);
        let r = w
            .run(
                "place",
                &json!({"width_mm": 30, "height_mm": 25, "edge_parts": [{"refdes": "J1", "edge": "left"}, {"refdes": "SW1", "edge": "right"}]}),
            )
            .unwrap();
        assert_eq!(r["placed"], 14);
        let r = w.run("route", &json!({})).unwrap();
        assert_eq!(r["complete"], true);
        let r = w.run("drc", &json!({})).unwrap();
        assert_eq!(r["clean"], true, "{r}");
        let d = w.run("describe", &json!({})).unwrap();
        assert_eq!(d["routing"], "complete");
        assert_eq!(d["drc"]["clean"], true);
        assert!(w.run("export_fab", &json!({})).unwrap_err().contains("dir"));
        assert!(w.run("nope", &json!({})).unwrap_err().contains("unknown tool"));
        assert!(
            w.run("place", &json!({"width_mm": 30, "height_mm": 25, "edge_parts": [{"refdes": "J1", "edge": "north"}]}))
                .unwrap_err()
                .contains("edge")
        );
    }

    #[test]
    fn route_without_a_board_places_the_default_first() {
        let mut w = Workspace::default();
        let r = w.run("route", &json!({})).unwrap();
        assert_eq!(r["complete"], true);
        assert_eq!(w.design, "sensor-node");
    }
}
