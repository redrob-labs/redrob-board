//! `board-gerber-compare <ours> <theirs> [pitch_um]` rasterises two Gerber files (or two Excellon
//! files, when both end in `.drl`) and exits 0 only when they are pixel-for-pixel (hole-for-hole)
//! identical. Both files are read with this crate's own reader; nothing else is trusted.

use std::process::ExitCode;

use board_gerber::{GerberFile, parse_drill};
use board_units::Nm;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!("usage: board-gerber-compare <ours> <theirs> [pitch_um]");
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
        let diff = ra.differing_pixels(&rb);
        Ok(if diff == 0 {
            None
        } else {
            Some(format!("{diff} of {} pixels differ (ours {} set, theirs {} set)", ra.bits.len(), ra.count(), rb.count()))
        })
    };
    match run() {
        Ok(None) => {
            println!("{}: identical", args[0]);
            ExitCode::SUCCESS
        }
        Ok(Some(msg)) => {
            println!("{}: {msg}", args[0]);
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("board-gerber-compare: {e}");
            ExitCode::from(2)
        }
    }
}
