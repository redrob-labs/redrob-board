//! Design rule check for Redrob Board.
//!
//! Every verdict is an integer comparison on nanometres. A gap of `g` violates clearance `c`
//! when `g < c`, which for two round things with radii `r1`, `r2` and centre distance `d` is
//! `d < r1 + r2 + c`, compared squared. No square roots, no floats.
//!
//! Pads are checked as their exact shape where that shape is a circle, and as their bounding
//! box otherwise. The box is conservative for rounded rectangles and ovals, so the checker may
//! report a clearance violation the oracle does not; it never misses one the oracle reports.
//! Stage 1's oracle comparison is what tightens this.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use board_geom::{Point, Polygon, Rect, Segment};
use board_layout::{Board, BoardPad, Layer, PadShape};
use board_netlist::{Netlist, PinRef};
use board_units::Nm;
use serde::{Deserialize, Serialize};

/// Manufacturing limits. Defaults follow a common two-layer prototype service's capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Copper to copper of a different net.
    pub clearance: Nm,
    pub min_track_width: Nm,
    /// Copper left around a drilled hole: `(pad size - drill) / 2`.
    pub min_annular_ring: Nm,
    pub min_drill: Nm,
    /// Copper to board edge.
    pub edge_clearance: Nm,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules {
            clearance: Nm::um(127),
            min_track_width: Nm::um(127),
            min_annular_ring: Nm::um(130),
            min_drill: Nm::um(300),
            edge_clearance: Nm::um(300),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rule {
    Short,
    Clearance,
    TrackWidth,
    AnnularRing,
    DrillSize,
    EdgeClearance,
    CourtyardOverlap,
    MissingFootprint,
    UnknownRefdes,
    Unrouted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub rule: Rule,
    pub message: String,
    pub at: Point,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} at ({}, {}): {}", self.rule, self.at.x, self.at.y, self.message)
    }
}

/// One piece of copper, resolved to the board.
#[derive(Clone, Debug)]
struct Item {
    name: String,
    net: Option<String>,
    layers: Vec<Layer>,
    shape: Shape,
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Rect(Rect),
    Circle {
        center: Point,
        radius: Nm,
    },
    /// A track: a segment swept by a disc of `half_width`.
    Stroke {
        segment: Segment,
        half_width: Nm,
    },
}

impl Shape {
    fn anchor(self) -> Point {
        match self {
            Shape::Rect(r) => r.center(),
            Shape::Circle { center, .. } => center,
            Shape::Stroke { segment, .. } => segment.a,
        }
    }

    fn bbox(self) -> Rect {
        match self {
            Shape::Rect(r) => r,
            Shape::Circle { center, radius } => Rect::centered(center, radius * 2, radius * 2),
            Shape::Stroke { segment, half_width } => segment.bbox().inflate(half_width),
        }
    }

    /// True when the gap between the two shapes is smaller than `clearance`. `clearance == 0`
    /// asks whether they touch or overlap.
    fn closer_than(self, other: Shape, clearance: Nm) -> bool {
        let c = clearance;
        match (self, other) {
            (Shape::Rect(a), Shape::Rect(b)) => {
                if c == Nm::ZERO {
                    a.intersects(&b)
                } else {
                    a.squared_gap(&b) < c.squared()
                }
            }
            (Shape::Circle { center: ca, radius: ra }, Shape::Circle { center: cb, radius: rb }) => {
                let limit = ra + rb + c;
                within(ca.squared_distance(cb), limit)
            }
            (Shape::Circle { center, radius }, Shape::Rect(r)) | (Shape::Rect(r), Shape::Circle { center, radius }) => {
                within(rect_point_squared_distance(&r, center), radius + c)
            }
            (Shape::Circle { center, radius }, Shape::Stroke { segment, half_width })
            | (Shape::Stroke { segment, half_width }, Shape::Circle { center, radius }) => {
                within(segment.squared_distance_to_point(center), radius + half_width + c)
            }
            (Shape::Stroke { segment: sa, half_width: ha }, Shape::Stroke { segment: sb, half_width: hb }) => {
                within(sa.squared_distance_to_segment(&sb), ha + hb + c)
            }
            (Shape::Rect(r), Shape::Stroke { segment, half_width }) | (Shape::Stroke { segment, half_width }, Shape::Rect(r)) => {
                within(rect_segment_squared_distance(&r, &segment), half_width + c)
            }
        }
    }
}

/// `distance < limit`, with the distance given squared. A zero limit means "touching counts",
/// so the comparison becomes `<=`.
fn within(squared_distance: i128, limit: Nm) -> bool {
    if limit == Nm::ZERO { squared_distance == 0 } else { squared_distance < limit.squared() }
}

fn rect_point_squared_distance(r: &Rect, p: Point) -> i128 {
    let dx = (r.min.x - p.x).max(p.x - r.max.x).max(Nm::ZERO);
    let dy = (r.min.y - p.y).max(p.y - r.max.y).max(Nm::ZERO);
    dx.squared() + dy.squared()
}

fn rect_segment_squared_distance(r: &Rect, s: &Segment) -> i128 {
    if r.contains(s.a) || r.contains(s.b) {
        return 0;
    }
    let c = r.corners();
    (0..4).map(|i| Segment::new(c[i], c[(i + 1) % 4]).squared_distance_to_segment(s)).min().unwrap_or(0)
}

fn pad_shape(p: &BoardPad) -> Shape {
    match p.shape {
        PadShape::Circle { diameter } => Shape::Circle { center: p.center, radius: diameter / 2 },
        _ => Shape::Rect(p.bbox),
    }
}

fn copper_items(board: &Board, netlist: &Netlist) -> Vec<Item> {
    let mut items = Vec::new();
    for pad in board.all_pads() {
        if pad.layers.is_empty() {
            continue; // non-plated hole: no copper
        }
        let net = netlist.net_of(&PinRef::new(pad.refdes.clone(), pad.number.clone())).map(|n| n.name.clone());
        items.push(Item { name: format!("{}.{}", pad.refdes, pad.number), net, layers: pad.layers.clone(), shape: pad_shape(&pad) });
    }
    for (i, t) in board.tracks.iter().enumerate() {
        items.push(Item {
            name: format!("track#{i}"),
            net: Some(t.net.clone()),
            layers: vec![t.layer],
            shape: Shape::Stroke { segment: t.segment, half_width: t.width / 2 },
        });
    }
    for (i, v) in board.vias.iter().enumerate() {
        items.push(Item {
            name: format!("via#{i}"),
            net: Some(v.net.clone()),
            layers: board.copper_layers.clone(),
            shape: Shape::Circle { center: v.at, radius: v.diameter / 2 },
        });
    }
    items
}

fn share_layer(a: &Item, b: &Item) -> bool {
    a.layers.iter().any(|l| b.layers.contains(l))
}

/// Run every rule. The result is empty exactly when the board passes.
pub fn check(board: &Board, netlist: &Netlist, rules: &Rules) -> Vec<Violation> {
    let mut out = Vec::new();
    let items = copper_items(board, netlist);
    // A net in one of the board's classes answers to that class's clearance and width; two items
    // of different classes keep the larger clearance.
    let clearance_of = |net: &Option<String>| net.as_deref().and_then(|n| board.class_of(n)).map_or(rules.clearance, |c| c.clearance);
    let min_width_of = |net: &str| board.class_of(net).map_or(rules.min_track_width, |c| c.track_width);

    // Clearance and shorts between copper of different nets.
    for (i, a) in items.iter().enumerate() {
        for b in &items[i + 1..] {
            if !share_layer(a, b) {
                continue;
            }
            let same_net = matches!((&a.net, &b.net), (Some(x), Some(y)) if x == y);
            if same_net {
                continue;
            }
            let clearance = clearance_of(&a.net).max(clearance_of(&b.net));
            if a.shape.closer_than(b.shape, Nm::ZERO) {
                out.push(Violation { rule: Rule::Short, message: format!("{} touches {}", a.name, b.name), at: a.shape.anchor() });
            } else if a.shape.closer_than(b.shape, clearance) {
                out.push(Violation {
                    rule: Rule::Clearance,
                    message: format!("{} to {} closer than {}", a.name, b.name, clearance),
                    at: a.shape.anchor(),
                });
            }
        }
    }

    for (i, t) in board.tracks.iter().enumerate() {
        let min = min_width_of(&t.net);
        if t.width < min {
            out.push(Violation { rule: Rule::TrackWidth, message: format!("track#{i} width {} below {}", t.width, min), at: t.segment.a });
        }
    }

    for pad in board.all_pads() {
        if let Some(drill) = pad.drill {
            let name = format!("{}.{}", pad.refdes, pad.number);
            if drill < rules.min_drill {
                out.push(Violation { rule: Rule::DrillSize, message: format!("{name} drill {drill} below {}", rules.min_drill), at: pad.center });
            }
            if !pad.layers.is_empty() {
                let (w, h) = pad.shape.size();
                let ring = (w.min(h) - drill) / 2;
                if ring < rules.min_annular_ring {
                    out.push(Violation {
                        rule: Rule::AnnularRing,
                        message: format!("{name} annular ring {ring} below {}", rules.min_annular_ring),
                        at: pad.center,
                    });
                }
            }
        }
    }
    for (i, v) in board.vias.iter().enumerate() {
        if v.drill < rules.min_drill {
            out.push(Violation { rule: Rule::DrillSize, message: format!("via#{i} drill {} below {}", v.drill, rules.min_drill), at: v.at });
        }
        let ring = (v.diameter - v.drill) / 2;
        if ring < rules.min_annular_ring {
            out.push(Violation {
                rule: Rule::AnnularRing,
                message: format!("via#{i} annular ring {ring} below {}", rules.min_annular_ring),
                at: v.at,
            });
        }
    }

    for item in &items {
        if !inside_with_margin(&board.outline, item.shape.bbox(), rules.edge_clearance) {
            out.push(Violation {
                rule: Rule::EdgeClearance,
                message: format!("{} closer than {} to the board edge", item.name, rules.edge_clearance),
                at: item.shape.anchor(),
            });
        }
    }

    for (i, a) in board.placements.iter().enumerate() {
        for b in &board.placements[i + 1..] {
            if a.flipped != b.flipped {
                continue;
            }
            let (ya, yb) = (a.board_courtyard(), b.board_courtyard());
            let overlap = ya.min.x < yb.max.x && yb.min.x < ya.max.x && ya.min.y < yb.max.y && yb.min.y < ya.max.y;
            if overlap {
                out.push(Violation {
                    rule: Rule::CourtyardOverlap,
                    message: format!("{} and {} courtyards overlap", a.refdes, b.refdes),
                    at: ya.center(),
                });
            }
        }
    }

    let placed: BTreeSet<&str> = board.placements.iter().map(|p| p.refdes.as_str()).collect();
    for c in &netlist.components {
        if !placed.contains(c.refdes.as_str()) {
            out.push(Violation {
                rule: Rule::MissingFootprint,
                message: format!("{} is in the netlist but not on the board", c.refdes),
                at: Point::ORIGIN,
            });
        }
    }
    for p in &board.placements {
        if netlist.component(&p.refdes).is_none() {
            out.push(Violation { rule: Rule::UnknownRefdes, message: format!("{} is on the board but not in the netlist", p.refdes), at: p.at });
        }
    }

    out.extend(unrouted(&items, netlist));
    out
}

/// Every corner of `bbox` inside the outline and at least `margin` from every edge. Exact for
/// rectangular outlines; conservative for others.
fn inside_with_margin(outline: &Polygon, bbox: Rect, margin: Nm) -> bool {
    bbox.corners().iter().all(|c| outline.contains(*c) && outline.edges().all(|e| e.squared_distance_to_point(*c) >= margin.squared()))
}

/// For each net, one violation per copper island beyond the first (the connections still missing).
fn unrouted(items: &[Item], netlist: &Netlist) -> Vec<Violation> {
    let mut out = Vec::new();
    for net in &netlist.nets {
        let members: Vec<&Item> = items.iter().filter(|i| i.net.as_deref() == Some(net.name.as_str())).collect();
        let pad_count = members.iter().filter(|i| !i.name.starts_with("track#") && !i.name.starts_with("via#")).count();
        if pad_count < 2 {
            continue;
        }
        let mut uf = UnionFind::new(members.len());
        for i in 0..members.len() {
            for j in i + 1..members.len() {
                if share_layer(members[i], members[j]) && members[i].shape.closer_than(members[j].shape, Nm::ZERO) {
                    uf.union(i, j);
                }
            }
        }
        let mut islands: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
        for (i, m) in members.iter().enumerate() {
            if !m.name.starts_with("track#") && !m.name.starts_with("via#") {
                islands.entry(uf.find(i)).or_default().push(&m.name);
            }
        }
        // One violation per missing connection, i.e. islands - 1: the number of ratsnest lines
        // still to route, which is also how the oracle counts.
        let groups: Vec<&Vec<&str>> = islands.values().collect();
        for (i, island) in groups.iter().enumerate().skip(1) {
            out.push(Violation {
                rule: Rule::Unrouted,
                message: format!("net {}: {:?} not connected to {:?}", net.name, island, groups[i - 1]),
                at: members[0].shape.anchor(),
            });
        }
    }
    out
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> UnionFind {
        UnionFind { parent: (0..n).collect() }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// Count of violations per rule, for reports.
pub fn summary(violations: &[Violation]) -> BTreeMap<Rule, usize> {
    let mut m = BTreeMap::new();
    for v in violations {
        *m.entry(v.rule).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_layout::{Track, Via, footprints};
    use board_schematic::Schematic;

    fn divider() -> Netlist {
        let mut sch = Schematic::new();
        let r1 = sch.passive("R1", "10k", "Resistor_SMD:R_0402_1005Metric");
        let r2 = sch.passive("R2", "10k", "Resistor_SMD:R_0402_1005Metric");
        let j1 = sch.part("J1", "Conn", "Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical", &[("1", "VIN"), ("2", "OUT"), ("3", "GND")]);
        sch.net("VIN", &[j1.pin("1"), r1.pin("1")]);
        sch.net("OUT", &[r1.pin("2"), r2.pin("1"), j1.pin("2")]);
        sch.net("GND", &[r2.pin("2"), j1.pin("3")]);
        sch.build().unwrap()
    }

    fn placed_board() -> Board {
        let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
        b.place("R1", footprints::chip_0402(), Point::mm(5, 5), 0, false);
        b.place("R2", footprints::chip_0402(), Point::mm(9, 5), 0, false);
        b.place("J1", footprints::pin_header_1xn(3), Point::mm(5, 2), 0, false);
        b
    }

    fn rules_of(v: &[Violation]) -> Vec<Rule> {
        v.iter().map(|v| v.rule).collect()
    }

    #[test]
    fn placed_but_unrouted_board_reports_only_unrouted_nets() {
        let v = check(&placed_board(), &divider(), &Rules::default());
        assert_eq!(rules_of(&v), vec![Rule::Unrouted; 4]); // VIN 1, OUT 2, GND 1 ratsnest lines
        assert_eq!(summary(&v).get(&Rule::Unrouted), Some(&4));
    }

    #[test]
    fn fully_routed_board_passes() {
        let mut b = placed_board();
        let w = Nm::um(250);
        let track = |net: &str, a: Point, bp: Point| Track { net: net.into(), layer: Layer::FrontCopper, segment: Segment::new(a, bp), width: w };
        // J1.1 (5,2) -> R1.1 (4.5,5)
        b.tracks.push(track("VIN", Point::mm(5, 2), Point::new(Nm::um(4500), Nm::mm(5))));
        // R1.2 (5.5,5) -> R2.1 (8.5,5)
        b.tracks.push(track("OUT", Point::new(Nm::um(5500), Nm::mm(5)), Point::new(Nm::um(8500), Nm::mm(5))));
        // J1.2 (7.54,2) -> (7,5) on OUT
        b.tracks.push(track("OUT", Point::new(Nm::um(7540), Nm::mm(2)), Point::mm(7, 5)));
        // R2.2 (9.5,5) -> via (12,5) -> back layer -> J1.3 (10.08,2)
        b.tracks.push(track("GND", Point::new(Nm::um(9500), Nm::mm(5)), Point::mm(12, 5)));
        b.vias.push(Via { net: "GND".into(), at: Point::mm(12, 5), diameter: Nm::um(600), drill: Nm::um(300) });
        b.tracks.push(Track { layer: Layer::BackCopper, ..track("GND", Point::mm(12, 5), Point::new(Nm::um(10080), Nm::mm(2))) });
        let v = check(&b, &divider(), &Rules::default());
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn clearance_and_short() {
        let mut b = placed_board();
        // Move R2 so its pad 1 sits 0.1 mm from R1 pad 2 (different nets? no: both OUT). Use GND track instead.
        b.tracks.push(Track {
            net: "GND".into(),
            layer: Layer::FrontCopper,
            segment: Segment::new(Point::new(Nm::um(5500), Nm::um(5400)), Point::mm(7, 7)),
            width: Nm::um(200),
        });
        let v = check(&b, &divider(), &Rules::default());
        // Track edge is at y=5.3; R1.2 pad top is at y=5.25: gap 50 um < 127 um.
        assert!(rules_of(&v).contains(&Rule::Clearance), "{v:#?}");
        assert!(!rules_of(&v).contains(&Rule::Short));

        let mut b = placed_board();
        b.tracks.push(Track {
            net: "GND".into(),
            layer: Layer::FrontCopper,
            segment: Segment::new(Point::mm(5, 5), Point::mm(7, 7)),
            width: Nm::um(200),
        });
        let v = check(&b, &divider(), &Rules::default());
        // The diagonal passes through R1.2's pad (short) and starts 0.1 mm from R1.1 (clearance).
        let shorts: Vec<_> = v.iter().filter(|v| v.rule == Rule::Short).collect();
        assert_eq!(shorts.len(), 1, "{v:#?}");
        assert!(shorts[0].message.contains("R1.2"));
        assert!(v.iter().any(|v| v.rule == Rule::Clearance && v.message.contains("R1.1")), "{v:#?}");
    }

    #[test]
    fn same_net_on_other_layer_is_fine() {
        let mut b = placed_board();
        // A GND track on the back layer right under R1: different layer, no violation.
        b.tracks.push(Track {
            net: "GND".into(),
            layer: Layer::BackCopper,
            segment: Segment::new(Point::mm(4, 5), Point::mm(6, 5)),
            width: Nm::um(200),
        });
        let v = check(&b, &divider(), &Rules::default());
        assert!(!rules_of(&v).contains(&Rule::Short) && !rules_of(&v).contains(&Rule::Clearance), "{v:#?}");
    }

    #[test]
    fn size_rules() {
        let mut b = placed_board();
        b.tracks.push(Track {
            net: "VIN".into(),
            layer: Layer::FrontCopper,
            segment: Segment::new(Point::mm(15, 8), Point::mm(17, 8)),
            width: Nm::um(100),
        });
        b.vias.push(Via { net: "VIN".into(), at: Point::mm(15, 5), diameter: Nm::um(400), drill: Nm::um(200) });
        let v = check(&b, &divider(), &Rules::default());
        let r = rules_of(&v);
        assert!(r.contains(&Rule::TrackWidth));
        assert!(r.contains(&Rule::DrillSize));
        assert!(r.contains(&Rule::AnnularRing));
    }

    #[test]
    fn edge_courtyard_and_netlist_mismatch() {
        let mut b = placed_board();
        b.place("R2", footprints::chip_0402(), Point::new(Nm::um(1000), Nm::mm(8)), 0, false); // duplicate refdes near edge
        b.place("C9", footprints::chip_0402(), Point::mm(9, 5), 0, false); // overlaps R2, not in netlist
        let v = check(&b, &divider(), &Rules::default());
        let r = rules_of(&v);
        assert!(r.contains(&Rule::EdgeClearance), "{v:#?}");
        assert!(r.contains(&Rule::CourtyardOverlap));
        assert!(r.contains(&Rule::UnknownRefdes));

        let mut b = placed_board();
        b.placements.retain(|p| p.refdes != "J1");
        let v = check(&b, &divider(), &Rules::default());
        assert!(rules_of(&v).contains(&Rule::MissingFootprint));
    }

    #[test]
    fn shape_distances() {
        let c = Shape::Circle { center: Point::ORIGIN, radius: Nm::mm(1) };
        let r = Shape::Rect(Rect::from_points(Point::mm(2, -1), Point::mm(3, 1)));
        assert!(!c.closer_than(r, Nm::mm(1))); // gap exactly 1 mm is not < 1 mm
        assert!(c.closer_than(r, Nm::um(1001)));
        let s = Shape::Stroke { segment: Segment::new(Point::mm(0, 3), Point::mm(5, 3)), half_width: Nm::um(500) };
        assert!(!s.closer_than(c, Nm::mm(1))); // 3 - 0.5 - 1 = 1.5 gap
        assert!(s.closer_than(c, Nm::mm(2)));
        assert!(s.closer_than(Shape::Rect(Rect::from_points(Point::mm(1, 2), Point::mm(2, 4))), Nm::ZERO));
    }
}
