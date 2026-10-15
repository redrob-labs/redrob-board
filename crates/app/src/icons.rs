//! Redrob Board's icon set, drawn in code (original work, MIT OR Apache-2.0 with the codebase).
//!
//! Icons are line drawings on a 24x24 design grid: light strokes for what exists, blue for what a
//! command creates and orange dots for the thing it acts on. Same grammar as Redrob CAD's icons.

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};

use crate::theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Load,
    Place,
    Route,
    Check,
    Export,
    ZoomExtents,
    Eye,
    EyeOff,
    Grid,
}

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    w: f32,
    base: Color32,
    acc: Color32,
    pt: Color32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        let s = self.r.width() / 24.0;
        pos2(self.r.min.x + x * s, self.r.min.y + y * s)
    }
    fn sc(&self) -> f32 {
        self.r.width() / 24.0
    }
    fn st(&self, c: Color32) -> Stroke {
        Stroke::new(self.w, c)
    }
    fn l(&self, x1: f32, y1: f32, x2: f32, y2: f32, c: Color32) {
        self.p.line_segment([self.at(x1, y1), self.at(x2, y2)], self.st(c));
    }
    fn poly(&self, pts: &[(f32, f32)], closed: bool, c: Color32) {
        let mut v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        if closed && let Some(f) = v.first().copied() {
            v.push(f);
        }
        self.p.line(v, self.st(c));
    }
    fn c(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_stroke(self.at(x, y), r * self.sc(), self.st(c));
    }
    fn fill_c(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.at(x, y), r * self.sc(), c);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true, c);
    }
    fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.p.rect_filled(Rect::from_min_max(self.at(x, y), self.at(x + w, y + h)), 0.0, c);
    }
    fn dot(&self, x: f32, y: f32) {
        self.fill_c(x, y, 1.6, self.pt);
    }
    fn arrow_tip(&self, x: f32, y: f32, dx: f32, dy: f32, c: Color32) {
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ux, uy) = (dx / l, dy / l);
        let (nx, ny) = (-uy, ux);
        let pts = [(x, y), (x - ux * 3.0 + nx * 1.4, y - uy * 3.0 + ny * 1.4), (x - ux * 3.0 - nx * 1.4, y - uy * 3.0 - ny * 1.4)];
        self.p.add(egui::Shape::convex_polygon(pts.iter().map(|(x, y)| self.at(*x, *y)).collect(), c, Stroke::NONE));
    }
}

/// Paint `icon` into `rect` (square), with `dim` for disabled.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, dim: bool) {
    let t = Tokens::get();
    let alpha = |c: Color32| if dim { c.gamma_multiply(0.4) } else { c };
    let pen = Pen { p, r: rect, w: (rect.width() / 16.0).clamp(1.0, 1.6), base: alpha(t.icon), acc: alpha(t.icon_accent), pt: alpha(t.icon_point) };
    let (b, a) = (pen.base, pen.acc);
    use Icon::*;
    match icon {
        Load => {
            // A netlist sheet with three nets coming in.
            pen.poly(&[(7.0, 3.0), (16.0, 3.0), (20.0, 7.0), (20.0, 21.0), (7.0, 21.0)], true, b);
            pen.poly(&[(16.0, 3.0), (16.0, 7.0), (20.0, 7.0)], false, b);
            for y in [11.0, 14.5, 18.0] {
                pen.l(3.0, y, 12.0, y, a);
                pen.fill_c(12.0, y, 1.0, a);
            }
        }
        Place => {
            // Board outline with parts dropping into it.
            pen.rect(3.0, 3.0, 18.0, 18.0, b);
            pen.fill_rect(6.0, 6.0, 5.0, 3.5, a);
            pen.fill_rect(13.0, 6.0, 5.0, 5.0, a);
            pen.fill_rect(6.0, 12.5, 3.5, 5.5, a);
            pen.dot(15.5, 15.5);
        }
        Route => {
            // Two pads joined by a track with a via.
            pen.fill_rect(3.0, 15.0, 5.0, 5.0, b);
            pen.fill_rect(16.0, 4.0, 5.0, 5.0, b);
            pen.poly(&[(5.5, 15.0), (5.5, 10.0), (12.0, 10.0), (12.0, 6.5), (16.0, 6.5)], false, a);
            pen.c(12.0, 10.0, 1.8, a);
            pen.dot(12.0, 10.0);
        }
        Check => {
            // Rule check: a shield with a tick.
            pen.poly(&[(12.0, 3.0), (20.0, 6.0), (19.0, 15.0), (12.0, 21.0), (5.0, 15.0), (4.0, 6.0)], true, b);
            pen.poly(&[(8.0, 12.0), (11.0, 15.0), (16.5, 8.5)], false, a);
        }
        Export => {
            // Box with an arrow out: the fabrication files.
            pen.poly(&[(4.0, 10.0), (4.0, 21.0), (20.0, 21.0), (20.0, 10.0)], false, b);
            pen.l(12.0, 15.0, 12.0, 3.5, a);
            pen.arrow_tip(12.0, 3.0, 0.0, -1.0, a);
            pen.l(8.0, 7.0, 12.0, 3.0, a);
            pen.l(16.0, 7.0, 12.0, 3.0, a);
        }
        ZoomExtents => {
            for (x, y, dx, dy) in [(3.0, 3.0, 1.0, 1.0), (21.0, 3.0, -1.0, 1.0), (3.0, 21.0, 1.0, -1.0), (21.0, 21.0, -1.0, -1.0)] {
                pen.l(x, y, x + 5.0 * dx, y, b);
                pen.l(x, y, x, y + 5.0 * dy, b);
            }
            pen.rect(8.5, 8.5, 7.0, 7.0, a);
        }
        Eye | EyeOff => {
            pen.poly(&[(2.5, 12.0), (7.0, 7.0), (12.0, 5.5), (17.0, 7.0), (21.5, 12.0), (17.0, 17.0), (12.0, 18.5), (7.0, 17.0)], true, b);
            pen.c(12.0, 12.0, 3.2, b);
            if icon == EyeOff {
                pen.l(4.0, 20.0, 20.0, 4.0, pen.pt);
            } else {
                pen.fill_c(12.0, 12.0, 1.4, a);
            }
        }
        Grid => {
            for i in 0..4 {
                let v = 3.0 + i as f32 * 6.0;
                pen.l(v, 3.0, v, 21.0, b);
                pen.l(3.0, v, 21.0, v, b);
            }
            pen.dot(9.0, 9.0);
        }
    }
}
