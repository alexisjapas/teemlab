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
use teemlab::genotype::TRAITS;
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
    ui.strong("Gene drift — mutable genes (normalized 0–1)");
    draw_traits(ui, history, config);
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

fn draw_traits(ui: &mut egui::Ui, history: &History, config: &SimConfig) {
    if history.is_empty() {
        ui.weak("(waiting for data…)");
        return;
    }
    // Only the genes that actually evolve: a frozen (non-mutable) gene stays flat and
    // would just clutter the plot. `trait_curves` is 1:1 with `TRAITS` (same order), so
    // we keep a curve iff its gene is mutable in **at least one** archetype.
    let curves: Vec<Curve> = trait_curves(history)
        .into_iter()
        .zip(TRAITS.iter())
        .filter(|(_, t)| config.archetypes.iter().any(|a| (t.mutable)(&a.mutable)))
        .map(|(curve, _)| curve)
        .collect();
    if curves.is_empty() {
        ui.weak("(no mutable genes)");
        return;
    }
    // Fixed bounds [0, 1]: drift is read against the gene's possible span.
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

/// The sample of `pts` (a `[time, value]` polyline) whose time is closest to `t` —
/// used by the hover readout to snap to the data.
fn nearest(pts: &[[f32; 2]], t: f32) -> Option<[f32; 2]> {
    pts.iter()
        .copied()
        .min_by(|a, b| (a[0] - t).abs().total_cmp(&(b[0] - t).abs()))
}

/// Plots the curves in a frame of height `height`, the Y axis bounded to
/// `[y_min, y_max]` and the X axis spanning the data's **time** extent. Homemade
/// `Painter` drawing (no `egui_plot`): a background, a light grid with Y value and X
/// time labels, the polylines, and a **hover readout** — a vertical cursor, a dot on
/// each curve at the hovered time, and a tooltip listing the time and each value.
fn plot(ui: &mut egui::Ui, height: f32, curves: &[Curve], y_min: f32, y_max: f32) {
    let width = ui.available_width().max(64.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
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

    // Plot area, with margins reserved on the right (Y labels) and bottom (X/time
    // labels) so the axis text never overlaps the data.
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 4.0, rect.top() + 4.0),
        egui::pos2(rect.right() - 26.0, rect.bottom() - 14.0),
    );
    let y_span = (y_max - y_min).max(1e-6);
    let x_span = (x_max - x_min).max(1e-6);
    let map = |x: f32, y: f32| {
        egui::pos2(
            inner.left() + (x - x_min) / x_span * inner.width(),
            inner.bottom() - (y - y_min) / y_span * inner.height(),
        )
    };

    // Light grid + axis labels. Y is fractional (gene drift, ≤1) or integer (counts).
    let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(36));
    let tick = egui::Color32::from_gray(130);
    let font = egui::FontId::monospace(9.0);
    let y_dec = if y_max <= 1.5 { 2 } else { 0 };
    const DIVS: usize = 4;
    for i in 0..=DIVS {
        let f = i as f32 / DIVS as f32;
        let y = inner.bottom() - f * inner.height();
        painter.line_segment(
            [egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
            grid,
        );
        let anchor = match i {
            0 => egui::Align2::LEFT_BOTTOM,
            DIVS => egui::Align2::LEFT_TOP,
            _ => egui::Align2::LEFT_CENTER,
        };
        let v = y_min + f * y_span;
        painter.text(
            egui::pos2(inner.right() + 3.0, y),
            anchor,
            format!("{v:.y_dec$}"),
            font.clone(),
            tick,
        );
    }
    for i in 0..=DIVS {
        let f = i as f32 / DIVS as f32;
        let x = inner.left() + f * inner.width();
        painter.line_segment(
            [egui::pos2(x, inner.top()), egui::pos2(x, inner.bottom())],
            grid,
        );
        // Time labels only at start / middle / end to avoid crowding a narrow plot.
        if i == 0 || i == DIVS || i == DIVS / 2 {
            let anchor = match i {
                0 => egui::Align2::LEFT_TOP,
                DIVS => egui::Align2::RIGHT_TOP,
                _ => egui::Align2::CENTER_TOP,
            };
            let t = x_min + f * x_span;
            painter.text(
                egui::pos2(x, inner.bottom() + 2.0),
                anchor,
                format!("{t:.0}s"),
                font.clone(),
                tick,
            );
        }
    }

    for c in curves {
        let stroke = egui::Stroke::new(1.5, rgb(c.color));
        for w in c.pts.windows(2) {
            painter.line_segment([map(w[0][0], w[0][1]), map(w[1][0], w[1][1])], stroke);
        }
    }

    // Hover readout: a vertical cursor + a dot on each curve at the hovered time.
    let hover_t = response.hover_pos().map(|pos| {
        let hx = pos.x.clamp(inner.left(), inner.right());
        x_min + (hx - inner.left()) / inner.width() * x_span
    });
    if let Some(t) = hover_t {
        let hx = inner.left() + (t - x_min) / x_span * inner.width();
        painter.line_segment(
            [egui::pos2(hx, inner.top()), egui::pos2(hx, inner.bottom())],
            egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
        );
        for c in curves {
            if let Some(p) = nearest(&c.pts, t) {
                painter.circle_filled(map(p[0], p[1]), 2.5, rgb(c.color));
            }
        }
    }
    // …and a tooltip listing the time and each curve's value at that time.
    response.on_hover_ui_at_pointer(|ui| {
        let Some(t) = hover_t else { return };
        ui.small(format!("t = {t:.0} s"));
        for c in curves {
            if let Some(p) = nearest(&c.pts, t) {
                ui.colored_label(rgb(c.color), format!("● {}: {:.y_dec$}", c.name, p[1]));
            }
        }
    });
}
