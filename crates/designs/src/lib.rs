//! Reference designs: the boards Redrob Board is measured against.
//!
//! Each design is a function returning its netlist (and, once placed and routed in later stages,
//! its board). They double as the CI oracle fixtures: the same board is exported to the oracle's
//! format and both DRC counts must agree.

#![forbid(unsafe_code)]

pub mod fixtures;

use board_layout::footprints;
use board_lib::Library;
use board_netlist::Netlist;
use board_schematic::Schematic;

/// Every footprint id used by the designs below resolves in this library.
pub fn library() -> Library {
    let mut lib = Library::builtin();
    lib.insert(footprints::chip_0603());
    lib.insert(footprints::sot23_5());
    lib.insert(footprints::tactile_6x6_2pad());
    lib.insert(footprints::aht20_3x3());
    lib.insert(esp32_c3_module_footprint());
    lib
}

/// The ESP32-C3-MINI-1 land pattern from the Espressif datasheet (53 pads).
pub fn esp32_c3_module_footprint() -> board_layout::Footprint {
    footprints::esp32_c3_mini_1()
}

/// Smart-home sensor node: ESP32-C3 module, I2C temperature/humidity sensor, 3.3 V regulator,
/// a 4-pin power + UART header, reset button and strapping/pull-up resistors.
///
/// U1 pin numbers and names are the ESP32-C3-MINI-1 datasheet's (table "Pin Definitions"); U2's
/// are the AHT20 datasheet's (table 5). Both were checked against the vendors' own KiCad symbols.
pub fn sensor_node() -> Netlist {
    let mut sch = Schematic::new();
    let module = esp32_c3_module_footprint().id;

    let mut u1_pins: Vec<(String, String)> = vec![
        ("3".into(), "3V3".into()),
        ("5".into(), "IO2".into()),
        ("8".into(), "EN".into()),
        ("12".into(), "IO0".into()),
        ("13".into(), "IO1".into()),
        ("16".into(), "IO10".into()),
        ("22".into(), "IO8".into()),
        ("23".into(), "IO9".into()),
        ("30".into(), "RXD0".into()),
        ("31".into(), "TXD0".into()),
    ];
    for n in [1, 2, 11, 14].into_iter().chain(36..=53) {
        u1_pins.push((n.to_string(), "GND".into()));
    }
    let u1_pins: Vec<(&str, &str)> = u1_pins.iter().map(|(n, f)| (n.as_str(), f.as_str())).collect();
    let u1 = sch.part("U1", "ESP32-C3-MINI-1", &module, &u1_pins);
    let u2 =
        sch.part("U2", "AHT20", "Redrob_Sensor:AHT20_3x3mm", &[("1", "NC"), ("2", "VDD"), ("3", "SCL"), ("4", "SDA"), ("5", "GND"), ("6", "NC")]);
    let u3 = sch.part("U3", "AP2112K-3.3", "Package_TO_SOT_SMD:SOT-23-5", &[("1", "VIN"), ("2", "GND"), ("3", "EN"), ("4", "NC"), ("5", "VOUT")]);
    let j1 = sch.part(
        "J1",
        "Conn_01x04",
        "Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical",
        &[("1", "5V"), ("2", "GND"), ("3", "TX"), ("4", "RX")],
    );
    let sw1 = sch.passive("SW1", "RESET", "Redrob_Button:SW_SMD_6x6_2pad");
    let r0402 = "Resistor_SMD:R_0402_1005Metric";
    let c0603 = "Resistor_SMD:R_0603_1608Metric";
    let r1 = sch.passive("R1", "10k", r0402); // EN pull-up
    let r2 = sch.passive("R2", "10k", r0402); // IO9 (boot) pull-up
    let r3 = sch.passive("R3", "4k7", r0402); // SDA pull-up
    let r4 = sch.passive("R4", "4k7", r0402); // SCL pull-up
    let c1 = sch.passive("C1", "10u", c0603); // LDO in
    let c2 = sch.passive("C2", "10u", c0603); // LDO out
    let c3 = sch.passive("C3", "100n", r0402); // module decoupling
    let c4 = sch.passive("C4", "100n", r0402); // sensor decoupling
    let c5 = sch.passive("C5", "1u", r0402); // EN RC

    sch.net("5V", &[j1.pin("1"), u3.pin("1"), u3.pin("3"), c1.pin("1")]);
    sch.net("GND", &[j1.pin("2"), u3.pin("2"), c1.pin("2"), c2.pin("2"), c3.pin("2"), c4.pin("2"), c5.pin("2"), u2.pin("5"), sw1.pin("2")]);
    let u1_gnd: Vec<_> = [1, 2, 11, 14].into_iter().chain(36..=53).map(|n| u1.pin(&n.to_string())).collect();
    sch.net("GND", &u1_gnd);
    sch.net(
        "3V3",
        &[u3.pin("5"), c2.pin("1"), c3.pin("1"), c4.pin("1"), u1.pin("3"), u2.pin("2"), r1.pin("1"), r2.pin("1"), r3.pin("1"), r4.pin("1")],
    );
    sch.net("EN", &[u1.pin("8"), r1.pin("2"), c5.pin("1"), sw1.pin("1")]);
    sch.net("IO9_BOOT", &[u1.pin("23"), r2.pin("2")]);
    sch.net("SDA", &[u1.pin("22"), u2.pin("4"), r3.pin("2")]);
    sch.net("SCL", &[u1.pin("16"), u2.pin("3"), r4.pin("2")]);
    sch.net("TXD0", &[u1.pin("31"), j1.pin("4")]);
    sch.net("RXD0", &[u1.pin("30"), j1.pin("3")]);

    sch.build().expect("sensor node netlist is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_units::Nm;

    #[test]
    fn sensor_node_is_valid_and_resolvable() {
        let n = sensor_node();
        assert!(n.validate().is_empty());
        assert_eq!(n.components.len(), 14);
        assert_eq!(n.nets.len(), 9);
        let lib = library();
        for c in &n.components {
            assert!(lib.get(&c.footprint).is_some(), "{} footprint {} missing", c.refdes, c.footprint);
        }
        // Every pin named GND on the module is actually in GND.
        let u1 = n.component("U1").unwrap();
        for p in u1.pins.iter().filter(|p| p.name == "GND") {
            assert_eq!(n.net_of(&board_netlist::PinRef::new("U1", p.number.clone())).unwrap().name, "GND");
        }
        // 9 off-module pins + 22 module ground pins (1, 2, 11, 14, 36-53).
        assert_eq!(n.net("GND").unwrap().pins.len(), 31);
    }

    #[test]
    fn module_footprint_shape() {
        let fp = esp32_c3_module_footprint();
        assert_eq!(fp.pads.len(), 53);
        assert_eq!(fp.pads[0].number, "1");
        assert_eq!(fp.pads[52].number, "53");
        // Pin 1 top-left, 11 bottom-left, 12 first on the bottom edge, 25 bottom-right, 35 top-right,
        // 36 first on the top edge (right end), 49 the ground plate under the body centre.
        assert!(fp.pads[0].at.x < Nm::ZERO && fp.pads[0].at.y > Nm::ZERO);
        assert!(fp.pads[10].at.x < Nm::ZERO && fp.pads[10].at.y < Nm::ZERO);
        assert!(fp.pads[11].at.y < fp.pads[10].at.y && fp.pads[11].at.x < Nm::ZERO);
        assert!(fp.pads[24].at.x > Nm::ZERO && fp.pads[24].at.y < Nm::ZERO);
        assert!(fp.pads[34].at.x > Nm::ZERO && fp.pads[34].at.y > Nm::ZERO);
        assert!(fp.pads[35].at.y > fp.pads[34].at.y && fp.pads[35].at.x > Nm::ZERO);
        assert_eq!(fp.pads[48].at, board_geom::Point::new(Nm::ZERO, -Nm::um(2700)));
        // Opposite edges are 11.8 mm apart (datasheet), pitch 0.8 mm.
        assert_eq!(fp.pads[24].at.x - fp.pads[0].at.x, Nm::um(11800));
        assert_eq!(fp.pads[0].at.y - fp.pads[1].at.y, Nm::um(800));
        // The lone footprint on a board passes clearance between its own pads.
        let mut b = board_layout::Board::two_layer(board_geom::Rect::from_points(board_geom::Point::ORIGIN, board_geom::Point::mm(30, 30)));
        b.place("U1", fp, board_geom::Point::mm(15, 15), 0, false);
        let v = board_drc::check(&b, &board_netlist::Netlist::default(), &board_drc::Rules::default());
        let v: Vec<_> = v.into_iter().filter(|v| v.rule != board_drc::Rule::UnknownRefdes).collect();
        assert!(v.is_empty(), "{v:#?}");
    }

    #[test]
    fn aht20_footprint_shape() {
        let fp = board_layout::footprints::aht20_3x3();
        assert_eq!(fp.pads.len(), 6);
        // Pin 1 top-left, 3 bottom-left, 4 bottom-right, 6 top-right; columns 2.0 mm apart, rows 1.0 mm.
        assert_eq!(fp.pads[0].at, board_geom::Point::new(-Nm::mm(1), Nm::mm(1)));
        assert_eq!(fp.pads[2].at, board_geom::Point::new(-Nm::mm(1), -Nm::mm(1)));
        assert_eq!(fp.pads[3].at, board_geom::Point::new(Nm::mm(1), -Nm::mm(1)));
        assert_eq!(fp.pads[5].at, board_geom::Point::new(Nm::mm(1), Nm::mm(1)));
    }
}
