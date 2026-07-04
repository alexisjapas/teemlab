//! Windowed-UI **theme**: the semantic color tokens and the global egui style.
//!
//! One place defines the UI's look. Before this module, the editor rode on egui's
//! default `Style` plus a font swap (cf. `fonts`), and every color was an ad-hoc
//! `Color32` literal scattered across the panels — the status amber alone was
//! re-hardcoded in four modules. Now: **semantic tokens** (named by role, not by
//! hue) and one global [`style`], applied once at startup by [`apply`] (called from
//! `fonts::setup_ui_fonts`, next to `ctx.set_fonts`).
//!
//! A module of the windowed *binary* only — the video `dataviz` keeps its own
//! palette (it renders through Bevy, not egui). Purely presentation: nothing here
//! touches the sim (DEV Rule 3).

use bevy_egui::egui;

/// Attention / pending state: the paused chip, the dirty document marker, a
/// breeding run in flight. The one amber — also fed to egui as `warn_fg_color`.
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(240, 180, 80);
/// A finished / successful state ("Done", a successful save).
pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(120, 200, 120);
/// Soft error ink (readable on the dark theme, less shouty than pure red) — also
/// fed to egui as `error_fg_color`.
pub const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 140, 120);

/// Perception-channel encodings of the inspector's bars: the **target** channel…
pub const TARGET: egui::Color32 = egui::Color32::from_rgb(220, 130, 40);
/// …and the **threat** channel.
pub const THREAT: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);

/// The ink ramp — the chrome grays of the dark theme, collapsing the per-module
/// one-off `from_gray` picks (18/25/36/80/90/130/140/165) to five steps: the darkest
/// **surface** (plot and graph backgrounds — also egui's `extreme_bg_color`)…
pub const SURFACE: egui::Color32 = egui::Color32::from_gray(18);
/// …the hairline **grid** of the plots…
pub const GRID: egui::Color32 = egui::Color32::from_gray(36);
/// …and three text inks: faint (hover cursors, structural strokes)…
pub const INK_FAINT: egui::Color32 = egui::Color32::from_gray(90);
/// …muted (axis ticks, secondary read-outs)…
pub const INK_MUTED: egui::Color32 = egui::Color32::from_gray(140);
/// …and full ink (primary painted text, e.g. the MLP graph's channel labels).
pub const INK: egui::Color32 = egui::Color32::from_gray(165);

/// MLP-graph encodings, one sign convention for nodes and edges: warm/orange =
/// positive, cold/blue = negative. A node's activation lerps from [`ACT_REST`]
/// toward [`ACT_WARM`] or [`ACT_COLD`]; an edge tints by its weight's sign.
pub const ACT_WARM: egui::Color32 = egui::Color32::from_rgb(240, 150, 40);
/// Cold pole of the activation scale (negative `tanh`).
pub const ACT_COLD: egui::Color32 = egui::Color32::from_rgb(60, 140, 240);
/// Resting node (activation ≈ 0) — the lerp's origin.
pub const ACT_REST: egui::Color32 = egui::Color32::from_gray(60);
/// Structural node (no activation data — the editor's preview).
pub const ACT_NEUTRAL: egui::Color32 = egui::Color32::from_gray(110);
/// Edge with a positive weight…
pub const EDGE_POS: egui::Color32 = egui::Color32::from_rgb(230, 150, 60);
/// …and with a negative one.
pub const EDGE_NEG: egui::Color32 = egui::Color32::from_rgb(70, 140, 230);

/// Body text size (egui points): the one size behind the Body / Button / Monospace
/// text styles, shared with `fonts::icon_label` so an icon+label button matches a
/// plain-text one by construction.
pub const BODY_SIZE: f32 = 14.0;

/// Converts an sRGB color `[r, g, b] ∈ [0, 1]` (the backend-agnostic scenario
/// encoding) to `Color32`. The single quantizer — it previously existed three times
/// (`hud::rgb`, `editor::archetype_color32`, and the dashboard through them).
pub fn rgb(c: [f32; 3]) -> egui::Color32 {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// The global egui [`egui::Style`]: dark visuals recolored by the tokens, slightly
/// rounded corners, and the text styles pinned to [`BODY_SIZE`]. **Pure** (no
/// context) so tests can assert on it; [`apply`] installs it.
pub fn style() -> egui::Style {
    use egui::{FontFamily, FontId, TextStyle};
    let mut style = egui::Style::default();

    let mut visuals = egui::Visuals::dark();
    visuals.warn_fg_color = ACCENT;
    visuals.error_fg_color = ERROR;
    visuals.extreme_bg_color = SURFACE;
    // Selected rows / text tie to the accent instead of egui's default blue.
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    let widget_radius = egui::CornerRadius::same(4);
    visuals.widgets.noninteractive.corner_radius = widget_radius;
    visuals.widgets.inactive.corner_radius = widget_radius;
    visuals.widgets.hovered.corner_radius = widget_radius;
    visuals.widgets.active.corner_radius = widget_radius;
    visuals.widgets.open.corner_radius = widget_radius;
    visuals.window_corner_radius = egui::CornerRadius::same(6);
    visuals.menu_corner_radius = egui::CornerRadius::same(6);
    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);

    let body = FontId::new(BODY_SIZE, FontFamily::Proportional);
    style.text_styles.insert(TextStyle::Body, body.clone());
    style.text_styles.insert(TextStyle::Button, body);
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(BODY_SIZE, FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style
}

/// Installs [`style`] on the egui context — once at startup, next to `set_fonts`
/// (cf. `fonts::setup_ui_fonts`). Unlike fonts, a style applies **immediately** (no
/// next-pass binding), so it needs no `FontsReady` gating.
pub fn apply(ctx: &egui::Context) {
    ctx.set_global_style(style());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_overrides_semantic_colors() {
        let s = style();
        assert_eq!(s.visuals.warn_fg_color, ACCENT);
        assert_eq!(s.visuals.error_fg_color, ERROR);
        assert_eq!(s.visuals.extreme_bg_color, SURFACE);
        assert_eq!(s.visuals.selection.bg_fill, ACCENT.gamma_multiply(0.35));
    }

    #[test]
    fn rgb_quantizes_and_clamps() {
        assert_eq!(rgb([1.0, 0.0, 0.5]), egui::Color32::from_rgb(255, 0, 128));
        // Out-of-range components clamp instead of wrapping.
        assert_eq!(rgb([-1.0, 2.0, 0.0]), egui::Color32::from_rgb(0, 255, 0));
    }

    #[test]
    fn body_size_consistent() {
        let s = style();
        for ts in [
            egui::TextStyle::Body,
            egui::TextStyle::Button,
            egui::TextStyle::Monospace,
        ] {
            assert_eq!(s.text_styles[&ts].size, BODY_SIZE, "{ts:?}");
        }
    }
}
