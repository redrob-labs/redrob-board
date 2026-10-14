//! Component placement for Redrob Board.
//!
//! Simulated annealing over integer positions on a grid. The cost is exact integer arithmetic:
//! half-perimeter wire length of every net (pad centres), plus penalties for courtyard overlap,
//! leaving the board and straying from a requested edge. Floating point appears only in the
//! acceptance probability, which decides how the search walks, never what a result means.
//!
//! The random source is a small xorshift generator seeded by the caller, so a placement is
//! reproducible from `(design, constraints, seed)`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use board_geom::{Point, Rect};
use board_layout::{Board, Footprint, Placement};
use board_lib::Library;
use board_netlist::Netlist;
use board_units::Nm;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraints {
    /// Parts that must sit against a board edge (connectors, buttons, LEDs).
    pub edge_parts: Vec<(String, Edge)>,
    /// Minimum gap kept between courtyards.
    pub courtyard_gap: Nm,
    /// Positions are multiples of this.
    pub grid: Nm,
    pub seed: u64,
    pub iterations: usize,
}

impl Default for Constraints {
    fn default() -> Constraints {
        Constraints { edge_parts: Vec::new(), courtyard_gap: Nm::um(250), grid: Nm::um(50), seed: 1, iterations: 60_000 }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlaceError {
    #[error("{refdes}: footprint {footprint} not in library")]
    MissingFootprint { refdes: String, footprint: String },
    #[error("{0} is in the edge constraints but not in the netlist")]
    UnknownEdgePart(String),
    #[error("board outline is too small for the parts")]
    TooSmall,
}

/// Half-perimeter wire length summed over every net, using pad centres on the board.
pub fn hpwl(board: &Board, netlist: &Netlist) -> Nm {
    let pads = board.all_pads();
    let mut total = Nm::ZERO;
    for net in &netlist.nets {
        let mut bbox: Option<Rect> = None;
        for p in &net.pins {
            if let Some(pad) = pads.iter().find(|bp| bp.refdes == p.refdes && bp.number == p.pin) {
                let r = Rect::from_points(pad.center, pad.center);
                bbox = Some(bbox.map_or(r, |b| b.union(&r)));
            }
        }
        if let Some(b) = bbox {
            total = total + b.width() + b.height();
        }
    }
    total
}

struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Part {
    refdes: String,
    footprint: Footprint,
    /// Pad centre per pad number, local coordinates.
    pads: Vec<(String, Point)>,
    edge: Option<Edge>,
}

struct Pose {
    at: Point,
    turns: u8,
}

struct Problem<'a> {
    parts: Vec<Part>,
    /// For each net, (part index, pad index) pairs.
    nets: Vec<Vec<(usize, usize)>>,
    outline: Rect,
    c: &'a Constraints,
}

impl Problem<'_> {
    fn pad_center(&self, i: usize, pad: usize, pose: &Pose) -> Point {
        self.parts[i].pads[pad].1.rotate_quarter_turns(pose.turns).translate(pose.at)
    }

    fn courtyard(&self, i: usize, pose: &Pose) -> Rect {
        let c = self.parts[i].footprint.courtyard.corners().map(|p| p.rotate_quarter_turns(pose.turns).translate(pose.at));
        c.iter().skip(1).fold(Rect::from_points(c[0], c[0]), |r, p| r.union(&Rect::from_points(*p, *p)))
    }

    fn cost(&self, poses: &[Pose]) -> i128 {
        let mut wire: i128 = 0;
        for net in &self.nets {
            let mut bbox: Option<Rect> = None;
            for &(i, pad) in net {
                let p = self.pad_center(i, pad, &poses[i]);
                let r = Rect::from_points(p, p);
                bbox = Some(bbox.map_or(r, |b| b.union(&r)));
            }
            if let Some(b) = bbox {
                wire += (b.width() + b.height()).0 as i128;
            }
        }
        let yards: Vec<Rect> = (0..self.parts.len()).map(|i| self.courtyard(i, &poses[i]).inflate(self.c.courtyard_gap / 2)).collect();
        let mut overlap: i128 = 0;
        for i in 0..yards.len() {
            for j in i + 1..yards.len() {
                let (a, b) = (&yards[i], &yards[j]);
                let w = (a.max.x.min(b.max.x) - a.min.x.max(b.min.x)).0;
                let h = (a.max.y.min(b.max.y) - a.min.y.max(b.min.y)).0;
                if w > 0 && h > 0 {
                    // Overlap penalised by its perimeter in nm, so it is commensurate with wire length
                    // and strongly dominates once any overlap exists.
                    overlap += 4 * (w + h) as i128;
                }
            }
        }
        let mut outside: i128 = 0;
        let mut edge: i128 = 0;
        for (i, y) in yards.iter().enumerate() {
            let o = &self.outline;
            let dx = (o.min.x - y.min.x).max(y.max.x - o.max.x).max(Nm::ZERO).0;
            let dy = (o.min.y - y.min.y).max(y.max.y - o.max.y).max(Nm::ZERO).0;
            outside += 8 * (dx + dy) as i128;
            if let Some(e) = self.parts[i].edge {
                let d = match e {
                    Edge::Left => (y.min.x - o.min.x).0,
                    Edge::Right => (o.max.x - y.max.x).0,
                    Edge::Bottom => (y.min.y - o.min.y).0,
                    Edge::Top => (o.max.y - y.max.y).0,
                };
                edge += 2 * d.max(0) as i128;
            }
        }
        wire + overlap + outside + edge
    }
}

fn snap(v: Nm, grid: Nm) -> Nm {
    let g = grid.0.max(1);
    Nm(((v.0 + g / 2).div_euclid(g)) * g)
}

/// Place every component of `netlist` inside `outline` (front side only).
pub fn place(netlist: &Netlist, library: &Library, outline: Rect, c: &Constraints) -> Result<Board, PlaceError> {
    let edges: BTreeMap<&str, Edge> = c.edge_parts.iter().map(|(r, e)| (r.as_str(), *e)).collect();
    for r in edges.keys() {
        if netlist.component(r).is_none() {
            return Err(PlaceError::UnknownEdgePart(r.to_string()));
        }
    }
    let mut parts = Vec::new();
    for comp in &netlist.components {
        let fp = library
            .get(&comp.footprint)
            .ok_or_else(|| PlaceError::MissingFootprint { refdes: comp.refdes.clone(), footprint: comp.footprint.clone() })?
            .clone();
        let pads = fp.pads.iter().map(|p| (p.number.clone(), p.at)).collect();
        parts.push(Part { refdes: comp.refdes.clone(), footprint: fp, pads, edge: edges.get(comp.refdes.as_str()).copied() });
    }
    let index: BTreeMap<&str, usize> = parts.iter().enumerate().map(|(i, p)| (p.refdes.as_str(), i)).collect();
    let nets: Vec<Vec<(usize, usize)>> = netlist
        .nets
        .iter()
        .map(|n| {
            n.pins
                .iter()
                .filter_map(|p| {
                    let i = *index.get(p.refdes.as_str())?;
                    let pad = parts[i].pads.iter().position(|(num, _)| *num == p.pin)?;
                    Some((i, pad))
                })
                .collect()
        })
        .collect();

    let area: i128 = parts
        .iter()
        .map(|p| (p.footprint.courtyard.width() + c.courtyard_gap).0 as i128 * (p.footprint.courtyard.height() + c.courtyard_gap).0 as i128)
        .sum();
    if area > outline.width().0 as i128 * outline.height().0 as i128 {
        return Err(PlaceError::TooSmall);
    }

    let problem = Problem { parts, nets, outline, c };
    let mut rng = Xorshift(c.seed.max(1) ^ 0x9E37_79B9_7F4A_7C15);

    // Initial poses: a grid walk across the outline, largest parts first.
    let mut order: Vec<usize> = (0..problem.parts.len()).collect();
    order.sort_by_key(|&i| {
        let y = problem.parts[i].footprint.courtyard;
        std::cmp::Reverse(y.width().0 as i128 * y.height().0 as i128)
    });
    let mut poses: Vec<Pose> = (0..problem.parts.len()).map(|_| Pose { at: outline.center(), turns: 0 }).collect();
    let mut cursor = outline.min;
    let mut row_h = Nm::ZERO;
    for &i in &order {
        let y = problem.parts[i].footprint.courtyard.inflate(c.courtyard_gap / 2);
        if cursor.x + y.width() > outline.max.x {
            cursor = Point::new(outline.min.x, cursor.y + row_h);
            row_h = Nm::ZERO;
        }
        poses[i] = Pose { at: Point::new(snap(cursor.x - y.min.x, c.grid), snap(cursor.y - y.min.y, c.grid)), turns: 0 };
        cursor.x = cursor.x + y.width();
        row_h = row_h.max(y.height());
    }

    let mut cost = problem.cost(&poses);
    let span = outline.width().max(outline.height()).0 as f64;
    let t0 = (cost as f64 / problem.parts.len().max(1) as f64).max(1.0);
    let t_end = (c.grid.0 as f64).max(1.0);
    let n = problem.parts.len();
    for step in 0..c.iterations {
        let frac = step as f64 / c.iterations.max(1) as f64;
        let t = t0 * (t_end / t0).powf(frac);
        let reach = ((span * (1.0 - frac) * 0.5).max(c.grid.0 as f64 * 4.0)) as i64;
        let i = rng.below(n as u64) as usize;
        let saved_i = Pose { at: poses[i].at, turns: poses[i].turns };
        let mut swapped: Option<(usize, Pose)> = None;
        match rng.below(10) {
            0..=6 => {
                let dx = rng.below(2 * reach as u64 + 1) as i64 - reach;
                let dy = rng.below(2 * reach as u64 + 1) as i64 - reach;
                let at = Point::new(snap(poses[i].at.x + Nm(dx), c.grid), snap(poses[i].at.y + Nm(dy), c.grid));
                poses[i].at = Point::new(at.x.clamp(outline.min.x, outline.max.x), at.y.clamp(outline.min.y, outline.max.y));
            }
            7..=8 => poses[i].turns = (poses[i].turns + 1 + rng.below(3) as u8) % 4,
            _ if n > 1 => {
                let mut j = rng.below(n as u64) as usize;
                if j == i {
                    j = (j + 1) % n;
                }
                swapped = Some((j, Pose { at: poses[j].at, turns: poses[j].turns }));
                let (ai, aj) = (poses[i].at, poses[j].at);
                poses[i].at = aj;
                poses[j].at = ai;
            }
            _ => {}
        }
        let new_cost = problem.cost(&poses);
        let delta = new_cost - cost;
        let accept = delta <= 0 || rng.unit() < (-(delta as f64) / t).exp();
        if accept {
            cost = new_cost;
        } else {
            poses[i] = saved_i;
            if let Some((j, p)) = swapped {
                poses[j] = p;
            }
        }
    }

    let mut board = Board::two_layer(outline);
    for (i, part) in problem.parts.iter().enumerate() {
        board.placements.push(Placement {
            refdes: part.refdes.clone(),
            footprint: part.footprint.clone(),
            at: poses[i].at,
            quarter_turns: poses[i].turns,
            flipped: false,
        });
    }
    Ok(board)
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_designs::fixtures::{divider_netlist, divider_placed};
    use board_designs::{library, sensor_node};
    use board_drc::{Rule, Rules, check, summary};

    fn clean_except_unrouted(board: &Board, netlist: &Netlist) {
        let s = summary(&check(board, netlist, &Rules::default()));
        let others: Vec<_> = s.keys().filter(|r| **r != Rule::Unrouted).collect();
        assert!(others.is_empty(), "{s:?}");
    }

    #[test]
    fn divider_beats_hand_placement() {
        let netlist = divider_netlist();
        let hand = hpwl(&divider_placed(), &netlist);
        let c = Constraints { edge_parts: vec![("J1".into(), Edge::Bottom)], iterations: 20_000, ..Constraints::default() };
        let board = place(&netlist, &library(), Rect::from_points(Point::ORIGIN, Point::mm(20, 10)), &c).unwrap();
        clean_except_unrouted(&board, &netlist);
        let auto = hpwl(&board, &netlist);
        assert!(auto * 10 <= hand * 8, "auto {auto} vs hand {hand}");
        let j1 = board.placement("J1").unwrap().board_courtyard();
        assert!(j1.min.y - Nm::ZERO <= Nm::mm(1), "J1 not at bottom edge: {j1:?}");
    }

    #[test]
    fn sensor_node_places_clean_and_reproducibly() {
        let netlist = sensor_node();
        let outline = Rect::from_points(Point::ORIGIN, Point::mm(30, 25));
        let c =
            Constraints { edge_parts: vec![("J1".into(), Edge::Left), ("SW1".into(), Edge::Right)], iterations: 30_000, ..Constraints::default() };
        let a = place(&netlist, &library(), outline, &c).unwrap();
        clean_except_unrouted(&a, &netlist);
        let b = place(&netlist, &library(), outline, &c).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.placements.len(), 14);
    }

    #[test]
    fn errors() {
        let netlist = divider_netlist();
        let c = Constraints { edge_parts: vec![("X9".into(), Edge::Top)], ..Constraints::default() };
        assert_eq!(
            place(&netlist, &library(), Rect::from_points(Point::ORIGIN, Point::mm(20, 10)), &c),
            Err(PlaceError::UnknownEdgePart("X9".into()))
        );
        assert_eq!(
            place(&netlist, &library(), Rect::from_points(Point::ORIGIN, Point::mm(2, 2)), &Constraints::default()),
            Err(PlaceError::TooSmall)
        );
        assert!(matches!(
            place(&netlist, &Library::new(), Rect::from_points(Point::ORIGIN, Point::mm(20, 10)), &Constraints::default()),
            Err(PlaceError::MissingFootprint { .. })
        ));
    }
}
