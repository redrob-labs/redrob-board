//! Drives the real app headlessly: load, place, route, DRC through the buttons, then checks the
//! state those buttons produced. The snapshot test renders the frame with wgpu and compares it to
//! the committed image; it runs where a software Vulkan adapter is available (Linux CI).

use board_app::BoardApp;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness<'a>() -> Harness<'a, BoardApp> {
    // Through eframe's `App::ui`, margin-free like the shipped window, so the snapshot is the window.
    Harness::builder().with_size(egui::vec2(1100.0, 720.0)).build_eframe(|_cc| BoardApp::default())
}

#[test]
fn buttons_drive_the_pipeline() {
    let mut h = harness();
    h.run();
    h.get_by_label("Place").click();
    h.run();
    assert_eq!(h.state().board.as_ref().map(|b| b.placements.len()), Some(14));
    h.get_by_label("Route").click();
    h.run();
    assert_eq!(h.state().route_complete, Some(true));
    h.get_by_label("Check").click();
    h.run();
    assert!(h.state().drc_run);
    assert!(h.state().violations.is_empty(), "{:?}", h.state().violations);
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
