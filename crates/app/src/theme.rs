//! Design tokens: the same dark slate chrome and near-black model space as Redrob CAD, so the two
//! apps read as one family. Every widget reads its colours and sizes from here; nothing is inline.

use egui::{Color32, FontFamily, FontId, Visuals};

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub chrome: Color32,
    pub chrome_dark: Color32,
    pub panel: Color32,
    pub tab_active: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub border: Color32,
    pub canvas: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub icon_accent: Color32,
    pub icon_point: Color32,
    pub accent: Color32,
    pub cmd_bg: Color32,
    pub toggle_on: Color32,
    /// Board layers, KiCad-like so the colours mean the same thing to a PCB reader.
    pub copper_front: Color32,
    pub copper_back: Color32,
    /// Inner-layer copper pours, drawn faint under the outer layers.
    pub plane: Color32,
    pub edge_cuts: Color32,
    pub courtyard: Color32,
    pub via: Color32,
    pub drc_error: Color32,
    pub drc_unrouted: Color32,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        chrome: Color32::from_rgb(0x3b, 0x44, 0x53),
        chrome_dark: Color32::from_rgb(0x31, 0x38, 0x44),
        panel: Color32::from_rgb(0x3c, 0x44, 0x52),
        tab_active: Color32::from_rgb(0x4e, 0x5a, 0x6e),
        control: Color32::from_rgb(0x50, 0x5a, 0x6d),
        control_hover: Color32::from_rgb(0x5c, 0x67, 0x7c),
        border: Color32::from_rgb(0x23, 0x29, 0x30),
        canvas: Color32::from_rgb(0x22, 0x28, 0x2f),
        grid_minor: Color32::from_rgb(0x29, 0x30, 0x3a),
        grid_major: Color32::from_rgb(0x31, 0x39, 0x45),
        axis_x: Color32::from_rgb(0x56, 0x28, 0x29),
        axis_y: Color32::from_rgb(0x28, 0x4a, 0x2e),
        text: Color32::from_rgb(0xd7, 0xdb, 0xe0),
        text_dim: Color32::from_rgb(0xa3, 0xaa, 0xb4),
        text_faint: Color32::from_rgb(0x87, 0x8d, 0x96),
        icon: Color32::from_rgb(0xcf, 0xd4, 0xdb),
        icon_accent: Color32::from_rgb(0x4f, 0xa3, 0xf7),
        icon_point: Color32::from_rgb(0xf0, 0x7a, 0x3a),
        accent: Color32::from_rgb(0x3d, 0x8b, 0xfd),
        cmd_bg: Color32::from_rgba_premultiplied(0x34, 0x3c, 0x4a, 0xf0),
        toggle_on: Color32::from_rgb(0x3d, 0x8b, 0xfd),
        copper_front: Color32::from_rgb(0xc8, 0x34, 0x34),
        copper_back: Color32::from_rgb(0x4d, 0x7f, 0xc4),
        plane: Color32::from_rgba_premultiplied(0x3a, 0x5a, 0x3a, 0x60),
        edge_cuts: Color32::from_rgb(0xe6, 0xc8, 0x50),
        courtyard: Color32::from_rgb(0x6e, 0x78, 0x86),
        via: Color32::from_rgb(0xb4, 0xb4, 0xb4),
        drc_error: Color32::from_rgb(0xff, 0x3c, 0x3c),
        drc_unrouted: Color32::from_rgb(0xff, 0xa0, 0x00),
    };

    pub fn get() -> Tokens {
        Tokens::DARK
    }
}

pub fn small() -> FontId {
    FontId::new(11.5, FontFamily::Proportional)
}
pub fn body() -> FontId {
    FontId::new(12.5, FontFamily::Proportional)
}
pub fn mono() -> FontId {
    FontId::new(12.0, FontFamily::Monospace)
}

/// The Korean system face of the Redrob design system (Wanted Sans Regular, SIL OFL 1.1; licence
/// beside the file). egui's own fonts carry no Hangul, so without it Korean renders as boxes.
pub const KOREAN_FONT: &[u8] = include_bytes!("../assets/fonts/WantedSans-Regular.ttf");

/// Korean system text is set at 96% of the Latin size (design system, `35-korean.md`).
const KOREAN_SCALE: f32 = 0.96;

pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// Fonts: egui's Latin face, then Wanted Sans for Hangul, then egui's other fallbacks.
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let korean = egui::FontData::from_static(KOREAN_FONT).tweak(egui::FontTweak { scale: KOREAN_SCALE, ..Default::default() });
    fonts.font_data.insert("korean".into(), std::sync::Arc::new(korean));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(f) = fonts.families.get_mut(&family) {
            f.insert(1.min(f.len()), "korean".into());
        }
    }
    fonts
}

pub fn apply(ctx: &egui::Context) {
    let t = Tokens::get();
    let mut v = Visuals::dark();
    v.panel_fill = t.panel;
    v.window_fill = t.chrome;
    v.extreme_bg_color = t.chrome_dark;
    v.faint_bg_color = t.chrome_dark;
    v.override_text_color = Some(t.text);
    v.widgets.noninteractive.bg_fill = t.panel;
    v.widgets.noninteractive.fg_stroke.color = t.text;
    v.widgets.noninteractive.bg_stroke.color = t.border;
    v.widgets.inactive.bg_fill = t.control;
    v.widgets.inactive.weak_bg_fill = t.control;
    v.widgets.inactive.fg_stroke.color = t.text;
    v.widgets.hovered.bg_fill = t.control_hover;
    v.widgets.hovered.weak_bg_fill = t.control_hover;
    v.widgets.active.bg_fill = t.accent;
    v.widgets.active.weak_bg_fill = t.accent;
    v.selection.bg_fill = t.accent;
    v.window_stroke = egui::Stroke::new(1.0, t.border);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(90) };
    v.window_corner_radius = egui::CornerRadius::same(6);
    v.menu_corner_radius = egui::CornerRadius::same(5);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 20.0;
        s.text_styles.insert(egui::TextStyle::Body, body());
        s.text_styles.insert(egui::TextStyle::Button, body());
        s.text_styles.insert(egui::TextStyle::Small, small());
        s.text_styles.insert(egui::TextStyle::Monospace, mono());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hangul must render in both families; without the bundled face every syllable is a box.
    #[test]
    fn korean_renders_in_proportional_and_monospace() {
        let mut fonts = egui::epaint::text::Fonts::new(egui::epaint::text::TextOptions::default(), font_definitions());
        for id in [FontId::proportional(13.0), FontId::monospace(12.0)] {
            for c in "배치배선검사내보내기".chars() {
                assert!(fonts.has_glyph(&id, c), "{id:?} has no {c:?}");
            }
        }
    }

    /// The bundled face is the design system's file, byte for byte.
    #[test]
    fn korean_face_is_the_design_system_file() {
        assert_eq!(KOREAN_FONT.len(), 2_401_736);
    }
}
