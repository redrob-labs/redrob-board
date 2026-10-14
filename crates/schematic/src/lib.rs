//! A schematic is Rust code. Build parts and wire their pins; `build()` hands back the netlist.
//!
//! There is no schematic file format to parse. A drawn schematic, when one is wanted, is a view
//! rendered from the netlist, never the source of truth.
//!
//! ```
//! use board_schematic::Schematic;
//!
//! let mut sch = Schematic::new();
//! let r1 = sch.part("R1", "10k", "Resistor_SMD:R_0402_1005Metric", &[("1", "1"), ("2", "2")]);
//! let c1 = sch.part("C1", "100n", "Capacitor_SMD:C_0402_1005Metric", &[("1", "1"), ("2", "2")]);
//! sch.net("VCC", &[r1.pin("1"), c1.pin("1")]);
//! sch.net("GND", &[r1.pin("2"), c1.pin("2")]);
//! let netlist = sch.build().unwrap();
//! assert_eq!(netlist.nets.len(), 2);
//! ```

#![forbid(unsafe_code)]

use board_netlist::{Component, Net, Netlist, NetlistError, Pin, PinRef};

/// Handle to a part added to a schematic. Cheap to copy; borrows nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartRef {
    refdes: String,
}

impl PartRef {
    pub fn refdes(&self) -> &str {
        &self.refdes
    }

    /// Address one pin by its number.
    pub fn pin(&self, number: &str) -> PinRef {
        PinRef::new(self.refdes.clone(), number)
    }
}

#[derive(Debug, Default)]
pub struct Schematic {
    components: Vec<Component>,
    nets: Vec<Net>,
    anonymous: usize,
}

impl Schematic {
    pub fn new() -> Schematic {
        Schematic::default()
    }

    /// Add a part. `pins` is `(number, name)` pairs.
    pub fn part(&mut self, refdes: &str, value: &str, footprint: &str, pins: &[(&str, &str)]) -> PartRef {
        self.components.push(Component {
            refdes: refdes.to_string(),
            value: value.to_string(),
            footprint: footprint.to_string(),
            pins: pins.iter().map(|(n, name)| Pin::new(*n, *name)).collect(),
        });
        PartRef { refdes: refdes.to_string() }
    }

    /// Two-pin passive with pins "1" and "2".
    pub fn passive(&mut self, refdes: &str, value: &str, footprint: &str) -> PartRef {
        self.part(refdes, value, footprint, &[("1", "1"), ("2", "2")])
    }

    /// Join pins under a name. Calling twice with the same name extends that net.
    pub fn net(&mut self, name: &str, pins: &[PinRef]) {
        match self.nets.iter_mut().find(|n| n.name == name) {
            Some(n) => n.pins.extend(pins.iter().cloned()),
            None => self.nets.push(Net { name: name.to_string(), pins: pins.to_vec() }),
        }
    }

    /// Join pins under a generated name (`N$1`, `N$2`, ...).
    pub fn wire(&mut self, pins: &[PinRef]) -> String {
        self.anonymous += 1;
        let name = format!("N${}", self.anonymous);
        self.net(&name, pins);
        name
    }

    /// Validate and hand back the netlist. Every problem is returned, not just the first.
    pub fn build(self) -> Result<Netlist, Vec<NetlistError>> {
        let netlist = Netlist { components: self.components, nets: self.nets };
        let errors = netlist.validate();
        if errors.is_empty() { Ok(netlist) } else { Err(errors) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_builds() {
        let mut sch = Schematic::new();
        let r1 = sch.passive("R1", "10k", "Resistor_SMD:R_0402_1005Metric");
        let r2 = sch.passive("R2", "10k", "Resistor_SMD:R_0402_1005Metric");
        let j1 =
            sch.part("J1", "Conn_01x03", "Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical", &[("1", "VIN"), ("2", "OUT"), ("3", "GND")]);
        sch.net("VIN", &[j1.pin("1"), r1.pin("1")]);
        let mid = sch.wire(&[r1.pin("2"), r2.pin("1")]);
        sch.net(&mid, &[j1.pin("2")]);
        sch.net("GND", &[r2.pin("2"), j1.pin("3")]);
        let n = sch.build().unwrap();
        assert_eq!(n.components.len(), 3);
        assert_eq!(n.net("N$1").unwrap().pins.len(), 3);
        assert_eq!(n.net_of(&j1.pin("2")).unwrap().name, "N$1");
    }

    #[test]
    fn build_reports_errors() {
        let mut sch = Schematic::new();
        let r1 = sch.passive("R1", "10k", "R");
        sch.net("A", &[r1.pin("1"), r1.pin("3")]);
        let errs = sch.build().unwrap_err();
        assert_eq!(errs, vec![NetlistError::UnknownPin { net: "A".into(), refdes: "R1".into(), pin: "3".into() }]);
    }
}
