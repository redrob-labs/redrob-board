//! Track routing for Redrob Board.
//!
//! A maze router on an integer grid, two copper layers with vias. For each net, pads are joined
//! one connection at a time: a shortest-path search (Dijkstra, 8 neighbours plus layer change)
//! from everything already connected to any pad not yet connected, then the path is committed as
//! tracks and becomes an obstacle for other nets. Nets are routed smallest first; nets that fail
//! get a second pass with everything else already in place, and a third pass at the front of the
//! order with the board cleared.
//!
//! Obstacles are stored as "which net may put a track centre here" per cell and layer, built by
//! inflating every copper item by the clearance plus half a track width. The final word on the
//! result is `board_drc::check`, which the tests run on every routed board.

#![forbid(unsafe_code)]

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use board_geom::{Point, Rect, Segment};
use board_layout::{Board, BoardPad, Layer, PadShape, Track, Via};
use board_netlist::{Netlist, PinRef};
use board_units::Nm;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRules {
    pub track_width: Nm,
    pub clearance: Nm,
    pub edge_clearance: Nm,
    pub via_diameter: Nm,
    pub via_drill: Nm,
    /// Grid pitch. Smaller finds more routes and costs more time.
    pub grid: Nm,
    /// Extra cost of a layer change, in grid steps (a straight step costs 10).
    pub via_cost: u32,
}

impl Default for RouteRules {
    fn default() -> RouteRules {
        RouteRules {
            track_width: Nm::um(250),
            clearance: Nm::um(150),
            edge_clearance: Nm::um(300),
            via_diameter: Nm::um(600),
            via_drill: Nm::um(300),
            grid: Nm::um(100),
            via_cost: 120,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteReport {
    /// Nets that could not be completed, with the number of connections still missing.
    pub failed: BTreeMap<String, usize>,
    pub tracks: usize,
    pub vias: usize,
}

impl RouteReport {
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty()
    }
}

const FREE: u16 = u16::MAX;
const BLOCKED: u16 = u16::MAX - 1;
const LAYERS: [Layer; 2] = [Layer::FrontCopper, Layer::BackCopper];

struct Grid {
    origin: Point,
    pitch: Nm,
    nx: usize,
    ny: usize,
    /// `owner[layer][cell]`: FREE, BLOCKED, or the net index allowed to run a track centre here.
    owner: [Vec<u16>; 2],
}

impl Grid {
    fn index(&self, x: usize, y: usize) -> usize {
        y * self.nx + x
    }

    fn center(&self, x: usize, y: usize) -> Point {
        Point::new(self.origin.x + self.pitch * x as i64, self.origin.y + self.pitch * y as i64)
    }

    fn cell_range(&self, r: Rect) -> (usize, usize, usize, usize) {
        let lo = |v: Nm, o: Nm| ((v - o).0.max(0) as f64 / self.pitch.0 as f64).ceil() as usize;
        let hi = |v: Nm, o: Nm, n: usize| (((v - o).0 as f64 / self.pitch.0 as f64).floor().max(-1.0) as isize).min(n as isize - 1);
        let x0 = lo(r.min.x, self.origin.x);
        let y0 = lo(r.min.y, self.origin.y);
        let x1 = hi(r.max.x, self.origin.x, self.nx);
        let y1 = hi(r.max.y, self.origin.y, self.ny);
        if x1 < 0 || y1 < 0 || x0 > x1 as usize || y0 > y1 as usize {
            return (1, 0, 1, 0);
        }
        (x0, x1 as usize, y0, y1 as usize)
    }

    /// Claim every cell whose centre is within `reach` of the shape for `net` (or block it).
    fn claim(&mut self, layer: usize, shape: Shape, reach: Nm, net: u16) {
        let bbox = shape.bbox().inflate(reach);
        let (x0, x1, y0, y1) = self.cell_range(bbox);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = self.center(x, y);
                if shape.within(c, reach) {
                    let i = self.index(x, y);
                    let cur = self.owner[layer][i];
                    self.owner[layer][i] = if cur == FREE || cur == net { net } else { BLOCKED };
                }
            }
        }
    }

    fn passable(&self, layer: usize, i: usize, net: u16) -> bool {
        let o = self.owner[layer][i];
        o == FREE || o == net
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Rect(Rect),
    Circle { center: Point, radius: Nm },
    Stroke { segment: Segment, half_width: Nm },
}

impl Shape {
    fn bbox(self) -> Rect {
        match self {
            Shape::Rect(r) => r,
            Shape::Circle { center, radius } => Rect::centered(center, radius * 2, radius * 2),
            Shape::Stroke { segment, half_width } => segment.bbox().inflate(half_width),
        }
    }

    /// Is `p` within `reach` of the shape's edge (or inside it)?
    fn within(self, p: Point, reach: Nm) -> bool {
        match self {
            Shape::Rect(r) => {
                let dx = (r.min.x - p.x).max(p.x - r.max.x).max(Nm::ZERO);
                let dy = (r.min.y - p.y).max(p.y - r.max.y).max(Nm::ZERO);
                dx.squared() + dy.squared() <= reach.squared()
            }
            Shape::Circle { center, radius } => center.squared_distance(p) <= (radius + reach).squared(),
            Shape::Stroke { segment, half_width } => segment.squared_distance_to_point(p) <= (half_width + reach).squared(),
        }
    }
}

fn pad_shape(p: &BoardPad) -> Shape {
    match p.shape {
        PadShape::Circle { diameter } => Shape::Circle { center: p.center, radius: diameter / 2 },
        _ => Shape::Rect(p.bbox),
    }
}

fn layer_index(l: Layer) -> Option<usize> {
    LAYERS.iter().position(|x| *x == l)
}

struct NetWork {
    index: u16,
    name: String,
    /// Pads of this net: (layer mask bits, cells inside the pad).
    pads: Vec<Vec<(usize, usize)>>, // per pad: list of (layer, cell)
}

/// Route every net of `netlist` on `board` (two copper layers). Existing tracks and vias are kept
/// and treated as already-routed copper. Nets in one of the board's net classes take that class's
/// track width, clearance and via; the rest take `rules`. Returns what could not be finished.
pub fn route(board: &mut Board, netlist: &Netlist, rules: &RouteRules) -> RouteReport {
    if board.outline.bbox().is_none() {
        return RouteReport { failed: BTreeMap::new(), tracks: 0, vias: 0 };
    }
    // Class 0 is the default; class k >= 1 is board.netclasses[k - 1].
    let classes: Vec<RouteRules> = std::iter::once(*rules)
        .chain(board.netclasses.iter().map(|c| RouteRules {
            track_width: c.track_width,
            clearance: c.clearance,
            via_diameter: c.via_diameter,
            via_drill: c.via_drill,
            ..*rules
        }))
        .collect();
    let class_of: Vec<usize> =
        netlist.nets.iter().map(|n| board.netclasses.iter().position(|c| c.nets.contains(&n.name)).map_or(0, |i| i + 1)).collect();
    let mut grids: Vec<Grid> = (0..classes.len()).map(|k| build_grid(board, netlist, &classes, &class_of, k)).collect();
    let nets = pad_cells(&grids[0], board, netlist);
    let nx = grids[0].nx;

    // Smallest nets first.
    let mut order: Vec<usize> = (0..nets.len()).filter(|&i| nets[i].pads.len() >= 2).collect();
    order.sort_by_key(|&i| {
        let mut b: Option<Rect> = None;
        for p in &nets[i].pads {
            if let Some(&(_, c)) = p.first() {
                let pt = grids[0].center(c % nx, c / nx);
                let r = Rect::from_points(pt, pt);
                b = Some(b.map_or(r, |x| x.union(&r)));
            }
        }
        b.map_or(0, |b| (b.width() + b.height()).0)
    });

    let mut failed: BTreeMap<String, usize> = BTreeMap::new();
    let mut new_tracks = Vec::new();
    let mut new_vias = Vec::new();
    let before_tracks = board.tracks.len();
    let before_vias = board.vias.len();

    for pass in 0..3 {
        let todo: Vec<usize> = if pass == 0 {
            order.clone()
        } else {
            let mut t: Vec<usize> = order.iter().copied().filter(|i| failed.contains_key(&nets[*i].name)).collect();
            if pass == 2 {
                // Clear everything routed so far and lead with the hard ones.
                grids = (0..classes.len()).map(|k| build_grid(board, netlist, &classes, &class_of, k)).collect();
                new_tracks.clear();
                new_vias.clear();
                t.extend(order.iter().copied().filter(|i| !failed.contains_key(&nets[*i].name)));
            }
            t
        };
        if todo.is_empty() {
            break;
        }
        failed.clear();
        for ni in todo {
            let k = class_of[ni];
            let r = &classes[k];
            let via_extra_cells = (((r.via_diameter / 2 - r.track_width / 2).0.max(0) as f64) / r.grid.0 as f64).ceil() as isize;
            let missing = route_net(&mut grids, &classes, k, &nets[ni], via_extra_cells, &mut new_tracks, &mut new_vias);
            if missing > 0 {
                failed.insert(nets[ni].name.clone(), missing);
            }
        }
    }

    board.tracks.extend(new_tracks);
    board.vias.extend(new_vias);
    RouteReport { failed, tracks: board.tracks.len() - before_tracks, vias: board.vias.len() - before_vias }
}

/// How far a track centre of class `k` must stay from copper of class `m`: half of k's width plus
/// the larger of the two clearances (the rule a checker applies to that pair).
fn reach(classes: &[RouteRules], k: usize, m: usize) -> Nm {
    classes[k].track_width / 2 + classes[k].clearance.max(classes[m].clearance)
}

/// Obstacle map for tracks of class `k`, from everything already on the board: outline, pads,
/// tracks, vias.
fn build_grid(board: &Board, netlist: &Netlist, classes: &[RouteRules], class_of: &[usize], k: usize) -> Grid {
    let rules = &classes[k];
    let bbox = board.outline.bbox().expect("outline checked by caller");
    let pitch = rules.grid;
    let nx = (bbox.width().0 / pitch.0) as usize + 1;
    let ny = (bbox.height().0 / pitch.0) as usize + 1;
    let mut grid = Grid { origin: bbox.min, pitch, nx, ny, owner: [vec![FREE; nx * ny], vec![FREE; nx * ny]] };
    let half = rules.track_width / 2;

    // A track centre must keep edge_clearance + half width from every outline edge, and cells
    // outside the outline are off limits.
    let edge_reach = rules.edge_clearance + half;
    for y in 0..ny {
        for x in 0..nx {
            let c = grid.center(x, y);
            let ok = board.outline.contains(c) && board.outline.edges().all(|e| e.squared_distance_to_point(c) >= edge_reach.squared());
            if !ok {
                let i = grid.index(x, y);
                grid.owner[0][i] = BLOCKED;
                grid.owner[1][i] = BLOCKED;
            }
        }
    }
    let net_index = |name: &str| netlist.nets.iter().position(|n| n.name == name).map(|i| i as u16);
    let class_reach = |owner: u16| if owner == BLOCKED { reach(classes, k, k) } else { reach(classes, k, class_of[owner as usize]) };
    for pad in board.all_pads() {
        let owner = if pad.layers.is_empty() {
            BLOCKED
        } else {
            netlist.net_of(&PinRef::new(pad.refdes.clone(), pad.number.clone())).and_then(|n| net_index(&n.name)).unwrap_or(BLOCKED)
        };
        let layers: Vec<usize> = if pad.layers.is_empty() { vec![0, 1] } else { pad.layers.iter().filter_map(|l| layer_index(*l)).collect() };
        for l in layers {
            grid.claim(l, pad_shape(&pad), class_reach(owner), owner);
        }
    }
    for t in &board.tracks {
        if let (Some(l), Some(n)) = (layer_index(t.layer), net_index(&t.net)) {
            grid.claim(l, Shape::Stroke { segment: t.segment, half_width: t.width / 2 }, class_reach(n), n);
        }
    }
    for v in &board.vias {
        if let Some(n) = net_index(&v.net) {
            for l in 0..2 {
                grid.claim(l, Shape::Circle { center: v.at, radius: v.diameter / 2 }, class_reach(n), n);
            }
        }
    }
    grid
}

/// For every net, the grid cells inside each of its pads (where a track may start or end).
fn pad_cells(grid: &Grid, board: &Board, netlist: &Netlist) -> Vec<NetWork> {
    let mut nets: Vec<NetWork> =
        netlist.nets.iter().enumerate().map(|(i, n)| NetWork { index: i as u16, name: n.name.clone(), pads: Vec::new() }).collect();
    for pad in board.all_pads() {
        if pad.layers.is_empty() {
            continue;
        }
        let Some(net) = netlist.net_of(&PinRef::new(pad.refdes.clone(), pad.number.clone())) else { continue };
        let Some(ni) = netlist.nets.iter().position(|n| n.name == net.name) else { continue };
        let shape = pad_shape(&pad);
        let layers: Vec<usize> = pad.layers.iter().filter_map(|l| layer_index(*l)).collect();
        let mut cells = Vec::new();
        for &l in &layers {
            let (x0, x1, y0, y1) = grid.cell_range(shape.bbox());
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if shape.within(grid.center(x, y), Nm::ZERO) {
                        cells.push((l, grid.index(x, y)));
                    }
                }
            }
        }
        if cells.is_empty() {
            // Pad smaller than the grid: the cell nearest its centre on each of its layers.
            let x = (((pad.center.x - grid.origin.x).0 as f64 / grid.pitch.0 as f64).round().max(0.0) as usize).min(grid.nx - 1);
            let y = (((pad.center.y - grid.origin.y).0 as f64 / grid.pitch.0 as f64).round().max(0.0) as usize).min(grid.ny - 1);
            for &l in &layers {
                cells.push((l, grid.index(x, y)));
            }
        }
        nets[ni].pads.push(cells);
    }
    nets
}

/// Join all pads of one net (class `k`). Returns the number of connections that could not be made.
fn route_net(
    grids: &mut [Grid],
    classes: &[RouteRules],
    k: usize,
    net: &NetWork,
    via_extra: isize,
    tracks: &mut Vec<Track>,
    vias: &mut Vec<Via>,
) -> usize {
    let n = grids[k].nx * grids[k].ny;
    // Which pad (index into net.pads) each cell belongs to, per layer; usize::MAX = none.
    let mut pad_of = [vec![usize::MAX; n], vec![usize::MAX; n]];
    for (pi, cells) in net.pads.iter().enumerate() {
        for &(l, c) in cells {
            pad_of[l][c] = pi;
        }
    }
    let mut connected = vec![false; net.pads.len()];
    connected[0] = true;
    // Cells that are part of the connected component (sources).
    let mut sources: Vec<(usize, usize)> = net.pads[0].clone();
    let mut missing = 0;
    for _ in 1..net.pads.len() {
        match search(&grids[k], net.index, &sources, &pad_of, &connected, &classes[k], via_extra) {
            Some((path, reached)) => {
                connected[reached] = true;
                sources.extend(net.pads[reached].iter().copied());
                commit(grids, classes, k, net, &path, tracks, vias);
                sources.extend(path.iter().copied());
            }
            None => {
                missing += 1;
                // Give up on this pad: mark the nearest unconnected pad connected so the loop ends.
                if let Some(i) = connected.iter().position(|c| !c) {
                    connected[i] = true;
                }
            }
        }
    }
    missing
}

#[allow(clippy::too_many_arguments)]
fn search(
    grid: &Grid,
    net: u16,
    sources: &[(usize, usize)],
    pad_of: &[Vec<usize>; 2],
    connected: &[bool],
    rules: &RouteRules,
    via_extra: isize,
) -> Option<(Vec<(usize, usize)>, usize)> {
    let n = grid.nx * grid.ny;
    let mut dist = [vec![u32::MAX; n], vec![u32::MAX; n]];
    let mut prev = [vec![u32::MAX; n], vec![u32::MAX; n]]; // encoded layer*n + cell
    let mut heap = BinaryHeap::new();
    for &(l, c) in sources {
        if dist[l][c] != 0 {
            dist[l][c] = 0;
            heap.push(Reverse((0u32, l as u8, c as u32)));
        }
    }
    let nx = grid.nx as isize;
    let ny = grid.ny as isize;
    let moves: [(isize, isize, u32); 8] = [(1, 0, 10), (-1, 0, 10), (0, 1, 10), (0, -1, 10), (1, 1, 14), (1, -1, 14), (-1, 1, 14), (-1, -1, 14)];
    while let Some(Reverse((d, l, c))) = heap.pop() {
        let (l, c) = (l as usize, c as usize);
        if d > dist[l][c] {
            continue;
        }
        let target = pad_of[l][c];
        if target != usize::MAX && !connected[target] {
            // Walk back.
            let mut path = vec![(l, c)];
            let mut cur = l * n + c;
            while prev[cur / n][cur % n] != u32::MAX {
                cur = prev[cur / n][cur % n] as usize;
                path.push((cur / n, cur % n));
            }
            path.reverse();
            return Some((path, target));
        }
        let (x, y) = ((c % grid.nx) as isize, (c / grid.nx) as isize);
        for (dx, dy, cost) in moves {
            let (mx, my) = (x + dx, y + dy);
            if mx < 0 || my < 0 || mx >= nx || my >= ny {
                continue;
            }
            let mc = (my * nx + mx) as usize;
            if !grid.passable(l, mc, net) {
                continue;
            }
            // Diagonal steps must not cut a blocked corner.
            if dx != 0 && dy != 0 {
                let a = (y * nx + mx) as usize;
                let b = (my * nx + x) as usize;
                if !grid.passable(l, a, net) || !grid.passable(l, b, net) {
                    continue;
                }
            }
            let nd = d + cost;
            if nd < dist[l][mc] {
                dist[l][mc] = nd;
                prev[l][mc] = (l * n + c) as u32;
                heap.push(Reverse((nd, l as u8, mc as u32)));
            }
        }
        // Layer change: needs room for the via on both layers.
        let ol = 1 - l;
        if via_ok(grid, net, x, y, via_extra) {
            let nd = d + rules.via_cost;
            if nd < dist[ol][c] {
                dist[ol][c] = nd;
                prev[ol][c] = (l * n + c) as u32;
                heap.push(Reverse((nd, ol as u8, c as u32)));
            }
        }
    }
    None
}

fn via_ok(grid: &Grid, net: u16, x: isize, y: isize, extra: isize) -> bool {
    for dy in -extra..=extra {
        for dx in -extra..=extra {
            let (mx, my) = (x + dx, y + dy);
            if mx < 0 || my < 0 || mx >= grid.nx as isize || my >= grid.ny as isize {
                return false;
            }
            let c = (my * grid.nx as isize + mx) as usize;
            if !grid.passable(0, c, net) || !grid.passable(1, c, net) {
                return false;
            }
        }
    }
    true
}

fn commit(
    grids: &mut [Grid],
    classes: &[RouteRules],
    k: usize,
    net: &NetWork,
    path: &[(usize, usize)],
    tracks: &mut Vec<Track>,
    vias: &mut Vec<Via>,
) {
    let rules = &classes[k];
    let half = rules.track_width / 2;
    let (nx, origin, pitch) = (grids[k].nx, grids[k].origin, grids[k].pitch);
    let pt = move |c: usize| Point::new(origin.x + pitch * (c % nx) as i64, origin.y + pitch * (c / nx) as i64);
    // New copper of class k is an obstacle for every class j at j's own reach against k.
    let claim_all = |grids: &mut [Grid], layers: &[usize], shape: Shape| {
        for (j, g) in grids.iter_mut().enumerate() {
            let r = reach(classes, j, k);
            for &l in layers {
                g.claim(l, shape, r, net.index);
            }
        }
    };
    let mut i = 0;
    while i + 1 < path.len() {
        let (l, c) = path[i];
        let (l2, c2) = path[i + 1];
        if l != l2 {
            let at = pt(c);
            vias.push(Via { net: net.name.clone(), at, diameter: rules.via_diameter, drill: rules.via_drill });
            claim_all(grids, &[0, 1], Shape::Circle { center: at, radius: rules.via_diameter / 2 });
            i += 1;
            continue;
        }
        // Extend a straight run as far as the direction holds on this layer.
        let (x0, y0) = ((c % nx) as isize, (c / nx) as isize);
        let (x1, y1) = ((c2 % nx) as isize, (c2 / nx) as isize);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let mut j = i + 1;
        while j + 1 < path.len() {
            let (lj, cj) = path[j];
            let (lk, ck) = path[j + 1];
            if lj != lk {
                break;
            }
            let (xj, yj) = ((cj % nx) as isize, (cj / nx) as isize);
            let (xk, yk) = ((ck % nx) as isize, (ck / nx) as isize);
            if (xk - xj, yk - yj) != (dx, dy) {
                break;
            }
            j += 1;
        }
        let seg = Segment::new(pt(c), pt(path[j].1));
        tracks.push(Track { net: net.name.clone(), layer: LAYERS[l], segment: seg, width: rules.track_width });
        claim_all(grids, &[l], Shape::Stroke { segment: seg, half_width: half });
        i = j;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_designs::fixtures::{divider_netlist, divider_placed, sensor_node_hand, sensor_node_power_class};
    use board_designs::{library, sensor_node};
    use board_drc::{Rules, check};
    use board_place::{Constraints, Edge, place};

    fn drc_rules(r: &RouteRules) -> Rules {
        Rules { clearance: r.clearance, min_track_width: r.track_width, edge_clearance: r.edge_clearance, ..Rules::default() }
    }

    #[test]
    fn divider_routes_clean() {
        let netlist = divider_netlist();
        let mut board = divider_placed();
        let rules = RouteRules::default();
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        let v = check(&board, &netlist, &drc_rules(&rules));
        assert!(v.is_empty(), "{v:#?}");
        assert!(report.tracks >= 3);
    }

    #[test]
    fn hand_placed_sensor_node_routes_clean() {
        let netlist = sensor_node();
        let mut board = sensor_node_hand();
        let rules = RouteRules::default();
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        let v = check(&board, &netlist, &drc_rules(&rules));
        assert!(v.is_empty(), "{} violations, first: {:?}", v.len(), v.first());
    }

    /// A Power class: its nets come out wide with wide vias, the rest at the default, and the
    /// checker (which applies the larger clearance between classes) finds nothing.
    #[test]
    fn net_classes_set_width_clearance_and_via_per_net() {
        let netlist = sensor_node();
        let mut board = sensor_node_power_class();
        let rules = RouteRules::default();
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        let power: Vec<_> = board.tracks.iter().filter(|t| t.net == "5V" || t.net == "3V3").collect();
        assert!(!power.is_empty());
        assert!(power.iter().all(|t| t.width == Nm::um(500)), "power tracks at class width");
        assert!(board.tracks.iter().filter(|t| t.net == "SDA").all(|t| t.width == Nm::um(250)), "signal tracks at default width");
        for v in board.vias.iter().filter(|v| v.net == "3V3") {
            assert_eq!((v.diameter, v.drill), (Nm::um(800), Nm::um(400)));
        }
        let v = check(&board, &netlist, &drc_rules(&rules));
        assert!(v.is_empty(), "{} violations, first: {:?}", v.len(), v.first());
        // Any 3V3 copper sits at least 0.2 mm from other nets' copper (the class clearance).
        let strict = Rules { clearance: Nm::um(200), ..drc_rules(&rules) };
        let strict_v = check(&board, &netlist, &strict);
        assert!(strict_v.iter().all(|v| !v.message.contains("3V3") && !v.message.contains("5V")), "{strict_v:?}");
    }

    #[test]
    fn auto_placed_sensor_node_routes_clean() {
        let netlist = sensor_node();
        let c = Constraints { edge_parts: vec![("J1".into(), Edge::Left), ("SW1".into(), Edge::Right)], ..Constraints::default() };
        let mut board = place(&netlist, &library(), Rect::from_points(Point::ORIGIN, Point::mm(30, 25)), &c).unwrap();
        let rules = RouteRules::default();
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        let v = check(&board, &netlist, &drc_rules(&rules));
        assert!(v.is_empty(), "{} violations, first: {:?}", v.len(), v.first());
    }
}
