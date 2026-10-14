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
use board_layout::{Board, Layer, PadShape};
use board_netlist::{Netlist, PinRef};
use board_units::Nm;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExportError {
    #[error("inner copper layers are not exported yet")]
    InnerLayers,
}

fn mm(v: Nm) -> String {
    v.mm_string()
}

fn xy(p: Point) -> String {
    format!("{} {}", mm(p.x), mm(-p.y))
}

fn layer_name(l: Layer) -> &'static str {
    match l {
        Layer::FrontCopper => "F.Cu",
        Layer::BackCopper => "B.Cu",
        Layer::InnerCopper(_) => "In1.Cu",
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

/// Write a `.kicad_pcb` document.
pub fn export_pcb(board: &Board, netlist: &Netlist) -> Result<String, ExportError> {
    if board.copper_layers.iter().any(|l| matches!(l, Layer::InnerCopper(_))) {
        return Err(ExportError::InnerLayers);
    }

    // Net numbers: 0 is KiCad's "no net".
    let net_index = |name: &str| netlist.nets.iter().position(|n| n.name == name).map_or(0, |i| i + 1);

    let mut s = String::new();
    s.push_str("(kicad_pcb (version 20240108) (generator \"redrob-board\") (generator_version \"0.0.1\")\n");
    s.push_str("  (general (thickness 1.6) (legacy_teardrops no))\n  (paper \"A4\")\n");
    s.push_str(
        "  (layers\n    (0 \"F.Cu\" signal)\n    (31 \"B.Cu\" signal)\n    (34 \"B.Paste\" user)\n    (35 \"F.Paste\" user)\n    (36 \"B.SilkS\" user \"B.Silkscreen\")\n    (37 \"F.SilkS\" user \"F.Silkscreen\")\n    (38 \"B.Mask\" user)\n    (39 \"F.Mask\" user)\n    (44 \"Edge.Cuts\" user)\n    (46 \"B.CrtYd\" user \"B.Courtyard\")\n    (47 \"F.CrtYd\" user \"F.Courtyard\")\n    (48 \"B.Fab\" user)\n    (49 \"F.Fab\" user)\n  )\n",
    );
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
        writeln!(
            s,
            "    (property \"Reference\" {} (at 0 0 0) (layer {}) (effects (font (size 1 1) (thickness 0.15))))",
            quote(&p.refdes),
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
        }
        s.push_str("  )\n");
    }

    for e in board.outline.edges() {
        writeln!(s, "  (gr_line (start {}) (end {}) (stroke (width 0.1) (type default)) (layer \"Edge.Cuts\"))", xy(e.a), xy(e.b)).unwrap();
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
/// The rule block and the Default net class carry our limits; KiCad supplies defaults for the rest.
/// Without the net class KiCad would check clearance at its own 0.2 mm default, not ours. The
/// net_settings block is written as schema version 3: measured against KiCad 9.0.8, version 4
/// without a `priority` field is silently ignored and the default class comes back.
pub fn export_project(rules: &Rules) -> String {
    let r = |v: Nm| v.to_mm_f64();
    format!(
        concat!(
            "{{\n  \"board\": {{\n    \"design_settings\": {{\n      \"rules\": {{\n",
            "        \"min_clearance\": {clearance},\n",
            "        \"min_copper_edge_clearance\": {edge},\n",
            "        \"min_hole_clearance\": {clearance},\n",
            "        \"min_hole_to_hole\": {clearance},\n",
            "        \"min_through_hole_diameter\": {drill},\n",
            "        \"min_track_width\": {track},\n",
            "        \"min_via_annular_width\": {ring},\n",
            "        \"min_via_diameter\": {via}\n",
            "      }}\n    }}\n  }},\n",
            "  \"net_settings\": {{\n    \"classes\": [\n      {{\n        \"name\": \"Default\",\n",
            "        \"clearance\": {clearance},\n        \"track_width\": {track},\n",
            "        \"via_diameter\": {via},\n        \"via_drill\": {drill},\n",
            "        \"bus_width\": 12, \"wire_width\": 6, \"diff_pair_gap\": {clearance}, \"diff_pair_width\": {track}, \"diff_pair_via_gap\": {clearance},\n",
            "        \"microvia_diameter\": {via}, \"microvia_drill\": {drill}, \"pcb_color\": \"rgba(0, 0, 0, 0.000)\", \"schematic_color\": \"rgba(0, 0, 0, 0.000)\", \"line_style\": 0\n",
            "      }}\n    ],\n    \"meta\": {{ \"version\": 3 }}\n  }},\n",
            "  \"meta\": {{ \"filename\": \"board.kicad_pro\", \"version\": 1 }}\n}}\n"
        ),
        clearance = r(rules.clearance),
        edge = r(rules.edge_clearance),
        drill = r(rules.min_drill),
        track = r(rules.min_track_width),
        ring = r(rules.min_annular_ring),
        via = r(rules.min_drill + rules.min_annular_ring * 2),
    )
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
        let text = export_project(&Rules::default());
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rules = &v["board"]["design_settings"]["rules"];
        assert_eq!(rules["min_clearance"], 0.127);
        assert_eq!(rules["min_track_width"], 0.127);
        assert_eq!(rules["min_through_hole_diameter"], 0.3);
        assert_eq!(rules["min_via_diameter"], 0.56);
    }
}
