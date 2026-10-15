//! Small boards with known DRC outcomes. The oracle workflow exports each one to KiCad and
//! requires KiCad's violation counts to agree with ours.

use board_geom::{Point, Rect, Segment};
use board_layout::{Board, Layer, NetClass, Track, Via, Zone, footprints};
use board_netlist::Netlist;
use board_schematic::Schematic;
use board_units::Nm;

/// A fixture: name, netlist, board.
pub struct Fixture {
    pub name: &'static str,
    pub netlist: Netlist,
    pub board: Board,
}

/// Resistive divider on a 3-pin header: three parts, three nets.
pub fn divider_netlist() -> Netlist {
    let mut sch = Schematic::new();
    let r1 = sch.passive("R1", "10k", "Resistor_SMD:R_0402_1005Metric");
    let r2 = sch.passive("R2", "10k", "Resistor_SMD:R_0402_1005Metric");
    let j1 = sch.part("J1", "Conn_01x03", "Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical", &[("1", "VIN"), ("2", "OUT"), ("3", "GND")]);
    sch.net("VIN", &[j1.pin("1"), r1.pin("1")]);
    sch.net("OUT", &[r1.pin("2"), r2.pin("1"), j1.pin("2")]);
    sch.net("GND", &[r2.pin("2"), j1.pin("3")]);
    sch.build().expect("divider netlist is valid")
}

/// Parts placed, nothing routed: every net is open.
pub fn divider_placed() -> Board {
    let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
    b.place("R1", footprints::chip_0402(), Point::mm(5, 5), 0, false);
    b.place("R2", footprints::chip_0402(), Point::mm(9, 5), 0, false);
    b.place("J1", footprints::pin_header_1xn(3), Point::mm(5, 2), 0, false);
    b
}

fn track(net: &str, layer: Layer, a: Point, b: Point) -> Track {
    Track { net: net.into(), layer, segment: Segment::new(a, b), width: Nm::um(250) }
}

/// Fully routed, passes every rule.
pub fn divider_routed() -> Board {
    let mut b = divider_placed();
    let f = Layer::FrontCopper;
    b.tracks.push(track("VIN", f, Point::mm(5, 2), Point::new(Nm::um(4500), Nm::mm(5))));
    b.tracks.push(track("OUT", f, Point::new(Nm::um(5500), Nm::mm(5)), Point::new(Nm::um(8500), Nm::mm(5))));
    b.tracks.push(track("OUT", f, Point::new(Nm::um(7540), Nm::mm(2)), Point::mm(7, 5)));
    b.tracks.push(track("GND", f, Point::new(Nm::um(9500), Nm::mm(5)), Point::mm(12, 5)));
    b.vias.push(Via { net: "GND".into(), at: Point::mm(12, 5), diameter: Nm::um(600), drill: Nm::um(300) });
    b.tracks.push(track("GND", Layer::BackCopper, Point::mm(12, 5), Point::new(Nm::um(10080), Nm::mm(2))));
    b
}

/// Routed, then broken on purpose: one thin track, one undersized via, one clearance miss near
/// a pad, one courtyard overlap at the board edge.
pub fn divider_broken() -> Board {
    let mut b = divider_routed();
    b.tracks[0].width = Nm::um(100); // TrackWidth
    b.vias[0] = Via { net: "GND".into(), at: Point::mm(12, 5), diameter: Nm::um(400), drill: Nm::um(200) }; // DrillSize + AnnularRing
    // GND stub from the via towards R2.1 (OUT), stopping 50 um short of its pad: Clearance.
    b.tracks.push(track("GND", Layer::FrontCopper, Point::mm(12, 5), Point::new(Nm::um(8975), Nm::mm(5))));
    b
}

/// R2 on the back side, rotated a quarter turn, with a VIN track on the back layer that lands
/// exactly on R2.1 (OUT): one short. Pins down the flip convention against the oracle.
pub fn flipped_short() -> Board {
    let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(20, 10)));
    b.place("R1", footprints::chip_0402(), Point::mm(5, 5), 0, false);
    b.place("R2", footprints::chip_0402(), Point::mm(9, 5), 1, true);
    b.place("J1", footprints::pin_header_1xn(3), Point::mm(5, 2), 0, false);
    // Our model puts R2.1 at (9, 5.5): local (-0.5, 0) mirrored in x then turned a quarter.
    b.tracks.push(track("VIN", Layer::BackCopper, Point::new(Nm::mm(9), Nm::um(5500)), Point::mm(5, 2)));
    b
}

/// The sensor node placed by hand on a 30 x 25 mm board: header on the left edge, button on the
/// right, module in the middle, regulator next to the header, sensor in a corner. The placer is
/// measured against this.
pub fn sensor_node_hand() -> Board {
    use crate::{esp32_c3_module_footprint, library};
    let lib = library();
    let fp = |id: &str| lib.get(id).expect("footprint in design library").clone();
    let mut b = Board::two_layer(Rect::from_points(Point::ORIGIN, Point::mm(30, 25)));
    b.place("J1", fp("Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical"), Point::mm(2, 8), 1, false);
    b.place("SW1", fp("Redrob_Button:SW_SMD_6x6_2pad"), Point::new(Nm::mm(26), Nm::um(12500)), 1, false);
    b.place("U1", esp32_c3_module_footprint(), Point::new(Nm::mm(14), Nm::um(12500)), 0, false);
    b.place("U3", fp("Package_TO_SOT_SMD:SOT-23-5"), Point::new(Nm::um(4500), Nm::mm(20)), 0, false);
    b.place("C1", fp("Resistor_SMD:R_0603_1608Metric"), Point::new(Nm::um(4500), Nm::mm(23)), 0, false);
    b.place("C2", fp("Resistor_SMD:R_0603_1608Metric"), Point::mm(23, 20), 0, false);
    b.place("U2", fp("Redrob_Sensor:AHT20_3x3mm"), Point::mm(24, 3), 0, false);
    b.place("C4", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(24, 6), 0, false);
    b.place("R3", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(26, 20), 0, false);
    b.place("R4", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(26, 22), 0, false);
    b.place("R1", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(23, 22), 0, false);
    b.place("R2", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(10, 23), 0, false);
    b.place("C3", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(14, 23), 0, false);
    b.place("C5", fp("Resistor_SMD:R_0402_1005Metric"), Point::mm(18, 23), 0, false);
    b
}

/// The sensor node placed by hand with a Power net class: 5V and 3V3 on 0.5 mm tracks with 0.2 mm
/// clearance and 0.8/0.4 mm vias. Routed by the oracle fixtures; both checkers must agree.
pub fn sensor_node_power_class() -> Board {
    let mut b = sensor_node_hand();
    b.netclasses.push(NetClass {
        name: "Power".into(),
        nets: vec!["5V".into(), "3V3".into()],
        track_width: Nm::um(500),
        clearance: Nm::um(200),
        via_diameter: Nm::um(800),
        via_drill: Nm::um(400),
    });
    b
}

/// The sensor node on four layers: the hand placement, In1.Cu a GND plane and In2.Cu a 3V3 plane
/// over the whole board (0.2 mm clearance, 0.2 mm minimum width). Signals route on the outer
/// layers and reach the planes through vias.
pub fn sensor_node_four_layer() -> Board {
    let hand = sensor_node_hand();
    let outline = Rect::from_points(Point::ORIGIN, Point::mm(30, 25));
    let mut b = Board::four_layer(outline);
    b.placements = hand.placements;
    let pour = board_geom::Polygon::rect(outline);
    b.zones.push(Zone {
        net: "GND".into(),
        layer: Layer::InnerCopper(1),
        outline: pour.clone(),
        clearance: Nm::um(200),
        min_width: Nm::um(200),
        fill: Vec::new(),
    });
    b.zones.push(Zone {
        net: "3V3".into(),
        layer: Layer::InnerCopper(2),
        outline: pour,
        clearance: Nm::um(200),
        min_width: Nm::um(200),
        fill: Vec::new(),
    });
    b
}

pub fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture { name: "divider_placed", netlist: divider_netlist(), board: divider_placed() },
        Fixture { name: "divider_routed", netlist: divider_netlist(), board: divider_routed() },
        Fixture { name: "divider_broken", netlist: divider_netlist(), board: divider_broken() },
        Fixture { name: "flipped_short", netlist: divider_netlist(), board: flipped_short() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_drc::{Rule, Rules, check, summary};

    #[test]
    fn fixture_verdicts_are_as_labelled() {
        let rules = Rules::default();
        let placed = summary(&check(&divider_placed(), &divider_netlist(), &rules));
        assert_eq!(placed.get(&Rule::Unrouted), Some(&4));
        assert_eq!(placed.len(), 1);

        assert!(check(&divider_routed(), &divider_netlist(), &rules).is_empty());

        let broken = summary(&check(&divider_broken(), &divider_netlist(), &rules));
        assert_eq!(broken.get(&Rule::TrackWidth), Some(&1));
        assert_eq!(broken.get(&Rule::DrillSize), Some(&1));
        assert_eq!(broken.get(&Rule::AnnularRing), Some(&1));
        assert_eq!(broken.get(&Rule::Clearance), Some(&1), "{broken:?}");
        assert_eq!(broken.get(&Rule::Short), None);

        let hand = summary(&check(&sensor_node_hand(), &crate::sensor_node(), &rules));
        assert_eq!(hand.keys().collect::<Vec<_>>(), vec![&Rule::Unrouted], "{hand:?}");

        let flipped = summary(&check(&flipped_short(), &divider_netlist(), &rules));
        assert_eq!(flipped.get(&Rule::Short), Some(&1), "{flipped:?}");
        assert_eq!(flipped.get(&Rule::Unrouted), Some(&4));
        assert_eq!(flipped.len(), 2, "{flipped:?}");
    }
}
