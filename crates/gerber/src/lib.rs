//! Gerber RS-274X writer and reader.
//!
//! The format is `%FSLAX46Y46*%` with millimetres, so a coordinate field is exactly the
//! nanometre integer the rest of the tools already hold: no rounding in or out. The reader exists
//! so CI can prove the writer round-trips and so other tools' Gerbers can be compared with ours.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use board_geom::{Point, Polygon, Rect, Segment};
use board_layout::{Board, Layer, PadShape};
use board_units::Nm;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aperture {
    Circle {
        diameter: Nm,
    },
    Rect {
        width: Nm,
        height: Nm,
    },
    /// Standard `O` aperture: a stadium.
    Obround {
        width: Nm,
        height: Nm,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Select aperture `Dnn`.
    Select(u32),
    /// `D02`: move without drawing.
    Move(Point),
    /// `D01`: draw a straight line to the point with the current aperture.
    Draw(Point),
    /// `D03`: flash the current aperture at the point.
    Flash(Point),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GerberFile {
    /// Aperture definitions, `D10` upwards.
    pub apertures: BTreeMap<u32, Aperture>,
    pub ops: Vec<Op>,
}

impl GerberFile {
    pub fn new() -> GerberFile {
        GerberFile::default()
    }

    /// Return the code for an aperture, defining it if it is new.
    pub fn aperture(&mut self, a: Aperture) -> u32 {
        if let Some((code, _)) = self.apertures.iter().find(|(_, v)| **v == a) {
            return *code;
        }
        let code = self.apertures.keys().next_back().map_or(10, |k| k + 1);
        self.apertures.insert(code, a);
        code
    }

    pub fn flash(&mut self, aperture: Aperture, at: Point) {
        let code = self.aperture(aperture);
        self.ops.push(Op::Select(code));
        self.ops.push(Op::Flash(at));
    }

    pub fn polyline(&mut self, width: Nm, points: &[Point], close: bool) {
        let Some((first, rest)) = points.split_first() else { return };
        let code = self.aperture(Aperture::Circle { diameter: width });
        self.ops.push(Op::Select(code));
        self.ops.push(Op::Move(*first));
        for p in rest {
            self.ops.push(Op::Draw(*p));
        }
        if close && rest.last() != Some(first) {
            self.ops.push(Op::Draw(*first));
        }
    }

    pub fn write(&self) -> String {
        let mut s = String::new();
        s.push_str("%TF.GenerationSoftware,Redrob,Board,0.0.1*%\n");
        s.push_str("%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\n");
        for (code, a) in &self.apertures {
            match a {
                Aperture::Circle { diameter } => writeln!(s, "%ADD{code}C,{}*%", mm(*diameter)).unwrap(),
                Aperture::Rect { width, height } => writeln!(s, "%ADD{code}R,{}X{}*%", mm(*width), mm(*height)).unwrap(),
                Aperture::Obround { width, height } => writeln!(s, "%ADD{code}O,{}X{}*%", mm(*width), mm(*height)).unwrap(),
            }
        }
        s.push_str("G01*\n");
        for op in &self.ops {
            match op {
                Op::Select(code) => writeln!(s, "D{code}*").unwrap(),
                Op::Move(p) => writeln!(s, "X{}Y{}D02*", p.x.0, p.y.0).unwrap(),
                Op::Draw(p) => writeln!(s, "X{}Y{}D01*", p.x.0, p.y.0).unwrap(),
                Op::Flash(p) => writeln!(s, "X{}Y{}D03*", p.x.0, p.y.0).unwrap(),
            }
        }
        s.push_str("M02*\n");
        s
    }

    pub fn parse(text: &str) -> Result<GerberFile, GerberError> {
        let mut file = GerberFile::new();
        let mut saw_format = false;
        let mut saw_units = false;
        // Current point is implicit when X or Y is omitted (modal coordinates).
        let mut cur = Point::ORIGIN;
        for raw in text.split('*') {
            let cmd = raw.trim_matches(['\n', '\r', ' ', '%']);
            if cmd.is_empty() || cmd.starts_with("TF.") {
                continue;
            }
            if let Some(fs) = cmd.strip_prefix("FS") {
                if fs != "LAX46Y46" {
                    return Err(GerberError::Unsupported(format!("coordinate format {fs}")));
                }
                saw_format = true;
            } else if let Some(mo) = cmd.strip_prefix("MO") {
                if mo != "MM" {
                    return Err(GerberError::Unsupported(format!("unit {mo}")));
                }
                saw_units = true;
            } else if cmd.starts_with("LP") || cmd == "G01" || cmd == "G04" || cmd.starts_with("G04 ") {
                // Polarity dark only; linear interpolation only; comments ignored.
                if cmd == "LPC" {
                    return Err(GerberError::Unsupported("clear polarity".into()));
                }
            } else if let Some(ad) = cmd.strip_prefix("ADD") {
                let (code, def) = split_code(ad)?;
                let aperture = if let Some(d) = def.strip_prefix("C,") {
                    Aperture::Circle { diameter: parse_mm(d)? }
                } else if let Some(r) = def.strip_prefix("R,") {
                    let (w, h) = r.split_once('X').ok_or_else(|| GerberError::Malformed(cmd.into()))?;
                    Aperture::Rect { width: parse_mm(w)?, height: parse_mm(h)? }
                } else if let Some(o) = def.strip_prefix("O,") {
                    let (w, h) = o.split_once('X').ok_or_else(|| GerberError::Malformed(cmd.into()))?;
                    Aperture::Obround { width: parse_mm(w)?, height: parse_mm(h)? }
                } else {
                    return Err(GerberError::Unsupported(format!("aperture {def}")));
                };
                file.apertures.insert(code, aperture);
            } else if cmd == "M02" {
                break;
            } else if let Some(d) = cmd.strip_prefix('D') {
                if let Ok(code) = d.parse::<u32>()
                    && code >= 10
                {
                    file.ops.push(Op::Select(code));
                } else {
                    return Err(GerberError::Malformed(cmd.into()));
                }
            } else if cmd.starts_with(['X', 'Y']) {
                let (coords, d) = cmd.split_once('D').ok_or_else(|| GerberError::Malformed(cmd.into()))?;
                let mut rest = coords;
                if let Some(after_x) = rest.strip_prefix('X') {
                    let end = after_x.find('Y').unwrap_or(after_x.len());
                    cur.x = Nm(after_x[..end].parse().map_err(|_| GerberError::Malformed(cmd.into()))?);
                    rest = &after_x[end..];
                }
                if let Some(after_y) = rest.strip_prefix('Y') {
                    cur.y = Nm(after_y.parse().map_err(|_| GerberError::Malformed(cmd.into()))?);
                }
                file.ops.push(match d {
                    "01" => Op::Draw(cur),
                    "02" => Op::Move(cur),
                    "03" => Op::Flash(cur),
                    _ => return Err(GerberError::Malformed(cmd.into())),
                });
            } else {
                return Err(GerberError::Unsupported(cmd.into()));
            }
        }
        if !saw_format || !saw_units {
            return Err(GerberError::Malformed("missing FS or MO".into()));
        }
        Ok(file)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GerberError {
    #[error("unsupported Gerber construct: {0}")]
    Unsupported(String),
    #[error("malformed Gerber command: {0}")]
    Malformed(String),
}

fn split_code(s: &str) -> Result<(u32, &str), GerberError> {
    let digits = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let code = s[..digits].parse().map_err(|_| GerberError::Malformed(s.into()))?;
    Ok((code, &s[digits..]))
}

fn mm(v: Nm) -> String {
    v.mm_string()
}

fn parse_mm(s: &str) -> Result<Nm, GerberError> {
    Nm::parse_mm(s).ok_or_else(|| GerberError::Malformed(s.into()))
}

/// Edge-cuts width factories conventionally expect.
pub const OUTLINE_WIDTH: Nm = Nm::um(100);

/// Render one layer of a board to a Gerber file.
///
/// Mask layers carry every pad on that side's copper at pad size (mask margin 0; vias tented).
/// Paste layers carry surface-mount pads only.
pub fn export_layer(board: &Board, layer: Layer) -> GerberFile {
    let mut g = GerberFile::new();
    match layer {
        Layer::EdgeCuts => outline(&mut g, &board.outline),
        l if l.is_copper() => {
            for pad in board.all_pads() {
                if pad.layers.contains(&l) {
                    g.flash(aperture_for(pad.shape, pad.quarter_turns), pad.center);
                }
            }
            for t in board.tracks.iter().filter(|t| t.layer == l) {
                g.polyline(t.width, &[t.segment.a, t.segment.b], false);
            }
            for v in &board.vias {
                g.flash(Aperture::Circle { diameter: v.diameter }, v.at);
            }
        }
        Layer::FrontMask | Layer::BackMask | Layer::FrontPaste | Layer::BackPaste => {
            let copper = if matches!(layer, Layer::FrontMask | Layer::FrontPaste) { Layer::FrontCopper } else { Layer::BackCopper };
            let paste = matches!(layer, Layer::FrontPaste | Layer::BackPaste);
            for pad in board.all_pads() {
                if pad.layers.contains(&copper) && !(paste && pad.drill.is_some()) {
                    g.flash(aperture_for(pad.shape, pad.quarter_turns), pad.center);
                }
            }
        }
        _ => {}
    }
    g
}

/// Excellon drill file: every plated hole (through-hole pads, vias) and non-plated hole, in
/// millimetres with decimal coordinates.
pub fn export_drill(board: &Board) -> String {
    let mut holes: Vec<(Nm, Point)> = Vec::new();
    for pad in board.all_pads() {
        if let Some(d) = pad.drill {
            holes.push((d, pad.center));
        }
    }
    for v in &board.vias {
        holes.push((v.drill, v.at));
    }
    let mut tools: Vec<Nm> = holes.iter().map(|h| h.0).collect();
    tools.sort();
    tools.dedup();
    let mut s = String::from("M48\nFMAT,2\nMETRIC\n");
    for (i, t) in tools.iter().enumerate() {
        writeln!(s, "T{}C{}", i + 1, t.mm_string()).unwrap();
    }
    s.push_str("%\nG90\nG05\n");
    for (i, t) in tools.iter().enumerate() {
        writeln!(s, "T{}", i + 1).unwrap();
        for (_, p) in holes.iter().filter(|h| h.0 == *t) {
            writeln!(s, "X{}Y{}", p.x.mm_string(), p.y.mm_string()).unwrap();
        }
    }
    s.push_str("T0\nM30\n");
    s
}

/// Read an Excellon file (decimal millimetres, as `export_drill` and the oracle write it) into a
/// sorted list of (diameter, position) so two files compare as sets.
pub fn parse_drill(text: &str) -> Result<Vec<(Nm, Point)>, GerberError> {
    let mut tools: BTreeMap<u32, Nm> = BTreeMap::new();
    let mut current: Option<Nm> = None;
    let mut holes = Vec::new();
    let mut in_header = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line == "M48" {
            in_header = true;
            continue;
        }
        if line == "%" || line == "M95" {
            in_header = false;
            continue;
        }
        if line == "M30" || line == "M00" {
            break;
        }
        if let Some(rest) = line.strip_prefix('T') {
            let (num, dia) = rest.split_once('C').unwrap_or((rest, ""));
            let num: u32 = num.parse().map_err(|_| GerberError::Malformed(line.into()))?;
            if in_header {
                tools.insert(num, parse_mm(dia)?);
            } else {
                current = if num == 0 { None } else { Some(*tools.get(&num).ok_or_else(|| GerberError::Malformed(line.into()))?) };
            }
            continue;
        }
        if let Some(coords) = line.strip_prefix('X') {
            let (x, y) = coords.split_once('Y').ok_or_else(|| GerberError::Malformed(line.into()))?;
            let d = current.ok_or_else(|| GerberError::Malformed(format!("hole before tool select: {line}")))?;
            holes.push((d, Point::new(parse_mm(x)?, parse_mm(y)?)));
            continue;
        }
        if line == "INCH" {
            return Err(GerberError::Unsupported("inch drill file".into()));
        }
        if line.starts_with('G') || line == "FMAT,2" || line == "METRIC" {
            continue;
        }
        return Err(GerberError::Unsupported(line.into()));
    }
    holes.sort_by_key(|(d, p)| (d.0, p.x.0, p.y.0));
    Ok(holes)
}

/// A bitmap of a Gerber file: pixel (x, y) is set when its centre is inside any drawn shape.
pub struct Raster {
    pub origin: Point,
    pub pitch: Nm,
    pub width: usize,
    pub height: usize,
    pub bits: Vec<bool>,
}

impl Raster {
    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }

    /// Pixels set in exactly one of the two rasters (same origin/pitch/size required).
    pub fn differing_pixels(&self, other: &Raster) -> usize {
        assert!(self.width == other.width && self.height == other.height && self.origin == other.origin && self.pitch == other.pitch);
        self.bits.iter().zip(&other.bits).filter(|(a, b)| a != b).count()
    }
}

impl GerberFile {
    /// Bounding box of everything drawn, including aperture extents.
    pub fn bbox(&self) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        let mut cur: Option<u32> = None;
        let mut pos = Point::ORIGIN;
        for op in &self.ops {
            let mut extend = |r: Rect| out = Some(out.map_or(r, |o| o.union(&r)));
            match op {
                Op::Select(c) => cur = Some(*c),
                Op::Move(p) => pos = *p,
                Op::Flash(p) => {
                    if let Some(a) = cur.and_then(|c| self.apertures.get(&c)) {
                        let (w, h) = aperture_size(*a);
                        extend(Rect::centered(*p, w, h));
                    }
                    pos = *p;
                }
                Op::Draw(p) => {
                    if let Some(a) = cur.and_then(|c| self.apertures.get(&c)) {
                        let (w, _) = aperture_size(*a);
                        extend(Rect::from_points(pos, *p).inflate(w / 2));
                    }
                    pos = *p;
                }
            }
        }
        out
    }

    /// Rasterise onto a grid covering `area` at `pitch` (pixel centres at origin + k * pitch).
    pub fn rasterise(&self, area: Rect, pitch: Nm) -> Raster {
        let width = (area.width().0 / pitch.0) as usize + 1;
        let height = (area.height().0 / pitch.0) as usize + 1;
        let mut r = Raster { origin: area.min, pitch, width, height, bits: vec![false; width * height] };
        let mut cur: Option<Aperture> = None;
        let mut pos = Point::ORIGIN;
        for op in &self.ops {
            match op {
                Op::Select(c) => cur = self.apertures.get(c).copied(),
                Op::Move(p) => pos = *p,
                Op::Flash(p) => {
                    if let Some(a) = cur {
                        paint(&mut r, Painted::Flash(a, *p));
                    }
                    pos = *p;
                }
                Op::Draw(p) => {
                    if let Some(a) = cur {
                        let (w, _) = aperture_size(a);
                        paint(&mut r, Painted::Stroke(Segment::new(pos, *p), w / 2));
                    }
                    pos = *p;
                }
            }
        }
        r
    }
}

fn aperture_size(a: Aperture) -> (Nm, Nm) {
    match a {
        Aperture::Circle { diameter } => (diameter, diameter),
        Aperture::Rect { width, height } | Aperture::Obround { width, height } => (width, height),
    }
}

enum Painted {
    Flash(Aperture, Point),
    Stroke(Segment, Nm),
}

fn paint(r: &mut Raster, shape: Painted) {
    let bbox = match &shape {
        Painted::Flash(a, p) => {
            let (w, h) = aperture_size(*a);
            Rect::centered(*p, w, h)
        }
        Painted::Stroke(s, hw) => s.bbox().inflate(*hw),
    };
    let lo = |v: Nm, o: Nm| ((v - o).0.max(0) as f64 / r.pitch.0 as f64).ceil() as i64;
    let hi = |v: Nm, o: Nm| ((v - o).0 as f64 / r.pitch.0 as f64).floor() as i64;
    let x0 = lo(bbox.min.x, r.origin.x);
    let y0 = lo(bbox.min.y, r.origin.y);
    let x1 = hi(bbox.max.x, r.origin.x).min(r.width as i64 - 1);
    let y1 = hi(bbox.max.y, r.origin.y).min(r.height as i64 - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let c = Point::new(r.origin.x + r.pitch * x, r.origin.y + r.pitch * y);
            let inside = match &shape {
                Painted::Flash(a, p) => match *a {
                    Aperture::Circle { diameter } => c.squared_distance(*p) <= (diameter / 2).squared(),
                    Aperture::Rect { width, height } => Rect::centered(*p, width, height).contains(c),
                    Aperture::Obround { width, height } => {
                        // Two semicircles joined by a rectangle along the longer side.
                        let rad = width.min(height) / 2;
                        let (ax, ay) = if width > height { ((width - height) / 2, Nm::ZERO) } else { (Nm::ZERO, (height - width) / 2) };
                        let seg = Segment::new(p.offset(-ax, -ay), p.offset(ax, ay));
                        seg.squared_distance_to_point(c) <= rad.squared()
                    }
                },
                Painted::Stroke(s, hw) => s.squared_distance_to_point(c) <= hw.squared(),
            };
            if inside {
                r.bits[y as usize * r.width + x as usize] = true;
            }
        }
    }
}

fn outline(g: &mut GerberFile, poly: &Polygon) {
    g.polyline(OUTLINE_WIDTH, &poly.vertices, true);
}

/// Pad shape to a standard aperture, rotated by quarter turns.
///
/// Rounded rectangles have no standard aperture; they are emitted as plain rectangles until the
/// stage-4 export adds aperture macros. Stage 4's pixel comparison against the oracle is what
/// forces that change, so the approximation cannot ship unnoticed.
pub fn aperture_for(shape: PadShape, quarter_turns: u8) -> Aperture {
    let swap = quarter_turns % 2 == 1;
    let (w, h) = shape.size();
    let (w, h) = if swap { (h, w) } else { (w, h) };
    match shape {
        PadShape::Circle { diameter } => Aperture::Circle { diameter },
        PadShape::Rect { .. } | PadShape::RoundRect { .. } => Aperture::Rect { width: w, height: h },
        PadShape::Oval { .. } => Aperture::Obround { width: w, height: h },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_geom::Rect;
    use board_layout::footprints;

    fn sample_board() -> Board {
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        b.place("R1", footprints::chip_0402(), Point::mm(5, 5), 0, false);
        b.place("C1", footprints::chip_0402(), Point::mm(8, 5), 1, true);
        b.place("J1", footprints::pin_header_1xn(3), Point::mm(12, 2), 0, false);
        b
    }

    #[test]
    fn roundtrip_every_layer() {
        let b = sample_board();
        for layer in [Layer::FrontCopper, Layer::BackCopper, Layer::EdgeCuts] {
            let g = export_layer(&b, layer);
            let text = g.write();
            let back = GerberFile::parse(&text).unwrap();
            assert_eq!(back, g, "layer {layer:?}");
            assert_eq!(back.write(), text);
        }
    }

    #[test]
    fn front_copper_has_expected_flashes() {
        let g = export_layer(&sample_board(), Layer::FrontCopper);
        let flashes = g.ops.iter().filter(|o| matches!(o, Op::Flash(_))).count();
        // R1 (2 pads) + J1 (3 through-hole pads); C1 is on the back.
        assert_eq!(flashes, 5);
        assert_eq!(g.apertures.len(), 2);
        let text = g.write();
        assert!(text.contains("%ADD10R,0.6X0.5*%"));
        assert!(text.contains("%ADD11C,1.7*%"));
        assert_eq!(
            aperture_for(PadShape::Oval { width: Nm::mm(2), height: Nm::mm(1) }, 1),
            Aperture::Obround { width: Nm::mm(1), height: Nm::mm(2) }
        );
        assert!(text.contains("X4500000Y5000000D03*"));
        let obround = "%FSLAX46Y46*%%MOMM*%%ADD10O,2X1*%D10*X0Y0D03*M02*";
        assert_eq!(GerberFile::parse(obround).unwrap().apertures.get(&10), Some(&Aperture::Obround { width: Nm::mm(2), height: Nm::mm(1) }));
    }

    #[test]
    fn edge_cuts_is_closed_outline() {
        let g = export_layer(&sample_board(), Layer::EdgeCuts);
        assert_eq!(g.apertures.get(&10), Some(&Aperture::Circle { diameter: OUTLINE_WIDTH }));
        let draws = g.ops.iter().filter(|o| matches!(o, Op::Draw(_))).count();
        assert_eq!(draws, 4);
        assert_eq!(g.ops.last(), Some(&Op::Draw(Point::ORIGIN)));
    }

    #[test]
    fn parser_handles_modal_coordinates_and_rejects_junk() {
        let text = "%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.25*%\nD10*\nX1000000Y2000000D02*\nX3000000D01*\nY0D01*\nM02*\n";
        let g = GerberFile::parse(text).unwrap();
        assert_eq!(g.ops, vec![Op::Select(10), Op::Move(Point::mm(1, 2)), Op::Draw(Point::mm(3, 2)), Op::Draw(Point::mm(3, 0))]);
        assert_eq!(GerberFile::parse("%FSLAX24Y24*%%MOMM*%M02*"), Err(GerberError::Unsupported("coordinate format LAX24Y24".into())));
        assert_eq!(GerberFile::parse("%FSLAX46Y46*%%MOIN*%M02*"), Err(GerberError::Unsupported("unit IN".into())));
        assert!(matches!(GerberFile::parse("%FSLAX46Y46*%%MOMM*%G36*M02*"), Err(GerberError::Unsupported(_))));
    }

    #[test]
    fn mask_paste_and_drill() {
        let b = sample_board();
        let flashes = |g: &GerberFile| g.ops.iter().filter(|o| matches!(o, Op::Flash(_))).count();
        // R1 pads + J1 pads on the front; C1 is on the back.
        assert_eq!(flashes(&export_layer(&b, Layer::FrontMask)), 5);
        assert_eq!(flashes(&export_layer(&b, Layer::FrontPaste)), 2);
        assert_eq!(flashes(&export_layer(&b, Layer::BackMask)), 5); // C1 + J1 through-hole
        assert_eq!(flashes(&export_layer(&b, Layer::BackPaste)), 2);

        let drill = export_drill(&b);
        assert!(drill.starts_with("M48\nFMAT,2\nMETRIC\nT1C1\n%\n"), "{drill}");
        let holes = parse_drill(&drill).unwrap();
        assert_eq!(holes.len(), 3);
        assert_eq!(holes[0], (Nm::mm(1), Point::mm(12, 2)));
        let kicad_style =
            "M48\n; DRILL file {KiCad 9}\nFMAT,2\nMETRIC\n; #@! TA.AperFunction,Plated,PTH,ViaDrill\nT1C0.300\n%\nG90\nG05\nT1\nX3.9Y12.1\nT0\nM30\n";
        assert_eq!(parse_drill(kicad_style).unwrap(), vec![(Nm::um(300), Point::new(Nm::um(3900), Nm::um(12100)))]);
    }

    #[test]
    fn raster_matches_shapes() {
        let b = sample_board();
        let g = export_layer(&b, Layer::FrontCopper);
        let area = Rect::from_points(Point::ORIGIN, Point::mm(20, 10));
        let r = g.rasterise(area, Nm::um(10));
        // One 0.6 x 0.5 pad covers 61 x 51 pixel centres; two of them plus three 1.7 mm circles.
        let mut circle = 0;
        for y in -85..=85_i64 {
            for x in -85..=85_i64 {
                if x * x + y * y <= 85 * 85 {
                    circle += 1;
                }
            }
        }
        assert_eq!(r.count(), 2 * 61 * 51 + 3 * circle);
        // The same drawing written and parsed back rasterises identically.
        let again = GerberFile::parse(&g.write()).unwrap().rasterise(area, Nm::um(10));
        assert_eq!(r.differing_pixels(&again), 0);
        assert_eq!(g.bbox(), Some(Rect::from_points(Point::new(Nm::um(4200), Nm::um(1150)), Point::new(Nm::um(17930), Nm::um(5250)))));
    }

    #[test]
    fn mm_text() {
        assert_eq!(parse_mm("0.254").unwrap(), Nm::mil(10));
        assert_eq!(parse_mm("1.7").unwrap(), Nm::um(1700));
        assert_eq!(parse_mm("-2").unwrap(), Nm::mm(-2));
        assert_eq!(mm(Nm::um(600)), "0.6");
        assert!(parse_mm("1.2345678").is_err());
    }
}
