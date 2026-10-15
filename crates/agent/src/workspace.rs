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

use crate::{drc_design, example_design, example_names, export_fab, export_zip, place_design, route_design};

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
            name: "catalog",
            description: "The parts catalogue: every part the board can be built from, with id, role (mcu, i2c_sensor, ldo, header, button, resistor, capacitor), value, footprint, LCSC number and unit price.",
            parameters: json!({"type": "object", "properties": {"role": {"type": "string", "enum": ["mcu", "i2c_sensor", "ldo", "header", "button", "resistor", "capacitor"], "description": "Only parts with this role"}}, "additionalProperties": false}),
        },
        ToolSpec {
            name: "write_schematic",
            description: "Write the netlist for a board from a specification: an MCU from the catalogue, I2C sensors, how it is powered and which extras it has. Replaces the current design (drops any board). Rails, decoupling, I2C pull-ups, the reset RC + button, the boot pull-up and the power/UART header follow from the rules.",
            parameters: json!({"type": "object", "required": ["mcu"], "properties": {
                "name": {"type": "string", "description": "A short name for the design, e.g. sensor-node"},
                "mcu": {"type": "string", "description": "Catalogue id of an mcu part, e.g. esp32-c3-mini-1"},
                "sensors": {"type": "array", "items": {"type": "string"}, "description": "Catalogue ids of i2c_sensor parts, one per sensor on the board"},
                "power": {"type": "string", "enum": ["usb5v_ldo3v3", "external3v3"], "description": "5 V in with an on-board 3.3 V LDO (default), or regulated 3.3 V in"},
                "uart_header": {"type": "boolean", "description": "1x4 header: power, GND, TX, RX (default true)"},
                "reset_button": {"type": "boolean", "description": "Reset button with its RC on EN (default true)"},
                "boot_pullup": {"type": "boolean", "description": "Pull-up on the boot strapping pin (default true)"}
            }, "additionalProperties": false}),
        },
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
            name: "export_zip",
            description: "Write every fabrication file (Gerbers, Excellon drill, BOM, pick-and-place) as one ZIP archive, the upload a board house takes.",
            parameters: json!({"type": "object", "required": ["path"], "properties": {"path": {"type": "string", "description": "Archive path, e.g. fab/sensor-node.zip"}}, "additionalProperties": false}),
        },
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

    pub fn write_schematic(&mut self, name: &str, spec: &board_catalog::Spec) -> Result<Value, String> {
        let n = board_catalog::write_schematic(spec).map_err(|e| e.to_string())?;
        self.design = if name.trim().is_empty() { "custom".to_string() } else { name.trim().to_string() };
        let out = json!({
            "design": self.design,
            "parts": n.components.len(),
            "nets": n.nets.len(),
            "refdes": n.components.iter().map(|c| format!("{} {}", c.refdes, c.value)).collect::<Vec<_>>(),
            "parts_cost_usd": (board_catalog::parts_cost_usd(&n) * 100.0).round() / 100.0,
        });
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
        let pairs: Vec<Value> = r
            .pairs
            .iter()
            .map(|p| json!({"p": p.p, "n": p.n, "len_p_mm": p.len_p.to_mm_f64(), "len_n_mm": p.len_n.to_mm_f64(), "coupled_permille": p.coupled_permille}))
            .collect();
        let lengths: Vec<Value> = r
            .lengths
            .iter()
            .map(|g| json!({"nets": g.nets.iter().map(|(n, l)| json!({"net": n, "mm": l.to_mm_f64()})).collect::<Vec<_>>(), "tolerance_mm": g.tolerance.to_mm_f64(), "matched": g.matched}))
            .collect();
        Ok(json!({"complete": r.is_complete(), "tracks": r.tracks, "vias": r.vias, "failed": r.failed, "pairs": pairs, "length_groups": lengths}))
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

    pub fn export_zip(&mut self, path: &str) -> Result<Value, String> {
        let (Some(board), Some(netlist)) = (&self.board, &self.netlist) else { return Err("nothing to export: place first".into()) };
        let n = export_zip(board, netlist, Path::new(path)).map_err(|e| e.to_string())?;
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(json!({"path": path, "files": n, "bytes": bytes}))
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
            "catalog" => {
                let role: Option<board_catalog::Role> = match args.get("role") {
                    Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| format!("catalog: bad role: {e}"))?),
                    None => None,
                };
                let parts: Vec<Value> = board_catalog::parts()
                    .into_iter()
                    .filter(|p| role.is_none_or(|r| p.role == r))
                    .map(|p| json!({"id": p.id, "role": p.role, "value": p.value, "manufacturer": p.manufacturer, "mpn": p.mpn, "footprint": p.footprint, "lcsc": p.lcsc, "unit_price_usd": p.price_usd}))
                    .collect();
                Ok(json!({"parts": parts, "prices_as_of": board_catalog::PRICES_AS_OF}))
            }
            "write_schematic" => {
                let mut spec_args = args.clone();
                let name = spec_args.as_object_mut().and_then(|o| o.remove("name")).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                let spec: board_catalog::Spec = serde_json::from_value(spec_args).map_err(|e| format!("write_schematic: bad spec: {e}"))?;
                self.write_schematic(&name, &spec)
            }
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
            "export_zip" => self.export_zip(&str_arg("path")?),
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
        assert_eq!(specs.len(), 10);
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
    fn write_schematic_then_pipeline_from_a_spec() {
        let mut w = Workspace::default();
        let r = w.run("catalog", &json!({"role": "i2c_sensor"})).unwrap();
        assert_eq!(r["parts"][0]["id"], "aht20");
        assert!(w.run("catalog", &json!({"role": "wheel"})).unwrap_err().contains("bad role"));
        let r = w.run("write_schematic", &json!({"name": "온습도 노드", "mcu": "esp32-c3-mini-1", "sensors": ["aht20"]})).unwrap();
        assert_eq!(r["parts"], 14);
        assert_eq!(r["nets"], 9);
        assert!(r["parts_cost_usd"].as_f64().unwrap() > 3.0);
        assert_eq!(w.design, "온습도 노드");
        let r = w
            .run(
                "place",
                &json!({"width_mm": 30, "height_mm": 25, "edge_parts": [{"refdes": "J1", "edge": "left"}, {"refdes": "SW1", "edge": "right"}]}),
            )
            .unwrap();
        assert_eq!(r["placed"], 14);
        assert_eq!(w.run("route", &json!({})).unwrap()["complete"], true);
        let d = w.run("drc", &json!({})).unwrap();
        assert_eq!(d["clean"], true, "{d}");
        assert!(w.run("write_schematic", &json!({"mcu": "aht20"})).unwrap_err().contains("not a Mcu"));
        let zip = std::env::temp_dir().join(format!("board-ws-{}.zip", std::process::id()));
        let r = w.run("export_zip", &json!({"path": zip.display().to_string()})).unwrap();
        assert_eq!(r["files"], 14);
        let entries = crate::zipfile::read_zip(&std::fs::read(&zip).unwrap()).unwrap();
        assert!(entries.iter().any(|(n, d)| n == "board-F_Cu.gtl" && d.ends_with(b"M02*\n")));
        assert!(entries.iter().any(|(n, _)| n == "bom.csv"));
        std::fs::remove_file(&zip).unwrap();
    }

    #[test]
    fn route_without_a_board_places_the_default_first() {
        let mut w = Workspace::default();
        let r = w.run("route", &json!({})).unwrap();
        assert_eq!(r["complete"], true);
        assert_eq!(w.design, "sensor-node");
    }
}
