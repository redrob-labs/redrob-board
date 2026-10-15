//! Linear circuit simulation of a board: the DC operating point and the AC small-signal response
//! of the resistors, capacitors and sources a netlist describes, by modified nodal analysis.
//!
//! It answers the questions a power and bias check needs -- does every rail sit where it should,
//! are the pull-ups high, where is the corner of the RC on EN -- and nothing more: no transistor
//! models, no transient. ICs are open pins except where the catalogue gives a linear stand-in (an
//! LDO is an ideal source on its output). Every analysis also comes out as a SPICE deck, so the
//! CI oracle (ngspice) can check the numbers.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use board_netlist::Netlist;
use serde::{Deserialize, Serialize};

/// Ground: node 0 in every analysis.
pub const GND: &str = "GND";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Element {
    Resistor {
        name: String,
        a: String,
        b: String,
        ohms: f64,
    },
    Capacitor {
        name: String,
        a: String,
        b: String,
        farads: f64,
    },
    /// Ideal voltage source: `p` is `volts` above `n`.
    VSource {
        name: String,
        p: String,
        n: String,
        volts: f64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Circuit {
    pub elements: Vec<Element>,
}

/// Parse a component value with SI prefixes and the "4k7" style: `10k`, `4k7`, `100n`, `1u`,
/// `2.2M`, `0R1`, `3V3`.
pub fn parse_value(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let scale = |c: char| -> Option<f64> {
        Some(match c {
            'p' => 1e-12,
            'n' => 1e-9,
            'u' | 'µ' => 1e-6,
            'm' => 1e-3,
            'k' | 'K' => 1e3,
            'M' => 1e6,
            'G' => 1e9,
            'R' | 'V' | 'F' | 'r' => 1.0,
            _ => return None,
        })
    };
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    // One letter somewhere: "4k7" (letter as decimal point), "10k", "100n", "100nF".
    let chars: Vec<char> = t.chars().collect();
    let pos = chars.iter().position(|c| c.is_ascii_alphabetic() || *c == 'µ')?;
    let head: String = chars[..pos].iter().collect();
    let letter = chars[pos];
    let mut tail: String = chars[pos + 1..].iter().collect();
    // Trailing unit letters add nothing: 100nF, 10kΩ, 1uF.
    tail = tail.trim_end_matches(['F', 'f', 'Ω', 'H', 'h']).to_string();
    let mult = scale(letter)?;
    let head_v: f64 = if head.is_empty() { 0.0 } else { head.parse().ok()? };
    if tail.is_empty() {
        return Some(head_v * mult);
    }
    let tail_v: f64 = tail.parse().ok()?;
    let digits = tail.len() as i32;
    Some((head_v + tail_v / 10f64.powi(digits)) * mult)
}

/// A stand-in for an IC the linear solver can hold: today only an LDO as an ideal output source.
#[derive(Clone, Debug, PartialEq)]
pub enum Model {
    Ldo { vout: f64 },
}

/// The model the catalogue implies for a part value (`AP2112K-3.3` -> 3.3 V LDO).
pub fn model_for(value: &str) -> Option<Model> {
    let part = board_catalog::part_by_value(value)?;
    match part.role {
        board_catalog::Role::Ldo => {
            let v = value.rsplit('-').next()?.parse::<f64>().ok()?;
            Some(Model::Ldo { vout: v })
        }
        _ => None,
    }
}

impl Circuit {
    /// Build the linear circuit of a netlist. `sources` are `(net, volts)` ideal supplies to GND.
    /// Resistors and capacitors are parts whose refdes starts with R or C and whose value parses;
    /// an LDO becomes an ideal source on its VOUT pin's net.
    pub fn from_netlist(netlist: &Netlist, sources: &[(String, f64)]) -> Circuit {
        let mut c = Circuit::default();
        for (i, (net, v)) in sources.iter().enumerate() {
            c.elements.push(Element::VSource { name: format!("VSRC{}", i + 1), p: net.clone(), n: GND.into(), volts: *v });
        }
        for comp in &netlist.components {
            let net_of = |pin: &str| netlist.net_of(&board_netlist::PinRef::new(comp.refdes.clone(), pin)).map(|n| n.name.clone());
            let kind = comp.refdes.chars().next().unwrap_or(' ');
            if (kind == 'R' || kind == 'C') && comp.pins.len() == 2 {
                let (Some(a), Some(b)) = (net_of(&comp.pins[0].number), net_of(&comp.pins[1].number)) else { continue };
                let Some(v) = parse_value(&comp.value) else { continue };
                if kind == 'R' {
                    c.elements.push(Element::Resistor { name: comp.refdes.clone(), a, b, ohms: v });
                } else {
                    c.elements.push(Element::Capacitor { name: comp.refdes.clone(), a, b, farads: v });
                }
                continue;
            }
            if let Some(Model::Ldo { vout }) = model_for(&comp.value) {
                let out_pin = comp.pins.iter().find(|p| p.name == "VOUT" || p.name == "OUT").map(|p| p.number.clone());
                if let Some(net) = out_pin.and_then(|p| net_of(&p)) {
                    c.elements.push(Element::VSource { name: format!("V{}", comp.refdes), p: net, n: GND.into(), volts: vout });
                }
            }
        }
        c
    }

    fn nodes(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut push = |n: &str| {
            if n != GND && !names.iter().any(|x| x == n) {
                names.push(n.to_string());
            }
        };
        for e in &self.elements {
            match e {
                Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } => {
                    push(a);
                    push(b);
                }
                Element::VSource { p, n, .. } => {
                    push(p);
                    push(n);
                }
            }
        }
        names
    }

    /// DC operating point: node voltages (GND = 0, omitted). Capacitors are open; every node gets
    /// a 1 pS leak to ground so a floating node reads 0 instead of breaking the solve.
    pub fn dc(&self) -> Result<BTreeMap<String, f64>, SimError> {
        let sol = self.solve(None)?;
        Ok(sol.into_iter().map(|(n, v)| (n, v.re)).collect())
    }

    /// AC small-signal magnitude and phase (degrees) per node at `hz`, with every source at its
    /// DC value as the stimulus (a 1 V source gives the transfer to that source).
    pub fn ac(&self, hz: f64) -> Result<BTreeMap<String, (f64, f64)>, SimError> {
        let sol = self.solve(Some(hz))?;
        Ok(sol.into_iter().map(|(n, v)| (n, (v.abs(), v.im.atan2(v.re).to_degrees()))).collect())
    }

    /// Complex MNA: unknowns are node voltages then source currents.
    fn solve(&self, hz: Option<f64>) -> Result<Vec<(String, C64)>, SimError> {
        let nodes = self.nodes();
        let idx = |name: &str| -> Option<usize> { if name == GND { None } else { nodes.iter().position(|n| n == name) } };
        let nv = nodes.len();
        let ns = self.elements.iter().filter(|e| matches!(e, Element::VSource { .. })).count();
        let n = nv + ns;
        if n == 0 {
            return Ok(Vec::new());
        }
        let mut a = vec![vec![C64::ZERO; n]; n];
        let mut z = vec![C64::ZERO; n];
        let gmin = C64::real(1e-12);
        for (i, row) in a.iter_mut().enumerate().take(nv) {
            row[i] = row[i] + gmin;
        }
        let stamp = |a: &mut Vec<Vec<C64>>, i: Option<usize>, j: Option<usize>, y: C64| {
            if let Some(i) = i {
                a[i][i] = a[i][i] + y;
            }
            if let Some(j) = j {
                a[j][j] = a[j][j] + y;
            }
            if let (Some(i), Some(j)) = (i, j) {
                a[i][j] = a[i][j] - y;
                a[j][i] = a[j][i] - y;
            }
        };
        let mut si = 0;
        for e in &self.elements {
            match e {
                Element::Resistor { a: na, b: nb, ohms, .. } => {
                    if *ohms <= 0.0 {
                        return Err(SimError::BadValue(na.clone(), *ohms));
                    }
                    stamp(&mut a, idx(na), idx(nb), C64::real(1.0 / ohms));
                }
                Element::Capacitor { a: na, b: nb, farads, .. } => {
                    if let Some(f) = hz {
                        let w = 2.0 * std::f64::consts::PI * f;
                        stamp(&mut a, idx(na), idx(nb), C64 { re: 0.0, im: w * farads });
                    }
                }
                Element::VSource { p, n: nn, volts, .. } => {
                    let row = nv + si;
                    si += 1;
                    if let Some(i) = idx(p) {
                        a[i][row] = a[i][row] + C64::real(1.0);
                        a[row][i] = a[row][i] + C64::real(1.0);
                    }
                    if let Some(j) = idx(nn) {
                        a[j][row] = a[j][row] - C64::real(1.0);
                        a[row][j] = a[row][j] - C64::real(1.0);
                    }
                    z[row] = C64::real(*volts);
                }
            }
        }
        let x = gauss(a, z)?;
        Ok(nodes.into_iter().zip(x).collect())
    }

    /// A SPICE deck for the same circuit: `.op` and, when `ac_hz` are given, a single-frequency
    /// `.ac` per point, printing every node so the oracle script can read the values back.
    pub fn spice_deck(&self, title: &str, ac_hz: &[f64]) -> String {
        let mut s = format!("* {title}\n");
        let node = |n: &str| if n == GND { "0".to_string() } else { n.replace(['+', '-', '/', ' '], "_") };
        for e in &self.elements {
            match e {
                Element::Resistor { name, a, b, ohms } => s.push_str(&format!("{name} {} {} {ohms}\n", node(a), node(b))),
                Element::Capacitor { name, a, b, farads } => s.push_str(&format!("{name} {} {} {farads}\n", node(a), node(b))),
                Element::VSource { name, p, n, volts } => s.push_str(&format!("{name} {} {} DC {volts} AC {volts}\n", node(p), node(n))),
            }
        }
        let nodes = self.nodes();
        let printed: Vec<String> = nodes.iter().map(|n| format!("v({})", node(n))).collect();
        s.push_str(".control\nset filetype=ascii\nop\n");
        s.push_str(&format!("print {}\n", printed.join(" ")));
        for f in ac_hz {
            s.push_str(&format!("ac lin 1 {f} {f}\n"));
            let mags: Vec<String> = nodes.iter().map(|n| format!("vm({})", node(n))).collect();
            s.push_str(&format!("print {}\n", mags.join(" ")));
        }
        s.push_str("quit\n.endc\n.end\n");
        s
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SimError {
    Singular,
    BadValue(String, f64),
}

impl std::fmt::Display for SimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimError::Singular => write!(f, "the circuit matrix is singular (a floating source loop?)"),
            SimError::BadValue(n, v) => write!(f, "bad value {v} at {n}"),
        }
    }
}

impl std::error::Error for SimError {}

/// A complex number, enough for MNA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    const ZERO: C64 = C64 { re: 0.0, im: 0.0 };
    fn real(re: f64) -> C64 {
        C64 { re, im: 0.0 }
    }
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
}
impl std::ops::Add for C64 {
    type Output = C64;
    fn add(self, o: C64) -> C64 {
        C64 { re: self.re + o.re, im: self.im + o.im }
    }
}
impl std::ops::Sub for C64 {
    type Output = C64;
    fn sub(self, o: C64) -> C64 {
        C64 { re: self.re - o.re, im: self.im - o.im }
    }
}
impl std::ops::Mul for C64 {
    type Output = C64;
    fn mul(self, o: C64) -> C64 {
        C64 { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
}
impl std::ops::Div for C64 {
    type Output = C64;
    fn div(self, o: C64) -> C64 {
        let d = o.re * o.re + o.im * o.im;
        C64 { re: (self.re * o.re + self.im * o.im) / d, im: (self.im * o.re - self.re * o.im) / d }
    }
}

/// Gaussian elimination with partial pivoting.
fn gauss(mut a: Vec<Vec<C64>>, mut z: Vec<C64>) -> Result<Vec<C64>, SimError> {
    let n = z.len();
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs())).ok_or(SimError::Singular)?;
        if a[piv][col].abs() < 1e-18 {
            return Err(SimError::Singular);
        }
        a.swap(col, piv);
        z.swap(col, piv);
        for r in col + 1..n {
            let f = a[r][col] / a[col][col];
            if f.abs() == 0.0 {
                continue;
            }
            let pivot_row = a[col].clone();
            for (c, v) in a[r].iter_mut().enumerate().skip(col) {
                *v = *v - f * pivot_row[c];
            }
            z[r] = z[r] - f * z[col];
        }
    }
    let mut x = vec![C64::ZERO; n];
    for r in (0..n).rev() {
        let mut acc = z[r];
        for c in r + 1..n {
            acc = acc - a[r][c] * x[c];
        }
        x[r] = acc / a[r][r];
    }
    Ok(x)
}

/// A rail expectation: `net` within `tolerance` (fraction) of `volts`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RailCheck {
    pub net: String,
    pub volts: f64,
    pub tolerance: f64,
}

/// Compare an operating point with rail expectations; each line says what was measured.
pub fn check_rails(dc: &BTreeMap<String, f64>, checks: &[RailCheck]) -> Vec<(String, bool, String)> {
    checks
        .iter()
        .map(|c| {
            let v = if c.net == GND { 0.0 } else { dc.get(&c.net).copied().unwrap_or(f64::NAN) };
            let ok = v.is_finite() && (v - c.volts).abs() <= c.tolerance * c.volts.abs().max(1e-9);
            (c.net.clone(), ok, format!("{} = {v:.4} V (want {} +-{:.0}%)", c.net, c.volts, c.tolerance * 100.0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_designs::fixtures::divider_netlist;
    use board_designs::sensor_node;

    #[test]
    fn values_parse() {
        let close = |s: &str, want: f64| {
            let got = parse_value(s).unwrap_or(f64::NAN);
            assert!(((got - want) / want).abs() < 1e-9, "{s}: {got} vs {want}");
        };
        close("10k", 10_000.0);
        close("4k7", 4_700.0);
        close("100n", 1e-7);
        close("1u", 1e-6);
        close("10uF", 1e-5);
        close("2.2M", 2.2e6);
        close("0R1", 0.1);
        close("47", 47.0);
        assert_eq!(parse_value("ESP32-C3-MINI-1"), None);
    }

    #[test]
    fn divider_halves_the_input() {
        let c = Circuit::from_netlist(&divider_netlist(), &[("VIN".into(), 5.0)]);
        let dc = c.dc().unwrap();
        assert!((dc["VIN"] - 5.0).abs() < 1e-9);
        assert!((dc["OUT"] - 2.5).abs() < 1e-6, "{dc:?}");
        let deck = c.spice_deck("divider", &[]);
        assert!(deck.contains("R1 VIN OUT 10000"), "{deck}");
        assert!(deck.contains("VSRC1 VIN 0 DC 5"));
    }

    #[test]
    fn sensor_node_rails_and_pullups_sit_where_they_should() {
        let c = Circuit::from_netlist(&sensor_node(), &[("5V".into(), 5.0)]);
        let dc = c.dc().unwrap();
        let checks = vec![
            RailCheck { net: "5V".into(), volts: 5.0, tolerance: 0.01 },
            RailCheck { net: "3V3".into(), volts: 3.3, tolerance: 0.01 },
            RailCheck { net: "EN".into(), volts: 3.3, tolerance: 0.01 },
            RailCheck { net: "IO9_BOOT".into(), volts: 3.3, tolerance: 0.01 },
            RailCheck { net: "SDA".into(), volts: 3.3, tolerance: 0.01 },
        ];
        let r = check_rails(&dc, &checks);
        assert!(r.iter().all(|(_, ok, _)| *ok), "{r:?}");
        // Nets with only IC pins (TXD0/RXD0) have no linear element, so they are not nodes at all.
        assert!(!dc.contains_key("TXD0"));
        // Pins of other ICs on the rails sit on them: the capacitor-only decoupling nodes are the rails.
        assert_eq!(c.elements.iter().filter(|e| matches!(e, Element::Capacitor { .. })).count(), 5);
    }

    #[test]
    fn rc_corner_is_where_the_formula_says() {
        // 10k + 1u on EN: f_c = 1 / (2 pi R C) = 15.9 Hz; the capacitor node is -3 dB there.
        let c = Circuit {
            elements: vec![
                Element::VSource { name: "V1".into(), p: "IN".into(), n: GND.into(), volts: 1.0 },
                Element::Resistor { name: "R1".into(), a: "IN".into(), b: "OUT".into(), ohms: 10_000.0 },
                Element::Capacitor { name: "C1".into(), a: "OUT".into(), b: GND.into(), farads: 1e-6 },
            ],
        };
        let fc = 1.0 / (2.0 * std::f64::consts::PI * 10_000.0 * 1e-6);
        let ac = c.ac(fc).unwrap();
        let (mag, phase) = ac["OUT"];
        assert!((mag - 1.0 / 2f64.sqrt()).abs() < 1e-6, "{mag}");
        assert!((phase + 45.0).abs() < 1e-6, "{phase}");
        assert!((c.ac(fc * 100.0).unwrap()["OUT"].0 - 0.01).abs() < 1e-3);
        let deck = c.spice_deck("rc", &[fc]);
        assert!(deck.contains("ac lin 1"));
    }

    #[test]
    fn source_loops_are_reported() {
        let c = Circuit {
            elements: vec![
                Element::VSource { name: "V1".into(), p: "A".into(), n: GND.into(), volts: 1.0 },
                Element::VSource { name: "V2".into(), p: "A".into(), n: GND.into(), volts: 2.0 },
            ],
        };
        assert_eq!(c.dc(), Err(SimError::Singular));
    }
}
