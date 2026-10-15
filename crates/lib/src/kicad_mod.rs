//! Reader for KiCad footprint files (`.kicad_mod`), both the current `(footprint ...)` form and
//! the older `(module ...)` form.
//!
//! Only the format is KiCad's; this code is ours. Coordinates are flipped from KiCad's y-down to
//! our y-up on the way in, and millimetre decimals become exact nanometres.

use board_geom::{Point, Rect};
use board_layout::{Footprint, Layer, Pad, PadShape};
use board_units::Nm;
use thiserror::Error;

use crate::sexp::{Sexp, SexpError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KicadModError {
    #[error(transparent)]
    Syntax(#[from] SexpError),
    #[error("not a footprint file: top-level head is {0:?}")]
    NotAFootprint(Option<String>),
    #[error("footprint has no name")]
    NoName,
    #[error("pad {pad}: {what}")]
    Pad { pad: String, what: String },
    #[error("bad number {0:?}")]
    BadNumber(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// Parse one footprint file. `library` is the library nickname that prefixes the id.
pub fn parse_kicad_mod(library: &str, text: &str) -> Result<Footprint, KicadModError> {
    let root = Sexp::parse(text)?;
    match root.head() {
        Some("footprint") | Some("module") => {}
        other => return Err(KicadModError::NotAFootprint(other.map(str::to_string))),
    }
    let name = root.atom_arg(0).ok_or(KicadModError::NoName)?;

    let mut pads = Vec::new();
    for p in root.children("pad") {
        if let Some(pad) = parse_pad(p)? {
            pads.push(pad);
        }
    }

    let courtyard = courtyard_from_graphics(&root)?.unwrap_or_else(|| {
        let mut it = pads.iter().map(Pad::bbox);
        let first = it.next().unwrap_or(Rect::from_points(Point::ORIGIN, Point::ORIGIN));
        it.fold(first, |r, b| r.union(&b)).inflate(Nm::um(250))
    });

    Ok(Footprint { id: format!("{library}:{name}"), pads, courtyard })
}

fn parse_pad(p: &Sexp) -> Result<Option<Pad>, KicadModError> {
    let number = p.atom_arg(0).unwrap_or("").to_string();
    let err = |what: &str| KicadModError::Pad { pad: number.clone(), what: what.to_string() };
    let kind = p.atom_arg(1).ok_or_else(|| err("missing type"))?;
    let shape_name = p.atom_arg(2).ok_or_else(|| err("missing shape"))?;

    let at = p.child("at").ok_or_else(|| err("missing (at)"))?;
    let x = num(at.atom_arg(0))?;
    let y = num(at.atom_arg(1))?;
    let rotation: i64 = match at.atom_arg(2) {
        None => 0,
        Some(r) => {
            let deg = Nm::parse_mm(r).ok_or_else(|| KicadModError::BadNumber(r.to_string()))?;
            if deg.0 % (90 * Nm::NM_PER_MM) != 0 {
                return Err(KicadModError::Unsupported(format!("pad {number} rotation {r} is not a multiple of 90")));
            }
            (deg.0 / Nm::NM_PER_MM).rem_euclid(360)
        }
    };

    let size = p.child("size").ok_or_else(|| err("missing (size)"))?;
    let (mut w, mut h) = (num(size.atom_arg(0))?, num(size.atom_arg(1))?);
    if rotation == 90 || rotation == 270 {
        std::mem::swap(&mut w, &mut h);
    }

    let shape = match shape_name {
        "circle" => PadShape::Circle { diameter: w },
        "rect" => PadShape::Rect { width: w, height: h },
        "oval" => {
            if w == h {
                PadShape::Circle { diameter: w }
            } else {
                PadShape::Oval { width: w, height: h }
            }
        }
        "roundrect" => {
            let ratio = p.child("roundrect_rratio").and_then(|r| r.atom_arg(0)).unwrap_or("0.25");
            // ratio is a plain fraction; reuse the exact decimal parser and scale by 1e6.
            let ratio = Nm::parse_mm(ratio).ok_or_else(|| KicadModError::BadNumber(ratio.to_string()))?;
            let radius = Nm((w.min(h).0 as i128 * ratio.0 as i128 / Nm::NM_PER_MM as i128) as i64);
            PadShape::RoundRect { width: w, height: h, radius }
        }
        other => return Err(KicadModError::Unsupported(format!("pad {number} shape {other}"))),
    };

    let drill = match p.child("drill") {
        None => None,
        Some(d) => {
            if d.atom_arg(0) == Some("oval") {
                return Err(KicadModError::Unsupported(format!("pad {number} oval drill")));
            }
            Some(num(d.atom_arg(0))?)
        }
    };

    let mut layers = Vec::new();
    if let Some(l) = p.child("layers") {
        for a in l.args() {
            match a.as_atom() {
                Some("F.Cu") => layers.push(Layer::FrontCopper),
                Some("B.Cu") => layers.push(Layer::BackCopper),
                Some("*.Cu") => {
                    layers.push(Layer::FrontCopper);
                    layers.push(Layer::BackCopper);
                }
                _ => {} // mask, paste, silk: derived from the copper layers on our side
            }
        }
    }

    match kind {
        "smd" | "connect" | "thru_hole" => {}
        "np_thru_hole" => layers.clear(),
        other => return Err(KicadModError::Unsupported(format!("pad {number} type {other}"))),
    }
    if kind == "thru_hole" && drill.is_none() {
        return Err(err("thru_hole pad without drill"));
    }
    if (kind == "smd" || kind == "connect") && layers.len() != 1 {
        return Err(err("surface pad must be on exactly one copper layer"));
    }

    Ok(Some(Pad { number, at: Point::new(x, -y), shape, layers, drill, paste: board_layout::Paste::Full }))
}

fn courtyard_from_graphics(root: &Sexp) -> Result<Option<Rect>, KicadModError> {
    let mut bbox: Option<Rect> = None;
    let mut extend = |p: Point| {
        let r = Rect::from_points(p, p);
        bbox = Some(bbox.map_or(r, |b| b.union(&r)));
    };
    for item in root.args() {
        let Some(head) = item.head() else { continue };
        if !matches!(head, "fp_line" | "fp_rect" | "fp_poly" | "fp_circle" | "fp_arc") {
            continue;
        }
        let on_courtyard = item.child("layer").and_then(|l| l.atom_arg(0)).is_some_and(|l| l == "F.CrtYd" || l == "B.CrtYd");
        if !on_courtyard {
            continue;
        }
        for key in ["start", "end", "mid", "center"] {
            if let Some(pt) = item.child(key) {
                extend(point(pt)?);
            }
        }
        if head == "fp_circle"
            && let (Some(c), Some(e)) = (item.child("center"), item.child("end"))
        {
            let c = point(c)?;
            let e = point(e)?;
            // Radius from centre to the end point, exact only for axis-aligned ends; good enough
            // for a bounding box, and KiCad writes courtyard circles axis-aligned.
            let r = (c.x - e.x).abs().max((c.y - e.y).abs());
            extend(c.offset(-r, -r));
            extend(c.offset(r, r));
        }
        if let Some(pts) = item.child("pts") {
            for xy in pts.children("xy") {
                extend(point(xy)?);
            }
        }
    }
    Ok(bbox)
}

fn point(s: &Sexp) -> Result<Point, KicadModError> {
    Ok(Point::new(num(s.atom_arg(0))?, -num(s.atom_arg(1))?))
}

fn num(s: Option<&str>) -> Result<Nm, KicadModError> {
    let s = s.ok_or_else(|| KicadModError::BadNumber(String::new()))?;
    Nm::parse_mm(s).ok_or_else(|| KicadModError::BadNumber(s.to_string()))
}
