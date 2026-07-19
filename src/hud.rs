//! egui HUD of the windowed build: **the evolution curves** (bottom panel).
//!
//! A module of the windowed *binary* only. Since sampling moved into the lib
//! ([`teemlab::metrics`]), this module only **composes** the two curve sections —
//! population per species and normalized gene drift — over the shared plot widget
//! ([`crate::plot`]); the drawing itself lives there and is reused by the breeding
//! dashboard. Read-only over the history, so the cardinal invariant holds: no
//! simulation logic here. The one thing it *writes* is the display-only graph filters
//! ([`SimConfig::gene_display`], [`SimConfig::species_display`]) — presentation state,
//! not the sim.

use bevy_egui::egui;
use teemlab::SimConfig;
use teemlab::genotype::TRAITS;
use teemlab::metrics::{
    Curve, History, displayed_trait_indices, drift_curves, drift_species_indices,
    fauna_species_indices, filter_population_curves, mutable_trait_indices, population_curves,
    trait_color,
};

use crate::fonts::{self, icons};
use crate::plot::{self, PlotConfig, YAxis};

/// The evolution curves — population per species then normalized gene drift. A single
/// **species selector** spans both plots (the shared species axis), then the two plots sit
/// side by side. Reads the history; **writes** only the display filters (`gene_display`,
/// `species_display`) when a chip is toggled — presentation settings, so `config` is passed
/// with change-detection bypassed (cf. [`crate::panels`]).
pub(crate) fn hud_section(ui: &mut egui::Ui, history: &mut History, config: &mut SimConfig) {
    ui.horizontal(|ui| {
        ui.weak(format!("{} samples", history.sample_count()));
        if ui
            .button(fonts::icon_label(icons::RESET, "Clear"))
            .clicked()
        {
            history.clear();
        }
        // The single species selector, shared by BOTH graphs, sits right beside Clear: it
        // focuses the population graph on the chosen species and splits the drift graph by
        // them (none = the pooled / all view).
        species_selector(ui, config, "Species:");
    });
    ui.separator();
    // The comp's curves strip: the two plots **side by side** across the full width
    // (population left, gene drift right), caption + plot + legend in each column.
    let each = (ui.available_height() - 56.0).clamp(64.0, 260.0);
    ui.columns(2, |cols| {
        crate::theme::caption(&mut cols[0], "Population / species");
        draw_population(&mut cols[0], history, config, each);
        crate::theme::caption(&mut cols[1], "Gene drift (normalized 0–1)");
        draw_traits(&mut cols[1], history, config, each);
    });
}

/// The shared **species selector**: a wrapped row of toggle chips, one per fauna species,
/// tinted with the species' color, behind an optional leading `label`. With none selected
/// (the default) the population graph shows every species plus the food line and the drift
/// graph pools the fauna; selecting species focuses the population graph on them and splits
/// the drift graph into their per-species curves. Stored in `config.species_display` (so a
/// recorded video honors the same choice — the live HUD and the record menu edit the same
/// field). All-off is a valid state (the "show all / pooled" default), so any chip can always
/// be turned off. Hidden when there is at most one fauna species.
pub(crate) fn species_selector(ui: &mut egui::Ui, config: &mut SimConfig, label: &str) {
    let fauna = fauna_species_indices(config);
    if fauna.len() <= 1 {
        return;
    }
    let selected: std::collections::HashSet<usize> =
        drift_species_indices(config).into_iter().collect();
    ui.horizontal_wrapped(|ui| {
        if !label.is_empty() {
            ui.weak(label);
        }
        for &s in &fauna {
            let Some(a) = config.archetypes.get(s) else {
                continue;
            };
            let on = selected.contains(&s);
            let label = egui::RichText::new(&a.name).color(crate::theme::rgb(a.color));
            if ui.selectable_label(on, label).clicked() {
                toggle_species(config, s, !on, &fauna);
            }
        }
    });
}

/// Set whether the fauna species at archetype index `s` is selected, editing
/// `config.species_display` in place (rebuilt in fauna order for a stable legend and file).
/// Emptying the list is allowed and means the default view (all species + food on the
/// population graph, the pooled mean on the drift graph).
fn toggle_species(config: &mut SimConfig, s: usize, on: bool, fauna: &[usize]) {
    let mut selected: std::collections::HashSet<usize> =
        drift_species_indices(config).into_iter().collect();
    if on {
        selected.insert(s);
    } else {
        selected.remove(&s);
    }
    config.species_display = fauna
        .iter()
        .filter(|&&i| selected.contains(&i))
        .filter_map(|&i| config.archetypes.get(i).map(|a| a.name.clone()))
        .collect();
}

fn draw_population(ui: &mut egui::Ui, history: &History, config: &SimConfig, height: f32) {
    if history.is_empty() {
        ui.weak("(waiting for data…)");
        return;
    }
    // Focused by the shared species selector (`species_display`): the same filter feeds the
    // plotted curves in the live HUD and the video (one computation, two backends).
    let (curves, _) = filter_population_curves(population_curves(history, config).0, config);
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

fn draw_traits(ui: &mut egui::Ui, history: &History, config: &mut SimConfig, height: f32) {
    if history.is_empty() {
        ui.weak("(waiting for data…)");
        return;
    }
    // Which genes to plot: a chip per evolving gene (stored in `config.gene_display`). The
    // species split is driven by the shared selector above (`species_display`).
    gene_chips(ui, config, "");
    // The two filters feed the curves (`drift_curves`), so the chips and the plot agree —
    // and so do the live HUD and the video (one computation, two backends).
    let curves: Vec<Curve> = drift_curves(history, config);
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

/// A wrapped row of toggle chips — one per evolving gene — driving the drift graph's
/// display filter (`gene_display`), behind an optional leading `label`. A chip is *on* when
/// its gene is plotted (tinted with the curve's color); clicking flips it. Nothing is drawn
/// when there is at most one candidate (a filter would be pointless). Shared by the live HUD
/// and the record menu (both edit the same field).
pub(crate) fn gene_chips(ui: &mut egui::Ui, config: &mut SimConfig, label: &str) {
    let candidates = mutable_trait_indices(config);
    if candidates.len() <= 1 {
        return;
    }
    let shown: std::collections::HashSet<usize> =
        displayed_trait_indices(config).into_iter().collect();
    ui.horizontal_wrapped(|ui| {
        if !label.is_empty() {
            ui.weak(label);
        }
        for &i in &candidates {
            let on = shown.contains(&i);
            let label =
                egui::RichText::new(TRAITS[i].name).color(crate::theme::rgb(trait_color(i)));
            if ui.selectable_label(on, label).clicked() {
                toggle_gene(config, i, !on, &candidates);
            }
        }
    });
}

/// Set whether the trait at [`TRAITS`] index `i` is shown, editing
/// `config.gene_display` in place. Upholds the field's "empty == show every
/// candidate" contract: selecting all candidates collapses the list back to empty
/// (the clean default RON), and the last shown gene cannot be turned off (the graph
/// is never empty). The explicit list is stored in [`TRAITS`] order for a stable file.
fn toggle_gene(config: &mut SimConfig, i: usize, on: bool, candidates: &[usize]) {
    let mut shown: std::collections::HashSet<usize> =
        displayed_trait_indices(config).into_iter().collect();
    if on {
        shown.insert(i);
    } else {
        if shown.len() <= 1 {
            return; // never hide the last gene → never an empty graph
        }
        shown.remove(&i);
    }
    if shown.len() == candidates.len() {
        config.gene_display.clear();
    } else {
        config.gene_display = candidates
            .iter()
            .filter(|&&j| shown.contains(&j))
            .map(|&j| TRAITS[j].name.to_string())
            .collect();
    }
}
