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

/// **Primary accent** — the amber for *attention / selection*: the paused chip, the
/// dirty document marker, a breeding run in flight, the active nav strip, sliders and
/// the layer toggles. Also fed to egui as `warn_fg_color` and the widget/selection
/// accent. (The design comp splits this into a gold `--accent` and a warmer `--amber`;
/// we collapse them — they read as one warm accent.)
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(240, 180, 80);
/// **Secondary accent** — the teal for *primary calls-to-action*: Play, New, Save-as-new,
/// Run, and the Library launch buttons (the comp's `--accent2`). Text on it is
/// [`ON_ACCENT2`].
pub const ACCENT2: egui::Color32 = egui::Color32::from_rgb(47, 145, 136);
/// Ink **on** the amber accent (dark, for text on an [`ACCENT`]-filled control).
pub const ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(24, 18, 6);
/// Ink **on** the teal accent (near-white, for text on an [`ACCENT2`] CTA).
pub const ON_ACCENT2: egui::Color32 = egui::Color32::from_rgb(234, 252, 248);
/// A finished / successful state ("Done", a successful save).
pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(120, 200, 120);
/// Soft error ink (readable on the dark theme, less shouty than pure red) — also
/// fed to egui as `error_fg_color`.
pub const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 140, 120);

/// Perception-channel encodings of the inspector's bars: the **target** channel…
pub const TARGET: egui::Color32 = egui::Color32::from_rgb(220, 130, 40);
/// …and the **threat** channel.
pub const THREAT: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
/// The green of a **producer** / plant species (the comp's `--flora`) — the motes on a
/// World thumbnail.
pub const FLORA: egui::Color32 = egui::Color32::from_rgb(78, 201, 138);

/// The surface ramp — four steps of depth (the comp's `--bg` / `--surface` / `--card` /
/// `--raised`): the app **background** behind everything (the arena's off-game, the
/// nav rail's gutter)…
pub const BG: egui::Color32 = egui::Color32::from_rgb(19, 19, 21);
/// …the **panel** fill (side panels, top strips — one step up from [`BG`])…
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(27, 27, 30);
/// …the **card** surface — the panel-within-a-panel tint (`editor::card`), a step up
/// again so grouping reads from tone, not strokes…
pub const CARD: egui::Color32 = egui::Color32::from_rgb(33, 33, 36);
/// …and the **raised** surface — steppers, segmented controls, a toggle's off track.
pub const RAISED: egui::Color32 = egui::Color32::from_rgb(43, 43, 47);
/// The hairline **line** — separators and card borders (the comp's `--line`, also the
/// plots' grid).
pub const GRID: egui::Color32 = egui::Color32::from_rgb(42, 42, 46);
/// Three text inks: faint (hover cursors, structural strokes, captions)…
pub const INK_FAINT: egui::Color32 = egui::Color32::from_rgb(110, 110, 116);
/// …muted (axis ticks, secondary read-outs, sub-labels)…
pub const INK_MUTED: egui::Color32 = egui::Color32::from_rgb(167, 167, 172);
/// …and full ink (primary text — near-white on this dark theme).
pub const INK: egui::Color32 = egui::Color32::from_rgb(236, 236, 238);

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
    // The surface ramp: panels a step above the app background, text-edit / scroll /
    // plot backgrounds the deepest, striped rows on the card tint.
    visuals.panel_fill = SURFACE;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = CARD;
    // Buttons read as cards: a card fill at rest, raised on hover / active / open.
    visuals.widgets.inactive.weak_bg_fill = CARD;
    visuals.widgets.hovered.weak_bg_fill = RAISED;
    visuals.widgets.active.weak_bg_fill = RAISED;
    visuals.widgets.open.weak_bg_fill = RAISED;
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
    // Quiet chrome: controls are FLAT at rest (no idle outline — the fill is enough)
    // and grow a hairline on hover; separators and frame strokes drop to the faint
    // grid gray, so structure reads from spacing and surface tones, not from lines.
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, GRID);
    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    // An 8-pt rhythm: roomier controls and menus — the cramped egui defaults read
    // as "dense instrument panel".
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.menu_margin = egui::Margin::same(8);

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

/// A colour at ~14 % opacity — the comp's `-soft` fills (an accent-tinted chip / panel
/// background). Over the dark surfaces this reads as a faint wash of the hue.
pub fn soft(color: egui::Color32) -> egui::Color32 {
    color.gamma_multiply(0.16)
}

/// A colour at ~35 % opacity — the comp's `-line` (a chip's or panel's accent hairline).
pub fn line(color: egui::Color32) -> egui::Color32 {
    color.gamma_multiply(0.4)
}

/// A section **caption** (the comp's block headers): UPPERCASE, mono, faint — quieter
/// and more structural than `ui.strong`. Use for "LIVE STATS", "IDENTITY", "GENES", …
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(11.0)
            .color(INK_FAINT),
    );
}

/// A **pill toggle** switch (the comp's Layers / run-record affordance): a rounded track
/// with a sliding knob, [`ACCENT`] on / [`RAISED`] off. Flips `*on` on click; returns the
/// `Response`. Replaces a bare `ui.checkbox` where the comp shows a switch.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(34.0, 20.0), egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let radius = rect.height() * 0.5;
    let track = if *on { ACCENT } else { RAISED };
    ui.painter().rect_filled(rect, radius, track);
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = if *on { ON_ACCENT } else { INK_MUTED };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), radius - 2.5, knob);
    resp
}

/// A labelled **toggle row** (the comp's Layers / options layout): the label on the
/// left, a [`toggle`] pinned right. Returns the toggle's `Response`.
pub fn toggle_row(ui: &mut egui::Ui, label: &str, on: &mut bool) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            toggle(ui, on)
        })
        .inner
    })
    .inner
}

/// A rounded **chip** — a soft-filled pill with `color` text (a viability flag, a
/// "deferred" badge, an inline warning). Soft fill + hairline in the same hue.
pub fn chip(ui: &mut egui::Ui, color: egui::Color32, text: impl Into<egui::RichText>) {
    egui::Frame::default()
        .fill(soft(color))
        .stroke(egui::Stroke::new(1.0, line(color)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.label(text.into().color(color));
        });
}

/// A **primary** call-to-action button (the comp's teal): [`ACCENT2`] fill, rounded. The
/// caller colours the text [`ON_ACCENT2`] (plain `RichText::color` or
/// `fonts::icon_label_tinted`). Returns the `Response`.
pub fn primary_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .fill(ACCENT2)
            .corner_radius(egui::CornerRadius::same(9)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_overrides_semantic_colors() {
        let s = style();
        assert_eq!(s.visuals.warn_fg_color, ACCENT);
        assert_eq!(s.visuals.error_fg_color, ERROR);
        assert_eq!(s.visuals.panel_fill, SURFACE);
        assert_eq!(s.visuals.extreme_bg_color, BG);
        assert_eq!(s.visuals.selection.bg_fill, ACCENT.gamma_multiply(0.35));
    }

    #[test]
    fn the_two_accents_and_their_inks_are_distinct() {
        // The primary (amber) and CTA (teal) accents must not collapse, and each has a
        // legible on-ink.
        assert_ne!(ACCENT, ACCENT2);
        assert_ne!(ON_ACCENT, ON_ACCENT2);
        // The surface ramp climbs from bg → surface → card → raised.
        for (lo, hi) in [(BG, SURFACE), (SURFACE, CARD), (CARD, RAISED)] {
            assert!(hi.r() > lo.r(), "surface ramp must lighten");
        }
    }

    #[test]
    fn toggle_flips_and_soft_is_translucent() {
        // `soft` reduces opacity (a wash, not the solid hue).
        assert!(soft(ACCENT).a() < ACCENT.a());
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
