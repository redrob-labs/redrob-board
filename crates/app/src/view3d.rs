//! A 3D preview drawn with the 2D painter: the board as a 1.6 mm slab, every part as a box over
//! its courtyard with a height from a small table, the outer copper on the two faces. Orthographic
//! projection with yaw and pitch; faces are sorted far to near (painter's algorithm), which is
//! exact for this scene of disjoint boxes on a slab.

use board_geom::Point;
use board_layout::{Board, Layer};
use board_units::Nm;
use egui::{Color32, Pos2, Stroke};

use crate::theme::Tokens;

/// Board thickness.
pub const SLAB_MM: f32 = 1.6;

/// Body height of a part above the board, by footprint id (mm). Unknown parts get 1.0 mm.
pub fn part_height_mm(footprint_id: &str) -> f32 {
    let id = footprint_id;
    if id.contains("ESP32") || id.contains("Castellated") {
        3.2
    } else if id.contains("SOT-23") {
        1.1
    } else if id.contains("0402") {
        0.5
    } else if id.contains("0603") {
        0.55
    } else if id.contains("PinHeader") {
        8.5
    } else if id.contains("SW_SMD_6x6") || id.contains("Button") {
        3.5
    } else {
        1.0 // AHT20 and anything unknown: a 1 mm package
    }
}

/// Camera: angles in radians, scale in points per mm, screen centre, world centre.
#[derive(Clone, Copy, Debug)]
pub struct View3d {
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for View3d {
    fn default() -> Self {
        View3d { yaw: -0.6, pitch: 0.95 }
    }
}

impl View3d {
    /// Project a world point (mm, z up) to screen, given scale, world centre and screen centre.
    /// Returns the screen position and the depth (larger = nearer the eye).
    pub fn project(&self, p: [f32; 3], centre: [f32; 3], scale: f32, screen_centre: Pos2) -> (Pos2, f32) {
        let (x, y, z) = (p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]);
        // Yaw about z, then pitch about the screen x axis.
        let (sy, cy) = self.yaw.sin_cos();
        let (x1, y1) = (x * cy - y * sy, x * sy + y * cy);
        let (sp, cp) = self.pitch.sin_cos();
        let sx = x1;
        let sy_ = y1 * cp + z * sp;
        let depth = -y1 * sp + z * cp;
        (Pos2::new(screen_centre.x + sx * scale, screen_centre.y - sy_ * scale), depth)
    }
}

struct Face {
    depth: f32,
    pts: Vec<Pos2>,
    fill: Color32,
    stroke: Stroke,
}

fn shade(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| ((v as f32) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(f(c.r()), f(c.g()), f(c.b()), c.a())
}

/// Draw the board. `rect` is the canvas; the board is scaled to fit it.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, board: &Board, view: &View3d) {
    let t = Tokens::get();
    let Some(bb) = board.outline.bbox() else { return };
    let mm = |v: Nm| v.to_mm_f64() as f32;
    let (w, h) = (mm(bb.width()), mm(bb.height()));
    let centre = [mm(bb.min.x) + w / 2.0, mm(bb.min.y) + h / 2.0, SLAB_MM / 2.0];
    let diag = (w * w + h * h).sqrt().max(1.0);
    let scale = (rect.width().min(rect.height()) * 0.72) / diag;
    let sc = rect.center();
    let proj = |p: [f32; 3]| view.project(p, centre, scale, sc);

    let mut faces: Vec<Face> = Vec::new();
    let mut quad = |pts: [[f32; 3]; 4], fill: Color32, stroke: Stroke| {
        let projected: Vec<(Pos2, f32)> = pts.iter().map(|p| proj(*p)).collect();
        let depth = projected.iter().map(|p| p.1).sum::<f32>() / 4.0;
        faces.push(Face { depth, pts: projected.into_iter().map(|p| p.0).collect(), fill, stroke });
    };
    let boxed =
        |quad: &mut dyn FnMut([[f32; 3]; 4], Color32, Stroke), x0: f32, y0: f32, x1: f32, y1: f32, z0: f32, z1: f32, c: Color32, edge: Stroke| {
            // Top, bottom, four sides, lit from the upper left.
            quad([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], c, edge);
            quad([[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]], shade(c, 0.55), edge);
            quad([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], shade(c, 0.8), edge);
            quad([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]], shade(c, 0.7), edge);
            quad([[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]], shade(c, 0.65), edge);
            quad([[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]], shade(c, 0.9), edge);
        };

    // The slab (green substrate), then copper on both faces, then parts.
    let substrate = Color32::from_rgb(0x2f, 0x6b, 0x3f);
    let edge = Stroke::new(1.0, t.border);
    boxed(&mut quad, mm(bb.min.x), mm(bb.min.y), mm(bb.max.x), mm(bb.max.y), 0.0, SLAB_MM, substrate, edge);
    for (layer, z, color) in [(Layer::FrontCopper, SLAB_MM + 0.02, t.copper_front), (Layer::BackCopper, -0.02, t.copper_back)] {
        for pad in board.all_pads() {
            if board.pad_copper_layers(&pad).contains(&layer) {
                let r = pad.bbox;
                quad(
                    [[mm(r.min.x), mm(r.min.y), z], [mm(r.max.x), mm(r.min.y), z], [mm(r.max.x), mm(r.max.y), z], [mm(r.min.x), mm(r.max.y), z]],
                    color,
                    Stroke::NONE,
                );
            }
        }
        for tr in board.tracks.iter().filter(|t| t.layer == layer) {
            // A track as a thin quad along its segment.
            let (a, b) = (tr.segment.a, tr.segment.b);
            let (dx, dy) = (mm(b.x) - mm(a.x), mm(b.y) - mm(a.y));
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let hw = mm(tr.width) / 2.0;
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            let (ax, ay, bx, by) = (mm(a.x), mm(a.y), mm(b.x), mm(b.y));
            quad([[ax + nx, ay + ny, z], [bx + nx, by + ny, z], [bx - nx, by - ny, z], [ax - nx, ay - ny, z]], color, Stroke::NONE);
        }
    }
    let body = Color32::from_rgb(0x1e, 0x20, 0x24);
    let body_edge = Stroke::new(1.0, Color32::from_rgb(0x55, 0x5a, 0x62));
    for p in &board.placements {
        let y = p.board_courtyard();
        let hgt = part_height_mm(&p.footprint.id);
        let (z0, z1) = if p.flipped { (-hgt, 0.0) } else { (SLAB_MM, SLAB_MM + hgt) };
        // Shrink the body a little inside the courtyard so neighbours read as separate parts.
        let inset = 0.15f32.min(mm(y.width()) / 4.0).min(mm(y.height()) / 4.0);
        boxed(&mut quad, mm(y.min.x) + inset, mm(y.min.y) + inset, mm(y.max.x) - inset, mm(y.max.y) - inset, z0, z1, body, body_edge);
    }

    faces.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    for f in faces {
        painter.add(egui::Shape::convex_polygon(f.pts, f.fill, f.stroke));
    }
    // A small axis gnomon in the corner.
    let o = Pos2::new(rect.left() + 34.0, rect.bottom() - 34.0);
    let g = |p: [f32; 3]| {
        let (q, _) = view.project(p, [0.0, 0.0, 0.0], 18.0, o);
        q
    };
    painter.line_segment([o, g([1.0, 0.0, 0.0])], Stroke::new(1.5, t.drc_error));
    painter.line_segment([o, g([0.0, 1.0, 0.0])], Stroke::new(1.5, Color32::from_rgb(0x4c, 0xc2, 0x5a)));
    painter.line_segment([o, g([0.0, 0.0, 1.0])], Stroke::new(1.5, t.icon_accent));
    let _ = Point::ORIGIN;
}
