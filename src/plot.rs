//! Windowed-UI **plot widget**: the homemade time-series plotter, shared by the
//! evolution curves (bottom panel) and the breeding dashboard.
//!
//! No external plotting dependency (`egui_plot`): we draw the polylines by hand with
//! egui's `Painter` — the project's "homemade" spirit — but the scaling, the grid
//! steps and the axis margins are now **computed** (autoscaled, round tick values,
//! label-width-aware margins) instead of hardcoded, and the whole thing is one widget
//! both call sites reuse. The scaling math is pure and unit-tested.
//!
//! A module of the windowed *binary* only; it only **reads** the already-sampled
//! [`Curve`]s (cf. [`teemlab::metrics`]) to display them — no simulation logic.

use bevy_egui::egui;
use teemlab::metrics::Curve;

use crate::theme::{self, rgb};

/// egui font of the axis tick labels.
fn axis_font() -> egui::FontId {
    egui::FontId::monospace(9.0)
}

/// How the vertical axis is bounded.
#[derive(Clone, Copy)]
pub enum YAxis {
    /// A fixed window (e.g. normalized gene drift on `[0, 1]`).
    Fixed { min: f32, max: f32 },
    /// Auto-scaled to the data: optionally forced to include zero (counts), padded by
    /// `pad` (a fraction of the span) so the extreme points don't touch the edges.
    Auto { include_zero: bool, pad: f32 },
}

/// A plot's presentation: its `height` (egui points), the Y-axis rule and the X unit
/// suffix shown on the time labels.
pub struct PlotConfig {
    pub height: f32,
    pub y: YAxis,
    pub x_unit: &'static str,
    /// Optional **accent marker** at this X value (a vertical line): the breeding dashboard
    /// marks the generation being inspected. `None` (the time-series call sites) draws none.
    pub marker_x: Option<f32>,
}

/// The Y bounds `(min, max)` for `curves` under `y`. For [`YAxis::Auto`], scans the
/// data, optionally folds in zero, guarantees a **non-degenerate** span (a perfectly
/// flat series gets a unit window centered on its value, so the line sits mid-plot
/// rather than on an edge) and pads. No data → a unit window.
fn y_bounds(curves: &[Curve], y: &YAxis) -> (f32, f32) {
    let (include_zero, pad) = match *y {
        YAxis::Fixed { min, max } => return (min, max),
        YAxis::Auto { include_zero, pad } => (include_zero, pad),
    };
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for c in curves {
        for p in &c.pts {
            lo = lo.min(p[1]);
            hi = hi.max(p[1]);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    if include_zero {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    let span = hi - lo;
    if span <= f32::EPSILON {
        return (lo - 0.5, hi + 0.5);
    }
    let p = span * pad;
    (lo - p, hi + p)
}

/// A "nice" grid step (a value of the form 1/2/5 × 10ᵏ) so `span` is divided into
/// roughly `target_ticks` intervals landing on round numbers.
fn nice_step(span: f32, target_ticks: usize) -> f32 {
    if span <= 0.0 || target_ticks == 0 {
        return 1.0;
    }
    let raw = span / target_ticks as f32;
    let mag = 10f32.powf(raw.log10().floor());
    let norm = raw / mag; // in [1, 10)
    let nice = if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// Decimal places needed to print a tick at `step` without rounding it away (0 for
/// integer steps, 1 for 0.2/0.5, 2 for 0.05, …).
fn tick_decimals(step: f32) -> usize {
    if step <= 0.0 {
        return 0;
    }
    (-step.log10().floor()).max(0.0) as usize
}

/// The data's `(min, max)` **time** extent, or `None` when there isn't a drawable span
/// (no points, or a single time).
fn x_extent(curves: &[Curve]) -> Option<(f32, f32)> {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for c in curves {
        for p in &c.pts {
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
        }
    }
    (lo.is_finite() && hi > lo).then_some((lo, hi))
}

/// The sample of `pts` (a `[time, value]` polyline) whose time is closest to `t` — used
/// by the hover readout to snap to the data.
fn nearest(pts: &[[f32; 2]], t: f32) -> Option<[f32; 2]> {
    pts.iter()
        .copied()
        .min_by(|a, b| (a[0] - t).abs().total_cmp(&(b[0] - t).abs()))
}

/// A small filled colour square aligned with the text — the legend marker (matching the
/// catalog's swatch, and avoiding a bare "●" glyph in favour of painted ink).
fn swatch(ui: &mut egui::Ui, color: [f32; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, rgb(color));
}

/// A legend: a colour swatch + the name of each curve.
pub fn legend(ui: &mut egui::Ui, curves: &[Curve]) {
    ui.horizontal_wrapped(|ui| {
        for c in curves {
            swatch(ui, c.color);
            ui.label(&c.name);
        }
    });
}

/// Plots `curves` in a frame of `cfg.height`, the Y axis bounded per `cfg.y` and the X
/// axis spanning the data's **time** extent. Homemade `Painter` drawing: a background,
/// a light grid on **round** Y values with axis labels, the polylines, and a **hover
/// readout** — a vertical cursor, a dot on each curve at the hovered time, and a tooltip
/// listing the time and each value. Axis margins are computed from the label sizes.
///
/// **Returns** the data-space X of a click inside the plot (else `None`) — the breeding
/// dashboard reads it to pick a generation directly on the fitness graph; the time-series
/// call sites ignore it.
pub fn plot(ui: &mut egui::Ui, cfg: &PlotConfig, curves: &[Curve]) -> Option<f32> {
    let width = ui.available_width().max(64.0);
    // Clickable so a caller can select an X on the plot (generation picking); the hover
    // readout still works (the pointer position is reported regardless of the sense).
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, cfg.height), egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(2), theme::SURFACE);

    let Some((x_min, x_max)) = x_extent(curves) else {
        // Not (yet) enough points for a line: a discreet marker.
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "…",
            axis_font(),
            theme::INK_FAINT,
        );
        return None;
    };

    let (y_min, y_max) = y_bounds(curves, &cfg.y);
    let y_span = (y_max - y_min).max(1e-6);
    let step = nice_step(y_span, 4);
    let dec = tick_decimals(step);
    let font = axis_font();
    let grid = egui::Stroke::new(1.0, theme::GRID);
    let tick = theme::INK_MUTED;

    // The Y ticks (round values inside the range) — collected once, used both to size
    // the right margin (widest label) and to draw the grid.
    let mut ticks = Vec::new();
    let first = (y_min / step).ceil() * step;
    let mut v = first;
    while v <= y_max + step * 1e-3 {
        ticks.push(v);
        v += step;
    }

    // Margins computed from the label sizes: right = widest Y-tick label + padding,
    // bottom = one axis row + padding. Measured via the painter's text layout.
    let measure = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), tick)
            .size()
    };
    let right_margin = ticks
        .iter()
        .map(|t| measure(&format!("{t:.dec$}")).x)
        .fold(0.0_f32, f32::max)
        + 6.0;
    let bottom_margin = measure("0").y + 4.0;

    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 4.0, rect.top() + 4.0),
        egui::pos2(rect.right() - right_margin, rect.bottom() - bottom_margin),
    );
    let x_span = (x_max - x_min).max(1e-6);
    let map = |x: f32, y: f32| {
        egui::pos2(
            inner.left() + (x - x_min) / x_span * inner.width(),
            inner.bottom() - (y - y_min) / y_span * inner.height(),
        )
    };

    // Horizontal grid + Y labels on the round tick values.
    for &t in &ticks {
        let y = map(x_min, t).y;
        painter.line_segment(
            [egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
            grid,
        );
        // Clamp the label's vertical anchor near the edges so it isn't clipped.
        let anchor = if (y - inner.top()).abs() < 6.0 {
            egui::Align2::LEFT_TOP
        } else if (inner.bottom() - y).abs() < 6.0 {
            egui::Align2::LEFT_BOTTOM
        } else {
            egui::Align2::LEFT_CENTER
        };
        painter.text(
            egui::pos2(inner.right() + 3.0, y),
            anchor,
            format!("{t:.dec$}"),
            font.clone(),
            tick,
        );
    }
    // Vertical grid + time labels at start / middle / end (avoids crowding a narrow plot).
    const DIVS: usize = 4;
    for i in 0..=DIVS {
        let f = i as f32 / DIVS as f32;
        let x = inner.left() + f * inner.width();
        painter.line_segment(
            [egui::pos2(x, inner.top()), egui::pos2(x, inner.bottom())],
            grid,
        );
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
                format!("{t:.0}{}", cfg.x_unit),
                font.clone(),
                tick,
            );
        }
    }

    // The inspected-generation marker (accent), under the curves so the data reads on top.
    if let Some(mx) = cfg.marker_x
        && (x_min..=x_max).contains(&mx)
    {
        let x = inner.left() + (mx - x_min) / x_span * inner.width();
        painter.line_segment(
            [egui::pos2(x, inner.top()), egui::pos2(x, inner.bottom())],
            egui::Stroke::new(1.5, theme::ACCENT),
        );
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
            egui::Stroke::new(1.0, theme::INK_FAINT),
        );
        for c in curves {
            if let Some(p) = nearest(&c.pts, t) {
                painter.circle_filled(map(p[0], p[1]), 2.5, rgb(c.color));
            }
        }
    }
    // …and a tooltip listing the time and each curve's value at that time.
    let response = response.on_hover_ui_at_pointer(|ui| {
        let Some(t) = hover_t else { return };
        ui.small(format!("{t:.0}{}", cfg.x_unit));
        for c in curves {
            if let Some(p) = nearest(&c.pts, t) {
                ui.horizontal(|ui| {
                    swatch(ui, c.color);
                    ui.label(format!("{}: {:.dec$}", c.name, p[1]));
                });
            }
        }
    });

    // A click inside the plot → its data-space X (clamped to the data extent), for the
    // caller to map to a selection (the breeding dashboard: a generation).
    if response.clicked() {
        response.interact_pointer_pos().map(|pos| {
            let hx = pos.x.clamp(inner.left(), inner.right());
            x_min + (hx - inner.left()) / inner.width() * x_span
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(pts: &[[f32; 2]]) -> Curve {
        Curve {
            name: "c".into(),
            color: [1.0, 1.0, 1.0],
            pts: pts.to_vec(),
        }
    }

    #[test]
    fn y_bounds_auto_pads_and_includes_zero() {
        let c = [curve(&[[0.0, 10.0], [1.0, 30.0]])];
        let (lo, hi) = y_bounds(
            &c,
            &YAxis::Auto {
                include_zero: true,
                pad: 0.05,
            },
        );
        assert!(lo <= 0.0, "zero included: {lo}");
        assert!(hi > 30.0, "padded above the max: {hi}");
        assert!(lo < 0.0, "padded below zero: {lo}");
    }

    #[test]
    fn y_bounds_flat_curve_non_degenerate() {
        let c = [curve(&[[0.0, 7.0], [1.0, 7.0]])];
        let (lo, hi) = y_bounds(
            &c,
            &YAxis::Auto {
                include_zero: false,
                pad: 0.1,
            },
        );
        assert!(hi > lo, "flat series still gets a span: {lo}..{hi}");
        assert!(lo < 7.0 && hi > 7.0, "window centered on the value");
    }

    #[test]
    fn y_bounds_negative_values() {
        // A Dominance fitness run goes negative; include_zero: false keeps it.
        let c = [curve(&[[0.0, -5.0], [1.0, -1.0]])];
        let (lo, hi) = y_bounds(
            &c,
            &YAxis::Auto {
                include_zero: false,
                pad: 0.1,
            },
        );
        assert!(lo < -5.0, "min padded downward: {lo}");
        assert!(hi < 0.0, "range stays negative (zero not forced): {hi}");
    }

    #[test]
    fn y_bounds_fixed_is_verbatim() {
        let (lo, hi) = y_bounds(&[], &YAxis::Fixed { min: 0.0, max: 1.0 });
        assert_eq!((lo, hi), (0.0, 1.0));
    }

    #[test]
    fn nice_step_returns_1_2_5_decades() {
        for &span in &[0.001_f32, 0.03, 0.5, 7.0, 42.0, 1234.0, 1e6] {
            let s = nice_step(span, 4);
            let mag = 10f32.powf(s.log10().floor());
            let norm = (s / mag).round();
            assert!(
                [1.0, 2.0, 5.0].contains(&norm),
                "span {span} → step {s} (norm {norm}) is not 1/2/5"
            );
        }
    }

    #[test]
    fn tick_decimals_from_step() {
        assert_eq!(tick_decimals(1.0), 0);
        assert_eq!(tick_decimals(2.0), 0);
        assert_eq!(tick_decimals(0.2), 1);
        assert_eq!(tick_decimals(0.05), 2);
    }

    #[test]
    fn nearest_snaps_to_closest_sample() {
        let pts = [[0.0, 1.0], [10.0, 2.0], [20.0, 3.0]];
        assert_eq!(nearest(&pts, 9.0), Some([10.0, 2.0]));
        assert_eq!(nearest(&pts, 1.0), Some([0.0, 1.0]));
        assert_eq!(nearest(&[], 1.0), None);
    }
}
