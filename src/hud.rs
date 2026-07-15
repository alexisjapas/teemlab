//! egui HUD of the windowed build: **the evolution curves** (bottom panel).
//!
//! A module of the windowed *binary* only. Since sampling moved into the lib
//! ([`teemlab::metrics`]), this module only **composes** the two curve sections —
//! population per species and normalized gene drift — over the shared plot widget
//! ([`crate::plot`]); the drawing itself lives there and is reused by the breeding
//! dashboard. Read-only over the history, so the cardinal invariant holds: no
//! simulation logic here.

use bevy_egui::egui;
use teemlab::SimConfig;
use teemlab::genotype::TRAITS;
use teemlab::metrics::{Curve, History, population_curves, trait_curves};

use crate::fonts::{self, icons};
use crate::plot::{self, PlotConfig, YAxis};

/// The evolution curves — population per species then normalized gene drift. The two
/// plots share the panel's available height (which the resizable bottom panel varies),
/// each clamped to a sensible band. Read-only over the history (and over `config` to
/// name/color/filter the species).
pub(crate) fn hud_section(ui: &mut egui::Ui, history: &mut History, config: &SimConfig) {
    ui.horizontal(|ui| {
        ui.weak(format!("{} samples", history.sample_count()));
        if ui
            .button(fonts::icon_label(icons::RESET, "Clear"))
            .clicked()
        {
            history.clear();
        }
    });
    ui.separator();
    // The comp's curves strip: the two plots **side by side** across the full width
    // (population left, gene drift right), caption + plot + legend in each column.
    let each = (ui.available_height() - 56.0).clamp(64.0, 260.0);
    let history: &History = history;
    ui.columns(2, |cols| {
        crate::theme::caption(&mut cols[0], "Population / species");
        draw_population(&mut cols[0], history, config, each);
        crate::theme::caption(&mut cols[1], "Gene drift — mutable genes (normalized 0–1)");
        draw_traits(&mut cols[1], history, config, each);
    });
}

fn draw_population(ui: &mut egui::Ui, history: &History, config: &SimConfig, height: f32) {
    if history.is_empty() {
        ui.weak("(waiting for data…)");
        return;
    }
    let (curves, _) = population_curves(history, config);
    if curves.is_empty() {
        ui.weak("(no living species)");
        return;
    }
    // Counts: auto-scaled, anchored at zero with a little headroom.
    let cfg = PlotConfig {
        height,
        y: YAxis::Auto {
            include_zero: true,
            pad: 0.05,
        },
        x_unit: "s",
        marker_x: None,
    };
    plot::plot(ui, &cfg, &curves);
    plot::legend(ui, &curves);
}

fn draw_traits(ui: &mut egui::Ui, history: &History, config: &SimConfig, height: f32) {
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
    let cfg = PlotConfig {
        height,
        y: YAxis::Fixed { min: 0.0, max: 1.0 },
        x_unit: "s",
        marker_x: None,
    };
    plot::plot(ui, &cfg, &curves);
    plot::legend(ui, &curves);
}
