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
    // Split the height left after the header/separator between the two plots: each gets
    // half, minus ~44 pt for its own strong label + legend + spacing. Clamped so neither
    // collapses nor grows unwieldy. The bottom panel's `size_range` floor (cf. `panels`)
    // is chosen so that at the minimum height `each` lands at its clamp floor and the two
    // plots still fit — the curves never clip.
    let each = (ui.available_height() / 2.0 - 44.0).clamp(64.0, 240.0);
    let history: &History = history;
    ui.strong("Population per species");
    draw_population(ui, history, config, each);
    ui.add_space(10.0);
    ui.strong("Gene drift — mutable genes (normalized 0–1)");
    draw_traits(ui, history, config, each);
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
    };
    plot::plot(ui, &cfg, &curves);
    plot::legend(ui, &curves);
}
