//! Hot **scenario** management, windowed build (item 13).
//!
//! A module of the windowed *binary* only (like [`crate::editor`], …). It owns the
//! whole **document model** of a scenario, made explicit because three states coexist
//! and used to diverge silently:
//!
//! - the **file on disk** — curated **examples** in [`EXAMPLES_DIR`] (committed) or
//!   user **saved** scenarios in [`SAVED_DIR`] (gitignored), the two Open groups;
//! - the **config in memory** ([`SimConfig`]) — edited live by the panels;
//! - the **running world** — rebuilt from the config only on a reset.
//!
//! The top-bar **Scenario menu** (New / Open / Revert / Save / Save As) drives the
//! first two; the bottom-bar **Reset** rebuilds the third. A **`●` marker** next to
//! the file name shows when the config differs from the last load/save (`dirty`,
//! derived by comparing against a [`RunsPanel::baseline`] snapshot — every config type
//! derives `PartialEq`). Destructive navigation (New / Open / Revert) **confirms**
//! before discarding unsaved edits, and **Save** refuses to silently overwrite a file
//! the user did not create this session (a bundled example): it offers *Save a copy*
//! instead — so the hand-curated `scenarios/examples/*.ron` (comments and compact form,
//! dropped by RON serialization) are never clobbered by accident.
//!
//! Like the editor and the reset (item 11), (re)loading is hand-triggered editing
//! (outside `FixedUpdate`): the menu only sets a *pending action*; it is the
//! `PreUpdate` system [`apply_scenario_load`] that applies it **before** the frame's
//! fixed loop, reusing the reset to rebuild the world — no duplicated population
//! logic. *Saving* (a plain file write, no world mutation) is done in place in the UI.

use bevy::prelude::*;
use bevy_egui::egui;
use teemlab::SimConfig;

use crate::controls::SimControls;
use crate::editor::Palette;
use crate::fonts::{self, icons};
use crate::status::UiStatus;

/// An action requested by the UI, applied at the next `PreUpdate`. Both rebuild the
/// world (via the reset); they differ only in the config they install.
#[derive(Clone)]
enum RunAction {
    /// Load this scenario file into the living world (config + reset).
    LoadScenario(String),
    /// Start over from an empty editor canvas ([`SimConfig::empty`]).
    NewEmpty,
}

/// A pending confirmation, shown as a modal until the user resolves it.
enum Confirm {
    /// Unsaved edits exist; confirm before this destructive navigation.
    DiscardThen(RunAction),
    /// Save would overwrite an existing file the user did not create this session
    /// (likely a bundled scenario) — offer *Save a copy* instead of clobbering it.
    OverwriteExternal(String),
    /// Save As names a file that already exists on disk — confirm before overwriting.
    OverwriteExisting(String),
}

/// The user's choice in the confirmation modal, collected inside the egui closure and
/// applied after it (so the borrow of the pending [`Confirm`] does not alias `panel`).
enum Choice {
    /// No button pressed this frame (keep the modal open).
    Pending,
    /// Primary action confirmed (Discard / Overwrite).
    Proceed,
    /// Dismissed.
    Cancel,
    /// "Save a copy…" — route to the Save As dialog with a suggested name.
    SaveCopy,
}

/// State of the "Scenario" panel — the document model (cf. module docs).
#[derive(Resource)]
pub struct RunsPanel {
    /// Paths of the committed example scenarios (`scenarios/examples/*.ron`), last scan.
    examples: Vec<String>,
    /// Paths of the user-saved scenarios (`scenarios/saved/*.ron`), last scan.
    saved: Vec<String>,
    /// Free-entry RON path for "Open from path".
    scenario_path: String,
    /// Action pending application in `PreUpdate`.
    pending: Option<RunAction>,
    /// Pending confirmation modal, if any.
    confirm: Option<Confirm>,
    /// Path of the **currently loaded** scenario (the launch argument, or the last
    /// load/save through this panel). `None` = the fresh editor canvas.
    loaded_path: Option<String>,
    /// Whether the loaded file was **created or claimed** by the user this session
    /// (Save As, or an explicit Overwrite). `false` for a file opened from disk → Save
    /// protects it (offers a copy). Moot when `loaded_path` is `None`.
    owns_loaded: bool,
    /// Snapshot of the config at the last load/save; `dirty = config != baseline`.
    baseline: SimConfig,
    /// Whether the "save as" dialog is open.
    save_dialog_open: bool,
    /// Working buffer for the "save as" dialog's file name.
    save_dialog_name: String,
    /// Whether the Scenario menu was open last frame — to rescan both scenario dirs once
    /// on the closed→open transition (no manual rescan button).
    menu_was_open: bool,
}

/// Curated **example** scenarios — committed; shown under Open ▸ Examples.
pub(crate) const EXAMPLES_DIR: &str = "scenarios/examples";
/// User-**saved** scenarios — gitignored; shown under Open ▸ Saved, and the default
/// Save target. The two categories live in sibling directories under `scenarios/`.
pub(crate) const SAVED_DIR: &str = "scenarios/saved";

/// Lists the RON files present in `dir`, sorted; a missing directory → empty list.
fn scan_dir(dir: &str) -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
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

/// Menu label for a scenario path: its file stem (`scenarios/examples/hunt.ron` → `hunt`).
fn scenario_label(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

/// Builds the panel at `Startup`, seeding the `baseline` from the launch config.
pub fn build_runs_panel(mut commands: Commands, config: Res<SimConfig>) {
    // A scenario passed on the CLI (cf. `SimConfig::from_cli_or`, which reads
    // `args().nth(1)`) is the loaded file at launch — opened from disk, so `owns_loaded`
    // is false and a later Save protects it. With no argument (the empty canvas)
    // `loaded_path` is None and the first Save asks for a name.
    let loaded_path = std::env::args().nth(1);
    commands.insert_resource(RunsPanel {
        examples: scan_dir(EXAMPLES_DIR),
        saved: scan_dir(SAVED_DIR),
        scenario_path: format!("{SAVED_DIR}/edited.ron"),
        pending: None,
        confirm: None,
        loaded_path,
        owns_loaded: false,
        baseline: config.clone(),
        save_dialog_open: false,
        save_dialog_name: String::new(),
        menu_was_open: false,
    });
}

/// Normalizes a user-typed name into a scenario path: ensures a `.ron` extension and,
/// if no folder is given, places the file under `scenarios/saved/` (saves default to the
/// gitignored category; an explicit folder still wins, e.g. to overwrite an example).
fn normalize_scenario_path(name: &str) -> String {
    let mut p = name.trim().to_string();
    if p.is_empty() {
        p.push_str("scenario");
    }
    if !p.ends_with(".ron") {
        p.push_str(".ron");
    }
    if !p.contains('/') {
        p = format!("{SAVED_DIR}/{p}");
    }
    p
}

/// Suggests a copy name for `path`: `scenarios/saved/run.ron` → `scenarios/saved/run copy.ron`.
fn suggest_copy_name(path: &str) -> String {
    let stem = path.strip_suffix(".ron").unwrap_or(path);
    format!("{stem} copy.ron")
}

/// The display name of the loaded file (basename), or `(unsaved)`.
fn loaded_name(loaded_path: Option<&str>) -> &str {
    match loaded_path {
        Some(p) => p.rsplit('/').next().unwrap_or(p),
        None => "(unsaved)",
    }
}

/// Writes the config to `path`, reporting through the status line. Returns whether it
/// succeeded (the caller then re-baselines and records the path). Creates the parent
/// directory if needed (e.g. `scenarios/saved/` on a fresh checkout).
fn write_scenario(path: &str, config: &SimConfig, status: &mut UiStatus) -> bool {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match config.save_ron_file(path) {
        Ok(()) => {
            status.set(format!("Saved → {path}"));
            true
        }
        Err(e) => {
            status.set(format!("Failed: {e}"));
            false
        }
    }
}

/// Saves to `path`; on success, **claims** the file (owned) and re-baselines (so the
/// `●` modified marker clears). Returns whether it succeeded.
fn save_and_claim(
    panel: &mut RunsPanel,
    config: &SimConfig,
    status: &mut UiStatus,
    path: String,
) -> bool {
    if !write_scenario(&path, config, status) {
        return false;
    }
    panel.loaded_path = Some(path);
    panel.owns_loaded = true;
    panel.baseline = config.clone();
    true
}

/// The "Scenario" section, rendered in the top strip (dock item): the **Scenario
/// menu** plus the current file name with a `*` *modified* marker. It only reads/writes
/// its own state, **saves** directly (a file write), and **sets a pending action** for
/// the (re)loads (which rebuild the world in `PreUpdate`).
pub(crate) fn scenario_section(
    ui: &mut egui::Ui,
    panel: &mut RunsPanel,
    config: &mut SimConfig,
    status: &mut UiStatus,
) {
    let dirty = *config != panel.baseline;
    // Local copies so the menu closures don't capture `panel` (avoids cross borrows);
    // intents collected here, then resolved against `dirty`/`owns_loaded` below.
    let examples = panel.examples.clone();
    let saved = panel.saved.clone();
    let mut scenario_path = panel.scenario_path.clone();
    let loaded_path = panel.loaded_path.clone();
    let mut want: Option<RunAction> = None;
    let mut want_save = false;
    let mut want_save_as = false;
    let mut menu_open = false;

    ui.menu_button(fonts::icon_label(icons::CARET_DOWN, "Scenario"), |ui| {
        menu_open = true;
        if ui
            .button(fonts::icon_label(icons::PLUS, "New (empty)"))
            .clicked()
        {
            want = Some(RunAction::NewEmpty);
        }
        ui.menu_button("Open", |ui| {
            // Two categories: curated Examples (committed) then your Saved scenarios.
            ui.label("Examples");
            for path in &examples {
                if ui.button(scenario_label(path)).clicked() {
                    want = Some(RunAction::LoadScenario(path.clone()));
                }
            }
            ui.separator();
            ui.label("Saved");
            if saved.is_empty() {
                ui.weak("(none yet)");
            }
            for path in &saved {
                if ui.button(scenario_label(path)).clicked() {
                    want = Some(RunAction::LoadScenario(path.clone()));
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut scenario_path)
                        .desired_width(170.0)
                        .hint_text("path/to.ron"),
                );
                if ui.button("Open path").clicked() {
                    want = Some(RunAction::LoadScenario(scenario_path.clone()));
                }
            });
        });
        ui.separator();
        if ui
            .button(fonts::icon_label(icons::FLOPPY, "Save"))
            .clicked()
        {
            want_save = true;
        }
        if ui
            .button(fonts::icon_label(icons::FLOPPY, "Save As…"))
            .clicked()
        {
            want_save_as = true;
        }
    });
    // The scenario list refreshes itself **when the menu opens** (no manual rescan):
    // we detect the closed→open transition and rescan once.
    let rescan = menu_open && !panel.menu_was_open;
    panel.menu_was_open = menu_open;

    // Current document name + a *modified* marker (amber name + "*"). We avoid a glyph
    // marker (e.g. "●"): the embedded DejaVu subset renders some symbols as tofu.
    let name = loaded_name(loaded_path.as_deref());
    let text = if dirty {
        egui::RichText::new(format!("{name} *")).color(egui::Color32::from_rgb(240, 180, 80))
    } else {
        egui::RichText::new(name)
    };
    ui.label(text).on_hover_text(match &loaded_path {
        Some(p) if dirty => format!("{p} — unsaved edits"),
        Some(p) => p.clone(),
        None => "Not saved yet".to_string(),
    });

    // Resolve intents (full &mut access here, no egui closure borrow in flight).
    panel.scenario_path = scenario_path;
    if rescan {
        panel.examples = scan_dir(EXAMPLES_DIR);
        panel.saved = scan_dir(SAVED_DIR);
    }
    // Destructive navigation guards on unsaved edits.
    if let Some(action) = want {
        if dirty {
            panel.confirm = Some(Confirm::DiscardThen(action));
        } else {
            panel.pending = Some(action);
        }
    }
    if want_save_as {
        open_save_as(panel);
    }
    if want_save {
        match panel.loaded_path.clone() {
            None => open_save_as(panel),
            Some(path) if panel.owns_loaded => {
                save_and_claim(panel, config, status, path);
            }
            // External / bundled file: protect it (offer a copy) instead of clobbering.
            Some(path) => panel.confirm = Some(Confirm::OverwriteExternal(path)),
        }
    }

    confirm_modal(ui, panel, config, status);
    save_as_dialog(ui, panel, config, status);
}

/// Primes and opens the "Save As" dialog (suggesting a name from the loaded file).
fn open_save_as(panel: &mut RunsPanel) {
    if panel.save_dialog_name.is_empty() {
        panel.save_dialog_name = panel
            .loaded_path
            .clone()
            .unwrap_or_else(|| format!("{SAVED_DIR}/untitled.ron"));
    }
    panel.save_dialog_open = true;
}

/// The pending confirmation modal (discard-edits, overwrite-external, or
/// overwrite-existing). The choice is collected inside the closure, then applied after
/// it so the borrow of the pending [`Confirm`] does not alias `panel`.
fn confirm_modal(
    ui: &mut egui::Ui,
    panel: &mut RunsPanel,
    config: &mut SimConfig,
    status: &mut UiStatus,
) {
    let Some(confirm) = panel.confirm.take() else {
        return;
    };
    let mut window_open = true;
    let mut choice = Choice::Pending;
    egui::Window::new("Scenario")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut window_open)
        .show(ui.ctx(), |ui| match &confirm {
            Confirm::DiscardThen(_) => {
                ui.label("You have unsaved edits. Discard them?");
                ui.horizontal(|ui| {
                    if ui.button("Discard").clicked() {
                        choice = Choice::Proceed;
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Choice::Cancel;
                    }
                });
            }
            Confirm::OverwriteExternal(path) => {
                ui.label(format!(
                    "“{path}” wasn't created here (it may be a bundled scenario).\n\
                     Overwriting rewrites the file and drops its comments."
                ));
                ui.horizontal(|ui| {
                    if ui.button("Save a copy…").clicked() {
                        choice = Choice::SaveCopy;
                    }
                    if ui.button("Overwrite").clicked() {
                        choice = Choice::Proceed;
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Choice::Cancel;
                    }
                });
            }
            Confirm::OverwriteExisting(path) => {
                ui.label(format!("“{path}” already exists. Overwrite it?"));
                ui.horizontal(|ui| {
                    if ui.button("Overwrite").clicked() {
                        choice = Choice::Proceed;
                    }
                    // Back to naming, keeping the typed name.
                    if ui.button("Rename…").clicked() {
                        choice = Choice::SaveCopy;
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Choice::Cancel;
                    }
                });
            }
        });

    match (choice, confirm) {
        (Choice::Proceed, Confirm::DiscardThen(action)) => panel.pending = Some(action),
        (Choice::Proceed, Confirm::OverwriteExternal(path) | Confirm::OverwriteExisting(path)) => {
            save_and_claim(panel, config, status, path);
        }
        (Choice::SaveCopy, Confirm::OverwriteExternal(path)) => {
            panel.save_dialog_name = suggest_copy_name(&path);
            panel.save_dialog_open = true;
        }
        // "Rename…" on an existing target → reopen Save As with the same name.
        (Choice::SaveCopy, Confirm::OverwriteExisting(_)) => panel.save_dialog_open = true,
        // No button yet and not closed via [x] → keep the modal up.
        (Choice::Pending, c) if window_open => panel.confirm = Some(c),
        // Cancelled, dismissed, or an impossible pairing → drop it.
        _ => {}
    }
}

/// The "Save As" dialog (name entry). On success: writes, records the path as
/// **owned**, and re-baselines (dirty cleared).
fn save_as_dialog(
    ui: &mut egui::Ui,
    panel: &mut RunsPanel,
    config: &mut SimConfig,
    status: &mut UiStatus,
) {
    if !panel.save_dialog_open {
        return;
    }
    let mut name = panel.save_dialog_name.clone();
    let mut window_open = true;
    let mut do_save = false;
    let mut cancel = false;
    egui::Window::new("Save scenario as…")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut window_open)
        .show(ui.ctx(), |ui| {
            ui.label("File name (.ron — saved to scenarios/saved/ if no folder given):");
            ui.text_edit_singleline(&mut name);
            ui.horizontal(|ui| {
                do_save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    panel.save_dialog_name = name;
    if do_save {
        let path = normalize_scenario_path(&panel.save_dialog_name);
        // Naming an **existing** file (other than the one already loaded) → confirm
        // before clobbering it, so a stray default name can't silently overwrite a
        // bundled scenario.
        let collides =
            std::path::Path::new(&path).exists() && panel.loaded_path.as_deref() != Some(&path);
        if collides {
            panel.confirm = Some(Confirm::OverwriteExisting(path));
            panel.save_dialog_open = false;
        } else if save_and_claim(panel, config, status, path) {
            panel.save_dialog_open = false;
            // A new file may have appeared (usually under saved/; rescan both to be safe).
            panel.examples = scan_dir(EXAMPLES_DIR);
            panel.saved = scan_dir(SAVED_DIR);
        }
    } else if cancel || !window_open {
        panel.save_dialog_open = false;
    }
}

/// Applies a pending scenario action into the living world: replaces the `SimConfig`
/// (from a file, or empty), resyncs the editor's palette, **pauses the sim**, then
/// **delegates the rebuild to the reset** (item 11) by raising its flag. Re-baselines
/// the document (dirty cleared) and marks an opened file as **not owned** (external →
/// Save protects it). Must run before `controls::apply_reset` (chained).
///
/// The pause (on `Time<Virtual>`, like [`crate::controls`]) before the reset: we
/// restart on a **frozen** new world, to place/edit/inspect it before launching —
/// modeled on the paused start (`controls::pause_at_launch`).
pub fn apply_scenario_load(
    mut panel: ResMut<RunsPanel>,
    mut config: ResMut<SimConfig>,
    mut palette: ResMut<Palette>,
    mut controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    mut status: ResMut<UiStatus>,
) {
    let Some(action) = panel.pending.take() else {
        return;
    };
    // Shared epilogue: pause, rebuild, resync the editor, re-baseline.
    let mut install = |panel: &mut RunsPanel, config: &SimConfig| {
        palette.selected = None;
        palette.dragging = None;
        vtime.pause();
        controls.reset_requested = true;
        panel.baseline = config.clone();
    };
    match action {
        RunAction::LoadScenario(path) => match SimConfig::from_ron_file(&path) {
            Ok(loaded) => {
                *config = loaded;
                install(&mut panel, &config);
                panel.loaded_path = Some(path.clone());
                panel.owns_loaded = false; // opened from disk → protected on Save
                status.set(format!("Scenario loaded (paused) ← {path}"));
            }
            Err(e) => status.set(format!("Failed: {e}")),
        },
        RunAction::NewEmpty => {
            *config = SimConfig::empty();
            install(&mut panel, &config);
            panel.loaded_path = None;
            panel.owns_loaded = false;
            status.set("New empty scenario (paused).".to_string());
        }
    }
}
