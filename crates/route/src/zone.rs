//! Copper pours. A zone is filled after routing: every cell of a fine raster inside the zone's
//! outline (and inside the board, off its edge) that keeps the zone's clearance from other nets'
//! copper becomes copper of the zone's net. Islands touching none of the net's pads or vias are
//! dropped, runs thinner than `min_width` are removed, and the result is stored on the zone as
//! rows of rectangles, which is what the Gerber writer flashes and the oracle export polygons.

use board_geom::{Point, Rect};
use board_layout::{Board, Layer, PadShape};
use board_netlist::{Netlist, PinRef};
use board_units::Nm;

/// Raster pitch for fills: fine enough that a 0.15 mm clearance is honoured to 0.05 mm.
pub const FILL_PITCH: Nm = Nm(50_000);

#[derive(Clone, Copy)]
enum Item {
    Rect(Rect),
    Circle(Point, Nm),
    Stroke(board_geom::Segment, Nm),
}

impl Item {
    fn bbox(self) -> Rect {
        match self {
            Item::Rect(r) => r,
            Item::Circle(c, r) => Rect::centered(c, r * 2, r * 2),
            Item::Stroke(s, hw) => s.bbox().inflate(hw),
        }
    }
    fn within(self, p: Point, reach: Nm) -> bool {
        match self {
            Item::Rect(r) => {
                let dx = (r.min.x - p.x).max(p.x - r.max.x).max(Nm::ZERO);
                let dy = (r.min.y - p.y).max(p.y - r.max.y).max(Nm::ZERO);
                dx.squared() + dy.squared() <= reach.squared()
            }
            Item::Circle(c, r) => c.squared_distance(p) <= (r + reach).squared(),
            Item::Stroke(s, hw) => s.squared_distance_to_point(p) <= (hw + reach).squared(),
        }
    }
}

/// Fill every zone of the board in place. `edge_clearance` keeps the pour off the board edge.
pub fn fill_zones(board: &mut Board, netlist: &Netlist, edge_clearance: Nm) {
    let zones = board.zones.clone();
    let mut fills = Vec::with_capacity(zones.len());
    for z in &zones {
        fills.push(fill_one(board, netlist, z, edge_clearance));
    }
    for (z, fill) in board.zones.iter_mut().zip(fills) {
        z.fill = fill;
    }
}

fn fill_one(board: &Board, netlist: &Netlist, zone: &board_layout::Zone, edge_clearance: Nm) -> Vec<Rect> {
    let Some(board_bbox) = board.outline.bbox() else { return Vec::new() };
    let Some(zb) = zone.outline.bbox() else { return Vec::new() };
    let area = Rect::from_points(
        Point::new(zb.min.x.max(board_bbox.min.x), zb.min.y.max(board_bbox.min.y)),
        Point::new(zb.max.x.min(board_bbox.max.x), zb.max.y.min(board_bbox.max.y)),
    );
    if area.width() <= Nm::ZERO || area.height() <= Nm::ZERO {
        return Vec::new();
    }
    let p = FILL_PITCH;
    let nx = (area.width().0 / p.0) as usize + 1;
    let ny = (area.height().0 / p.0) as usize + 1;
    let center = |x: usize, y: usize| Point::new(area.min.x + p * x as i64, area.min.y + p * y as i64);

    // Copper on this layer (and through it), split by net.
    let mut others: Vec<Item> = Vec::new();
    let mut ours: Vec<Item> = Vec::new();
    for pad in board.all_pads() {
        if pad.layers.is_empty() {
            others.push(pad_item(&pad)); // a non-plated hole: keep clear
            continue;
        }
        if !board.pad_copper_layers(&pad).contains(&zone.layer) {
            continue;
        }
        let net = netlist.net_of(&PinRef::new(pad.refdes.clone(), pad.number.clone())).map(|n| n.name.as_str());
        if net == Some(zone.net.as_str()) { ours.push(pad_item(&pad)) } else { others.push(pad_item(&pad)) }
    }
    for t in board.tracks.iter().filter(|t| t.layer == zone.layer) {
        let it = Item::Stroke(t.segment, t.width / 2);
        if t.net == zone.net { ours.push(it) } else { others.push(it) }
    }
    for v in &board.vias {
        let it = Item::Circle(v.at, v.diameter / 2);
        if v.net == zone.net { ours.push(it) } else { others.push(it) }
    }

    // 1. Raw mask: inside the zone and the board, clear of other nets.
    let mut mask = vec![false; nx * ny];
    // Cells are squares of side p around their centre: keep the centre a half-diagonal further.
    let margin = Nm((p.0 as f64 * 0.7072).ceil() as i64);
    let edge_reach = edge_clearance + margin;
    for y in 0..ny {
        for x in 0..nx {
            let c = center(x, y);
            if !zone.outline.contains(c) || !board.outline.contains(c) {
                continue;
            }
            if board.outline.edges().any(|e| e.squared_distance_to_point(c) < edge_reach.squared()) {
                continue;
            }
            if others.iter().any(|o| o.bbox().inflate(zone.clearance + p * 2).contains(c) && o.within(c, zone.clearance + margin)) {
                continue;
            }
            mask[y * nx + x] = true;
        }
    }
    // 2. Slivers: drop row and column runs shorter than min_width.
    let min_cells = ((zone.min_width.0 + p.0 - 1) / p.0) as usize;
    thin_runs(&mut mask, nx, ny, min_cells);
    // 3. Islands: keep only components that touch the net's own copper.
    let mut keep = vec![false; nx * ny];
    let mut seen = vec![false; nx * ny];
    for start in 0..nx * ny {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        let mut comp = Vec::new();
        let mut anchored = false;
        seen[start] = true;
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % nx, i / nx);
            if !anchored {
                let c = center(x, y);
                anchored = ours.iter().any(|o| o.within(c, Nm::ZERO));
            }
            let mut push = |j: usize| {
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < nx {
                push(i + 1);
            }
            if y > 0 {
                push(i - nx);
            }
            if y + 1 < ny {
                push(i + nx);
            }
        }
        if anchored {
            for i in comp {
                keep[i] = true;
            }
        }
    }
    // 4. Rows of rectangles, one per run.
    let mut out = Vec::new();
    for y in 0..ny {
        let mut x = 0;
        while x < nx {
            if !keep[y * nx + x] {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < nx && keep[y * nx + x] {
                x += 1;
            }
            let a = center(x0, y);
            let b = center(x - 1, y);
            out.push(Rect::from_points(Point::new(a.x - p / 2, a.y - p / 2), Point::new(b.x + p / 2, b.y + p / 2)));
        }
    }
    out
}

fn pad_item(p: &board_layout::BoardPad) -> Item {
    match p.shape {
        PadShape::Circle { diameter } => Item::Circle(p.center, diameter / 2),
        _ => Item::Rect(p.bbox),
    }
}

/// Clear every horizontal and vertical run of set cells shorter than `min` cells.
fn thin_runs(mask: &mut [bool], nx: usize, ny: usize, min: usize) {
    if min <= 1 {
        return;
    }
    let snapshot = mask.to_vec();
    for y in 0..ny {
        let mut x = 0;
        while x < nx {
            if !snapshot[y * nx + x] {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < nx && snapshot[y * nx + x] {
                x += 1;
            }
            if x - x0 < min {
                for xx in x0..x {
                    mask[y * nx + xx] = false;
                }
            }
        }
    }
    let snapshot = mask.to_vec();
    for x in 0..nx {
        let mut y = 0;
        while y < ny {
            if !snapshot[y * nx + x] {
                y += 1;
                continue;
            }
            let y0 = y;
            while y < ny && snapshot[y * nx + x] {
                y += 1;
            }
            if y - y0 < min {
                for yy in y0..y {
                    mask[yy * nx + x] = false;
                }
            }
        }
    }
}

/// Copper area of a zone's fill, in square millimetres.
pub fn fill_area_mm2(zone: &board_layout::Zone) -> f64 {
    zone.fill.iter().map(|r| r.width().to_mm_f64() * r.height().to_mm_f64()).sum()
}

/// Whether `layer` is a plane: a zone of `net` covers it, so other nets may only cross it with vias.
pub fn plane_net(board: &Board, layer: Layer) -> Option<&str> {
    board.zones.iter().find(|z| z.layer == layer).map(|z| z.net.as_str())
}
