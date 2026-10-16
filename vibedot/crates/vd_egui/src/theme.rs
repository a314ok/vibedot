//! The Vibedot look: a calm dark theme with a violet → cyan accent,
//! generous spacing and rounded, flat widgets.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};

pub const BG_DEEP: Color32 = Color32::from_rgb(11, 11, 16);
pub const BG: Color32 = Color32::from_rgb(19, 19, 27);
pub const PANEL: Color32 = Color32::from_rgb(24, 24, 34);
pub const SURFACE: Color32 = Color32::from_rgb(33, 33, 46);
pub const SURFACE_HI: Color32 = Color32::from_rgb(44, 44, 61);
pub const BORDER: Color32 = Color32::from_rgb(52, 52, 72);
pub const TEXT: Color32 = Color32::from_rgb(226, 226, 238);
pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 140, 165);
pub const ACCENT: Color32 = Color32::from_rgb(139, 92, 246);
pub const ACCENT_2: Color32 = Color32::from_rgb(34, 211, 238);
pub const OK: Color32 = Color32::from_rgb(74, 222, 128);
pub const WARN: Color32 = Color32::from_rgb(251, 191, 36);
pub const ERR: Color32 = Color32::from_rgb(248, 113, 113);

/// Category colors for node types / blueprint pins.
pub fn category_color(category: &str) -> Color32 {
    match category {
        "2D" => Color32::from_rgb(96, 165, 250),
        "Rendering" => Color32::from_rgb(244, 114, 182),
        "Physics" => Color32::from_rgb(251, 146, 60),
        "Core" => Color32::from_rgb(163, 163, 190),
        "Events" => Color32::from_rgb(239, 68, 68),
        "Flow" => Color32::from_rgb(226, 232, 240),
        "Variables" => Color32::from_rgb(52, 211, 153),
        "Math" | "Vector" | "Logic" => Color32::from_rgb(132, 204, 22),
        "Node" | "Transform" => Color32::from_rgb(96, 165, 250),
        "Signals" => Color32::from_rgb(250, 204, 21),
        "Input" => Color32::from_rgb(192, 132, 252),
        "Game" => Color32::from_rgb(45, 212, 191),
        "Text" | "Literals" | "Color" | "Array" => Color32::from_rgb(148, 163, 184),
        _ => ACCENT,
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(8.0, 6.0);
        s.button_padding = egui::vec2(10.0, 5.0);
        s.window_margin = Margin::same(12);
        s.menu_margin = Margin::same(8);
        s.interact_size = egui::vec2(36.0, 24.0);
        s.indent = 16.0;
        s.slider_width = 140.0;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = PANEL;
        v.window_fill = BG;
        v.extreme_bg_color = BG_DEEP;
        v.faint_bg_color = SURFACE;
        v.code_bg_color = BG_DEEP;
        v.hyperlink_color = ACCENT_2;
        v.warn_fg_color = WARN;
        v.error_fg_color = ERR;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(140) };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(120) };
        v.selection.bg_fill = ACCENT.linear_multiply(0.55);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.striped = true;
        v.indent_has_left_vline = false;
        v.collapsing_header_frame = false;

        let radius = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = radius;
        w.inactive.bg_fill = SURFACE;
        w.inactive.weak_bg_fill = SURFACE;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = radius;
        w.hovered.bg_fill = SURFACE_HI;
        w.hovered.weak_bg_fill = SURFACE_HI;
        w.hovered.bg_stroke = Stroke::new(1.0, ACCENT.linear_multiply(0.7));
        w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.hovered.corner_radius = radius;
        w.active.bg_fill = ACCENT;
        w.active.weak_bg_fill = ACCENT;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_2);
        w.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
        w.active.corner_radius = radius;
        w.open.bg_fill = SURFACE_HI;
        w.open.weak_bg_fill = SURFACE_HI;
        w.open.corner_radius = radius;
    });
}

/// A pill-shaped accent button (primary actions like Play / Export).
pub fn accent_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, fill: Color32) -> egui::Response {
    let button = egui::Button::new(text.into().color(Color32::WHITE).strong()).fill(fill).corner_radius(CornerRadius::same(12));
    ui.add(button)
}

/// Section header used in docks.
pub fn section(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).color(ACCENT_2));
        ui.label(egui::RichText::new(title.to_uppercase()).small().strong().color(TEXT_DIM));
    });
}
