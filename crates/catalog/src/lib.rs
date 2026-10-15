//! The parts catalogue and the rules that turn a board specification into a netlist.
//!
//! A `Part` is what a purchasing agent needs to know: manufacturer part number, the footprint it
//! lands on, every pin with its function, the distributor id (LCSC, so a JLCPCB assembly order can
//! name it) and a reference unit price. A `Spec` is the few decisions a board needs -- which MCU,
//! which sensors, how it is powered, which human-facing extras -- and `write_schematic` wires them
//! into a netlist by the rules every such board follows (rails, decoupling per IC, an I2C bus with
//! one pair of pull-ups, a reset RC with its button, a boot pull-up, a power + UART header).
//!
//! Nothing here calls a distributor. Prices and ids are the catalogue's own records, dated below.

#![forbid(unsafe_code)]

use board_netlist::Netlist;
use board_schematic::{PartRef, Schematic};
use serde::{Deserialize, Serialize};

/// What a part does on a board; the rules key on this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Mcu,
    I2cSensor,
    Ldo,
    Header,
    Button,
    Resistor,
    Capacitor,
}

/// One catalogue record.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Part {
    /// Catalogue id, what a spec names (`esp32-c3-mini-1`, `aht20`, `r-0402-10k`).
    pub id: &'static str,
    pub manufacturer: &'static str,
    pub mpn: &'static str,
    /// The value written on the schematic (`10k`, `100n`, `ESP32-C3-MINI-1`).
    pub value: &'static str,
    pub footprint: &'static str,
    pub role: Role,
    /// `(pin number, function)`; a two-pin passive uses `("1", "1"), ("2", "2")`.
    pub pins: &'static [(&'static str, &'static str)],
    /// LCSC part number, the id JLCPCB assembly takes; `None` when the part is not stocked there.
    pub lcsc: Option<&'static str>,
    /// Reference unit price in USD at quantity 10, as recorded on `PRICES_AS_OF`.
    pub price_usd: f64,
}

/// The date the unit prices below were recorded. Prices move; the BOM states this date.
pub const PRICES_AS_OF: &str = "2026-10-15";

const R_0402: &str = "Resistor_SMD:R_0402_1005Metric";
const C_0603: &str = "Resistor_SMD:R_0603_1608Metric";
const TWO_PIN: &[(&str, &str)] = &[("1", "1"), ("2", "2")];

/// Every part this catalogue knows.
pub fn parts() -> Vec<Part> {
    vec![
        Part {
            id: "esp32-c3-mini-1",
            manufacturer: "Espressif",
            mpn: "ESP32-C3-MINI-1-N4",
            value: "ESP32-C3-MINI-1",
            footprint: "Redrob_RF_Module:ESP32-C3-MINI-1",
            role: Role::Mcu,
            pins: ESP32_C3_MINI_1_PINS,
            lcsc: Some("C2838500"),
            price_usd: 2.35,
        },
        Part {
            id: "aht20",
            manufacturer: "ASAIR",
            mpn: "AHT20",
            value: "AHT20",
            footprint: "Redrob_Sensor:AHT20_3x3mm",
            role: Role::I2cSensor,
            pins: &[("1", "NC"), ("2", "VDD"), ("3", "SCL"), ("4", "SDA"), ("5", "GND"), ("6", "NC")],
            lcsc: Some("C2757850"),
            price_usd: 1.05,
        },
        Part {
            id: "ap2112k-3.3",
            manufacturer: "Diodes Incorporated",
            mpn: "AP2112K-3.3TRG1",
            value: "AP2112K-3.3",
            footprint: "Package_TO_SOT_SMD:SOT-23-5",
            role: Role::Ldo,
            pins: &[("1", "VIN"), ("2", "GND"), ("3", "EN"), ("4", "NC"), ("5", "VOUT")],
            lcsc: Some("C51118"),
            price_usd: 0.14,
        },
        Part {
            id: "header-1x4",
            manufacturer: "generic",
            mpn: "PinHeader_1x04_P2.54mm_Vertical",
            value: "Conn_01x04",
            footprint: "Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical",
            role: Role::Header,
            pins: &[("1", "1"), ("2", "2"), ("3", "3"), ("4", "4")],
            lcsc: Some("C2337"),
            price_usd: 0.04,
        },
        Part {
            id: "tactile-6x6",
            manufacturer: "generic",
            mpn: "TS-6x6-2pad",
            value: "RESET",
            footprint: "Redrob_Button:SW_SMD_6x6_2pad",
            role: Role::Button,
            pins: TWO_PIN,
            lcsc: Some("C318884"),
            price_usd: 0.05,
        },
        Part {
            id: "r-0402-10k",
            manufacturer: "generic",
            mpn: "RC0402FR-0710KL",
            value: "10k",
            footprint: R_0402,
            role: Role::Resistor,
            pins: TWO_PIN,
            lcsc: Some("C25744"),
            price_usd: 0.002,
        },
        Part {
            id: "r-0402-4k7",
            manufacturer: "generic",
            mpn: "RC0402FR-074K7L",
            value: "4k7",
            footprint: R_0402,
            role: Role::Resistor,
            pins: TWO_PIN,
            lcsc: Some("C25900"),
            price_usd: 0.002,
        },
        Part {
            id: "c-0402-100n",
            manufacturer: "generic",
            mpn: "CL05B104KO5NNNC",
            value: "100n",
            footprint: R_0402,
            role: Role::Capacitor,
            pins: TWO_PIN,
            lcsc: Some("C1525"),
            price_usd: 0.003,
        },
        Part {
            id: "c-0402-1u",
            manufacturer: "generic",
            mpn: "CL05A105KA5NQNC",
            value: "1u",
            footprint: R_0402,
            role: Role::Capacitor,
            pins: TWO_PIN,
            lcsc: Some("C52923"),
            price_usd: 0.004,
        },
        Part {
            id: "c-0603-10u",
            manufacturer: "generic",
            mpn: "CL10A106KP8NNNC",
            value: "10u",
            footprint: C_0603,
            role: Role::Capacitor,
            pins: TWO_PIN,
            lcsc: Some("C19702"),
            price_usd: 0.01,
        },
    ]
}

/// ESP32-C3-MINI-1 pins, from the datasheet's pin definitions (functions this catalogue names).
const ESP32_C3_MINI_1_PINS: &[(&str, &str)] = &[
    ("1", "GND"),
    ("2", "GND"),
    ("3", "3V3"),
    ("5", "IO2"),
    ("6", "IO3"),
    ("8", "EN"),
    ("11", "GND"),
    ("12", "IO0"),
    ("13", "IO1"),
    ("14", "GND"),
    ("16", "IO10"),
    ("18", "IO4"),
    ("19", "IO5"),
    ("20", "IO6"),
    ("21", "IO7"),
    ("22", "IO8"),
    ("23", "IO9"),
    ("26", "IO18"),
    ("27", "IO19"),
    ("30", "RXD0"),
    ("31", "TXD0"),
    ("36", "GND"),
    ("37", "GND"),
    ("38", "GND"),
    ("39", "GND"),
    ("40", "GND"),
    ("41", "GND"),
    ("42", "GND"),
    ("43", "GND"),
    ("44", "GND"),
    ("45", "GND"),
    ("46", "GND"),
    ("47", "GND"),
    ("48", "GND"),
    ("49", "GND"),
    ("50", "GND"),
    ("51", "GND"),
    ("52", "GND"),
    ("53", "GND"),
];

pub fn part(id: &str) -> Option<Part> {
    parts().into_iter().find(|p| p.id == id)
}

/// Find a part by the value written on a schematic (`10k`, `AHT20`), for BOM lookups.
pub fn part_by_value(value: &str) -> Option<Part> {
    parts().into_iter().find(|p| p.value == value)
}

pub fn ids_with_role(role: Role) -> Vec<&'static str> {
    parts().into_iter().filter(|p| p.role == role).map(|p| p.id).collect()
}

/// How the board is powered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    /// 5 V in on the header, a 3.3 V LDO on the board.
    #[default]
    Usb5vLdo3v3,
    /// 3.3 V comes in regulated; no LDO.
    External3v3,
}

/// The decisions a board needs. Everything else follows from the rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    /// Catalogue id of an `Mcu` part.
    pub mcu: String,
    /// Catalogue ids of `I2cSensor` parts, in refdes order.
    #[serde(default)]
    pub sensors: Vec<String>,
    #[serde(default)]
    pub power: Power,
    /// A 1x4 header: power in, GND, TX, RX.
    #[serde(default = "yes")]
    pub uart_header: bool,
    /// Reset button on EN with its RC.
    #[serde(default = "yes")]
    pub reset_button: bool,
    /// Pull-up on the boot strapping pin (IO9 on the ESP32-C3).
    #[serde(default = "yes")]
    pub boot_pullup: bool,
}

fn yes() -> bool {
    true
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            mcu: "esp32-c3-mini-1".into(),
            sensors: vec!["aht20".into()],
            power: Power::default(),
            uart_header: true,
            reset_button: true,
            boot_pullup: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecError {
    UnknownPart(String),
    WrongRole { id: String, expected: Role, found: Role },
    McuLacks(&'static str),
    Netlist(Vec<String>),
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecError::UnknownPart(id) => write!(f, "no catalogue part `{id}`"),
            SpecError::WrongRole { id, expected, found } => write!(f, "`{id}` is a {found:?}, not a {expected:?}"),
            SpecError::McuLacks(what) => write!(f, "the MCU has no pin named {what}"),
            SpecError::Netlist(errs) => write!(f, "netlist invalid: {}", errs.join("; ")),
        }
    }
}

impl std::error::Error for SpecError {}

/// A refdes counter per prefix: R1, R2, ... C1, C2, ...
#[derive(Default)]
struct RefDes(std::collections::BTreeMap<&'static str, usize>);

impl RefDes {
    fn next(&mut self, prefix: &'static str) -> String {
        let n = self.0.entry(prefix).or_insert(0);
        *n += 1;
        format!("{prefix}{n}")
    }
}

fn add(sch: &mut Schematic, refs: &mut RefDes, prefix: &'static str, p: &Part) -> PartRef {
    let refdes = refs.next(prefix);
    sch.part(&refdes, p.value, p.footprint, p.pins)
}

fn resolve(id: &str, expected: Role) -> Result<Part, SpecError> {
    let p = part(id).ok_or_else(|| SpecError::UnknownPart(id.to_string()))?;
    if p.role != expected {
        return Err(SpecError::WrongRole { id: id.to_string(), expected, found: p.role });
    }
    Ok(p)
}

fn pins_named(p: &Part, name: &str) -> Vec<&'static str> {
    p.pins.iter().filter(|(_, f)| *f == name).map(|(n, _)| *n).collect()
}

fn pin_named(p: &Part, name: &'static str) -> Result<&'static str, SpecError> {
    p.pins.iter().find(|(_, f)| *f == name).map(|(n, _)| *n).ok_or(SpecError::McuLacks(name))
}

/// The rules. Pins the MCU uses for the bus and the console are the ESP32-C3 defaults the
/// reference sensor node shipped with: SDA on IO8, SCL on IO10, boot strap on IO9, U0 on RXD0/TXD0.
pub fn write_schematic(spec: &Spec) -> Result<Netlist, SpecError> {
    let mcu = resolve(&spec.mcu, Role::Mcu)?;
    let sensors = spec.sensors.iter().map(|s| resolve(s, Role::I2cSensor)).collect::<Result<Vec<_>, _>>()?;
    let ldo = resolve("ap2112k-3.3", Role::Ldo)?;
    let header = resolve("header-1x4", Role::Header)?;
    let button = resolve("tactile-6x6", Role::Button)?;
    let r10k = resolve("r-0402-10k", Role::Resistor)?;
    let r4k7 = resolve("r-0402-4k7", Role::Resistor)?;
    let c100n = resolve("c-0402-100n", Role::Capacitor)?;
    let c1u = resolve("c-0402-1u", Role::Capacitor)?;
    let c10u = resolve("c-0603-10u", Role::Capacitor)?;

    let mut sch = Schematic::new();
    let mut refs = RefDes::default();
    let u_mcu = add(&mut sch, &mut refs, "U", &mcu);
    let u_sensors: Vec<(PartRef, Part)> = sensors.iter().map(|p| (add(&mut sch, &mut refs, "U", p), p.clone())).collect();
    let u_ldo = if spec.power == Power::Usb5vLdo3v3 { Some(add(&mut sch, &mut refs, "U", &ldo)) } else { None };
    let j_hdr = if spec.uart_header { Some(add(&mut sch, &mut refs, "J", &header)) } else { None };
    let sw_reset = if spec.reset_button { Some(add(&mut sch, &mut refs, "SW", &button)) } else { None };

    let mut rail_3v3 = Vec::new();
    let mut gnd = Vec::new();
    let mut rail_in = Vec::new(); // 5V (LDO) or the header's 3V3 in

    // MCU rails.
    for n in pins_named(&mcu, "3V3") {
        rail_3v3.push(u_mcu.pin(n));
    }
    for n in pins_named(&mcu, "GND") {
        gnd.push(u_mcu.pin(n));
    }
    // Decoupling for every IC.
    let c = add(&mut sch, &mut refs, "C", &c100n); // MCU
    rail_3v3.push(c.pin("1"));
    gnd.push(c.pin("2"));

    // Reset: EN pull-up, RC, button.
    let en = pin_named(&mcu, "EN")?;
    let r_en = add(&mut sch, &mut refs, "R", &r10k);
    rail_3v3.push(r_en.pin("1"));
    let mut en_net = vec![u_mcu.pin(en), r_en.pin("2")];
    if spec.reset_button {
        let c_en = add(&mut sch, &mut refs, "C", &c1u);
        en_net.push(c_en.pin("1"));
        gnd.push(c_en.pin("2"));
        if let Some(sw) = &sw_reset {
            en_net.push(sw.pin("1"));
            gnd.push(sw.pin("2"));
        }
    }
    sch.net("EN", &en_net);

    // Boot strap pull-up.
    if spec.boot_pullup {
        let io9 = pin_named(&mcu, "IO9")?;
        let r = add(&mut sch, &mut refs, "R", &r10k);
        rail_3v3.push(r.pin("1"));
        sch.net("IO9_BOOT", &[u_mcu.pin(io9), r.pin("2")]);
    }

    // I2C bus: one pair of pull-ups, every sensor on it, 100n per sensor.
    if !u_sensors.is_empty() {
        let sda = pin_named(&mcu, "IO8")?;
        let scl = pin_named(&mcu, "IO10")?;
        let r_sda = add(&mut sch, &mut refs, "R", &r4k7);
        let r_scl = add(&mut sch, &mut refs, "R", &r4k7);
        rail_3v3.push(r_sda.pin("1"));
        rail_3v3.push(r_scl.pin("1"));
        let mut sda_net = vec![u_mcu.pin(sda), r_sda.pin("2")];
        let mut scl_net = vec![u_mcu.pin(scl), r_scl.pin("2")];
        for (u, p) in &u_sensors {
            for n in pins_named(p, "VDD") {
                rail_3v3.push(u.pin(n));
            }
            for n in pins_named(p, "GND") {
                gnd.push(u.pin(n));
            }
            for n in pins_named(p, "SDA") {
                sda_net.push(u.pin(n));
            }
            for n in pins_named(p, "SCL") {
                scl_net.push(u.pin(n));
            }
            let c = add(&mut sch, &mut refs, "C", &c100n);
            rail_3v3.push(c.pin("1"));
            gnd.push(c.pin("2"));
        }
        sch.net("SDA", &sda_net);
        sch.net("SCL", &scl_net);
    }

    // Power in.
    match (&u_ldo, &j_hdr) {
        (Some(u), hdr) => {
            // 5 V in -> LDO -> 3V3. EN tied to VIN; 10 u on each side.
            rail_in.push(u.pin("1"));
            rail_in.push(u.pin("3"));
            gnd.push(u.pin("2"));
            rail_3v3.push(u.pin("5"));
            let cin = add(&mut sch, &mut refs, "C", &c10u);
            let cout = add(&mut sch, &mut refs, "C", &c10u);
            rail_in.push(cin.pin("1"));
            gnd.push(cin.pin("2"));
            rail_3v3.push(cout.pin("1"));
            gnd.push(cout.pin("2"));
            if let Some(j) = hdr {
                rail_in.push(j.pin("1"));
            }
            sch.net("5V", &rail_in);
        }
        (None, Some(j)) => rail_3v3.push(j.pin("1")),
        (None, None) => {}
    }
    if let Some(j) = &j_hdr {
        gnd.push(j.pin("2"));
        let tx = pin_named(&mcu, "TXD0")?;
        let rx = pin_named(&mcu, "RXD0")?;
        sch.net("TXD0", &[u_mcu.pin(tx), j.pin("4")]);
        sch.net("RXD0", &[u_mcu.pin(rx), j.pin("3")]);
    }
    sch.net("3V3", &rail_3v3);
    sch.net("GND", &gnd);

    sch.build().map_err(|errs| SpecError::Netlist(errs.iter().map(|e| format!("{e:?}")).collect()))
}

/// One BOM line with the catalogue's purchasing data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BomLine {
    pub refdes: Vec<String>,
    pub value: String,
    pub footprint: String,
    pub manufacturer: String,
    pub mpn: String,
    pub lcsc: Option<String>,
    pub unit_price_usd: Option<f64>,
}

/// Group a netlist's parts by (value, footprint) and attach the catalogue record where one matches.
pub fn bom(netlist: &Netlist) -> Vec<BomLine> {
    let mut lines: Vec<BomLine> = Vec::new();
    for c in &netlist.components {
        if let Some(l) = lines.iter_mut().find(|l| l.value == c.value && l.footprint == c.footprint) {
            l.refdes.push(c.refdes.clone());
            continue;
        }
        let p = parts().into_iter().find(|p| p.value == c.value && p.footprint == c.footprint);
        let p = p.as_ref();
        lines.push(BomLine {
            refdes: vec![c.refdes.clone()],
            value: c.value.clone(),
            footprint: c.footprint.clone(),
            manufacturer: p.map(|p| p.manufacturer.to_string()).unwrap_or_default(),
            mpn: p.map(|p| p.mpn.to_string()).unwrap_or_default(),
            lcsc: p.and_then(|p| p.lcsc.map(str::to_string)),
            unit_price_usd: p.map(|p| p.price_usd),
        });
    }
    lines
}

/// Estimated parts cost of one board, from the catalogue's reference prices.
pub fn parts_cost_usd(netlist: &Netlist) -> f64 {
    bom(netlist).iter().map(|l| l.unit_price_usd.unwrap_or(0.0) * l.refdes.len() as f64).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_records_are_complete() {
        let ps = parts();
        assert!(ps.len() >= 10);
        for p in &ps {
            assert!(!p.mpn.is_empty() && !p.footprint.is_empty(), "{}", p.id);
            assert!(p.price_usd > 0.0, "{}", p.id);
            let mut nums: Vec<_> = p.pins.iter().map(|(n, _)| *n).collect();
            nums.sort_unstable();
            nums.dedup();
            assert_eq!(nums.len(), p.pins.len(), "{} has duplicate pin numbers", p.id);
        }
        assert_eq!(ids_with_role(Role::Mcu), vec!["esp32-c3-mini-1"]);
        assert_eq!(part_by_value("4k7").unwrap().id, "r-0402-4k7");
    }

    #[test]
    fn default_spec_reproduces_the_sensor_node_topology() {
        let n = write_schematic(&Spec::default()).unwrap();
        assert_eq!(n.components.len(), 14);
        assert_eq!(n.nets.len(), 9);
        let mut names: Vec<_> = n.nets.iter().map(|n| n.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["3V3", "5V", "EN", "GND", "IO9_BOOT", "RXD0", "SCL", "SDA", "TXD0"]);
        assert_eq!(n.net("GND").unwrap().pins.len(), 31);
        assert_eq!(n.net("SDA").unwrap().pins.len(), 3);
        // Every module pin named GND is in GND, and 3V3 reaches the MCU, the sensor and the pull-ups.
        let u1 = n.component("U1").unwrap();
        for p in u1.pins.iter().filter(|p| p.name == "GND") {
            assert_eq!(n.net_of(&board_netlist::PinRef::new("U1", p.number.clone())).unwrap().name, "GND");
        }
        assert_eq!(n.net("3V3").unwrap().pins.len(), 10);
        assert!(n.validate().is_empty());
    }

    #[test]
    fn spec_options_change_the_board() {
        let bare =
            Spec { sensors: vec![], power: Power::External3v3, uart_header: false, reset_button: false, boot_pullup: false, ..Spec::default() };
        let n = write_schematic(&bare).unwrap();
        // MCU + 100n + EN pull-up only.
        assert_eq!(n.components.len(), 3);
        assert_eq!(n.nets.len(), 3);
        let two = Spec { sensors: vec!["aht20".into(), "aht20".into()], ..Spec::default() };
        let n = write_schematic(&two).unwrap();
        assert_eq!(n.components.len(), 16); // one more sensor, one more 100n
        assert_eq!(n.net("SDA").unwrap().pins.len(), 4);
        assert_eq!(n.component("U2").unwrap().value, "AHT20");
        assert_eq!(n.component("U3").unwrap().value, "AHT20");
    }

    #[test]
    fn spec_errors_name_the_problem() {
        let e = write_schematic(&Spec { mcu: "nope".into(), ..Spec::default() }).unwrap_err();
        assert_eq!(e, SpecError::UnknownPart("nope".into()));
        let e = write_schematic(&Spec { sensors: vec!["r-0402-10k".into()], ..Spec::default() }).unwrap_err();
        assert!(matches!(e, SpecError::WrongRole { expected: Role::I2cSensor, found: Role::Resistor, .. }));
        let s: Spec = serde_json::from_str(r#"{"mcu":"esp32-c3-mini-1","sensors":["aht20"]}"#).unwrap();
        assert_eq!(s, Spec::default());
    }

    #[test]
    fn bom_carries_purchasing_data() {
        let n = write_schematic(&Spec::default()).unwrap();
        let b = bom(&n);
        let r10k = b.iter().find(|l| l.value == "10k").unwrap();
        assert_eq!(r10k.refdes.len(), 2);
        assert_eq!(r10k.lcsc.as_deref(), Some("C25744"));
        let mcu = b.iter().find(|l| l.value == "ESP32-C3-MINI-1").unwrap();
        assert_eq!(mcu.mpn, "ESP32-C3-MINI-1-N4");
        let cost = parts_cost_usd(&n);
        assert!(cost > 3.5 && cost < 4.0, "{cost}");
    }
}
