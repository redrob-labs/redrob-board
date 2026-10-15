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

pub mod zone;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use board_geom::{Point, Rect, Segment};
use board_layout::{Board, BoardPad, Layer, LengthMatch, PadShape, Track, Via};
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
    pub pairs: Vec<PairReport>,
    pub lengths: Vec<LengthReport>,
}

/// How a differential pair came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairReport {
    pub p: String,
    pub n: String,
    pub len_p: Nm,
    pub len_n: Nm,
    /// Share of N's path cells that sit in the corridor beside P, per mille.
    pub coupled_permille: u32,
}

/// How a length-matched group came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LengthReport {
    pub nets: Vec<(String, Nm)>,
    pub tolerance: Nm,
    pub matched: bool,
}

impl RouteReport {
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty()
    }
}

const FREE: u16 = u16::MAX;
const BLOCKED: u16 = u16::MAX - 1;

struct Grid {
    origin: Point,
    pitch: Nm,
    nx: usize,
    ny: usize,
    /// `owner[layer][cell]`: FREE, BLOCKED, or the net index allowed to run a track centre here.
    /// The board's copper layers, outer to inner to outer, as `owner` indexes them.
    layers: Vec<Layer>,
    owner: Vec<Vec<u16>>,
    /// Per layer: the net whose zone covers it (tracks of other nets stay off; vias may cross).
    plane: Vec<Option<u16>>,
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

impl Grid {
    fn layer_index(&self, l: Layer) -> Option<usize> {
        self.layers.iter().position(|x| *x == l)
    }
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
        return RouteReport { failed: BTreeMap::new(), tracks: 0, vias: 0, pairs: Vec::new(), lengths: Vec::new() };
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

    // A pair's N follows its P directly, so P's path exists when N is routed.
    let net_idx = |name: &str| netlist.nets.iter().position(|n| n.name == name);
    let mut pair_of_n: BTreeMap<usize, (usize, Nm)> = BTreeMap::new(); // N index -> (P index, gap)
    for pair in &board.diff_pairs {
        if let (Some(pi), Some(ni)) = (net_idx(&pair.p), net_idx(&pair.n)) {
            pair_of_n.insert(ni, (pi, pair.gap));
            if let (Some(a), Some(b)) = (order.iter().position(|&i| i == pi), order.iter().position(|&i| i == ni)) {
                order.remove(b);
                let a = if b < a { a - 1 } else { a };
                order.insert(a + 1, ni);
            }
        }
    }

    let mut failed: BTreeMap<String, usize> = BTreeMap::new();
    let mut coupled: BTreeMap<usize, (usize, usize)> = BTreeMap::new(); // N index -> (cells in corridor, cells)
    let mut new_tracks: Vec<Track> = Vec::new();
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
        coupled.clear();
        for ni in todo {
            let k = class_of[ni];
            let r = &classes[k];
            let via_extra_cells = (((r.via_diameter / 2 - r.track_width / 2).0.max(0) as f64) / r.grid.0 as f64).ceil() as isize;
            let corridor = pair_of_n.get(&ni).map(|&(pi, gap)| {
                let p_name = &nets[pi].name;
                let p_tracks: Vec<&Track> = new_tracks.iter().filter(|t| &t.net == p_name).collect();
                corridor_cells(&grids[k], &p_tracks, gap + r.track_width)
            });
            let (missing, in_corridor) =
                route_net(&mut grids, &classes, k, &nets[ni], via_extra_cells, corridor.as_deref(), &mut new_tracks, &mut new_vias);
            if missing > 0 {
                failed.insert(nets[ni].name.clone(), missing);
            }
            if corridor.is_some() {
                coupled.insert(ni, in_corridor);
            }
        }
    }

    board.tracks.extend(new_tracks);
    board.vias.extend(new_vias);
    let lengths = match_lengths(board, netlist, &mut grids, &classes, &class_of);
    zone::fill_zones(board, netlist, rules.edge_clearance);
    let pairs = board
        .diff_pairs
        .iter()
        .filter_map(|pair| {
            let ni = net_idx(&pair.n)?;
            let (hit, total) = coupled.get(&ni).copied().unwrap_or((0, 0));
            Some(PairReport {
                p: pair.p.clone(),
                n: pair.n.clone(),
                len_p: board.net_length(&pair.p),
                len_n: board.net_length(&pair.n),
                coupled_permille: (hit * 1000).checked_div(total).unwrap_or(0) as u32,
            })
        })
        .collect();
    RouteReport { failed, tracks: board.tracks.len() - before_tracks, vias: board.vias.len() - before_vias, pairs, lengths }
}

/// Cells (per layer) at about `spacing` (centre to centre) from P's tracks on the same layer: where
/// N's centre line should run. The band is one grid step wide so a diagonal P still has a corridor.
fn corridor_cells(grid: &Grid, p_tracks: &[&Track], spacing: Nm) -> Vec<Vec<bool>> {
    let n = grid.nx * grid.ny;
    let mut out = vec![vec![false; n]; grid.layers.len()];
    let lo = spacing - grid.pitch / 2;
    let hi = spacing + grid.pitch / 2;
    for t in p_tracks {
        let Some(l) = grid.layer_index(t.layer) else { continue };
        let (x0, x1, y0, y1) = grid.cell_range(t.segment.bbox().inflate(hi));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d2 = t.segment.squared_distance_to_point(grid.center(x, y));
                if d2 >= lo.squared() && d2 <= hi.squared() {
                    out[l][grid.index(x, y)] = true;
                }
            }
        }
    }
    out
}

/// Bring every length group within tolerance by meandering its shorter nets: a straight run of a
/// track becomes a trombone (out, along, back) that adds twice the depth. Only free cells of the
/// net's own class grid are used, so the result still passes the checker.
fn match_lengths(board: &mut Board, netlist: &Netlist, grids: &mut [Grid], classes: &[RouteRules], class_of: &[usize]) -> Vec<LengthReport> {
    let mut reports = Vec::new();
    // A differential pair is a length group of its own: skew within 5% of the longer leg.
    let pair_groups: Vec<LengthMatch> = board
        .diff_pairs
        .iter()
        .map(|p| {
            let longer = board.net_length(&p.p).max(board.net_length(&p.n));
            LengthMatch { nets: vec![p.p.clone(), p.n.clone()], tolerance: Nm(longer.0 / 20) }
        })
        .collect();
    for (gi, group) in pair_groups.into_iter().chain(board.length_groups.clone()).enumerate() {
        let target = group.nets.iter().map(|n| board.net_length(n)).max().unwrap_or(Nm::ZERO);
        for net in &group.nets {
            let Some(ni) = netlist.nets.iter().position(|n| &n.name == net) else { continue };
            let k = class_of[ni];
            let mut guard = 0;
            while target - board.net_length(net) > group.tolerance && guard < 64 {
                guard += 1;
                let need = target - board.net_length(net);
                if !add_meander(board, grids, classes, k, ni as u16, net, need) {
                    break;
                }
            }
        }
        let nets: Vec<(String, Nm)> = group.nets.iter().map(|n| (n.clone(), board.net_length(n))).collect();
        let (min, max) = nets.iter().fold((Nm(i64::MAX), Nm::ZERO), |(lo, hi), (_, l)| (lo.min(*l), hi.max(*l)));
        // Pairs report through PairReport; only the explicit groups get a LengthReport.
        if gi >= board.diff_pairs.len() {
            reports.push(LengthReport { matched: nets.is_empty() || max - min <= group.tolerance, nets, tolerance: group.tolerance });
        }
    }
    reports
}

/// One trombone on `net`: pick its longest axis-aligned track with room beside it and bend its
/// middle third outward by up to `need / 2`. Returns false when no track has room.
fn add_meander(board: &mut Board, grids: &mut [Grid], classes: &[RouteRules], k: usize, net: u16, name: &str, need: Nm) -> bool {
    let pitch = grids[k].pitch;
    let mut candidates: Vec<usize> = (0..board.tracks.len())
        .filter(|&i| {
            let t = &board.tracks[i];
            t.net == name && (t.segment.a.x == t.segment.b.x || t.segment.a.y == t.segment.b.y) && t.segment.length() >= pitch * 6
        })
        .collect();
    candidates.sort_by_key(|&i| std::cmp::Reverse(board.tracks[i].segment.length().0));
    for ti in candidates {
        let t = board.tracks[ti].clone();
        let Some(l) = grids[k].layer_index(t.layer) else { continue };
        let horizontal = t.segment.a.y == t.segment.b.y;
        let forward = if horizontal { t.segment.a.x < t.segment.b.x } else { t.segment.a.y < t.segment.b.y };
        let (a, b) = if forward { (t.segment.a, t.segment.b) } else { (t.segment.b, t.segment.a) };
        let len = t.segment.length();
        // The bent part: the middle third, snapped to the grid.
        let third = Nm(len.0 / 3 / pitch.0 * pitch.0);
        if third < pitch * 2 {
            continue;
        }
        let (m1, m2) = if horizontal {
            (Point::new(a.x + third, a.y), Point::new(b.x - third, a.y))
        } else {
            (Point::new(a.x, a.y + third), Point::new(a.x, b.y - third))
        };
        // Depth: half the need, in grid steps, at least one; shrink until the cells are free.
        let want = ((need.0 as f64 / 2.0 / pitch.0 as f64).ceil() as i64).max(1);
        for side in [1i64, -1] {
            let mut depth = want;
            while depth >= 1 {
                let off = pitch * depth * side;
                let (o1, o2) = if horizontal {
                    (Point::new(m1.x, m1.y + off), Point::new(m2.x, m2.y + off))
                } else {
                    (Point::new(m1.x + off, m1.y), Point::new(m2.x + off, m2.y))
                };
                let legs = [Segment::new(m1, o1), Segment::new(o1, o2), Segment::new(o2, m2)];
                let reach_k = classes[k].track_width / 2;
                let free = legs.iter().all(|seg| {
                    let g = &grids[k];
                    let (x0, x1, y0, y1) = g.cell_range(seg.bbox().inflate(reach_k));
                    (y0..=y1).all(|y| {
                        (x0..=x1).all(|x| {
                            let c = g.center(x, y);
                            // Cells the existing track already covers are not new copper.
                            seg.squared_distance_to_point(c) > reach_k.squared()
                                || t.segment.squared_distance_to_point(c) <= reach_k.squared()
                                || g.passable(l, g.index(x, y), net)
                        })
                    })
                });
                if free {
                    board.tracks.remove(ti);
                    for seg in [Segment::new(a, m1), legs[0], legs[1], legs[2], Segment::new(m2, b)] {
                        board.tracks.push(Track { net: name.to_string(), layer: t.layer, segment: seg, width: t.width });
                        for (j, g) in grids.iter_mut().enumerate() {
                            g.claim(l, Shape::Stroke { segment: seg, half_width: t.width / 2 }, reach(classes, j, k), net);
                        }
                    }
                    return true;
                }
                depth -= 1;
            }
        }
    }
    false
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
    let layers = board.copper_layers.clone();
    let plane: Vec<Option<u16>> =
        layers.iter().map(|l| zone::plane_net(board, *l).and_then(|n| netlist.nets.iter().position(|x| x.name == n).map(|i| i as u16))).collect();
    let mut grid = Grid { origin: bbox.min, pitch, nx, ny, owner: vec![vec![FREE; nx * ny]; layers.len()], layers, plane };
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
                for layer in &mut grid.owner {
                    layer[i] = BLOCKED;
                }
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
        let layers: Vec<usize> = if pad.layers.is_empty() {
            (0..grid.layers.len()).collect()
        } else {
            board.pad_copper_layers(&pad).iter().filter_map(|l| grid.layer_index(*l)).collect()
        };
        for l in layers {
            grid.claim(l, pad_shape(&pad), class_reach(owner), owner);
        }
    }
    for t in &board.tracks {
        if let (Some(l), Some(n)) = (grid.layer_index(t.layer), net_index(&t.net)) {
            grid.claim(l, Shape::Stroke { segment: t.segment, half_width: t.width / 2 }, class_reach(n), n);
        }
    }
    for v in &board.vias {
        if let Some(n) = net_index(&v.net) {
            for l in 0..grid.layers.len() {
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
        let layers: Vec<usize> = board.pad_copper_layers(&pad).iter().filter_map(|l| grid.layer_index(*l)).collect();
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

/// Join all pads of one net (class `k`). Returns the number of connections that could not be made
/// and, for a pair's N, how many path cells lay in the corridor beside P out of how many.
#[allow(clippy::too_many_arguments)]
fn route_net(
    grids: &mut [Grid],
    classes: &[RouteRules],
    k: usize,
    net: &NetWork,
    via_extra: isize,
    corridor: Option<&[Vec<bool>]>,
    tracks: &mut Vec<Track>,
    vias: &mut Vec<Via>,
) -> (usize, (usize, usize)) {
    let n = grids[k].nx * grids[k].ny;
    // Which pad (index into net.pads) each cell belongs to, per layer; usize::MAX = none.
    let mut pad_of = vec![vec![usize::MAX; n]; grids[k].layers.len()];
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
    let mut in_corridor = (0usize, 0usize);
    for _ in 1..net.pads.len() {
        match search(&grids[k], net.index, &sources, &pad_of, &connected, &classes[k], via_extra, corridor) {
            Some((path, reached)) => {
                connected[reached] = true;
                sources.extend(net.pads[reached].iter().copied());
                if let Some(c) = corridor {
                    in_corridor.0 += path.iter().filter(|&&(l, cell)| c[l][cell]).count();
                    in_corridor.1 += path.len();
                }
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
    (missing, in_corridor)
}

#[allow(clippy::too_many_arguments)]
fn search(
    grid: &Grid,
    net: u16,
    sources: &[(usize, usize)],
    pad_of: &[Vec<usize>],
    connected: &[bool],
    rules: &RouteRules,
    via_extra: isize,
    corridor: Option<&[Vec<bool>]>,
) -> Option<(Vec<(usize, usize)>, usize)> {
    let n = grid.nx * grid.ny;
    let nl = grid.layers.len();
    let mut dist = vec![vec![u32::MAX; n]; nl];
    let mut prev = vec![vec![u32::MAX; n]; nl]; // encoded layer*n + cell
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
            if !grid.passable(l, mc, net) || grid.plane[l].is_some_and(|z| z != net) {
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
            // Beside a pair's P, straying out of the corridor costs triple.
            let cost = match corridor {
                Some(c) if !c[l][mc] => cost * 3,
                _ => cost,
            };
            let nd = d + cost;
            if nd < dist[l][mc] {
                dist[l][mc] = nd;
                prev[l][mc] = (l * n + c) as u32;
                heap.push(Reverse((nd, l as u8, mc as u32)));
            }
        }
        // Layer change: a through via needs room on every copper layer and reaches all of them.
        if nl > 1 && via_ok(grid, net, x, y, via_extra) {
            let nd = d + rules.via_cost;
            for ol in (0..nl).filter(|&ol| ol != l) {
                if nd < dist[ol][c] {
                    dist[ol][c] = nd;
                    prev[ol][c] = (l * n + c) as u32;
                    heap.push(Reverse((nd, ol as u8, c as u32)));
                }
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
            if (0..grid.layers.len()).any(|l| !grid.passable(l, c, net)) {
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
            let all: Vec<usize> = (0..grids[k].layers.len()).collect();
            claim_all(grids, &all, Shape::Circle { center: at, radius: rules.via_diameter / 2 });
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
        tracks.push(Track { net: net.name.clone(), layer: grids[k].layers[l], segment: seg, width: rules.track_width });
        claim_all(grids, &[l], Shape::Stroke { segment: seg, half_width: half });
        i = j;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_designs::fixtures::{divider_netlist, divider_placed, sensor_node_four_layer, sensor_node_hand, sensor_node_power_class};
    use board_designs::{library, sensor_node};
    use board_drc::{Rules, check};
    use board_place::{Constraints, Edge, place};

    fn drc_rules(r: &RouteRules) -> Rules {
        Rules { clearance: r.clearance, min_track_width: r.track_width, edge_clearance: r.edge_clearance, ..Rules::default() }
    }

    /// Two 1x2 headers facing each other across a 20 x 10 board: pin i of one to pin i of the
    /// other, two nets side by side. The raw material for the pair and length tests.
    fn two_lanes(turn_right: bool) -> (Netlist, Board) {
        let mut sch = board_schematic::Schematic::new();
        let a = sch.part("J1", "Conn_01x02", "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical", &[("1", "1"), ("2", "2")]);
        let b = sch.part("J2", "Conn_01x02", "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical", &[("1", "1"), ("2", "2")]);
        sch.net("D_P", &[a.pin("1"), b.pin("1")]);
        sch.net("D_N", &[a.pin("2"), b.pin("2")]);
        let netlist = sch.build().unwrap();
        let mut board = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        // J1 stands vertically (pins along y). J2 does too, unless `turn_right` lays it along x,
        // which makes D_N's run longer than D_P's.
        board.place("J1", board_layout::footprints::pin_header_1xn(2), Point::mm(3, 3), 1, false);
        board.place("J2", board_layout::footprints::pin_header_1xn(2), Point::mm(15, 3), if turn_right { 0 } else { 1 }, false);
        (netlist, board)
    }

    /// Four layers with a GND and a 3V3 plane: signals route on the outer layers, the planes fill
    /// around every via and pad of other nets, both planes reach their own pads, and nothing of
    /// another net is drawn on a plane layer.
    #[test]
    fn four_layer_board_with_planes_routes_and_fills_clean() {
        let netlist = sensor_node();
        let mut board = sensor_node_four_layer();
        let rules = RouteRules::default();
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        for t in &board.tracks {
            if matches!(t.layer, Layer::InnerCopper(_)) {
                let plane = zone::plane_net(&board, t.layer).unwrap();
                assert_eq!(t.net, plane, "only the plane's own net may run tracks on {:?}", t.layer);
            }
        }
        for z in &board.zones {
            let area = zone::fill_area_mm2(z);
            assert!(area > 400.0, "{} plane covers {area:.0} mm2 of 750", z.net);
        }
        let v = check(&board, &netlist, &drc_rules(&rules));
        assert!(v.is_empty(), "{} violations, first: {:?}", v.len(), v.first());
    }

    #[test]
    fn a_differential_pair_runs_coupled_and_equal() {
        let (netlist, mut board) = two_lanes(false);
        board.diff_pairs.push(board_layout::DiffPair { p: "D_P".into(), n: "D_N".into(), gap: Nm::um(200) });
        let rules = RouteRules::default();
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        let pair = &report.pairs[0];
        let (lp, ln) = (pair.len_p.0 as f64, pair.len_n.0 as f64);
        assert!((lp - ln).abs() / lp.max(ln) <= 0.05, "{pair:?}");
        assert!(pair.coupled_permille >= 700, "{pair:?}");
        assert!(check(&board, &netlist, &drc_rules(&rules)).is_empty());
    }

    #[test]
    fn length_groups_are_matched_with_meanders() {
        // The right header lies along x, so D_N's run is longer than D_P's by a few millimetres.
        let (netlist, mut board) = two_lanes(true);
        board.length_groups.push(board_layout::LengthMatch { nets: vec!["D_P".into(), "D_N".into()], tolerance: Nm::um(100) });
        let rules = RouteRules::default();
        let before = {
            let (n2, mut b2) = two_lanes(true);
            route(&mut b2, &n2, &rules);
            (b2.net_length("D_P"), b2.net_length("D_N"))
        };
        assert!((before.0 - before.1).0.abs() > Nm::mm(1).0, "the lanes differ before matching: {before:?}");
        let report = route(&mut board, &netlist, &rules);
        assert!(report.is_complete(), "{report:?}");
        let lr = &report.lengths[0];
        assert!(lr.matched, "{lr:?}");
        let (a, b) = (board.net_length("D_P"), board.net_length("D_N"));
        assert!((a - b).0.abs() <= Nm::um(100).0, "{a} vs {b}");
        assert!(check(&board, &netlist, &drc_rules(&rules)).is_empty(), "{:?}", check(&board, &netlist, &drc_rules(&rules)));
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
