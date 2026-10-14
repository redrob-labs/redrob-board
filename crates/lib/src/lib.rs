//! Footprint library for Redrob Board.
//!
//! A `Library` maps ids such as `Resistor_SMD:R_0402_1005Metric` to footprints. Footprints come
//! from code (`board_layout::footprints`) or from KiCad `.kicad_mod` files read with our own
//! reader. The KiCad library data itself is not bundled here; point `load_pretty_dir` at a
//! `.pretty` directory you have.

#![forbid(unsafe_code)]

pub mod kicad_mod;
pub mod sexp;

use std::collections::BTreeMap;
use std::path::Path;

use board_layout::Footprint;
use thiserror::Error;

pub use kicad_mod::{KicadModError, parse_kicad_mod};

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: KicadModError,
    },
    #[error("directory name {0:?} does not end in .pretty")]
    NotAPrettyDir(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    footprints: BTreeMap<String, Footprint>,
}

impl Library {
    pub fn new() -> Library {
        Library::default()
    }

    /// The footprints every example needs, built in code.
    pub fn builtin() -> Library {
        use board_layout::footprints;
        let mut lib = Library::new();
        lib.insert(footprints::chip_0402());
        for n in 1..=8 {
            lib.insert(footprints::pin_header_1xn(n));
        }
        lib
    }

    pub fn insert(&mut self, fp: Footprint) {
        self.footprints.insert(fp.id.clone(), fp);
    }

    pub fn get(&self, id: &str) -> Option<&Footprint> {
        self.footprints.get(id)
    }

    pub fn len(&self) -> usize {
        self.footprints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.footprints.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.footprints.keys().map(String::as_str)
    }

    /// Read every `.kicad_mod` in a KiCad `Something.pretty` directory under the nickname
    /// `Something`. Files that fail to parse are returned as errors; the rest are still loaded.
    pub fn load_pretty_dir(&mut self, dir: &Path) -> Result<Vec<LibraryError>, LibraryError> {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let nickname = name.strip_suffix(".pretty").ok_or_else(|| LibraryError::NotAPrettyDir(name.to_string()))?.to_string();
        let io = |source| LibraryError::Io { path: dir.display().to_string(), source };
        let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(io)?.collect::<Result<_, _>>().map_err(io)?;
        entries.sort_by_key(|e| e.file_name());
        let mut errors = Vec::new();
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("kicad_mod") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    errors.push(LibraryError::Io { path: path.display().to_string(), source: e });
                    continue;
                }
            };
            match parse_kicad_mod(&nickname, &text) {
                Ok(fp) => self.insert(fp),
                Err(e) => errors.push(LibraryError::Parse { path: path.display().to_string(), source: e }),
            }
        }
        Ok(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_geom::{Point, Rect};
    use board_layout::{Layer, PadShape};
    use board_units::Nm;

    const R_0402: &str = r#"(footprint "R_0402_1005Metric" (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (layer "F.Cu")
  (descr "Resistor SMD 0402 (1005 Metric)")
  (tags "resistor")
  (attr smd)
  (fp_text reference "REF**" (at 0 -1.17) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (fp_line (start -0.93 -0.47) (end 0.93 -0.47) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (fp_line (start -0.93 0.47) (end -0.93 -0.47) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (fp_line (start 0.93 -0.47) (end 0.93 0.47) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (fp_line (start 0.93 0.47) (end -0.93 0.47) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
  (fp_line (start -0.107836 -0.36) (end 0.107836 -0.36) (stroke (width 0.1) (type solid)) (layer "F.SilkS"))
  (pad "1" smd roundrect (at -0.51 0) (size 0.54 0.64) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
  (pad "2" smd roundrect (at 0.51 0) (size 0.54 0.64) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25))
  (model "${KICAD8_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0402_1005Metric.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))
)"#;

    const HEADER_OLD: &str = r#"(module PinHeader_1x02_P2.54mm_Vertical (layer F.Cu) (tedit 59FED5CC)
  (descr "Through hole straight pin header")
  (fp_text reference REF** (at 0 -2.33) (layer F.SilkS) (effects (font (size 1 1) (thickness 0.15))))
  (fp_line (start -1.8 -1.8) (end -1.8 4.35) (layer F.CrtYd) (width 0.05))
  (fp_line (start 1.8 4.35) (end 1.8 -1.8) (layer F.CrtYd) (width 0.05))
  (pad 1 thru_hole rect (at 0 0) (size 1.7 1.7) (drill 1) (layers *.Cu *.Mask))
  (pad 2 thru_hole oval (at 0 2.54 90) (size 1.7 2.0) (drill 1) (layers *.Cu *.Mask))
  (pad "" np_thru_hole circle (at 5 0) (size 3 3) (drill 3) (layers *.Cu *.Mask))
)"#;

    #[test]
    fn reads_modern_smd_footprint() {
        let fp = parse_kicad_mod("Resistor_SMD", R_0402).unwrap();
        assert_eq!(fp.id, "Resistor_SMD:R_0402_1005Metric");
        assert_eq!(fp.pads.len(), 2);
        let p1 = &fp.pads[0];
        assert_eq!(p1.number, "1");
        assert_eq!(p1.at, Point::new(Nm::um(-510), Nm::ZERO));
        assert_eq!(p1.shape, PadShape::RoundRect { width: Nm::um(540), height: Nm::um(640), radius: Nm::um(135) });
        assert_eq!(p1.layers, vec![Layer::FrontCopper]);
        assert_eq!(p1.drill, None);
        assert_eq!(fp.courtyard, Rect::from_points(Point::new(Nm::um(-930), Nm::um(-470)), Point::new(Nm::um(930), Nm::um(470))));
    }

    #[test]
    fn reads_legacy_through_hole_footprint() {
        let fp = parse_kicad_mod("Connector_PinHeader_2.54mm", HEADER_OLD).unwrap();
        assert_eq!(fp.id, "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical");
        assert_eq!(fp.pads.len(), 3);
        assert_eq!(fp.pads[0].shape, PadShape::Rect { width: Nm::um(1700), height: Nm::um(1700) });
        assert_eq!(fp.pads[0].layers, vec![Layer::FrontCopper, Layer::BackCopper]);
        assert_eq!(fp.pads[0].drill, Some(Nm::mm(1)));
        // y flipped to y-up; 90-degree rotation swaps the oval's size.
        assert_eq!(fp.pads[1].at, Point::new(Nm::ZERO, Nm::um(-2540)));
        assert_eq!(fp.pads[1].shape, PadShape::Oval { width: Nm::mm(2), height: Nm::um(1700) });
        // Non-plated hole: drill, no copper.
        assert_eq!(fp.pads[2].layers, Vec::<Layer>::new());
        assert_eq!(fp.pads[2].drill, Some(Nm::mm(3)));
        assert_eq!(fp.courtyard, Rect::from_points(Point::new(Nm::um(-1800), Nm::um(-4350)), Point::new(Nm::um(1800), Nm::um(1800))));
    }

    #[test]
    fn courtyard_falls_back_to_pads() {
        let text = r#"(footprint "X" (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu")))"#;
        let fp = parse_kicad_mod("L", text).unwrap();
        assert_eq!(fp.courtyard, Rect::centered(Point::ORIGIN, Nm::um(1500), Nm::um(1500)));
    }

    #[test]
    fn rejects_what_it_cannot_represent() {
        let rot = r#"(footprint "X" (pad "1" smd rect (at 0 0 45) (size 1 1) (layers "F.Cu")))"#;
        assert!(matches!(parse_kicad_mod("L", rot), Err(KicadModError::Unsupported(_))));
        let shape = r#"(footprint "X" (pad "1" smd trapezoid (at 0 0) (size 1 1) (layers "F.Cu")))"#;
        assert!(matches!(parse_kicad_mod("L", shape), Err(KicadModError::Unsupported(_))));
        let two = r#"(footprint "X" (pad "1" smd rect (at 0 0) (size 1 1) (layers "*.Cu")))"#;
        assert!(matches!(parse_kicad_mod("L", two), Err(KicadModError::Pad { .. })));
        assert_eq!(parse_kicad_mod("L", "(kicad_pcb)"), Err(KicadModError::NotAFootprint(Some("kicad_pcb".into()))));
    }

    #[test]
    fn loads_a_pretty_directory() {
        let dir = std::env::temp_dir().join(format!("board-lib-test-{}.pretty", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("R_0402_1005Metric.kicad_mod"), R_0402).unwrap();
        std::fs::write(dir.join("broken.kicad_mod"), "(footprint").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let mut lib = Library::builtin();
        let before = lib.len();
        let errors = lib.load_pretty_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(lib.len(), before + 1);
        assert!(lib.get(&format!("board-lib-test-{}:R_0402_1005Metric", std::process::id())).is_some());
        assert!(matches!(lib.load_pretty_dir(Path::new("/nonexistent")), Err(LibraryError::NotAPrettyDir(_))));
    }
}
