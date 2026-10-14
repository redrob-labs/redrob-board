//! Redrob Board desktop app (egui). One window: pick a design, place, route, check, export.
//! Every button calls the library the agent uses; there is nothing on screen that does not work.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use board_agent::{drc_design, example_design, example_names, export_fab, place_design, route_design};
use board_designs::library;
use board_drc::{Rule, Violation};
use board_geom::{Point, Rect};
use board_layout::{Board, Layer};
use board_netlist::Netlist;
use board_place::Edge;
use board_units::Nm;
use egui::{Color32, Pos2, Sense, Stroke, Vec2};

pub struct BoardApp {
    pub design: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub edge_parts: Vec<(String, Edge)>,
    pub netlist: Option<Netlist>,
    pub board: Option<Board>,
    pub violations: Vec<Violation>,
    pub drc_run: bool,
    pub route_complete: Option<bool>,
    pub export_dir: String,
    pub log: Vec<String>,
    /// View: millimetres per screen point and the world point at the canvas centre.
    zoom: f32,
    center: Option<Pos2>,
}

impl Default for BoardApp {
    fn default() -> Self {
        BoardApp {
            design: "sensor-node".into(),
            width_mm: 30.0,
            height_mm: 25.0,
            edge_parts: vec![("J1".into(), Edge::Left), ("SW1".into(), Edge::Right)],
            netlist: None,
            board: None,
            violations: Vec::new(),
            drc_run: false,
            route_complete: None,
            export_dir: "fab".into(),
            log: Vec::new(),
            zoom: 12.0,
            center: None,
        }
    }
}

impl BoardApp {
    pub fn load(&mut self) {
        match example_design(&self.design) {
            Some(n) => {
                self.log.push(format!("loaded {}: {} parts, {} nets", self.design, n.components.len(), n.nets.len()));
                self.netlist = Some(n);
                self.board = None;
                self.violations.clear();
                self.drc_run = false;
                self.route_complete = None;
            }
            None => self.log.push(format!("no design named {}", self.design)),
        }
    }

    pub fn place(&mut self) {
        if self.netlist.is_none() {
            self.load();
        }
        let Some(netlist) = &self.netlist else { return };
        match place_design(netlist, &library(), self.width_mm, self.height_mm, &self.edge_parts, 1) {
            Ok(b) => {
                self.log.push(format!("placed {} parts on {} x {} mm", b.placements.len(), self.width_mm, self.height_mm));
                self.board = Some(b);
                self.violations.clear();
                self.drc_run = false;
                self.route_complete = None;
                self.center = None;
            }
            Err(e) => self.log.push(format!("place failed: {e}")),
        }
    }

    pub fn route(&mut self) {
        if self.board.is_none() {
            self.place();
        }
        let (Some(board), Some(netlist)) = (&mut self.board, &self.netlist) else { return };
        let r = route_design(board, netlist);
        self.route_complete = Some(r.is_complete());
        let verdict = if r.is_complete() { "complete".to_string() } else { format!("incomplete {:?}", r.failed) };
        self.log.push(format!("routed: {} tracks, {} vias, {verdict}", r.tracks, r.vias));
        self.drc_run = false;
    }

    pub fn drc(&mut self) {
        let (Some(board), Some(netlist)) = (&self.board, &self.netlist) else {
            self.log.push("nothing to check: place first".into());
            return;
        };
        self.violations = drc_design(board, netlist);
        self.drc_run = true;
        self.log.push(format!("drc: {} violations", self.violations.len()));
    }

    pub fn export(&mut self) {
        let (Some(board), Some(netlist)) = (&self.board, &self.netlist) else {
            self.log.push("nothing to export: place first".into());
            return;
        };
        match export_fab(board, netlist, &PathBuf::from(&self.export_dir)) {
            Ok(files) => self.log.push(format!("exported {} files to {}", files.len(), self.export_dir)),
            Err(e) => self.log.push(format!("export failed: {e}")),
        }
    }

    pub fn ui(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(root, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("design").selected_text(self.design.clone()).show_ui(ui, |ui| {
                    for n in example_names() {
                        ui.selectable_value(&mut self.design, n.to_string(), *n);
                    }
                });
                ui.label("outline mm");
                ui.add(egui::DragValue::new(&mut self.width_mm).range(5.0..=300.0).speed(0.5));
                ui.label("x");
                ui.add(egui::DragValue::new(&mut self.height_mm).range(5.0..=300.0).speed(0.5));
                ui.separator();
                if ui.button("Load").clicked() {
                    self.load();
                }
                if ui.button("Place").clicked() {
                    self.place();
                }
                if ui.button("Route").clicked() {
                    self.route();
                }
                if ui.button("Check").clicked() {
                    self.drc();
                }
                ui.text_edit_singleline(&mut self.export_dir);
                if ui.button("Export").clicked() {
                    self.export();
                }
            });
        });
        egui::Panel::right("status").default_size(280.0).show(root, |ui| {
            ui.heading("Status");
            match &self.board {
                None => ui.label("no board"),
                Some(b) => ui.label(format!("{} parts, {} tracks, {} vias", b.placements.len(), b.tracks.len(), b.vias.len())),
            };
            if let Some(c) = self.route_complete {
                ui.label(if c { "routing complete" } else { "routing incomplete" });
            }
            if self.drc_run {
                ui.label(if self.violations.is_empty() { "DRC clean".to_string() } else { format!("DRC: {} violations", self.violations.len()) });
            }
            ui.separator();
            ui.heading("DRC");
            egui::ScrollArea::vertical().id_salt("drc").max_height(220.0).show(ui, |ui| {
                for v in &self.violations {
                    ui.label(v.to_string());
                }
            });
            ui.separator();
            ui.heading("Log");
            egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
                for line in self.log.iter().rev() {
                    ui.label(line);
                }
            });
        });
        egui::CentralPanel::default().show(root, |ui| self.canvas(ui));
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        let outline = Rect::from_points(Point::ORIGIN, Point::new(Nm::from_mm_f64(self.width_mm), Nm::from_mm_f64(self.height_mm)));
        if self.center.is_none() {
            let c = outline.center();
            self.center = Some(Pos2::new(c.x.to_mm_f64() as f32, c.y.to_mm_f64() as f32));
            let fit = (rect.width() / (self.width_mm as f32 * 1.1)).min(rect.height() / (self.height_mm as f32 * 1.1));
            if fit.is_finite() && fit > 0.0 {
                self.zoom = fit;
            }
        }
        if response.dragged() {
            let d = response.drag_delta() / self.zoom;
            if let Some(c) = &mut self.center {
                c.x -= d.x;
                c.y += d.y;
            }
        }
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if response.hovered() && scroll != 0.0 {
            self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(0.5, 400.0);
        }
        let center = self.center.unwrap_or_default();
        let to_screen = |p: Point| -> Pos2 {
            let x = (p.x.to_mm_f64() as f32 - center.x) * self.zoom + rect.center().x;
            let y = rect.center().y - (p.y.to_mm_f64() as f32 - center.y) * self.zoom;
            Pos2::new(x, y)
        };
        let mm = |v: Nm| v.to_mm_f64() as f32 * self.zoom;

        painter.rect_filled(rect, 0.0, Color32::from_gray(24));
        let Some(board) = &self.board else {
            painter.rect_stroke(
                egui::Rect::from_min_max(to_screen(outline.max), to_screen(outline.min)).translate(Vec2::ZERO),
                0.0,
                Stroke::new(1.0, Color32::from_gray(120)),
                egui::StrokeKind::Middle,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Place a design to see it here",
                egui::FontId::proportional(16.0),
                Color32::from_gray(160),
            );
            return;
        };
        // Outline.
        let pts: Vec<Pos2> = board.outline.vertices.iter().map(|p| to_screen(*p)).collect();
        painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, Color32::from_rgb(230, 200, 80))));
        // Courtyards.
        for p in &board.placements {
            let y = p.board_courtyard();
            painter.rect_stroke(
                egui::Rect::from_two_pos(to_screen(y.min), to_screen(y.max)),
                0.0,
                Stroke::new(0.5, Color32::from_gray(90)),
                egui::StrokeKind::Middle,
            );
            painter.text(
                to_screen(y.center()),
                egui::Align2::CENTER_CENTER,
                &p.refdes,
                egui::FontId::proportional((mm(Nm::mm(1))).clamp(8.0, 14.0)),
                Color32::from_gray(200),
            );
        }
        // Back copper first so front draws on top.
        for layer in [Layer::BackCopper, Layer::FrontCopper] {
            let color = if layer == Layer::FrontCopper { Color32::from_rgb(200, 52, 52) } else { Color32::from_rgb(77, 127, 196) };
            for t in board.tracks.iter().filter(|t| t.layer == layer) {
                painter.line_segment([to_screen(t.segment.a), to_screen(t.segment.b)], Stroke::new(mm(t.width).max(1.0), color));
            }
            for pad in board.all_pads() {
                if pad.layers.contains(&layer) {
                    painter.rect_filled(egui::Rect::from_two_pos(to_screen(pad.bbox.min), to_screen(pad.bbox.max)), 0.0, color);
                }
            }
        }
        for v in &board.vias {
            painter.circle_filled(to_screen(v.at), mm(v.diameter) / 2.0, Color32::from_gray(180));
            painter.circle_filled(to_screen(v.at), mm(v.drill) / 2.0, Color32::from_gray(24));
        }
        for v in &self.violations {
            let color = if v.rule == Rule::Unrouted { Color32::from_rgb(255, 160, 0) } else { Color32::from_rgb(255, 60, 60) };
            painter.circle_stroke(to_screen(v.at), 6.0, Stroke::new(2.0, color));
        }
    }
}

impl eframe::App for BoardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui(ui);
    }
}
