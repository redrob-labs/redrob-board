//! `redrob-board gerber-compare <ours> <theirs> [pitch_um]` rasterises two Gerber files (or two Excellon
//! files, when both end in `.drl`) and exits 0 only when they are pixel-for-pixel (hole-for-hole)
//! identical. Both files are read with this crate's own reader; nothing else is trusted.
//!
//! Silkscreen files (`*Silkscreen*`) are compared loosely: the two use different stroke fonts for
//! the same reference designators, so identity is impossible; instead both must have ink, their ink
//! bounding boxes must overlap by at least half (intersection over union), and the ink counts must
//! be within a factor of two. That catches a missing, misplaced or unmirrored label.

use std::process::ExitCode;

use board_gerber::{GerberFile, parse_drill};
use board_units::Nm;

pub fn main(args: &[String]) -> ExitCode {
    if args.len() < 2 || args.len() > 3 {
        eprintln!("usage: redrob-board gerber-compare <ours> <theirs> [pitch_um]");
        return ExitCode::from(2);
    }
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let run = || -> Result<Option<String>, String> {
        let (a, b) = (read(&args[0])?, read(&args[1])?);
        if args[0].ends_with(".drl") {
            let (ha, hb) = (parse_drill(&a).map_err(|e| e.to_string())?, parse_drill(&b).map_err(|e| e.to_string())?);
            return Ok(if ha == hb { None } else { Some(format!("drill: ours {} holes, theirs {} holes, sets differ", ha.len(), hb.len())) });
        }
        let pitch = Nm::um(args.get(2).and_then(|p| p.parse().ok()).unwrap_or(10));
        let (ga, gb) = (GerberFile::parse(&a).map_err(|e| e.to_string())?, GerberFile::parse(&b).map_err(|e| e.to_string())?);
        let area = match (ga.bbox(), gb.bbox()) {
            (Some(x), Some(y)) => x.union(&y),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => return Ok(None),
        };
        let (ra, rb) = (ga.rasterise(area, pitch), gb.rasterise(area, pitch));
        if args[0].contains("Silkscreen") {
            let (ba, bb) = (ga.bbox().ok_or("ours: no silk ink")?, gb.bbox().ok_or("theirs: no silk ink")?);
            let iw = (ba.max.x.min(bb.max.x).0 - ba.min.x.max(bb.min.x).0).max(0) as f64;
            let ih = (ba.max.y.min(bb.max.y).0 - ba.min.y.max(bb.min.y).0).max(0) as f64;
            let inter = iw * ih;
            let union = ba.width().0 as f64 * ba.height().0 as f64 + bb.width().0 as f64 * bb.height().0 as f64 - inter;
            let iou = if union > 0.0 { inter / union } else { 0.0 };
            let (ca, cb) = (ra.count() as f64, rb.count() as f64);
            let ratio = if cb > 0.0 { ca / cb } else { f64::INFINITY };
            return Ok(if iou >= 0.5 && (0.5..=2.0).contains(&ratio) {
                None
            } else {
                Some(format!("silk: bbox IoU {iou:.2}, ink ratio {ratio:.2} (ours {ca}, theirs {cb})"))
            });
        }
        let diff = ra.differing_pixels(&rb);
        Ok(if diff == 0 {
            None
        } else {
            Some(format!("{diff} of {} pixels differ (ours {} set, theirs {} set)", ra.bits.len(), ra.count(), rb.count()))
        })
    };
    match run() {
        Ok(None) => {
            println!("{}: {}", args[0], if args[0].contains("Silkscreen") { "matches (loose)" } else { "identical" });
            ExitCode::SUCCESS
        }
        Ok(Some(msg)) => {
            println!("{}: {msg}", args[0]);
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("redrob-board gerber-compare: {e}");
            ExitCode::from(2)
        }
    }
}
