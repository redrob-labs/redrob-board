//! Board houses: how each one wants the archive named, and the order sheet that goes with it.
//!
//! Neither JLCPCB nor PCBWay offers a public ordering API without a merchant account, so there is
//! no "order" call here: the package is what their upload forms take, and the checklist is what
//! their forms ask, read off the board so nothing is typed twice.

use board_layout::{Board, Layer};
use board_netlist::Netlist;
use board_units::Nm;

/// Where the archive is going. `Generic` keeps KiCad-style names, which every house accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FabHouse {
    #[default]
    Generic,
    Jlcpcb,
    Pcbway,
}

impl FabHouse {
    pub const ALL: [FabHouse; 3] = [FabHouse::Generic, FabHouse::Jlcpcb, FabHouse::Pcbway];

    pub fn parse(s: &str) -> Option<FabHouse> {
        match s.to_ascii_lowercase().as_str() {
            "generic" | "" => Some(FabHouse::Generic),
            "jlcpcb" | "jlc" => Some(FabHouse::Jlcpcb),
            "pcbway" => Some(FabHouse::Pcbway),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            FabHouse::Generic => "generic",
            FabHouse::Jlcpcb => "jlcpcb",
            FabHouse::Pcbway => "pcbway",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FabHouse::Generic => "Generic (KiCad names)",
            FabHouse::Jlcpcb => "JLCPCB",
            FabHouse::Pcbway => "PCBWay",
        }
    }

    /// The archive entry name for one of our generic file names. JLCPCB's reader recognises
    /// layers by its own names; PCBWay's recognises the KiCad names, so they stay.
    pub fn rename(self, generic: &str) -> String {
        if self != FabHouse::Jlcpcb {
            return generic.to_string();
        }
        let n = match generic {
            "board-F_Cu.gtl" => "Gerber_TopLayer.GTL",
            "board-B_Cu.gbl" => "Gerber_BottomLayer.GBL",
            "board-F_Mask.gts" => "Gerber_TopSolderMaskLayer.GTS",
            "board-B_Mask.gbs" => "Gerber_BottomSolderMaskLayer.GBS",
            "board-F_Paste.gtp" => "Gerber_TopPasteMaskLayer.GTP",
            "board-B_Paste.gbp" => "Gerber_BottomPasteMaskLayer.GBP",
            "board-F_Silkscreen.gto" => "Gerber_TopSilkscreenLayer.GTO",
            "board-B_Silkscreen.gbo" => "Gerber_BottomSilkscreenLayer.GBO",
            "board-Edge_Cuts.gm1" => "Gerber_BoardOutlineLayer.GKO",
            "board.drl" => "Drill_PTH_Through.DRL",
            "bom.csv" => "BOM.csv",
            "cpl.csv" => "CPL.csv",
            other => {
                // board-In{n}_Cu.g{n} -> Gerber_InnerLayer{n}.G{n}
                if let Some(rest) = other.strip_prefix("board-In")
                    && let Some((n, _)) = rest.split_once('_')
                {
                    return format!("Gerber_InnerLayer{n}.G{n}");
                }
                other
            }
        };
        n.to_string()
    }

    /// Where the archive is uploaded, one line.
    pub fn upload_hint(self) -> &'static str {
        match self {
            FabHouse::Generic => "Upload the ZIP to the board house's instant-quote form.",
            FabHouse::Jlcpcb => {
                "jlcpcb.com > Order now > Add gerber file: upload the ZIP; for assembly tick PCB Assembly and upload BOM.csv and CPL.csv when asked."
            }
            FabHouse::Pcbway => {
                "pcbway.com > PCB Instant Quote > Quick-order PCB: upload the ZIP; for assembly add the BOM and pick-and-place files."
            }
        }
    }
}

/// What the order form asks, filled from the board.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderSheet {
    pub house: FabHouse,
    pub layers: usize,
    pub size_mm: (f64, f64),
    pub thickness_mm: f64,
    pub copper_oz: u32,
    pub finish: &'static str,
    pub min_track_mm: f64,
    pub min_clearance_mm: f64,
    pub min_drill_mm: f64,
    pub smd_parts: usize,
    pub parts_in_stock: usize,
    pub parts_missing: Vec<String>,
}

impl OrderSheet {
    pub fn from_board(board: &Board, netlist: &Netlist, house: FabHouse, rules: &board_route::RouteRules) -> OrderSheet {
        let mm = |v: Nm| v.to_mm_f64();
        let bb = board.outline.bbox().unwrap_or_else(|| board_geom::Rect::from_points(board_geom::Point::ORIGIN, board_geom::Point::ORIGIN));
        let min_drill = board.vias.iter().map(|v| v.drill).min().map(mm).unwrap_or(0.0);
        let catalog = board_catalog::parts();
        let mut missing = Vec::new();
        let mut stocked = 0;
        for c in &netlist.components {
            match catalog.iter().find(|p| p.value == c.value && p.footprint == c.footprint).and_then(|p| p.lcsc) {
                Some(_) => stocked += 1,
                None => missing.push(c.refdes.clone()),
            }
        }
        OrderSheet {
            house,
            layers: board.copper_layers.iter().filter(|l| matches!(l, Layer::FrontCopper | Layer::BackCopper | Layer::InnerCopper(_))).count(),
            size_mm: (mm(bb.width()), mm(bb.height())),
            thickness_mm: 1.6,
            copper_oz: 1,
            finish: "HASL (lead-free)",
            min_track_mm: mm(rules.track_width),
            min_clearance_mm: mm(rules.clearance),
            min_drill_mm: min_drill,
            smd_parts: netlist.components.len(),
            parts_in_stock: stocked,
            parts_missing: missing,
        }
    }

    /// The sheet as the README.txt that rides inside the archive.
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Redrob Board order sheet -- {}\n\n", self.house.label()));
        s.push_str(&format!("Layers:          {}\n", self.layers));
        s.push_str(&format!("Size:            {:.2} x {:.2} mm\n", self.size_mm.0, self.size_mm.1));
        s.push_str(&format!("Thickness:       {:.1} mm\n", self.thickness_mm));
        s.push_str(&format!("Copper:          {} oz outer\n", self.copper_oz));
        s.push_str(&format!("Surface finish:  {}\n", self.finish));
        s.push_str(&format!("Min track:       {:.3} mm\n", self.min_track_mm));
        s.push_str(&format!("Min clearance:   {:.3} mm\n", self.min_clearance_mm));
        if self.min_drill_mm > 0.0 {
            s.push_str(&format!("Min drill:       {:.2} mm\n", self.min_drill_mm));
        }
        s.push_str(&format!("\nAssembly:        {} SMD parts, {} with an LCSC stock number\n", self.smd_parts, self.parts_in_stock));
        if !self.parts_missing.is_empty() {
            s.push_str(&format!("Not in catalogue: {}\n", self.parts_missing.join(" ")));
        }
        s.push_str(&format!("\nUpload: {}\n", self.house.upload_hint()));
        s.push_str("Ordering needs an account at the board house; this archive is what its form takes.\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jlcpcb_names() {
        let h = FabHouse::Jlcpcb;
        assert_eq!(h.rename("board-F_Cu.gtl"), "Gerber_TopLayer.GTL");
        assert_eq!(h.rename("board-In2_Cu.g2"), "Gerber_InnerLayer2.G2");
        assert_eq!(h.rename("board.drl"), "Drill_PTH_Through.DRL");
        assert_eq!(h.rename("board.json"), "board.json");
        assert_eq!(FabHouse::Pcbway.rename("board-F_Cu.gtl"), "board-F_Cu.gtl");
        assert_eq!(FabHouse::parse("JLC"), Some(FabHouse::Jlcpcb));
        assert_eq!(FabHouse::parse("osh"), None);
    }
}
