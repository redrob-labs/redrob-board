//! Netlist intermediate representation shared by every Redrob Board tool.
//!
//! A netlist is a list of components, each with numbered pins, and a list of nets, each naming the
//! pins it joins. Nothing geometric lives here; the layout crate attaches footprints and positions
//! by reference designator.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A pin on a component. `number` is what the footprint pad is labelled with (`"1"`, `"A3"`);
/// `name` is the datasheet function (`"VDD"`, `"GPIO4"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pin {
    pub number: String,
    pub name: String,
}

impl Pin {
    pub fn new(number: impl Into<String>, name: impl Into<String>) -> Pin {
        Pin { number: number.into(), name: name.into() }
    }
}

/// One placed part: a reference designator (`R1`, `U3`), its value (`10k`, `ESP32-C3-MINI-1`) and
/// the footprint library id it needs (`Resistor_SMD:R_0402_1005Metric`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub refdes: String,
    pub value: String,
    pub footprint: String,
    pub pins: Vec<Pin>,
}

impl Component {
    pub fn pin(&self, number: &str) -> Option<&Pin> {
        self.pins.iter().find(|p| p.number == number)
    }
}

/// A (component, pin) address inside a net.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub refdes: String,
    pub pin: String,
}

impl PinRef {
    pub fn new(refdes: impl Into<String>, pin: impl Into<String>) -> PinRef {
        PinRef { refdes: refdes.into(), pin: pin.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Net {
    pub name: String,
    pub pins: Vec<PinRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Netlist {
    pub components: Vec<Component>,
    pub nets: Vec<Net>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NetlistError {
    #[error("duplicate reference designator {0}")]
    DuplicateRefdes(String),
    #[error("duplicate pin number {pin} on {refdes}")]
    DuplicatePin { refdes: String, pin: String },
    #[error("duplicate net name {0}")]
    DuplicateNet(String),
    #[error("net {net} refers to unknown component {refdes}")]
    UnknownComponent { net: String, refdes: String },
    #[error("net {net} refers to unknown pin {pin} on {refdes}")]
    UnknownPin { net: String, refdes: String, pin: String },
    #[error("pin {pin} on {refdes} is in two nets: {first} and {second}")]
    PinInTwoNets { refdes: String, pin: String, first: String, second: String },
    #[error("net {0} has fewer than two pins")]
    DanglingNet(String),
}

impl Netlist {
    pub fn component(&self, refdes: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.refdes == refdes)
    }

    pub fn net(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }

    /// Which net a pin belongs to, if any.
    pub fn net_of(&self, pin: &PinRef) -> Option<&Net> {
        self.nets.iter().find(|n| n.pins.contains(pin))
    }

    /// Every rule a netlist must satisfy before layout. Returns all problems, not just the first,
    /// so an agent can fix them in one pass.
    pub fn validate(&self) -> Vec<NetlistError> {
        let mut errors = Vec::new();
        let mut refdes_seen = BTreeSet::new();
        for c in &self.components {
            if !refdes_seen.insert(c.refdes.as_str()) {
                errors.push(NetlistError::DuplicateRefdes(c.refdes.clone()));
            }
            let mut pins_seen = BTreeSet::new();
            for p in &c.pins {
                if !pins_seen.insert(p.number.as_str()) {
                    errors.push(NetlistError::DuplicatePin { refdes: c.refdes.clone(), pin: p.number.clone() });
                }
            }
        }
        let mut net_seen = BTreeSet::new();
        let mut pin_owner: BTreeMap<&PinRef, &str> = BTreeMap::new();
        for n in &self.nets {
            if !net_seen.insert(n.name.as_str()) {
                errors.push(NetlistError::DuplicateNet(n.name.clone()));
            }
            if n.pins.len() < 2 {
                errors.push(NetlistError::DanglingNet(n.name.clone()));
            }
            for p in &n.pins {
                match self.component(&p.refdes) {
                    None => errors.push(NetlistError::UnknownComponent { net: n.name.clone(), refdes: p.refdes.clone() }),
                    Some(c) if c.pin(&p.pin).is_none() => {
                        errors.push(NetlistError::UnknownPin { net: n.name.clone(), refdes: p.refdes.clone(), pin: p.pin.clone() })
                    }
                    Some(_) => {}
                }
                if let Some(first) = pin_owner.insert(p, n.name.as_str())
                    && first != n.name
                {
                    errors.push(NetlistError::PinInTwoNets {
                        refdes: p.refdes.clone(),
                        pin: p.pin.clone(),
                        first: first.to_string(),
                        second: n.name.clone(),
                    });
                }
            }
        }
        errors
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("netlist is always serialisable")
    }

    pub fn from_json(s: &str) -> Result<Netlist, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_resistors() -> Netlist {
        let r = |refdes: &str| Component {
            refdes: refdes.into(),
            value: "10k".into(),
            footprint: "Resistor_SMD:R_0402_1005Metric".into(),
            pins: vec![Pin::new("1", "1"), Pin::new("2", "2")],
        };
        Netlist {
            components: vec![r("R1"), r("R2")],
            nets: vec![
                Net { name: "VCC".into(), pins: vec![PinRef::new("R1", "1"), PinRef::new("R2", "1")] },
                Net { name: "GND".into(), pins: vec![PinRef::new("R1", "2"), PinRef::new("R2", "2")] },
            ],
        }
    }

    #[test]
    fn valid_netlist_has_no_errors() {
        let n = two_resistors();
        assert!(n.validate().is_empty());
        assert_eq!(n.net_of(&PinRef::new("R2", "2")).map(|n| n.name.as_str()), Some("GND"));
    }

    #[test]
    fn json_roundtrip() {
        let n = two_resistors();
        assert_eq!(Netlist::from_json(&n.to_json()).unwrap(), n);
    }

    #[test]
    fn every_rule_reports() {
        let mut n = two_resistors();
        n.components.push(n.components[0].clone()); // duplicate R1
        n.components[1].pins.push(Pin::new("2", "dup")); // duplicate pin on R2
        n.nets.push(Net { name: "GND".into(), pins: vec![PinRef::new("R9", "1"), PinRef::new("R1", "7")] });
        n.nets.push(Net { name: "ALONE".into(), pins: vec![PinRef::new("R1", "1")] });
        let errors = n.validate();
        assert!(errors.contains(&NetlistError::DuplicateRefdes("R1".into())));
        assert!(errors.contains(&NetlistError::DuplicatePin { refdes: "R2".into(), pin: "2".into() }));
        assert!(errors.contains(&NetlistError::DuplicateNet("GND".into())));
        assert!(errors.contains(&NetlistError::UnknownComponent { net: "GND".into(), refdes: "R9".into() }));
        assert!(errors.contains(&NetlistError::UnknownPin { net: "GND".into(), refdes: "R1".into(), pin: "7".into() }));
        assert!(errors.contains(&NetlistError::DanglingNet("ALONE".into())));
        assert!(errors.contains(&NetlistError::PinInTwoNets { refdes: "R1".into(), pin: "1".into(), first: "VCC".into(), second: "ALONE".into() }));
    }
}
