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
}

impl Pad {
    pub fn smd(number: impl Into<String>, at: Point, shape: PadShape, layer: Layer) -> Pad {
        Pad { number: number.into(), at, shape, layers: vec![layer], drill: None }
    }

    pub fn through_hole(number: impl Into<String>, at: Point, shape: PadShape, drill: Nm) -> Pad {
        Pad { number: number.into(), at, shape, layers: vec![Layer::FrontCopper, Layer::BackCopper], drill: Some(drill) }
    }

    /// Bounding box in local coordinates.
    pub fn bbox(&self) -> Rect {
        let (w, h) = self.shape.size();
        Rect::centered(self.at, w, h)
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
