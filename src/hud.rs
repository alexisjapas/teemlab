//! egui HUD of the windowed build: **plotting the evolution curves**.
//!
//! A module of the windowed *binary* only. Since sampling moved into the lib
//! ([`teemlab::metrics`]), this module only **plots** (with egui's `Painter`)
//! the already-computed data — the same [`Curve`]s as the native Bevy
//! visualizer ([`teemlab::dataviz`]), hence identical curves between the egui
//! preview and the video.
//!
//! We respect the cardinal invariant: no simulation logic here — this module
//! only **reads** the history to display it.
//!
//! No external plotting dependency (egui_plot): we draw the polylines by hand
//! with egui's `Painter` — in the project's "homemade" spirit.

use bevy_egui::egui;
use teemlab::SimConfig;
use teemlab::metrics::{Curve, History, population_curves, trait_curves};

/// Converts an sRGB color `[r, g, b] ∈ [0, 1]` (backend-agnostic) to `Color32`.
fn rgb(c: [f32; 3]) -> egui::Color32 {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// The evolution curves — population per species then normalized gene drift.
/// Rendered in the bottom panel. Read-only over the history (and over `config`
/// to name/color/filter the species).
pub(crate) fn hud_section(ui: &mut egui::Ui, history: &mut History, config: &SimConfig) {
    ui.horizontal(|ui| {
        ui.weak(format!("{} samples", history.sample_count()));
        if ui.button("↻ Clear").clicked() {
            history.clear();
        }
    });
    ui.separator();
    let history: &History = history;
    ui.strong("Population per species");
    draw_population(ui, history, config);
    ui.add_space(10.0);
    ui.strong("Gene drift (normalized 0–1)");
    draw_traits(ui, history);
}

fn draw_population(ui: &mut egui::Ui, history: &History, config: &SimConfig) {
    if history.is_empty() {
        ui.weak("(waiting for data…)");
        return;
    }
    let (curves, y_max) = population_curves(history, config);
    if curves.is_empty() {
        ui.weak("(no living species)");
        return;
    }
    plot(ui, 110.0, &curves, 0.0, y_max * 1.1);
    legend(ui, &curves);
}

fn draw_traits(ui: &mut egui::Ui, history: &History) {
    if history.is_empty() {
        ui.weak("(waiting for data…)");
        return;
    }
    // Fixed bounds [0, 1]: drift is read against the gene's possible span.
    let curves = trait_curves(history);
    plot(ui, 110.0, &curves, 0.0, 1.0);
    legend(ui, &curves);
}

/// A legend: a colored dot + the name of each curve.
fn legend(ui: &mut egui::Ui, curves: &[Curve]) {
    ui.horizontal_wrapped(|ui| {
        for c in curves {
            ui.colored_label(rgb(c.color), format!("● {}", c.name));
        }
    });
}

/// Plots the curves in a frame of height `height`, the Y axis bounded to
/// `[y_min, y_max]` and the X axis spanning the data's time extent. Homemade
/// `Painter` drawing: a background, the polylines, and the Y tick marks.
fn plot(ui: &mut egui::Ui, height: f32, curves: &[Curve], y_min: f32, y_max: f32) {
    let width = ui.available_width().max(64.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(2),
        egui::Color32::from_gray(18),
    );

    let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
    for c in curves {
        for q in &c.pts {
            x_min = x_min.min(q[0]);
            x_max = x_max.max(q[0]);
        }
    }
    // Not (yet) enough points for a line: we show a marker.
    if !x_max.is_finite() || x_max <= x_min {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "…",
            egui::FontId::monospace(12.0),
            egui::Color32::from_gray(90),
        );
        return;
    }

    let inner = rect.shrink(4.0);
    let y_span = (y_max - y_min).max(1e-6);
    let map = |x: f32, y: f32| {
        egui::pos2(
            inner.left() + (x - x_min) / (x_max - x_min) * inner.width(),
            inner.bottom() - (y - y_min) / y_span * inner.height(),
        )
    };

    for c in curves {
        let stroke = egui::Stroke::new(1.5, rgb(c.color));
        for w in c.pts.windows(2) {
            painter.line_segment([map(w[0][0], w[0][1]), map(w[1][0], w[1][1])], stroke);
        }
    }

    // Y tick marks: top and bottom of the frame.
    let tick = egui::Color32::from_gray(130);
    let font = egui::FontId::monospace(9.0);
    painter.text(
        rect.right_top() + egui::vec2(-2.0, 1.0),
        egui::Align2::RIGHT_TOP,
        format!("{y_max:.0}"),
        font.clone(),
        tick,
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-2.0, -1.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{y_min:.0}"),
        font,
        tick,
    );
}
