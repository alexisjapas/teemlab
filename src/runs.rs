//! Hot **scenario** management, windowed build (item 13).
//!
//! A module of the windowed *binary* only (like [`crate::editor`], …). All
//! scenario IO is gathered here:
//!
//! - **scenario selector**: the list of `scenarios/*.ron`, to choose and
//!   **reload into the living world** without restarting the binary;
//! - **save / load by path**: write the current `SimConfig` to a `.ron`, or
//!   reload one into the living world.
//!
//! Like the editor and the reset (item 11), reloading is hand-triggered editing
//! (outside `FixedUpdate`): the button only sets a *pending action*; it is the
//! `PreUpdate` system [`apply_scenario_load`] that applies it **before** the
//! frame's fixed loop, reusing the reset (item 11) to rebuild the world — no
//! duplicated population logic. The *save* (a plain file write, without mutating
//! the world) is done in place in the UI section.

use bevy::prelude::*;
use bevy_egui::egui;
use teemlab::SimConfig;

use crate::controls::SimControls;
use crate::editor::Palette;

/// An action requested by the UI, applied at the next `PreUpdate`.
enum RunAction {
    /// Reload this scenario into the living world (config + reset).
    LoadScenario(String),
}

/// State of the "Scenario" panel.
#[derive(Resource)]
pub struct RunsPanel {
    /// Paths of the `scenarios/*.ron` found at launch.
    scenarios: Vec<String>,
    /// Index of the selected scenario in the list.
    selected: Option<usize>,
    /// Free-entry RON save/load path.
    scenario_path: String,
    /// Last message (success/failure).
    status: String,
    /// Action pending application in `PreUpdate`.
    pending: Option<RunAction>,
}

/// Lists the RON scenarios present in `scenarios/`, sorted.
fn scan_scenarios() -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir("scenarios") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ron")
                && let Some(s) = path.to_str()
            {
                found.push(s.to_string());
            }
        }
    }
    found.sort();
    found
}

/// Builds the panel at `Startup`.
pub fn build_runs_panel(mut commands: Commands) {
    commands.insert_resource(RunsPanel {
        scenarios: scan_scenarios(),
        selected: None,
        scenario_path: "scenarios/edited.ron".to_string(),
        status: String::new(),
        pending: None,
    });
}

/// The "Scenario" section, rendered in the top strip (dock item). Reads/writes
/// its own state, **saves** the current scenario directly (a file write, not a
/// world mutation → in place), and **sets a pending action** for the
/// (re)loads (which, for their part, rebuild the world in `PreUpdate`).
pub(crate) fn scenario_section(ui: &mut egui::Ui, panel: &mut RunsPanel, config: &mut SimConfig) {
    // We work on local copies so the combo's egui closure does not capture
    // `panel` (avoids cross borrows).
    let scenarios = panel.scenarios.clone();
    let mut selected = panel.selected;
    let mut scenario_path = panel.scenario_path.clone();
    let mut pending = None;
    let mut rescan = false;

    // Emitted **inline** (no dedicated `ui.horizontal`): `top_bar` wraps scenario
    // + recording in a single `horizontal_wrapped` → one line at the top.
    ui.strong("Scenario:");
    let label = selected
        .and_then(|i| scenarios.get(i))
        .map(String::as_str)
        .unwrap_or("(choose…)");
    egui::ComboBox::from_id_salt("scenario_combo")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (i, path) in scenarios.iter().enumerate() {
                ui.selectable_value(&mut selected, Some(i), path);
            }
        });
    if ui.button("↻").on_hover_text("Rescan scenarios/").clicked() {
        rescan = true;
    }
    if ui
        .add_enabled(selected.is_some(), egui::Button::new("⟲ Reload"))
        .on_hover_text("Loads the selected scenario and restarts the run.")
        .clicked()
        && let Some(path) = selected.and_then(|i| scenarios.get(i))
    {
        pending = Some(RunAction::LoadScenario(path.clone()));
    }

    ui.separator();
    ui.label("RON:");
    ui.add(egui::TextEdit::singleline(&mut scenario_path).desired_width(140.0))
        .on_hover_text("Scenario file (.ron)");
    if ui
        .button("💾")
        .on_hover_text("Save the current scenario")
        .clicked()
    {
        panel.status = match config.save_ron_file(&scenario_path) {
            Ok(()) => format!("Saved → {scenario_path}"),
            Err(e) => format!("Failed: {e}"),
        };
    }
    if ui
        .button("📂")
        .on_hover_text("Load this file and restart the run")
        .clicked()
    {
        pending = Some(RunAction::LoadScenario(scenario_path.clone()));
    }

    if !panel.status.is_empty() {
        ui.weak(&panel.status);
    }

    // Copy the local copies back to the resource.
    panel.selected = selected;
    panel.scenario_path = scenario_path;
    if rescan {
        panel.scenarios = scan_scenarios();
        panel.selected = None;
    }
    if pending.is_some() {
        panel.pending = pending;
    }
}

/// Reloads a scenario into the living world: replaces the `SimConfig`, resyncs
/// the editor's palette, **pauses the sim**, then **delegates the rebuild to the
/// reset** (item 11) by raising its flag. Must run before `controls::apply_reset`
/// (chained).
///
/// The pause (on `Time<Virtual>`, like [`crate::controls`]) before the reset: we
/// restart on a **frozen** new world, to place/edit/inspect it before launching
/// — modeled on the paused start (`controls::pause_at_launch`).
pub fn apply_scenario_load(
    mut panel: ResMut<RunsPanel>,
    mut config: ResMut<SimConfig>,
    mut palette: ResMut<Palette>,
    mut controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
) {
    if !matches!(panel.pending, Some(RunAction::LoadScenario(_))) {
        return;
    }
    let Some(RunAction::LoadScenario(path)) = panel.pending.take() else {
        return;
    };
    panel.status = match SimConfig::from_ron_file(&path) {
        Ok(loaded) => {
            *config = loaded;
            palette.selected = None;
            palette.dragging = None;
            // Pause before the rebuild: the new world is born frozen.
            vtime.pause();
            controls.reset_requested = true;
            format!("Scenario reloaded (paused) ← {path}")
        }
        Err(e) => format!("Failed: {e}"),
    };
}
