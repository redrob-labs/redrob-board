//! `board-sim-fixtures <outdir>`: for every simulation fixture write `<name>.cir` (the SPICE deck
//! the oracle runs) and `<name>.ours.json` (our node voltages and AC magnitudes), so
//! `scripts/sim_oracle.py` can compare the two.

use std::process::ExitCode;

use board_designs::fixtures::divider_netlist;
use board_designs::sensor_node;
use board_sim::{Circuit, Element, GND};

fn fixtures() -> Vec<(&'static str, Circuit, Vec<f64>)> {
    let rc = Circuit {
        elements: vec![
            Element::VSource { name: "V1".into(), p: "IN".into(), n: GND.into(), volts: 1.0 },
            Element::Resistor { name: "R1".into(), a: "IN".into(), b: "OUT".into(), ohms: 10_000.0 },
            Element::Capacitor { name: "C1".into(), a: "OUT".into(), b: GND.into(), farads: 1e-6 },
        ],
    };
    vec![
        ("divider", Circuit::from_netlist(&divider_netlist(), &[("VIN".into(), 5.0)]), vec![]),
        ("rc_lowpass", rc, vec![1.0, 15.915494309189533, 1000.0]),
        ("sensor_node_dc", Circuit::from_netlist(&sensor_node(), &[("5V".into(), 5.0)]), vec![]),
    ]
}

fn main() -> ExitCode {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: board-sim-fixtures <outdir>");
        return ExitCode::from(2);
    };
    let out = std::path::Path::new(&out);
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("{}: {e}", out.display());
        return ExitCode::from(2);
    }
    for (name, circuit, ac) in fixtures() {
        let dc = match circuit.dc() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{name}: {e}");
                return ExitCode::from(1);
            }
        };
        let mut ours = serde_json::json!({"dc": dc});
        for f in &ac {
            let mags: std::collections::BTreeMap<String, f64> = circuit.ac(*f).unwrap_or_default().into_iter().map(|(n, (m, _))| (n, m)).collect();
            ours["ac"][format!("{f}")] = serde_json::json!(mags);
        }
        let deck = circuit.spice_deck(name, &ac);
        if let Err(e) = std::fs::write(out.join(format!("{name}.cir")), deck)
            .and_then(|_| std::fs::write(out.join(format!("{name}.ours.json")), ours.to_string()))
        {
            eprintln!("{name}: {e}");
            return ExitCode::from(2);
        }
        println!("{name}: {} nodes", dc.len());
    }
    ExitCode::SUCCESS
}
