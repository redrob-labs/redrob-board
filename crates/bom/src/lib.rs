//! Bill of materials and pick-and-place (CPL) files, in the column layout board houses accept.
//!
//! Both are plain CSV. The CPL gives each part's centre in millimetres (board origin at the
//! bottom-left, y up), its side and its rotation in degrees counter-clockwise, which is what the
//! assembly house's placement machine consumes. Values are the exact decimals of our nanometre
//! coordinates; no rounding happens here.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use board_layout::Board;
use board_netlist::Netlist;

/// One BOM line: identical parts grouped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BomLine {
    pub value: String,
    pub footprint: String,
    pub designators: Vec<String>,
}

pub fn bom(netlist: &Netlist) -> Vec<BomLine> {
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for c in &netlist.components {
        groups.entry((c.value.clone(), c.footprint.clone())).or_default().push(c.refdes.clone());
    }
    groups
        .into_iter()
        .map(|((value, footprint), mut designators)| {
            designators.sort_by_key(|d| natural(d));
            BomLine { value, footprint, designators }
        })
        .collect()
}

/// `Comment,Designator,Footprint,Quantity` -- the columns a board house's BOM upload wants.
pub fn bom_csv(netlist: &Netlist) -> String {
    let mut s = String::from("Comment,Designator,Footprint,Quantity\n");
    for line in bom(netlist) {
        writeln!(s, "{},{},{},{}", csv(&line.value), csv(&line.designators.join(" ")), csv(&line.footprint), line.designators.len()).unwrap();
    }
    s
}

/// `Designator,Mid X,Mid Y,Layer,Rotation` -- one row per placed part, millimetres and degrees.
pub fn cpl_csv(board: &Board) -> String {
    let mut s = String::from("Designator,Mid X,Mid Y,Layer,Rotation\n");
    let mut placements: Vec<_> = board.placements.iter().collect();
    placements.sort_by_key(|p| natural(&p.refdes));
    for p in placements {
        let side = if p.flipped { "Bottom" } else { "Top" };
        writeln!(s, "{},{}mm,{}mm,{},{}", csv(&p.refdes), p.at.x.mm_string(), p.at.y.mm_string(), side, p.quarter_turns as u32 * 90).unwrap();
    }
    s
}

fn csv(field: &str) -> String {
    if field.contains([',', '"', '\n']) { format!("\"{}\"", field.replace('"', "\"\"")) } else { field.to_string() }
}

/// Sort key so `R2` comes before `R10`: letters, then the number.
fn natural(refdes: &str) -> (String, u64, String) {
    let letters: String = refdes.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let rest = &refdes[letters.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (letters, digits.parse().unwrap_or(0), rest[digits.len()..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_designs::fixtures::sensor_node_hand;
    use board_designs::sensor_node;

    #[test]
    fn bom_groups_identical_parts() {
        let lines = bom(&sensor_node());
        let r10k = lines.iter().find(|l| l.value == "10k").unwrap();
        assert_eq!(r10k.designators, vec!["R1", "R2"]);
        let c100n = lines.iter().find(|l| l.value == "100n").unwrap();
        assert_eq!(c100n.designators, vec!["C3", "C4"]);
        let csv = bom_csv(&sensor_node());
        assert!(csv.starts_with("Comment,Designator,Footprint,Quantity\n"));
        assert!(csv.contains("10k,R1 R2,Resistor_SMD:R_0402_1005Metric,2\n"), "{csv}");
        assert_eq!(csv.lines().count(), 1 + lines.len());
    }

    #[test]
    fn cpl_lists_every_placement() {
        let csv = cpl_csv(&sensor_node_hand());
        assert_eq!(csv.lines().count(), 15);
        assert!(csv.contains("J1,2mm,8mm,Top,90\n"), "{csv}");
        assert!(csv.contains("U1,14mm,12.5mm,Top,0\n"));
        // Natural order: C1..C5 before R1..R4, R2 before R10 would too.
        let order: Vec<&str> = csv.lines().skip(1).map(|l| l.split(',').next().unwrap()).collect();
        assert_eq!(order[0], "C1");
        assert_eq!(order[5], "J1");
        assert_eq!(natural("R10").cmp(&natural("R2")), std::cmp::Ordering::Greater);
    }
}
