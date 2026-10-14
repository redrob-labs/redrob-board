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
use board_units::Nm;

/// Every footprint id used by the designs below resolves in this library.
pub fn library() -> Library {
    let mut lib = Library::builtin();
    lib.insert(footprints::chip_0603());
    lib.insert(footprints::sot23_5());
    lib.insert(footprints::tactile_6x6_2pad());
    lib.insert(esp32_c3_module_footprint());
    lib
}

/// Module outline used for the ESP32-C3 part: 13 pads per edge on three edges, 13.2 x 16.6 mm.
pub fn esp32_c3_module_footprint() -> board_layout::Footprint {
    footprints::castellated_module_3side(13, Nm::um(13200), Nm::um(16600))
}

/// Smart-home sensor node: ESP32-C3 module, I2C temperature/humidity sensor, 3.3 V regulator,
/// a 4-pin power + UART header, reset button and strapping/pull-up resistors.
///
/// Pin numbers on U1 follow the generic three-edge module footprint (see
/// `castellated_module_3side`); the names are the functions this design needs. Mapping those names
/// onto the real part's pins is the stage-6 datasheet check, done before anything is ordered.
pub fn sensor_node() -> Netlist {
    let mut sch = Schematic::new();
    let module = esp32_c3_module_footprint().id;

    let u1 = sch.part(
        "U1",
        "ESP32-C3-MINI-1",
        &module,
        &[
            ("1", "GND"),
            ("2", "GND"),
            ("3", "3V3"),
            ("5", "IO2"),
            ("8", "EN"),
            ("12", "IO0"),
            ("13", "IO1"),
            ("18", "IO10"),
            ("19", "IO8"),
            ("20", "IO9"),
            ("27", "RXD0"),
            ("29", "TXD0"),
            ("36", "GND"),
        ],
    );
    let u2 = sch.part(
        "U2",
        "AHT20",
        "Package_TO_SOT_SMD:SOT-23-5", // stand-in outline until the 3x3 DFN footprint lands in the library
        &[("1", "VDD"), ("2", "SCL"), ("3", "GND"), ("4", "SDA"), ("5", "NC")],
    );
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
    sch.net("GND", &[j1.pin("2"), u3.pin("2"), c1.pin("2"), c2.pin("2"), c3.pin("2"), c4.pin("2"), c5.pin("2"), u2.pin("3"), sw1.pin("2")]);
    sch.net("GND", &[u1.pin("1"), u1.pin("2"), u1.pin("36")]);
    sch.net(
        "3V3",
        &[u3.pin("5"), c2.pin("1"), c3.pin("1"), c4.pin("1"), u1.pin("3"), u2.pin("1"), r1.pin("1"), r2.pin("1"), r3.pin("1"), r4.pin("1")],
    );
    sch.net("EN", &[u1.pin("8"), r1.pin("2"), c5.pin("1"), sw1.pin("1")]);
    sch.net("IO9_BOOT", &[u1.pin("20"), r2.pin("2")]);
    sch.net("SDA", &[u1.pin("19"), u2.pin("4"), r3.pin("2")]);
    sch.net("SCL", &[u1.pin("18"), u2.pin("2"), r4.pin("2")]);
    sch.net("TXD0", &[u1.pin("29"), j1.pin("4")]);
    sch.net("RXD0", &[u1.pin("27"), j1.pin("3")]);

    sch.build().expect("sensor node netlist is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(n.net("GND").unwrap().pins.len(), 12);
    }

    #[test]
    fn module_footprint_shape() {
        let fp = esp32_c3_module_footprint();
        assert_eq!(fp.pads.len(), 39);
        assert_eq!(fp.pads[0].number, "1");
        assert_eq!(fp.pads[38].number, "39");
        // Pin 1 top-left, pin 13 bottom-left, pin 14 bottom-left corner on the bottom edge.
        assert!(fp.pads[0].at.x < Nm::ZERO && fp.pads[0].at.y > Nm::ZERO);
        assert!(fp.pads[12].at.x < Nm::ZERO && fp.pads[12].at.y < Nm::ZERO);
        assert!(fp.pads[13].at.y < fp.pads[12].at.y);
        assert!(fp.pads[38].at.x > Nm::ZERO && fp.pads[38].at.y > Nm::ZERO);
    }
}
