//! Gerber RS-274X writer and reader.
//!
//! The format is `%FSLAX46Y46*%` with millimetres, so a coordinate field is exactly the
//! nanometre integer the rest of the tools already hold: no rounding in or out. The reader exists
//! so CI can prove the writer round-trips and so other tools' Gerbers can be compared with ours.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use board_geom::{Point, Polygon};
use board_layout::{Board, Layer, PadShape};
use board_units::Nm;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aperture {
    Circle { diameter: Nm },
    Rect { width: Nm, height: Nm },
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

/// Millimetres with six decimals and no trailing zeros, as aperture definitions want them.
fn mm(v: Nm) -> String {
    let s = v.to_string();
    s.trim_end_matches("mm").to_string()
}

fn parse_mm(s: &str) -> Result<Nm, GerberError> {
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if frac.len() > 6 || whole.is_empty() && frac.is_empty() {
        return Err(GerberError::Malformed(s.into()));
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| GerberError::Malformed(s.into()))? };
    let frac: i64 = if frac.is_empty() { 0 } else { format!("{frac:0<6}").parse().map_err(|_| GerberError::Malformed(s.into()))? };
    let v = whole * Nm::NM_PER_MM + frac;
    Ok(Nm(if neg { -v } else { v }))
}

/// Edge-cuts width factories conventionally expect.
pub const OUTLINE_WIDTH: Nm = Nm::um(100);

/// Render one layer of a board to a Gerber file.
pub fn export_layer(board: &Board, layer: Layer) -> GerberFile {
    let mut g = GerberFile::new();
    if layer == Layer::EdgeCuts {
        outline(&mut g, &board.outline);
        return g;
    }
    if layer.is_copper() {
        for pad in board.all_pads() {
            if !pad.layers.contains(&layer) {
                continue;
            }
            let shape = if pad.bbox.width() == pad.bbox.height() && pad.drill.is_some() {
                Aperture::Circle { diameter: pad.bbox.width() }
            } else {
                Aperture::Rect { width: pad.bbox.width(), height: pad.bbox.height() }
            };
            g.flash(shape, pad.center);
        }
        for t in board.tracks.iter().filter(|t| t.layer == layer) {
            g.polyline(t.width, &[t.segment.a, t.segment.b], false);
        }
        for v in &board.vias {
            g.flash(Aperture::Circle { diameter: v.diameter }, v.at);
        }
    }
    g
}

fn outline(g: &mut GerberFile, poly: &Polygon) {
    g.polyline(OUTLINE_WIDTH, &poly.vertices, true);
}

/// Pad shape to aperture, for callers building Gerbers by hand.
pub fn aperture_for(shape: PadShape) -> Aperture {
    match shape {
        PadShape::Circle { diameter } => Aperture::Circle { diameter },
        PadShape::Rect { width, height } => Aperture::Rect { width, height },
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
        assert!(text.contains("X4500000Y5000000D03*"));
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
    fn mm_text() {
        assert_eq!(parse_mm("0.254").unwrap(), Nm::mil(10));
        assert_eq!(parse_mm("1.7").unwrap(), Nm::um(1700));
        assert_eq!(parse_mm("-2").unwrap(), Nm::mm(-2));
        assert_eq!(mm(Nm::um(600)), "0.6");
        assert!(parse_mm("1.2345678").is_err());
    }
}
