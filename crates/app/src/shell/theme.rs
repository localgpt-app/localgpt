//! The shell's colours: the page's (`crates/app/ui/src/theme.css`), so the
//! two interfaces read as one while both exist.

use eframe::egui::{self, Color32};

pub const PANEL: Color32 = Color32::from_rgb(0x0f, 0x12, 0x18);
pub const RAISED: Color32 = Color32::from_rgb(0x16, 0x1b, 0x25);
pub const LINE: Color32 = Color32::from_rgb(0x23, 0x29, 0x37);
pub const INK: Color32 = Color32::from_rgb(0xe6, 0xe9, 0xef);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x93, 0xa6);
pub const FAINT: Color32 = Color32::from_rgb(0x59, 0x61, 0x73);
pub const ACCENT: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xff);
pub const DANGER: Color32 = Color32::from_rgb(0xf7, 0x76, 0x8e);
/// An attached device's dot.
pub const ATTACHED: Color32 = Color32::from_rgb(0x9e, 0xce, 0x6a);

pub const SHOT: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xff);
pub const CAMERA: Color32 = Color32::from_rgb(0x7d, 0xcf, 0xff);
pub const SOUND: Color32 = Color32::from_rgb(0x9e, 0xce, 0x6a);
pub const MODULATION: Color32 = Color32::from_rgb(0xbb, 0x9a, 0xf7);
pub const BEHAVIOR: Color32 = Color32::from_rgb(0xe0, 0xaf, 0x68);
pub const AMBIENCE: Color32 = Color32::from_rgb(0x73, 0xda, 0xca);

/// A colour at a fraction of its opacity.
pub fn alpha(color: Color32, a: f32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// egui's dark look in the page's colours.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = RAISED;
    visuals.extreme_bg_color = Color32::from_rgb(0x0b, 0x0d, 0x12);
    visuals.faint_bg_color = RAISED;
    visuals.override_text_color = Some(INK);
    visuals.selection.bg_fill = alpha(ACCENT, 0.35);
    visuals.widgets.noninteractive.bg_stroke.color = LINE;
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);
}
