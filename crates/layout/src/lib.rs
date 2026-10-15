//! The board itself: layer stack, outline, placed footprints with pads, tracks and vias.
//!
//! Positions are nanometres (`board_units::Nm`). Rotation is quarter turns for now; arbitrary
//! angles arrive with the placer. Every item carries the net it belongs to so DRC and the router
//! need no second lookup.

#![forbid(unsafe_code)]

use board_geom::{Point, Polygon, Rect, Segment};
use board_units::Nm;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Layer {
    FrontCopper,
    BackCopper,
    /// Inner copper layer, numbered from the top starting at 1.
    InnerCopper(u8),
    FrontSilk,
    BackSilk,
    FrontMask,
    BackMask,
    FrontPaste,
    BackPaste,
    FrontCourtyard,
    BackCourtyard,
    EdgeCuts,
}

impl Layer {
    pub fn is_copper(self) -> bool {
        matches!(self, Layer::FrontCopper | Layer::BackCopper | Layer::InnerCopper(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PadShape {
    Circle {
        diameter: Nm,
    },
    Rect {
        width: Nm,
        height: Nm,
    },
    /// Rectangle with rounded corners of the given radius.
    RoundRect {
        width: Nm,
        height: Nm,
        radius: Nm,
    },
    /// Stadium: a rectangle whose short sides are full semicircles.
    Oval {
        width: Nm,
        height: Nm,
    },
}

impl PadShape {
    pub fn size(self) -> (Nm, Nm) {
        match self {
            PadShape::Circle { diameter } => (diameter, diameter),
            PadShape::Rect { width, height } | PadShape::RoundRect { width, height, .. } | PadShape::Oval { width, height } => (width, height),
        }
    }
}

/// How solder paste covers a pad. Big pads (a module's ground plate) get a grid of windows so the
/// paste volume suits reflow; the copper and mask stay whole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Paste {
    /// Paste over the whole pad (every ordinary SMD pad).
    #[default]
    Full,
    /// An `n` x `n` grid of `cell`-sized squares, outer edges flush with the pad's edges.
    Grid { n: u8, cell: Nm },
    /// No paste (a test point, a thermal pad soldered by hand).
    None,
}

/// A pad in footprint-local coordinates (origin at the footprint anchor, no rotation).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pad {
    pub number: String,
    pub at: Point,
    pub shape: PadShape,
    /// Copper layers this pad lives on. A through-hole pad names both outer layers.
    pub layers: Vec<Layer>,
    /// Drill diameter; `None` for surface-mount.
    pub drill: Option<Nm>,
    #[serde(default)]
    pub paste: Paste,
}

impl Pad {
    pub fn smd(number: impl Into<String>, at: Point, shape: PadShape, layer: Layer) -> Pad {
        Pad { number: number.into(), at, shape, layers: vec![layer], drill: None, paste: Paste::Full }
    }

    pub fn through_hole(number: impl Into<String>, at: Point, shape: PadShape, drill: Nm) -> Pad {
        Pad { number: number.into(), at, shape, layers: vec![Layer::FrontCopper, Layer::BackCopper], drill: Some(drill), paste: Paste::Full }
    }

    /// Bounding box in local coordinates.
    pub fn bbox(&self) -> Rect {
        let (w, h) = self.shape.size();
        Rect::centered(self.at, w, h)
    }

    /// Where paste goes, in footprint-local coordinates: `(centre, square side)` per window.
    /// `Full` gives the pad itself (as its bbox size); `None` gives nothing.
    pub fn paste_windows(&self) -> Vec<(Point, Nm)> {
        match self.paste {
            Paste::None => Vec::new(),
            Paste::Full => {
                let (w, h) = self.shape.size();
                vec![(self.at, w.min(h))]
            }
            Paste::Grid { n, cell } => {
                let (w, h) = self.shape.size();
                let n = n.max(1) as i64;
                let step = |extent: Nm| if n > 1 { (extent - cell) / (n - 1) } else { Nm::ZERO };
                let (sx, sy) = (step(w), step(h));
                let (x0, y0) = (self.at.x - (w - cell) / 2, self.at.y - (h - cell) / 2);
                let mut out = Vec::new();
                for i in 0..n {
                    for j in 0..n {
                        out.push((Point::new(x0 + sx * i, y0 + sy * j), cell));
                    }
                }
                out
            }
        }
    }
}

/// A footprint definition: pads plus courtyard, both in local coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    pub id: String,
    pub pads: Vec<Pad>,
    pub courtyard: Rect,
}

/// A footprint placed on the board for one component.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub refdes: String,
    pub footprint: Footprint,
    pub at: Point,
    pub quarter_turns: u8,
    pub flipped: bool,
}

impl Placement {
    /// Where a local point lands on the board.
    pub fn to_board(&self, local: Point) -> Point {
        let p = if self.flipped { Point::new(-local.x, local.y) } else { local };
        p.rotate_quarter_turns(self.quarter_turns).translate(self.at)
    }

    /// The copper layer a local layer maps to once the part is flipped to the back.
    pub fn layer(&self, local: Layer) -> Layer {
        if !self.flipped {
            return local;
        }
        match local {
            Layer::FrontCopper => Layer::BackCopper,
            Layer::BackCopper => Layer::FrontCopper,
            Layer::FrontSilk => Layer::BackSilk,
            Layer::BackSilk => Layer::FrontSilk,
            Layer::FrontMask => Layer::BackMask,
            Layer::BackMask => Layer::FrontMask,
            Layer::FrontPaste => Layer::BackPaste,
            Layer::BackPaste => Layer::FrontPaste,
            Layer::FrontCourtyard => Layer::BackCourtyard,
            Layer::BackCourtyard => Layer::FrontCourtyard,
            other => other,
        }
    }

    /// Pads in board coordinates with their board layers.
    pub fn board_pads(&self) -> Vec<BoardPad> {
        self.footprint
            .pads
            .iter()
            .map(|p| {
                let bbox = p.bbox();
                let corners = bbox.corners().map(|c| self.to_board(c));
                let bbox = corners.iter().skip(1).fold(Rect::from_points(corners[0], corners[0]), |r, c| r.union(&Rect::from_points(*c, *c)));
                BoardPad {
                    refdes: self.refdes.clone(),
                    number: p.number.clone(),
                    center: self.to_board(p.at),
                    shape: p.shape,
                    quarter_turns: self.quarter_turns,
                    bbox,
                    layers: p.layers.iter().map(|l| self.layer(*l)).collect(),
                    drill: p.drill,
                    paste_kind: if p.drill.is_some() { Paste::None } else { p.paste },
                    paste: if p.drill.is_some() {
                        Vec::new()
                    } else {
                        p.paste_windows().into_iter().map(|(c, side)| (self.to_board(c), side)).collect()
                    },
                }
            })
            .collect()
    }

    pub fn board_courtyard(&self) -> Rect {
        let c = self.footprint.courtyard.corners().map(|c| self.to_board(c));
        c.iter().skip(1).fold(Rect::from_points(c[0], c[0]), |r, p| r.union(&Rect::from_points(*p, *p)))
    }
}

/// A pad resolved to board coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardPad {
    pub refdes: String,
    pub number: String,
    pub center: Point,
    /// Shape in footprint-local orientation; rotate by `quarter_turns` to get board orientation.
    pub shape: PadShape,
    pub quarter_turns: u8,
    pub bbox: Rect,
    pub layers: Vec<Layer>,
    pub drill: Option<Nm>,
    /// How paste covers this pad (`Full` means its own shape).
    pub paste_kind: Paste,
    /// Paste windows in board coordinates: `(centre, square side)`; empty when the pad takes none.
    pub paste: Vec<(Point, Nm)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub net: String,
    pub layer: Layer,
    pub segment: Segment,
    pub width: Nm,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Via {
    pub net: String,
    pub at: Point,
    pub diameter: Nm,
    pub drill: Nm,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub outline: Polygon,
    pub copper_layers: Vec<Layer>,
    pub placements: Vec<Placement>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
}

impl Board {
    /// A two-layer board with a rectangular outline.
    pub fn two_layer(outline: Rect) -> Board {
        Board {
            outline: Polygon::rect(outline),
            copper_layers: vec![Layer::FrontCopper, Layer::BackCopper],
            placements: Vec::new(),
            tracks: Vec::new(),
            vias: Vec::new(),
        }
    }

    pub fn place(&mut self, refdes: &str, footprint: Footprint, at: Point, quarter_turns: u8, flipped: bool) {
        self.placements.push(Placement { refdes: refdes.to_string(), footprint, at, quarter_turns, flipped });
    }

    pub fn placement(&self, refdes: &str) -> Option<&Placement> {
        self.placements.iter().find(|p| p.refdes == refdes)
    }

    pub fn all_pads(&self) -> Vec<BoardPad> {
        self.placements.iter().flat_map(|p| p.board_pads()).collect()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("board is always serialisable")
    }

    pub fn from_json(s: &str) -> Result<Board, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Footprints every example needs; a real library arrives in stage 1.
pub mod footprints {
    use super::*;

    /// 0402 chip resistor or capacitor: two 0.6 x 0.5 mm pads 1.0 mm apart (IPC nominal).
    pub fn chip_0402() -> Footprint {
        let shape = PadShape::Rect { width: Nm::um(600), height: Nm::um(500) };
        Footprint {
            id: "Resistor_SMD:R_0402_1005Metric".into(),
            pads: vec![
                Pad::smd("1", Point::new(-Nm::um(500), Nm::ZERO), shape, Layer::FrontCopper),
                Pad::smd("2", Point::new(Nm::um(500), Nm::ZERO), shape, Layer::FrontCopper),
            ],
            courtyard: Rect::centered(Point::ORIGIN, Nm::um(1860), Nm::um(960)),
        }
    }

    /// 0603 chip part: two 0.9 x 0.95 mm pads 1.6 mm apart.
    pub fn chip_0603() -> Footprint {
        let shape = PadShape::Rect { width: Nm::um(900), height: Nm::um(950) };
        Footprint {
            id: "Resistor_SMD:R_0603_1608Metric".into(),
            pads: vec![
                Pad::smd("1", Point::new(-Nm::um(800), Nm::ZERO), shape, Layer::FrontCopper),
                Pad::smd("2", Point::new(Nm::um(800), Nm::ZERO), shape, Layer::FrontCopper),
            ],
            courtyard: Rect::centered(Point::ORIGIN, Nm::um(2960), Nm::um(1460)),
        }
    }

    /// SOT-23-5: pitch 0.95 mm, pads 1.06 x 0.65 mm at x = +-1.35 mm. Pins 1-3 left (top to
    /// bottom), 4-5 right (bottom to top), as the package numbers them.
    pub fn sot23_5() -> Footprint {
        let shape = PadShape::Rect { width: Nm::um(1060), height: Nm::um(650) };
        let (x, p) = (Nm::um(1350), Nm::um(950));
        Footprint {
            id: "Package_TO_SOT_SMD:SOT-23-5".into(),
            pads: vec![
                Pad::smd("1", Point::new(-x, p), shape, Layer::FrontCopper),
                Pad::smd("2", Point::new(-x, Nm::ZERO), shape, Layer::FrontCopper),
                Pad::smd("3", Point::new(-x, -p), shape, Layer::FrontCopper),
                Pad::smd("4", Point::new(x, -p), shape, Layer::FrontCopper),
                Pad::smd("5", Point::new(x, p), shape, Layer::FrontCopper),
            ],
            courtyard: Rect::centered(Point::ORIGIN, Nm::um(4140), Nm::um(3500)),
        }
    }

    /// Two-pad surface-mount tactile switch, 6 x 6 mm body, pads 2.0 x 1.5 mm at x = +-4.0 mm.
    pub fn tactile_6x6_2pad() -> Footprint {
        let shape = PadShape::Rect { width: Nm::um(2000), height: Nm::um(1500) };
        Footprint {
            id: "Redrob_Button:SW_SMD_6x6_2pad".into(),
            pads: vec![
                Pad::smd("1", Point::new(-Nm::mm(4), Nm::ZERO), shape, Layer::FrontCopper),
                Pad::smd("2", Point::new(Nm::mm(4), Nm::ZERO), shape, Layer::FrontCopper),
            ],
            courtyard: Rect::centered(Point::ORIGIN, Nm::um(10500), Nm::um(6500)),
        }
    }

    /// Castellated radio module: `per_side` pads down the left and up the right edge at 1.27 mm
    /// pitch, and as many pads along the bottom edge as fit between the side columns with
    /// clearance. Pads are 0.7 x 1.0 mm, body `width` x `height`. Pin 1 is top-left; numbering runs
    /// down the left edge, along the bottom left to right, then up the right edge. This is a generic
    /// module outline: the pin-to-function map is the schematic's business and is checked against
    /// the part's datasheet before order.
    pub fn castellated_module_3side(per_side: usize, width: Nm, height: Nm) -> Footprint {
        let pitch = Nm::um(1270);
        let along = PadShape::Rect { width: Nm::um(1000), height: Nm::um(700) }; // left/right edges
        let across = PadShape::Rect { width: Nm::um(700), height: Nm::um(1000) }; // bottom edge
        let half_w = width / 2;
        let half_h = height / 2;
        let span = pitch * (per_side as i64 - 1);
        // Bottom row must stay 0.2 mm clear of the side columns (which reach 1.0 mm in from the edge).
        let bottom_reach = half_w - Nm::um(1200) - Nm::um(350);
        let bottom_n = if bottom_reach > Nm::ZERO { ((bottom_reach * 2).0 / pitch.0) as usize + 1 } else { 0 };
        let bottom_span = pitch * (bottom_n as i64 - 1).max(0);
        let mut pads = Vec::new();
        let mut n = 1;
        let mut push = |at: Point, shape: PadShape| {
            pads.push(Pad::smd(n.to_string(), at, shape, Layer::FrontCopper));
            n += 1;
        };
        for i in 0..per_side as i64 {
            push(Point::new(-half_w + Nm::um(500), span / 2 - pitch * i), along);
        }
        for i in 0..bottom_n as i64 {
            push(Point::new(-bottom_span / 2 + pitch * i, -half_h + Nm::um(500)), across);
        }
        for i in 0..per_side as i64 {
            push(Point::new(half_w - Nm::um(500), -span / 2 + pitch * i), along);
        }
        Footprint {
            id: format!("Redrob_Module:Castellated_{per_side}x{bottom_n}x{per_side}_P1.27mm_{}x{}", width.mm_string(), height.mm_string()),
            pads,
            courtyard: Rect::centered(Point::ORIGIN, width + Nm::um(500), height + Nm::um(500)),
        }
    }

    /// ESP32-C3-MINI-1 (Espressif datasheet, section "Recommended PCB Land Pattern"): 13.2 x 16.6
    /// mm module, 0.8 mm pitch. Pins 1-11 down the left edge (pin 1 top), 12-24 along the bottom
    /// left to right, 25-35 up the right edge, 36-48 along the top right to left under the antenna
    /// keep-out, 49 the 5.4 x 5.4 mm ground plate under the body, 50-53 the corner ground pads.
    /// Pin functions are the datasheet's (GND 1,2,11,14,36-53; 3V3 3; EN 8; IO0 12; IO1 13; IO10
    /// 16; IO4-IO9 18-23; IO18/19 26/27; IO20/U0RXD 30; IO21/U0TXD 31) and live in the schematic.
    /// The courtyard spans the whole module including the antenna area (13.6 x 17.0 mm).
    pub fn esp32_c3_mini_1() -> Footprint {
        let pitch = Nm::um(800);
        let along_y = PadShape::Rect { width: Nm::um(800), height: Nm::um(400) }; // left/right edges
        let along_x = PadShape::Rect { width: Nm::um(400), height: Nm::um(800) }; // top/bottom edges
        let corner = PadShape::Rect { width: Nm::um(700), height: Nm::um(700) };
        let (edge_x, top_y, bottom_y) = (Nm::um(5900), Nm::um(2200), -Nm::um(7600));
        let mut pads = Vec::new();
        let mut n = 1;
        let mut push = |at: Point, shape: PadShape| {
            pads.push(Pad::smd(n.to_string(), at, shape, Layer::FrontCopper));
            n += 1;
        };
        for i in 0..11 {
            push(Point::new(-edge_x, Nm::um(1300) - pitch * i), along_y);
        }
        for i in 0..13 {
            push(Point::new(-Nm::um(4800) + pitch * i, bottom_y), along_x);
        }
        for i in 0..11 {
            push(Point::new(edge_x, -Nm::um(6700) + pitch * i), along_y);
        }
        for i in 0..13 {
            push(Point::new(Nm::um(4800) - pitch * i, top_y), along_x);
        }
        push(Point::new(Nm::ZERO, -Nm::um(2700)), PadShape::Rect { width: Nm::um(5400), height: Nm::um(5400) });
        push(Point::new(Nm::um(5950), Nm::um(2250)), corner);
        push(Point::new(Nm::um(5950), -Nm::um(7650)), corner);
        push(Point::new(-Nm::um(5950), -Nm::um(7650)), corner);
        push(Point::new(-Nm::um(5950), Nm::um(2250)), corner);
        // Datasheet paste pattern for the ground plate: nine 1.45 mm windows at 1.975 mm pitch.
        pads[48].paste = Paste::Grid { n: 3, cell: Nm::um(1450) };
        Footprint { id: "Redrob_RF_Module:ESP32-C3-MINI-1".into(), pads, courtyard: Rect::centered(Point::ORIGIN, Nm::um(13600), Nm::um(17000)) }
    }

    /// AHT20 humidity/temperature sensor (ASAIR datasheet V1.0, figure 8): 3 x 3 mm six-pad
    /// no-lead package, two columns 2.0 mm apart, three rows at 1.0 mm, pads 0.8 x 0.5 mm. Pin 1
    /// top-left, 2 and 3 below it, 4 bottom-right, 5 and 6 above it (top view). Functions: 1 NC,
    /// 2 VDD, 3 SCL, 4 SDA, 5 GND, 6 NC.
    pub fn aht20_3x3() -> Footprint {
        let shape = PadShape::Rect { width: Nm::um(800), height: Nm::um(500) };
        let (x, p) = (Nm::mm(1), Nm::mm(1));
        Footprint {
            id: "Redrob_Sensor:AHT20_3x3mm".into(),
            pads: vec![
                Pad::smd("1", Point::new(-x, p), shape, Layer::FrontCopper),
                Pad::smd("2", Point::new(-x, Nm::ZERO), shape, Layer::FrontCopper),
                Pad::smd("3", Point::new(-x, -p), shape, Layer::FrontCopper),
                Pad::smd("4", Point::new(x, -p), shape, Layer::FrontCopper),
                Pad::smd("5", Point::new(x, Nm::ZERO), shape, Layer::FrontCopper),
                Pad::smd("6", Point::new(x, p), shape, Layer::FrontCopper),
            ],
            courtyard: Rect::centered(Point::ORIGIN, Nm::um(3500), Nm::um(3500)),
        }
    }

    /// 2.54 mm pin header, 1 x n, vertical. Pin 1 at the origin, others along +x.
    pub fn pin_header_1xn(n: usize) -> Footprint {
        let pitch = Nm::um(2540);
        let pads = (0..n)
            .map(|i| {
                Pad::through_hole((i + 1).to_string(), Point::new(pitch * i as i64, Nm::ZERO), PadShape::Circle { diameter: Nm::um(1700) }, Nm::mm(1))
            })
            .collect();
        let half = pitch / 2;
        Footprint {
            id: format!("Connector_PinHeader_2.54mm:PinHeader_1x{n:02}_P2.54mm_Vertical"),
            pads,
            courtyard: Rect::from_points(Point::new(-half, -half), Point::new(pitch * (n as i64 - 1) + half, half)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_transforms_pads() {
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        b.place("R1", footprints::chip_0402(), Point::mm(5, 5), 1, false);
        let pads = b.all_pads();
        assert_eq!(pads.len(), 2);
        // Rotated a quarter turn: pads now stack vertically around (5, 5).
        assert_eq!(pads[0].center, Point::new(Nm::mm(5), Nm::mm(5) - Nm::um(500)));
        assert_eq!(pads[1].center, Point::new(Nm::mm(5), Nm::mm(5) + Nm::um(500)));
        // Width and height swap under rotation.
        assert_eq!(pads[0].bbox.width(), Nm::um(500));
        assert_eq!(pads[0].bbox.height(), Nm::um(600));
        assert_eq!(pads[0].layers, vec![Layer::FrontCopper]);
    }

    #[test]
    fn flipped_part_goes_to_back() {
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        b.place("C1", footprints::chip_0402(), Point::mm(10, 5), 0, true);
        let pads = b.all_pads();
        assert_eq!(pads[0].layers, vec![Layer::BackCopper]);
        // Mirrored in x: pad 1 is now on the +x side.
        assert_eq!(pads[0].center, Point::new(Nm::mm(10) + Nm::um(500), Nm::mm(5)));
        let yard = b.placement("C1").unwrap().board_courtyard();
        assert_eq!(yard.center(), Point::mm(10, 5));
    }

    #[test]
    fn header_pads_and_json() {
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        b.place("J1", footprints::pin_header_1xn(3), Point::mm(2, 2), 0, false);
        let pads = b.all_pads();
        assert_eq!(pads.len(), 3);
        assert_eq!(pads[2].center, Point::new(Nm::mm(2) + Nm::um(5080), Nm::mm(2)));
        assert_eq!(pads[2].drill, Some(Nm::mm(1)));
        assert!(pads[2].layers.contains(&Layer::BackCopper));
        assert_eq!(Board::from_json(&b.to_json()).unwrap(), b);
    }
}
