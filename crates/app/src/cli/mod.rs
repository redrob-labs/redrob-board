//! The command line: `redrob-board <command>` works without opening the window.
//!
//! ```text
//! redrob-board pipeline --design NAME --outline WxH [--edge REF=left|right|top|bottom]... [--seed N] --out DIR
//! redrob-board tool '<json ToolCall>'                 one tool call, JSON result on stdout
//! redrob-board examples                               the example designs
//! redrob-board drc <board.json> <netlist.json> [rules.json]
//! redrob-board kicad-export <board.json> <netlist.json> <outdir> [rules.json]
//! redrob-board gerber-compare <ours> <theirs> [pitch_um]
//! ```
//!
//! Each command's exit code is its verdict (0 pass, 1 fail, 2 usage or read error).

mod drc;
mod gerber;
mod kicad;
mod pipeline;

use std::process::ExitCode;

const COMMANDS: [&str; 6] = ["pipeline", "tool", "examples", "drc", "kicad-export", "gerber-compare"];

/// Runs the command line when the first argument is a command; `None` opens the window.
pub fn dispatch(args: &[String]) -> Option<ExitCode> {
    let cmd = args.first()?.as_str();
    if !COMMANDS.contains(&cmd) && !matches!(cmd, "help" | "--help" | "-h") {
        return None;
    }
    attach_console();
    Some(match cmd {
        "pipeline" | "tool" | "examples" => pipeline::main(args),
        "drc" => drc::main(&args[1..]),
        "kicad-export" => kicad::main(&args[1..]),
        "gerber-compare" => gerber::main(&args[1..]),
        _ => {
            println!("usage: redrob-board {}", COMMANDS.join(" | "));
            println!("       redrob-board [--version]   (opens the window)");
            ExitCode::SUCCESS
        }
    })
}

/// The release build on Windows is a GUI program with no console of its own, so its output would
/// go nowhere; the command line writes to the console it was started from instead.
#[cfg(windows)]
fn attach_console() {
    let _ = winsafe::AttachConsole(winsafe::PidParent::Parent);
}

#[cfg(not(windows))]
fn attach_console() {}
