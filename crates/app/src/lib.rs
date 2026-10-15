//! Redrob Board desktop app (egui). One window: pick a design, place, route, check, export.
//! Every button calls the library the agent uses; there is nothing on screen that does not work.
//!
//! Layout, in the Redrob CAD chrome: a title row and an icon tool bar across the top, a Layers +
//! Parts palette on the left, Properties + Status + DRC on the right, the command log along the
//! bottom and a status bar under it; the board canvas (grid, outline, copper, DRC marks) fills
//! the middle.

#![forbid(unsafe_code)]

pub mod icons;
pub mod theme;

use std::path::PathBuf;

use board_agent::{drc_design, example_design, example_names, export_fab, place_design, route_design};
use board_designs::library;
use board_drc::{Rule, Violation};
use board_geom::{Point, Rect};
use board_layout::{Board, Layer};
use board_netlist::Netlist;
use board_place::Edge;
use board_units::Nm;
use egui::{Color32, Pos2, Sense, Stroke, Vec2, pos2, vec2};
use icons::Icon;
use theme::Tokens;

const TITLE_H: f32 = 26.0;
const TOOLBAR_H: f32 = 34.0;
const STATUS_H: f32 = 24.0;
const LOG_H: f32 = 110.0;
const PALETTE_W: f32 = 230.0;

/// Which drawing layers the canvas shows. Every toggle is a real filter on the painter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visible {
    pub grid: bool,
    pub courtyards: bool,
    pub copper_front: bool,
    pub copper_back: bool,
    pub vias: bool,
    pub drc: bool,
    pub refdes: bool,
}

impl Default for Visible {
    fn default() -> Self {
        Visible { grid: true, courtyards: true, copper_front: true, copper_back: true, vias: true, drc: true, refdes: true }
    }
}

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
    pub visible: Visible,
    /// Cursor position over the canvas, in board millimetres (status bar).
    cursor_mm: Option<(f64, f64)>,
    /// View: screen points per millimetre and the world point at the canvas centre.
    zoom: f32,
    center: Option<Pos2>,
    styled: bool,
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
            visible: Visible::default(),
            cursor_mm: None,
            zoom: 12.0,
            center: None,
            styled: false,
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

    pub fn zoom_extents(&mut self) {
        self.center = None;
    }

    pub fn ui(&mut self, root: &mut egui::Ui) {
        if !self.styled {
            theme::install_fonts(root.ctx());
            theme::apply(root.ctx());
            self.styled = true;
        }
        self.title_and_toolbar(root);
        self.status_bar(root);
        self.log_panel(root);
        self.left_palette(root);
        self.right_palette(root);
        let t = Tokens::get();
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(root, |ui| self.canvas(ui));
    }

    // ---- chrome -------------------------------------------------------------------------------

    fn title_and_toolbar(&mut self, root: &mut egui::Ui) {
        let t = Tokens::get();
        egui::Panel::top("rb_title_toolbar").exact_size(TITLE_H + TOOLBAR_H).frame(egui::Frame::NONE.fill(t.chrome)).show(root, |ui| {
            let r = ui.max_rect();
            let title = match &self.board {
                Some(b) => {
                    format!("Redrob Board      {}  ·  {} x {} mm  ·  {} parts", self.design, self.width_mm, self.height_mm, b.placements.len())
                }
                None => format!("Redrob Board      {}", self.design),
            };
            ui.painter().text(
                pos2(r.center().x, r.top() + TITLE_H / 2.0 + 1.0),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(13.0),
                t.text_dim,
            );
            ui.painter().hline(r.x_range(), r.top() + TITLE_H, Stroke::new(1.0, t.border));

            let row = egui::Rect::from_min_size(pos2(r.left(), r.top() + TITLE_H), vec2(r.width(), TOOLBAR_H));
            let mut x = r.left() + 12.0;
            let size = 26.0;
            let groups: &[&[(Icon, &str, &str)]] = &[
                &[(Icon::Load, "Load", "Load the design's netlist")],
                &[
                    (Icon::Place, "Place", "Place the parts on the outline"),
                    (Icon::Route, "Route", "Route every net"),
                    (Icon::Check, "Check", "Run the design rule check"),
                ],
                &[(Icon::Export, "Export", "Write the fabrication files")],
                &[(Icon::ZoomExtents, "Zoom Extents", "Fit the board in the view"), (Icon::Grid, "Grid", "Show or hide the 1 mm grid")],
            ];
            let mut clicked: Option<&str> = None;
            for group in groups {
                for (icon, label, tip) in group.iter() {
                    let br = egui::Rect::from_min_size(pos2(x, row.center().y - size / 2.0), vec2(size, size));
                    let resp = ui.interact(br, ui.id().with(("tb", *label)), Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *label));
                    let on = *icon == Icon::Grid && self.visible.grid;
                    if on {
                        ui.painter().rect_filled(br, 3.0, t.toggle_on.gamma_multiply(0.35));
                    } else if resp.hovered() {
                        ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
                    }
                    icons::paint(ui.painter(), br.shrink(4.0), *icon, false);
                    if resp.on_hover_text(*tip).clicked() {
                        clicked = Some(label);
                    }
                    x += size + 6.0;
                }
                ui.painter().vline(x + 5.0, (row.top() + 8.0)..=(row.bottom() - 8.0), Stroke::new(1.0, t.border));
                x += 18.0;
            }
            match clicked {
                Some("Load") => self.load(),
                Some("Place") => self.place(),
                Some("Route") => self.route(),
                Some("Check") => self.drc(),
                Some("Export") => self.export(),
                Some("Zoom Extents") => self.zoom_extents(),
                Some("Grid") => self.visible.grid = !self.visible.grid,
                _ => {}
            }
        });
    }

    fn status_bar(&mut self, root: &mut egui::Ui) {
        let t = Tokens::get();
        egui::Panel::bottom("rb_status").exact_size(STATUS_H).frame(egui::Frame::NONE.fill(t.chrome)).show(root, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, t.border));
            let p = ui.painter();
            let mut cells: Vec<(String, Color32)> = Vec::new();
            cells.push((
                match self.cursor_mm {
                    Some((x, y)) => format!("{x:>8.3}, {y:>8.3} mm"),
                    None => "   —   ,    —    mm".to_string(),
                },
                t.text,
            ));
            cells.push((format!("{:.0}%", self.zoom / 12.0 * 100.0), t.text_dim));
            match &self.board {
                Some(b) => cells.push((format!("{} parts · {} tracks · {} vias", b.placements.len(), b.tracks.len(), b.vias.len()), t.text_dim)),
                None => cells.push(("no board".into(), t.text_faint)),
            }
            if let Some(c) = self.route_complete {
                cells.push(if c { ("routing complete".into(), t.text) } else { ("routing incomplete".into(), t.drc_unrouted) });
            }
            if self.drc_run {
                cells.push(if self.violations.is_empty() {
                    ("DRC clean".into(), t.icon_accent)
                } else {
                    (format!("DRC: {} violations", self.violations.len()), t.drc_error)
                });
            }
            let mut x = r.left() + 10.0;
            for (i, (text, color)) in cells.iter().enumerate() {
                let font = if i == 0 { theme::mono() } else { theme::body() };
                let g = p.layout_no_wrap(text.clone(), font, *color);
                p.galley(pos2(x, r.center().y - g.size().y / 2.0), g.clone(), *color);
                x += g.size().x + 14.0;
                p.vline(x - 7.0, (r.top() + 6.0)..=(r.bottom() - 6.0), Stroke::new(1.0, t.border));
            }
            // Accessible mirror of the verdicts for screen readers and the headless test.
            if self.drc_run {
                let label = if self.violations.is_empty() { "DRC clean".to_string() } else { format!("DRC: {} violations", self.violations.len()) };
                ui.allocate_rect(egui::Rect::from_min_size(pos2(x, r.top()), vec2(1.0, 1.0)), Sense::hover())
                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone()));
            }
        });
    }

    fn log_panel(&mut self, root: &mut egui::Ui) {
        let t = Tokens::get();
        egui::Panel::bottom("rb_log").exact_size(LOG_H).frame(egui::Frame::NONE.fill(t.cmd_bg).inner_margin(egui::Margin::symmetric(10, 6))).show(
            root,
            |ui| {
                ui.painter().hline(ui.max_rect().x_range(), ui.max_rect().top() - 6.0, Stroke::new(1.0, t.border));
                egui::ScrollArea::vertical().id_salt("log").stick_to_bottom(true).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if self.log.is_empty() {
                        ui.label(
                            egui::RichText::new("Load a design, or press Place to run the whole pipeline on it.")
                                .font(theme::mono())
                                .color(t.text_faint),
                        );
                    }
                    for line in &self.log {
                        ui.label(egui::RichText::new(format!("> {line}")).font(theme::mono()).color(t.text));
                    }
                });
            },
        );
    }

    fn palette_header(ui: &mut egui::Ui, title: &str) {
        let t = Tokens::get();
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        ui.painter().rect_filled(r, 0.0, t.chrome_dark);
        ui.painter().text(pos2(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, title, theme::small(), t.text_dim);
    }

    fn left_palette(&mut self, root: &mut egui::Ui) {
        let t = Tokens::get();
        egui::Panel::left("rb_left").exact_size(PALETTE_W).frame(egui::Frame::NONE.fill(t.panel)).show(root, |ui| {
            ui.painter().vline(ui.max_rect().right(), ui.max_rect().y_range(), Stroke::new(1.0, t.border));
            Self::palette_header(ui, "LAYERS");
            let rows: [(&str, Color32, &mut bool); 6] = [
                ("F.Cu", t.copper_front, &mut self.visible.copper_front),
                ("B.Cu", t.copper_back, &mut self.visible.copper_back),
                ("Vias", t.via, &mut self.visible.vias),
                ("Courtyards", t.courtyard, &mut self.visible.courtyards),
                ("Reference designators", t.text, &mut self.visible.refdes),
                ("DRC markers", t.drc_error, &mut self.visible.drc),
            ];
            for (name, color, on) in rows {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, name));
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, t.control.gamma_multiply(0.5));
                }
                let eye = egui::Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(16.0, 16.0));
                icons::paint(ui.painter(), eye, if *on { Icon::Eye } else { Icon::EyeOff }, !*on);
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(pos2(r.left() + 36.0, r.center().y), vec2(12.0, 12.0)),
                    2.0,
                    if *on { color } else { color.gamma_multiply(0.3) },
                );
                ui.painter().text(
                    pos2(r.left() + 48.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    theme::body(),
                    if *on { t.text } else { t.text_faint },
                );
                if resp.clicked() {
                    *on = !*on;
                }
            }
            ui.add_space(6.0);
            Self::palette_header(ui, "PARTS");
            egui::ScrollArea::vertical().id_salt("parts").show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                match (&self.board, &self.netlist) {
                    (Some(b), _) => {
                        for p in &b.placements {
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                            ui.painter().text(pos2(r.left() + 10.0, r.center().y), egui::Align2::LEFT_CENTER, &p.refdes, theme::body(), t.text);
                            let layer = if p.flipped { "B" } else { "F" };
                            let fp = p.footprint.id.rsplit(':').next().unwrap_or(&p.footprint.id);
                            let g = ui.painter().layout_no_wrap(format!("{layer}  {fp}"), theme::small(), t.text_faint);
                            ui.painter().galley(pos2(r.left() + 50.0, r.center().y - g.size().y / 2.0), g, t.text_faint);
                        }
                    }
                    (None, Some(n)) => {
                        for c in &n.components {
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                            ui.painter().text(pos2(r.left() + 10.0, r.center().y), egui::Align2::LEFT_CENTER, &c.refdes, theme::body(), t.text);
                            ui.painter().text(pos2(r.left() + 50.0, r.center().y), egui::Align2::LEFT_CENTER, &c.value, theme::small(), t.text_faint);
                        }
                    }
                    (None, None) => {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("   no design loaded").font(theme::small()).color(t.text_faint));
                    }
                }
            });
        });
    }

    fn right_palette(&mut self, root: &mut egui::Ui) {
        let t = Tokens::get();
        egui::Panel::right("rb_right").exact_size(PALETTE_W).frame(egui::Frame::NONE.fill(t.panel)).show(root, |ui| {
            ui.painter().vline(ui.max_rect().left(), ui.max_rect().y_range(), Stroke::new(1.0, t.border));
            Self::palette_header(ui, "PROPERTIES");
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                egui::Grid::new("props").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                    ui.label(egui::RichText::new("Design").color(t.text_dim));
                    egui::ComboBox::from_id_salt("design").selected_text(self.design.clone()).width(120.0).show_ui(ui, |ui| {
                        for n in example_names() {
                            ui.selectable_value(&mut self.design, n.to_string(), *n);
                        }
                    });
                    ui.end_row();
                    ui.label(egui::RichText::new("Outline mm").color(t.text_dim));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.width_mm).range(5.0..=300.0).speed(0.5));
                        ui.label(egui::RichText::new("x").color(t.text_faint));
                        ui.add(egui::DragValue::new(&mut self.height_mm).range(5.0..=300.0).speed(0.5));
                    });
                    ui.end_row();
                    ui.label(egui::RichText::new("Edges").color(t.text_dim));
                    ui.label(
                        egui::RichText::new(self.edge_parts.iter().map(|(r, e)| format!("{r}={e:?}")).collect::<Vec<_>>().join(", "))
                            .font(theme::small()),
                    );
                    ui.end_row();
                    ui.label(egui::RichText::new("Export to").color(t.text_dim));
                    ui.add(egui::TextEdit::singleline(&mut self.export_dir).desired_width(120.0));
                    ui.end_row();
                });
            });
            Self::palette_header(ui, "STATUS");
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String, c: Color32| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(k).color(t.text_dim));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(v).color(c)));
                    });
                };
                match &self.netlist {
                    Some(n) => row(ui, "Netlist", format!("{} parts, {} nets", n.components.len(), n.nets.len()), t.text),
                    None => row(ui, "Netlist", "not loaded".into(), t.text_faint),
                }
                match &self.board {
                    Some(b) => row(ui, "Board", format!("{} tracks, {} vias", b.tracks.len(), b.vias.len()), t.text),
                    None => row(ui, "Board", "not placed".into(), t.text_faint),
                }
                match self.route_complete {
                    Some(true) => row(ui, "Routing", "complete".into(), t.icon_accent),
                    Some(false) => row(ui, "Routing", "incomplete".into(), t.drc_unrouted),
                    None => row(ui, "Routing", "—".into(), t.text_faint),
                }
                if self.drc_run {
                    if self.violations.is_empty() {
                        row(ui, "DRC", "clean".into(), t.icon_accent);
                    } else {
                        row(ui, "DRC", format!("{} violations", self.violations.len()), t.drc_error);
                    }
                } else {
                    row(ui, "DRC", "not run".into(), t.text_faint);
                }
            });
            Self::palette_header(ui, "DRC");
            egui::ScrollArea::vertical().id_salt("drc").show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.drc_run && self.violations.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("   no violations").font(theme::small()).color(t.text_faint));
                }
                for v in &self.violations {
                    let c = if v.rule == Rule::Unrouted { t.drc_unrouted } else { t.drc_error };
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                    ui.painter().circle_filled(pos2(r.left() + 12.0, r.center().y), 3.5, c);
                    let g = ui.painter().layout_no_wrap(v.to_string(), theme::small(), t.text);
                    ui.painter().galley(pos2(r.left() + 22.0, r.center().y - g.size().y / 2.0), g, t.text);
                }
            });
        });
    }

    // ---- canvas -------------------------------------------------------------------------------

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get();
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        let outline = Rect::from_points(Point::ORIGIN, Point::new(Nm::from_mm_f64(self.width_mm), Nm::from_mm_f64(self.height_mm)));
        if self.center.is_none() {
            let c = outline.center();
            self.center = Some(Pos2::new(c.x.to_mm_f64() as f32, c.y.to_mm_f64() as f32));
            let fit = (rect.width() / (self.width_mm as f32 * 1.15)).min(rect.height() / (self.height_mm as f32 * 1.15));
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
        let zoom = self.zoom;
        let to_screen = |p: Point| -> Pos2 {
            let x = (p.x.to_mm_f64() as f32 - center.x) * zoom + rect.center().x;
            let y = rect.center().y - (p.y.to_mm_f64() as f32 - center.y) * zoom;
            Pos2::new(x, y)
        };
        let to_world = |s: Pos2| -> (f64, f64) {
            let x = (s.x - rect.center().x) / zoom + center.x;
            let y = (rect.center().y - s.y) / zoom + center.y;
            (x as f64, y as f64)
        };
        let mm = |v: Nm| v.to_mm_f64() as f32 * zoom;
        self.cursor_mm = response.hover_pos().map(to_world);

        painter.rect_filled(rect, 0.0, t.canvas);
        if self.visible.grid {
            let (x0, y1) = to_world(rect.min);
            let (x1, y0) = to_world(rect.max);
            let step = if zoom >= 6.0 {
                1.0
            } else if zoom >= 1.5 {
                5.0
            } else {
                10.0
            };
            let mut gx = (x0 / step).floor() * step;
            while gx <= x1 {
                let major = (gx / (step * 10.0)).fract().abs() < 1e-6;
                let sx = to_screen(Point::new(Nm::from_mm_f64(gx), Nm::ZERO)).x;
                painter.vline(sx, rect.y_range(), Stroke::new(1.0, if major { t.grid_major } else { t.grid_minor }));
                gx += step;
            }
            let mut gy = (y0 / step).floor() * step;
            while gy <= y1 {
                let major = (gy / (step * 10.0)).fract().abs() < 1e-6;
                let sy = to_screen(Point::new(Nm::ZERO, Nm::from_mm_f64(gy))).y;
                painter.hline(rect.x_range(), sy, Stroke::new(1.0, if major { t.grid_major } else { t.grid_minor }));
                gy += step;
            }
            let o = to_screen(Point::ORIGIN);
            painter.hline(rect.x_range(), o.y, Stroke::new(1.0, t.axis_x));
            painter.vline(o.x, rect.y_range(), Stroke::new(1.0, t.axis_y));
        }
        let Some(board) = &self.board else {
            painter.rect_stroke(
                egui::Rect::from_two_pos(to_screen(outline.min), to_screen(outline.max)).translate(Vec2::ZERO),
                0.0,
                Stroke::new(1.0, t.edge_cuts.gamma_multiply(0.6)),
                egui::StrokeKind::Middle,
            );
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Place a design to see it here", egui::FontId::proportional(15.0), t.text_faint);
            return;
        };
        // Outline.
        let pts: Vec<Pos2> = board.outline.vertices.iter().map(|p| to_screen(*p)).collect();
        painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, t.edge_cuts)));
        // Courtyards and reference designators.
        for p in &board.placements {
            let y = p.board_courtyard();
            if self.visible.courtyards {
                painter.rect_stroke(
                    egui::Rect::from_two_pos(to_screen(y.min), to_screen(y.max)),
                    0.0,
                    Stroke::new(0.5, t.courtyard),
                    egui::StrokeKind::Middle,
                );
            }
            if self.visible.refdes {
                painter.text(
                    to_screen(y.center()),
                    egui::Align2::CENTER_CENTER,
                    &p.refdes,
                    egui::FontId::proportional(mm(Nm::mm(1)).clamp(8.0, 14.0)),
                    t.text,
                );
            }
        }
        // Back copper first so front draws on top.
        for layer in [Layer::BackCopper, Layer::FrontCopper] {
            let (color, on) =
                if layer == Layer::FrontCopper { (t.copper_front, self.visible.copper_front) } else { (t.copper_back, self.visible.copper_back) };
            if !on {
                continue;
            }
            for tr in board.tracks.iter().filter(|t| t.layer == layer) {
                painter.line_segment([to_screen(tr.segment.a), to_screen(tr.segment.b)], Stroke::new(mm(tr.width).max(1.0), color));
            }
            for pad in board.all_pads() {
                if pad.layers.contains(&layer) {
                    painter.rect_filled(egui::Rect::from_two_pos(to_screen(pad.bbox.min), to_screen(pad.bbox.max)), 0.0, color);
                }
            }
        }
        if self.visible.vias {
            for v in &board.vias {
                painter.circle_filled(to_screen(v.at), mm(v.diameter) / 2.0, t.via);
                painter.circle_filled(to_screen(v.at), mm(v.drill) / 2.0, t.canvas);
            }
        }
        if self.visible.drc {
            for v in &self.violations {
                let color = if v.rule == Rule::Unrouted { t.drc_unrouted } else { t.drc_error };
                painter.circle_stroke(to_screen(v.at), 6.0, Stroke::new(2.0, color));
            }
        }
    }
}

impl eframe::App for BoardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui(ui);
    }
}
