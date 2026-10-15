//! Drives the real app headlessly: load, place, route, DRC through the buttons, then checks the
//! state those buttons produced. The snapshot test renders the frame with wgpu and compares it to
//! the committed image; it runs where a software Vulkan adapter is available (Linux CI).

use board_app::BoardApp;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness<'a>() -> Harness<'a, BoardApp> {
    // Through eframe's `App::ui`, margin-free like the shipped window, so the snapshot is the window.
    Harness::builder().with_size(egui::vec2(1100.0, 720.0)).build_eframe(|_cc| BoardApp::default().without_agent())
}

#[test]
fn buttons_drive_the_pipeline() {
    let mut h = harness();
    h.run();
    h.get_by_label("Place").click();
    h.run();
    assert_eq!(h.state().ws.board.as_ref().map(|b| b.placements.len()), Some(14));
    h.get_by_label("Route").click();
    h.run();
    assert_eq!(h.state().ws.route_complete, Some(true));
    h.get_by_label("Check").click();
    h.run();
    assert!(h.state().ws.drc_run);
    assert!(h.state().ws.violations.is_empty(), "{:?}", h.state().ws.violations);
    h.get_by_label("DRC clean");
    assert!(h.state().log.iter().any(|l| l.starts_with("routed:")));
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "needs a software Vulkan adapter")]
fn routed_sensor_node_snapshot() {
    let mut h = harness();
    h.run();
    h.get_by_label("Route").click();
    h.run();
    h.get_by_label("Check").click();
    h.run();
    h.snapshot("sensor_node_routed");
}

/// Stage 5's pass condition: one chat line turns into a fabrication ZIP. The engine is a scripted
/// stand-in (board_chat::fake_engine), the palette, the tools and the pipeline are the real ones.
#[test]
fn a_chat_line_becomes_a_fabrication_zip() {
    use board_chat::fake_engine::{serve, text_reply, tool_call_reply};
    let zip = std::env::temp_dir().join(format!("board-ui-{}.zip", std::process::id()));
    let zip_s = zip.display().to_string();
    let (base, server) = serve(vec![
        tool_call_reply(&[("c1", "write_schematic", r#"{"name":"온습도 노드","mcu":"esp32-c3-mini-1","sensors":["aht20"]}"#)]),
        tool_call_reply(&[(
            "c2",
            "place",
            r#"{"width_mm":30,"height_mm":25,"edge_parts":[{"refdes":"J1","edge":"left"},{"refdes":"SW1","edge":"right"}]}"#,
        )]),
        tool_call_reply(&[("c3", "route", "{}"), ("c4", "drc", "{}")]),
        tool_call_reply(&[("c5", "export_zip", &format!(r#"{{"path":{}}}"#, serde_json::to_string(&zip_s).unwrap()))]),
        text_reply("온습도 노드: 14부품, 배선 완료, DRC 0. ZIP을 썼습니다."),
    ]);
    let client = board_chat::ChatClient::new(base, vec![("Authorization", "Basic test".into()), ("x-redrob-app", "board".into())]);
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 720.0)).build_eframe(|_cc| BoardApp::default().with_engine(client));
    h.run();
    h.get_by_label("Ask the agent").focus();
    h.run();
    h.get_by_label("Ask the agent").type_text("ESP32 온습도 센서 노드 30x25, 커넥터 왼쪽");
    h.run();
    assert_eq!(h.state().chat.input, "ESP32 온습도 센서 노드 30x25, 커넥터 왼쪽");
    h.get_by_label("Send").click();
    // While the agent works the palette asks for repaints, so step frame by frame instead of run().
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        h.step();
        let done = h.state().chat.lines.iter().any(|l| matches!(l, board_app::ChatLine::Agent(t) if t.contains("ZIP")));
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let lines = &h.state().chat.lines;
    assert!(lines.iter().any(|l| matches!(l, board_app::ChatLine::User(t) if t.contains("온습도"))), "{lines:?}");
    assert!(lines.iter().any(|l| matches!(l, board_app::ChatLine::Tool(t) if t.starts_with("write_schematic("))), "{lines:?}");
    assert!(lines.iter().any(|l| matches!(l, board_app::ChatLine::Agent(t) if t.contains("ZIP"))), "{lines:?}");
    assert_eq!(h.state().ws.design, "온습도 노드");
    assert_eq!(h.state().ws.board.as_ref().map(|b| b.placements.len()), Some(14));
    assert_eq!(h.state().ws.route_complete, Some(true));
    assert!(h.state().ws.drc_run && h.state().ws.violations.is_empty());
    let bytes = std::fs::read(&zip).unwrap();
    let entries = board_agent::zipfile::read_zip(&bytes).unwrap();
    assert_eq!(entries.len(), 15);
    assert!(entries.iter().any(|(n, _)| n == "board-F_Cu.gtl"));
    std::fs::remove_file(&zip).unwrap();
    assert_eq!(server.join().unwrap().len(), 5);
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "needs a software Vulkan adapter")]
fn three_d_preview_snapshot() {
    let mut h = harness();
    h.run();
    h.get_by_label("Route").click();
    h.run();
    h.get_by_label("3D").click();
    h.run();
    assert!(h.state().three_d);
    h.snapshot("sensor_node_3d");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "needs a software Vulkan adapter")]
fn labs_order_palette_is_absent_until_switched_on() {
    let mut h = harness();
    h.run();
    assert!(h.query_by_label("Package ZIP").is_none(), "Labs off: nothing drawn");
    h.get_by_label("Route").click();
    h.run();
    h.state_mut().labs_order = true;
    h.state_mut().export_dir = std::env::temp_dir().join(format!("board-labs-{}", std::process::id())).display().to_string();
    h.run();
    h.get_by_label("Package ZIP").click();
    h.run();
    let sheet = h.state().order_sheet.clone().expect("order sheet after packaging");
    assert!(sheet.contains("Layers:          2"), "{sheet}");
    let dir = h.state().export_dir.clone();
    assert!(std::path::Path::new(&dir).join("sensor-node-generic.zip").exists());
    std::fs::remove_dir_all(&dir).ok();
}
