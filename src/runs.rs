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
    /// Path of the **currently loaded** scenario — the launch argument, or the last
    /// reload/load through this panel. `Some` → 💾 **overwrites** it silently;
    /// `None` (fresh editor canvas) → 💾 opens the "save as" dialog. This is the
    /// intuitive save model.
    loaded_path: Option<String>,
    /// Whether the "save as" dialog is open (no scenario loaded → ask for a name).
    save_dialog_open: bool,
    /// Working buffer for the "save as" dialog's file name.
    save_dialog_name: String,
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
    // A scenario passed on the CLI (cf. `SimConfig::from_cli_or`, which reads
    // `args().nth(1)`) is the loaded file at launch → 💾 overwrites it. With no
    // argument (the empty editor canvas) the first save asks for a name.
    let loaded_path = std::env::args().nth(1);
    commands.insert_resource(RunsPanel {
        scenarios: scan_scenarios(),
        selected: None,
        scenario_path: "scenarios/edited.ron".to_string(),
        status: String::new(),
        pending: None,
        loaded_path,
        save_dialog_open: false,
        save_dialog_name: String::new(),
    });
}

/// Normalizes a user-typed name into a scenario path: ensures a `.ron` extension
/// and, if no folder is given, places the file under `scenarios/`.
fn normalize_scenario_path(name: &str) -> String {
    let mut p = name.trim().to_string();
    if p.is_empty() {
        p.push_str("scenario");
    }
    if !p.ends_with(".ron") {
        p.push_str(".ron");
    }
    if !p.contains('/') {
        p = format!("scenarios/{p}");
    }
    p
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

    // SAVE — the intuitive model: a **loaded** scenario is overwritten silently;
    // with none loaded, a "save as" dialog asks for a name (`save_dialog_open`).
    if ui
        .button("💾 Save")
        .on_hover_text("Overwrites the loaded scenario; if none is loaded, asks for a name.")
        .clicked()
    {
        match panel.loaded_path.clone() {
            Some(path) => {
                panel.status = match config.save_ron_file(&path) {
                    Ok(()) => format!("Saved → {path}"),
                    Err(e) => format!("Failed: {e}"),
                };
            }
            None => {
                if panel.save_dialog_name.is_empty() {
                    panel.save_dialog_name = "scenarios/new_scenario.ron".to_string();
                }
                panel.save_dialog_open = true;
            }
        }
    }
    ui.weak(format!(
        "file: {}",
        panel.loaded_path.as_deref().unwrap_or("(unsaved)")
    ));

    ui.separator();
    ui.label("Load:");
    ui.add(egui::TextEdit::singleline(&mut scenario_path).desired_width(140.0))
        .on_hover_text("Scenario file to load (.ron)");
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

    // "Save as" dialog, shown only when no scenario is loaded. A floating window
    // over the docked panels; driven through locals, then copied back.
    if panel.save_dialog_open {
        let mut name = panel.save_dialog_name.clone();
        let mut window_open = true; // the window's [x] close button
        let mut do_save = false;
        let mut cancel = false;
        egui::Window::new("Save scenario as…")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut window_open)
            .show(ui.ctx(), |ui| {
                ui.label("File name (.ron — placed in scenarios/ if no folder given):");
                ui.text_edit_singleline(&mut name);
                ui.horizontal(|ui| {
                    do_save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        panel.save_dialog_name = name;
        if do_save {
            let path = normalize_scenario_path(&panel.save_dialog_name);
            match config.save_ron_file(&path) {
                Ok(()) => {
                    panel.status = format!("Saved → {path}");
                    panel.loaded_path = Some(path);
                    panel.save_dialog_open = false;
                    rescan = true;
                }
                Err(e) => panel.status = format!("Failed: {e}"),
            }
        } else if cancel || !window_open {
            panel.save_dialog_open = false;
        }
    }

    // Copy the local copies back to the resource.
    panel.selected = selected;
    panel.scenario_path = scenario_path;
    if rescan {
        panel.scenarios = scan_scenarios();
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
    // Computed first, then assigned, to avoid overlapping borrows of `panel`
    // (the success arm also records `loaded_path`).
    let result = match SimConfig::from_ron_file(&path) {
        Ok(loaded) => {
            *config = loaded;
            palette.selected = None;
            palette.dragging = None;
            // Pause before the rebuild: the new world is born frozen.
            vtime.pause();
            controls.reset_requested = true;
            // This is now the loaded file → a later 💾 overwrites it (item: save UX).
            panel.loaded_path = Some(path.clone());
            format!("Scenario reloaded (paused) ← {path}")
        }
        Err(e) => format!("Failed: {e}"),
    };
    panel.status = result;
}
