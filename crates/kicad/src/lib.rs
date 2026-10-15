//! Export a Redrob Board design to KiCad's board format, so KiCad can be run in CI as the DRC
//! oracle. Nothing is imported from this format into the product; this is a one-way bridge
//! used only for comparison.
//!
//! Coordinates flip from our y-up to KiCad's y-down. Footprints are written with their pads in
//! local coordinates and the placement as `(at x y angle)`.
//!
//! Back-side placements: our model mirrors a flipped footprint across its local x axis before
//! rotating; KiCad mirrors across the y axis. The two differ by a half turn, so a flipped part
//! with `quarter_turns = t` is written on `B.Cu` with angle `t * 90 + 180`. The `flipped_short`
//! fixture holds the oracle to this.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

use board_drc::Rules;
use board_geom::Point;
use board_layout::{Board, Layer, PadShape, Paste};
use board_netlist::{Netlist, PinRef};
use board_units::Nm;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExportError {
    #[error("inner copper layers beyond In4.Cu are not exported")]
    InnerLayers,
}

fn mm(v: Nm) -> String {
    v.mm_string()
}

fn xy(p: Point) -> String {
    format!("{} {}", mm(p.x), mm(-p.y))
}

/// The inner copper layer entries of the layer table, numbered 1.. between F.Cu (0) and B.Cu (31).
fn inner_layers(board: &Board) -> String {
    let mut s = String::new();
    for l in &board.copper_layers {
        if let Layer::InnerCopper(n) = l {
            writeln!(s, "    ({n} {} signal)", quote(layer_name(*l))).unwrap();
        }
    }
    s
}

fn layer_name(l: Layer) -> &'static str {
    match l {
        Layer::FrontCopper => "F.Cu",
        Layer::BackCopper => "B.Cu",
        Layer::InnerCopper(1) => "In1.Cu",
        Layer::InnerCopper(2) => "In2.Cu",
        Layer::InnerCopper(3) => "In3.Cu",
        Layer::InnerCopper(_) => "In4.Cu",
        Layer::FrontSilk => "F.SilkS",
        Layer::BackSilk => "B.SilkS",
        Layer::FrontMask => "F.Mask",
        Layer::BackMask => "B.Mask",
        Layer::FrontPaste => "F.Paste",
        Layer::BackPaste => "B.Paste",
        Layer::FrontCourtyard => "F.CrtYd",
        Layer::BackCourtyard => "B.CrtYd",
        Layer::EdgeCuts => "Edge.Cuts",
    }
}

/// Write a `.kicad_pcb` document. Zones carry their outline and rules but no fill: the oracle's
/// DRC refills zones itself, and a fill written as rows of rectangles would be read as hundreds
/// of islands. [`export_pcb_with_fills`] is the plotting variant.
pub fn export_pcb(board: &Board, netlist: &Netlist) -> Result<String, ExportError> {
    export_pcb_inner(board, netlist, false)
}

/// The same document with our zone fills as filled polygons, for plotting Gerbers to compare.
pub fn export_pcb_with_fills(board: &Board, netlist: &Netlist) -> Result<String, ExportError> {
    export_pcb_inner(board, netlist, true)
}

fn export_pcb_inner(board: &Board, netlist: &Netlist, fills: bool) -> Result<String, ExportError> {
    if board.copper_layers.iter().any(|l| matches!(l, Layer::InnerCopper(n) if *n == 0 || *n > 4)) {
        return Err(ExportError::InnerLayers);
    }

    // Net numbers: 0 is KiCad's "no net".
    let net_index = |name: &str| netlist.nets.iter().position(|n| n.name == name).map_or(0, |i| i + 1);

    let mut s = String::new();
    s.push_str("(kicad_pcb (version 20240108) (generator \"redrob-board\") (generator_version \"0.0.1\")\n");
    s.push_str("  (general (thickness 1.6) (legacy_teardrops no))\n  (paper \"A4\")\n");
    s.push_str("  (layers\n    (0 \"F.Cu\" signal)\n");
    s.push_str(&inner_layers(board));
    s.push_str("    (31 \"B.Cu\" signal)\n    (34 \"B.Paste\" user)\n    (35 \"F.Paste\" user)\n    (36 \"B.SilkS\" user \"B.Silkscreen\")\n    (37 \"F.SilkS\" user \"F.Silkscreen\")\n    (38 \"B.Mask\" user)\n    (39 \"F.Mask\" user)\n    (44 \"Edge.Cuts\" user)\n    (46 \"B.CrtYd\" user \"B.Courtyard\")\n    (47 \"F.CrtYd\" user \"F.Courtyard\")\n    (48 \"B.Fab\" user)\n    (49 \"F.Fab\" user)\n  )\n");
    s.push_str("  (setup (pad_to_mask_clearance 0) (allow_soldermask_bridges_in_footprints no))\n");
    s.push_str("  (net 0 \"\")\n");
    for (i, n) in netlist.nets.iter().enumerate() {
        writeln!(s, "  (net {} {})", i + 1, quote(&n.name)).unwrap();
    }

    for p in &board.placements {
        let angle = (p.quarter_turns as i64 * 90 + if p.flipped { 180 } else { 0 }) % 360;
        let (cu, silk, paste, mask, yard_layer) =
            if p.flipped { ("B.Cu", "B.SilkS", "B.Paste", "B.Mask", "B.CrtYd") } else { ("F.Cu", "F.SilkS", "F.Paste", "F.Mask", "F.CrtYd") };
        writeln!(s, "  (footprint {} (layer {}) (at {} {})", quote(&p.footprint.id), quote(cu), xy(p.at), angle).unwrap();
        // The reference sits at the courtyard centre, where our silkscreen draws it; angle 0 is
        // absolute here (like a pad's), so both labels stay horizontal.
        let label_at = p.footprint.courtyard.center();
        writeln!(
            s,
            "    (property \"Reference\" {} (at {} 0) (layer {}) (effects (font (size 1 1) (thickness 0.15))))",
            quote(&p.refdes),
            xy(label_at),
            quote(silk)
        )
        .unwrap();
        let smd = p.footprint.pads.iter().all(|pad| pad.drill.is_none());
        writeln!(s, "    (attr {})", if smd { "smd" } else { "through_hole" }).unwrap();
        let yard = p.footprint.courtyard;
        writeln!(
            s,
            "    (fp_rect (start {}) (end {}) (stroke (width 0.05) (type default)) (fill none) (layer {}))",
            xy(yard.min),
            xy(yard.max),
            quote(yard_layer)
        )
        .unwrap();
        for pad in &p.footprint.pads {
            let (w, h) = pad.shape.size();
            let (kind, layers) = match (pad.drill, pad.layers.is_empty()) {
                // A pad with paste windows keeps copper and mask; its paste is the aperture pads below.
                (None, _) if pad.paste != Paste::Full => ("smd", format!("{} {}", quote(cu), quote(mask))),
                (None, _) => ("smd", format!("{} {} {}", quote(cu), quote(paste), quote(mask))),
                (Some(_), true) => ("np_thru_hole", format!("{} {}", quote("*.Cu"), quote("*.Mask"))),
                (Some(_), false) => ("thru_hole", format!("{} {}", quote("*.Cu"), quote("*.Mask"))),
            };
            let shape = match pad.shape {
                PadShape::Circle { .. } => "circle".to_string(),
                PadShape::Rect { .. } => "rect".to_string(),
                PadShape::Oval { .. } => "oval".to_string(),
                PadShape::RoundRect { .. } => "roundrect".to_string(),
            };
            // In this format a pad's angle is absolute (footprint angle plus its own), so every pad
            // repeats the footprint angle; without it KiCad rotates the position but not the shape.
            write!(s, "    (pad {} {kind} {shape} (at {} {angle}) (size {} {})", quote(&pad.number), xy(pad.at), mm(w), mm(h)).unwrap();
            if let Some(d) = pad.drill {
                write!(s, " (drill {})", mm(d)).unwrap();
            }
            write!(s, " (layers {layers})").unwrap();
            if let PadShape::RoundRect { radius, width, height } = pad.shape {
                let ratio = radius.0 as f64 / width.min(height).0 as f64;
                write!(s, " (roundrect_rratio {ratio:.4})").unwrap();
            }
            if !pad.layers.is_empty()
                && let Some(net) = netlist.net_of(&PinRef::new(p.refdes.clone(), pad.number.clone()))
            {
                write!(s, " (net {} {})", net_index(&net.name), quote(&net.name)).unwrap();
            }
            s.push_str(")\n");
            // Paste windows as aperture pads (no number, paste layer only): the same geometry our
            // own paste Gerber carries, so the oracle's plot and ours stay comparable.
            if let Paste::Grid { .. } = pad.paste {
                for (c, side) in pad.paste_windows() {
                    writeln!(s, "    (pad \"\" smd rect (at {} {angle}) (size {} {}) (layers {}))", xy(c), mm(side), mm(side), quote(paste)).unwrap();
                }
            }
        }
        s.push_str("  )\n");
    }

    for e in board.outline.edges() {
        writeln!(s, "  (gr_line (start {}) (end {}) (stroke (width 0.1) (type default)) (layer \"Edge.Cuts\"))", xy(e.a), xy(e.b)).unwrap();
    }
    // Zones carry our fill as their filled polygons (one per rectangle), solid pad connections and
    // the same clearance, so the oracle plots and checks the copper we computed.
    for z in &board.zones {
        let net = netlist.nets.iter().position(|n| n.name == z.net).map_or(0, |i| i + 1);
        write!(
            s,
            "  (zone (net {net}) (net_name {}) (layer {}) (hatch edge 0.5) (connect_pads yes (clearance {}))",
            quote(&z.net),
            quote(layer_name(z.layer)),
            mm(z.clearance)
        )
        .unwrap();
        write!(
            s,
            " (min_thickness {}) (filled_areas_thickness no) (fill yes (thermal_gap {}) (thermal_bridge_width {}))",
            mm(z.min_width),
            mm(z.clearance),
            mm(z.min_width)
        )
        .unwrap();
        s.push_str("\n    (polygon (pts");
        for v in &z.outline.vertices {
            write!(s, " (xy {})", xy(*v)).unwrap();
        }
        s.push_str("))\n");
        for r in z.fill.iter().filter(|_| fills) {
            writeln!(
                s,
                "    (filled_polygon (layer {}) (pts (xy {}) (xy {}) (xy {}) (xy {})))",
                quote(layer_name(z.layer)),
                xy(Point::new(r.min.x, r.min.y)),
                xy(Point::new(r.max.x, r.min.y)),
                xy(Point::new(r.max.x, r.max.y)),
                xy(Point::new(r.min.x, r.max.y))
            )
            .unwrap();
        }
        s.push_str("  )\n");
    }
    for t in &board.tracks {
        writeln!(
            s,
            "  (segment (start {}) (end {}) (width {}) (layer {}) (net {}))",
            xy(t.segment.a),
            xy(t.segment.b),
            mm(t.width),
            quote(layer_name(t.layer)),
            net_index(&t.net)
        )
        .unwrap();
    }
    for v in &board.vias {
        writeln!(
            s,
            "  (via (at {}) (size {}) (drill {}) (layers \"F.Cu\" \"B.Cu\") (net {}))",
            xy(v.at),
            mm(v.diameter),
            mm(v.drill),
            net_index(&v.net)
        )
        .unwrap();
    }
    s.push_str(")\n");
    Ok(s)
}

/// Write the `.kicad_pro` project file that carries the design rules the oracle must apply.
/// The rule block and the Default net class carry our limits; every board net class becomes a
/// KiCad net class with its nets listed as patterns, so the oracle checks the same width and
/// clearance per net that our router and checker used. Without the Default class KiCad would check
/// clearance at its own 0.2 mm default, not ours. The net_settings block is written as schema
/// version 3: measured against KiCad 9.0.8, version 4 without a `priority` field is silently
/// ignored and the default class comes back.
pub fn export_project(rules: &Rules, board: &Board) -> String {
    use serde_json::json;
    let r = |v: Nm| v.to_mm_f64();
    let via = rules.min_drill + rules.min_annular_ring * 2;
    let class = |name: &str, clearance: Nm, track: Nm, via_d: Nm, via_drill: Nm| {
        json!({
            "name": name, "clearance": r(clearance), "track_width": r(track), "via_diameter": r(via_d), "via_drill": r(via_drill),
            "bus_width": 12, "wire_width": 6, "diff_pair_gap": r(clearance), "diff_pair_width": r(track), "diff_pair_via_gap": r(clearance),
            "microvia_diameter": r(via_d), "microvia_drill": r(via_drill),
            "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)", "line_style": 0
        })
    };
    let mut classes = vec![class("Default", rules.clearance, rules.min_track_width, via, rules.min_drill)];
    let mut patterns = Vec::new();
    for c in &board.netclasses {
        classes.push(class(&c.name, c.clearance, c.track_width, c.via_diameter, c.via_drill));
        for n in &c.nets {
            patterns.push(json!({"netclass": c.name, "pattern": n}));
        }
    }
    let v = json!({
        "board": {"design_settings": {"rules": {
            "min_clearance": r(rules.clearance),
            "min_copper_edge_clearance": r(rules.edge_clearance),
            "min_hole_clearance": r(rules.clearance),
            "min_hole_to_hole": r(rules.clearance),
            "min_through_hole_diameter": r(rules.min_drill),
            "min_track_width": r(rules.min_track_width),
            "min_via_annular_width": r(rules.min_annular_ring),
            "min_via_diameter": r(via)
        }}},
        "net_settings": {"classes": classes, "netclass_patterns": patterns, "meta": {"version": 3}},
        "meta": {"filename": "board.kicad_pro", "version": 1}
    });
    serde_json::to_string_pretty(&v).expect("json") + "\n"
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_geom::{Rect, Segment};
    use board_layout::{Track, Via, footprints};
    use board_lib::sexp::Sexp;
    use board_schematic::Schematic;

    fn fixture() -> (Board, Netlist) {
        let mut sch = Schematic::new();
        let r1 = sch.passive("R1", "10k", "Resistor_SMD:R_0402_1005Metric");
        let j1 = sch.part("J1", "Conn", "hdr", &[("1", "A"), ("2", "B")]);
        sch.net("A", &[r1.pin("1"), j1.pin("1")]);
        sch.net("B", &[r1.pin("2"), j1.pin("2")]);
        let netlist = sch.build().unwrap();
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        b.place("R1", footprints::chip_0402(), Point::mm(5, 5), 1, false);
        b.place("J1", footprints::pin_header_1xn(2), Point::mm(10, 5), 0, false);
        b.tracks.push(Track {
            net: "A".into(),
            layer: Layer::FrontCopper,
            segment: Segment::new(Point::mm(5, 5), Point::mm(10, 5)),
            width: Nm::um(250),
        });
        b.vias.push(Via { net: "B".into(), at: Point::mm(15, 5), diameter: Nm::um(600), drill: Nm::um(300) });
        (b, netlist)
    }

    #[test]
    fn export_is_well_formed_and_complete() {
        let (b, n) = fixture();
        let text = export_pcb(&b, &n).unwrap();
        let doc = Sexp::parse(&text).unwrap();
        assert_eq!(doc.head(), Some("kicad_pcb"));
        assert_eq!(doc.children("net").count(), 3);
        assert_eq!(doc.children("footprint").count(), 2);
        assert_eq!(doc.children("gr_line").count(), 4);
        assert_eq!(doc.children("segment").count(), 1);
        assert_eq!(doc.children("via").count(), 1);
        let r1 = doc.children("footprint").next().unwrap();
        assert_eq!(r1.child("at").unwrap().args().iter().map(|a| a.as_atom().unwrap()).collect::<Vec<_>>(), vec!["5", "-5", "90"]);
        let pads: Vec<_> = r1.children("pad").collect();
        assert_eq!(pads.len(), 2);
        assert_eq!(pads[0].child("net").unwrap().atom_arg(1), Some("A"));
        assert!(text.contains("(pad \"1\" thru_hole circle (at 0 0 0) (size 1.7 1.7) (drill 1) (layers \"*.Cu\" \"*.Mask\") (net 1 \"A\"))"));
        // R1 is turned a quarter: its pads carry the footprint angle too.
        assert!(text.contains("(pad \"1\" smd rect (at -0.5 0 90) (size 0.6 0.5)"), "{text}");
        // y is flipped: our (10, 5) is KiCad (10, -5).
        assert!(text.contains("(segment (start 5 -5) (end 10 -5) (width 0.25) (layer \"F.Cu\") (net 1))"));
    }

    #[test]
    fn flipped_goes_to_back_with_half_turn() {
        let (mut b, n) = fixture();
        b.place("C1", footprints::chip_0402(), Point::mm(2, 2), 1, true);
        let text = export_pcb(&b, &n).unwrap();
        assert!(text.contains("(footprint \"Resistor_SMD:R_0402_1005Metric\" (layer \"B.Cu\") (at 2 -2 270)"), "{text}");
        assert!(text.contains("(layers \"B.Cu\" \"B.Paste\" \"B.Mask\")"));
    }

    #[test]
    fn project_carries_rules() {
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(10, 10)));
        b.netclasses.push(board_layout::NetClass {
            name: "Power".into(),
            nets: vec!["5V".into(), "GND".into()],
            track_width: Nm::um(500),
            clearance: Nm::um(200),
            via_diameter: Nm::um(800),
            via_drill: Nm::um(400),
        });
        let text = export_project(&Rules::default(), &b);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["net_settings"]["classes"][1]["name"], "Power");
        assert_eq!(v["net_settings"]["classes"][1]["track_width"], 0.5);
        assert_eq!(v["net_settings"]["netclass_patterns"][1], serde_json::json!({"netclass": "Power", "pattern": "GND"}));
        let rules = &v["board"]["design_settings"]["rules"];
        assert_eq!(rules["min_clearance"], 0.127);
        assert_eq!(rules["min_track_width"], 0.127);
        assert_eq!(rules["min_through_hole_diameter"], 0.3);
        assert_eq!(rules["min_via_diameter"], 0.56);
    }
}
