//! **Docked** layout of the windowed build: fixed egui panels around the central
//! simulation area.
//!
//! A module of the windowed *binary* only. We invent nothing: each panel calls
//! the reusable `*_section(ui, …)` already exposed by its tool module
//! (`controls`, `editor`, `runs`, `hud`, `recorder`, `inspector`). The role of
//! these systems is purely **layout** — reserving the edges of the egui screen.
//!
//! **Semantic** split: *world* on the left, *entities* on the right, *scenario +
//! recording* in the top strip, *controls + stats* then *curves + inspector* at
//! the bottom.
//!
//! This is what makes the central area self-fitting: the
//! `SidePanel`/`TopBottomPanel`s *reserve* their space, so `ctx.available_rect()`
//! shrinks to the center, and `main::set_sim_camera` (which runs last in the egui
//! pass) frames the sim there — hence a simulation always **centered and fully
//! visible**, whatever the panels' size. No `CentralPanel`: the center stays
//! "transparent" and lets the Bevy rendering show through.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use teemlab::config::Archetype;
use teemlab::genotype::Genotype;
use teemlab::metrics::History;
use teemlab::selection::Selection;

use crate::controls::{self, SimControls};
use crate::editor::{self, Palette};
use crate::hud;
use crate::inspector;
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};

/// Top strip, **a single line**: **scenario** (choose / reload / save-load)
/// pinned left, video **recording** pinned **right** — all of a run's
/// "input/output" IO. Recording is right-aligned via a `right_to_left` layout; a
/// nested `left_to_right` preserves the reading order of its widgets.
pub fn top_bar(
    mut contexts: EguiContexts,
    mut runs_panel: ResMut<RunsPanel>,
    mut config: ResMut<SimConfig>,
    mut recorder_panel: ResMut<RecorderPanel>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        // Bar **forced to full width**: otherwise the `right_to_left` has no space
        // to push recording to the right. Scenario on the left (normal order),
        // recording pinned right (emitted in reverse order, cf. `recorder_section`).
        let row_h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                runs::scenario_section(ui, &mut runs_panel, &mut config);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    recorder::recorder_section(ui, &mut recorder_panel);
                });
            },
        );
    });
    Ok(())
}

/// Left column, resizable: the **world** — global scenario parameters (arena,
/// rate, seed, backgrounds), gene bounds and relation table.
pub fn left_tools(mut contexts: EguiContexts, mut config: ResMut<SimConfig>) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::left("left_tools")
        .resizable(true)
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.heading("World");
            egui::ScrollArea::vertical().show(ui, |ui| editor::world_section(ui, &mut config));
        });
    Ok(())
}

/// Right column, resizable: the **entities** — archetype palette (selector +
/// species library) and editor of the selected archetype.
pub fn right_panel(
    mut contexts: EguiContexts,
    mut palette: ResMut<Palette>,
    mut config: ResMut<SimConfig>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::right("right_panel")
        .resizable(true)
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.heading("Entities");
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::CollapsingHeader::new("Archetypes")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::selector_section(ui, &mut palette, &mut config)
                    });
                egui::CollapsingHeader::new("Archetype editor")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::editor_section(ui, &mut palette, &mut config)
                    });
            });
        });
    Ok(())
}

/// Bottom bar (just below the sim): **controls** (pause/step/speed/reset) on the
/// left, **global stats** on the right, same line. (The native Bevy visualizer
/// exists only for the video, cf. `bin/record`; there is no toggle here.)
pub fn bottom_bar(
    mut contexts: EguiContexts,
    mut sim_controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    agents: Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            controls::controls_section(ui, &mut sim_controls, &mut vtime);
            ui.separator();
            editor::stats_section(ui, &agents);
        });
    });
    Ok(())
}

/// Bottom panel, resizable: evolution curves (left) and agent inspector (right),
/// in two columns.
pub fn bottom_panel(
    mut contexts: EguiContexts,
    mut history: ResMut<History>,
    selection: Res<Selection>,
    mut config: ResMut<SimConfig>,
    mut palette: ResMut<Palette>,
    agents: Query<
        (
            &Species,
            &Reserve,
            &Genotype,
            &Vision,
            &Perception,
            &Action,
            &Brain,
            &Generation,
            &Age,
        ),
        With<Agent>,
    >,
) -> Result {
    /// Minimum width of the central panel below which we no longer try to split it
    /// into two columns. `egui::Ui::columns` computes a negative column width — and
    /// **panics** (`ui.rs:958`, "Negative width makes no sense") — when the
    /// available space is nearly zero, which happens as soon as the side columns
    /// (left + right) eat almost the whole window. Below it, we stack the two
    /// sections instead of crashing (and taking down the whole egui pass, the
    /// recording panel included).
    const MIN_TWO_COLUMN_WIDTH: f32 = 160.0;

    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .default_height(220.0)
        .show(ctx, |ui| {
            // Curves: we wrap them in a scroll area (they have none).
            let mut curves = |ui: &mut egui::Ui| {
                egui::ScrollArea::vertical()
                    .id_salt("curves")
                    .show(ui, |ui| {
                        ui.strong("Evolution — curves");
                        hud::hud_section(ui, &mut history, &config);
                    });
            };
            // Inspector: `inspector_section` already carries its own `ScrollArea`.
            // It does not mutate the sim: if there is a capture request, it
            // **returns** the derived archetype, which we add to the config after
            // the closures (so as not to borrow `config` mutably while the curves
            // read it).
            let mut capture_request: Option<Archetype> = None;
            let mut inspector = |ui: &mut egui::Ui| {
                ui.strong("Agent inspector");
                if let Some(arch) = inspector::inspector_section(ui, &selection, &config, &agents) {
                    capture_request = Some(arch);
                }
            };

            if ui.available_width() >= MIN_TWO_COLUMN_WIDTH {
                ui.columns(2, |cols| {
                    curves(&mut cols[0]);
                    inspector(&mut cols[1]);
                });
            } else {
                // Central panel too narrow: stacked, each keeping its scroll area.
                curves(ui);
                ui.separator();
                inspector(ui);
            }

            // Capture confirmed: the derived archetype joins the config and becomes
            // the editor's selection (the closures above, which borrowed `config`
            // shared, are no longer used here → mutable borrow allowed).
            if let Some(arch) = capture_request {
                let from = arch.captured_from.clone().unwrap_or_default();
                config.archetypes.push(arch);
                palette.selected = Some(config.archetypes.len() - 1);
                palette.status = format!("Archetype captured from {from}.");
            }
        });
    Ok(())
}
