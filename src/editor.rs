//! Editor of the windowed build: **manual placement** by drag-and-drop (item 4).
//!
//! A module of the windowed *binary* only (never compiled into the headless
//! build): everything touching egui, the camera or the window lives here, apart
//! from the render-agnostic core. We respect the cardinal invariant — it is
//! **manual editing** triggered by the user (like tweaking the scenario by hand),
//! not simulation logic: it can therefore live outside `FixedUpdate`. The created
//! entities then join the sim loop normally.
//!
//! Layout: **floating windows** above the full-frame sim — archetype **selector**
//! (where one picks by drag-and-drop), **editor** of the chosen archetype, and
//! **statistics** ([`stats_ui`]).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::collections::{HashMap, HashSet};
use teemlab::SimConfig;
use teemlab::brain::{Brain, BrainKind, MlpBrain};
use teemlab::components::{Agent, Reserve, Species};
use teemlab::config::{Archetype, Relation, Source};
use teemlab::genotype::{GeneCategory, Genotype, TRAITS};
use teemlab::metrics;
use teemlab::spawn::spawn_agent;
use teemlab::visuals::Layers;

use crate::fonts::{self, icons};
use crate::help;
use crate::status::UiStatus;

/// The palette / the editor's state. The **archetype list** now lives in
/// [`SimConfig::archetypes`] (the central data); the palette only keeps the
/// interaction state. Editing an archetype therefore writes *directly* into the
/// `SimConfig`, without a copy or a sync pass.
#[derive(Resource, Default)]
pub struct Palette {
    /// Index (in `config.archetypes`) of the archetype currently being dragged.
    pub dragging: Option<usize>,
    /// Index of the archetype selected for editing.
    pub selected: Option<usize>,
    /// Rolling seed to give a distinct stream to the brain of each hand-placed agent.
    pub next_seed: u64,
    /// Species library catalog: each `species/*.ron` with its loaded definition and
    /// **cross-scenario usage**, cached by [`scan_library`] — refreshed when the
    /// library section opens (it reads every species and scenario file, so never per
    /// frame), not by a manual reload button.
    pub library: Vec<LibraryEntry>,
    /// Whether the "Species library" section was open last frame — to rescan the catalog
    /// once on the closed→open transition (cf. `runs::RunsPanel::menu_was_open`).
    pub library_was_open: bool,
    /// Editor view state (not scenario data): when on, the gene editor shows a
    /// **“mutable”** checkbox beside each gene. Off by default — the toggle sits at
    /// the top of the gene panel so it stays discoverable.
    pub show_mutability: bool,
}

/// egui color of an archetype, from its stored color (`[r, g, b]` ∈ [0, 1]).
fn archetype_color32(a: &Archetype) -> egui::Color32 {
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(a.color[0]), q(a.color[1]), q(a.color[2]))
}

/// A color button that writes back **only on a real edit**. egui's
/// `color_edit_button_rgb` round-trips the `[f32; 3]` through HSVA *every frame*, so
/// binding it directly to the config drifts the stored color by sub-LSB amounts each
/// frame — saved as such, and (since the scenario document model) read forever as
/// "modified". Gating on `response.changed()` keeps the value byte-stable until the
/// user actually picks a color.
fn color_button(ui: &mut egui::Ui, value: &mut [f32; 3]) {
    let mut local = *value;
    if ui.color_edit_button_rgb(&mut local).changed() {
        *value = local;
    }
}

/// Builds the palette at `Startup`, after [`SimConfig`] is inserted by the sim
/// plugin.
pub fn build_palette(mut commands: Commands, config: Res<SimConfig>) {
    commands.insert_resource(Palette {
        dragging: None,
        selected: None,
        next_seed: config.seed ^ 0xED17,
        library: scan_library(),
        library_was_open: false,
        show_mutability: false,
    });
}

/// All `*.ron` paths directly under `dir`, sorted; a missing directory → empty. The
/// shared scan behind both the species library and the cross-scenario usage lookup.
fn ron_files(dir: &str) -> Vec<String> {
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

/// One library species (`species/*.ron`) with its loaded definition and the
/// scenarios that import it — the cached unit of the catalog (cf. [`scan_library`]).
pub struct LibraryEntry {
    /// File path, e.g. `species/hunter.ron`.
    pub path: String,
    /// The parsed species (`None` if the file failed to load).
    pub def: Option<Archetype>,
    /// Scenarios that import this species (by `source`), each with its sync state.
    pub usage: Vec<LibraryUsage>,
}

/// One scenario importing a library species, with whether its copy is up to date
/// (cf. [`LibraryEntry::usage`]).
pub struct LibraryUsage {
    /// Scenario file path, e.g. `scenarios/predator_prey.ron`.
    pub scenario: String,
    /// `false` if the scenario's copy is outdated vs the current library definition.
    pub in_sync: bool,
}

/// Builds the species-library catalog: every `species/*.ron` loaded, cross-referenced
/// against every `scenarios/*.ron` to find which scenarios import it (and whether each
/// copy is still in sync). One pass over the scenarios (parsed once), so it scales with
/// the file count, not their product. A manual rescan — never per frame.
fn scan_library() -> Vec<LibraryEntry> {
    // Index every scenario's imported archetypes by their `source` path in one pass.
    let mut imports: HashMap<String, Vec<(String, Archetype)>> = HashMap::new();
    for scenario in ron_files("scenarios") {
        let Ok(cfg) = SimConfig::from_ron_file(&scenario) else {
            continue;
        };
        for arch in cfg.archetypes {
            if let Some(src) = arch.source.clone() {
                imports
                    .entry(src)
                    .or_default()
                    .push((scenario.clone(), arch));
            }
        }
    }
    ron_files("species")
        .into_iter()
        .map(|path| {
            let def = Archetype::from_ron_file(&path).ok();
            let usage = imports
                .get(&path)
                .into_iter()
                .flatten()
                .map(|(scenario, arch)| LibraryUsage {
                    scenario: scenario.clone(),
                    in_sync: def.as_ref().is_some_and(|d| species_in_sync(arch, d)),
                })
                .collect();
            LibraryEntry { path, def, usage }
        })
        .collect()
}

/// Resolution of an archetype's drag-and-drop into the play area. A **distinct**
/// system, ordered after `panels::dock`: the central rect feeding
/// [`crate::panels::pointer_over_ui`] is then current, otherwise a drop over a panel
/// (bottom or left) would place an entity hidden under the UI. `viewport_to_world_2d`
/// accounts for the viewport's offset (centered sim, cf. `set_sim_camera`) → the
/// window cursor remains the correct input.
pub fn resolve_drag(
    mut contexts: EguiContexts,
    central: Res<crate::panels::CentralRect>,
    mut palette: ResMut<Palette>,
    mut commands: Commands,
    config: Res<SimConfig>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
) -> Result {
    let Some(i) = palette.dragging else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    if ctx.input(|input| input.pointer.any_released()) {
        // Dropped outside any egui panel = above the play area.
        // `let` chain (edition 2024): camera, window, cursor, world.
        if !crate::panels::pointer_over_ui(ctx, central.0)
            && let Ok((camera, cam_tf)) = cameras.single()
            && let Ok(window) = windows.single()
            && let Some(cursor) = window.cursor_position()
            && let Ok(world) = camera.viewport_to_world_2d(cam_tf, cursor)
        {
            place(&mut commands, &config, &mut palette, i, world);
        }
        palette.dragging = None;
    }

    Ok(())
}

/// "Selector" section: the list of the **scenario's archetypes** (drag to place,
/// click to edit), plus creation (agent / food) and deletion. The list *is*
/// [`SimConfig::archetypes`] — creating or deleting here therefore modifies the
/// scenario directly.
pub(crate) fn selector_section(
    ui: &mut egui::Ui,
    palette: &mut Palette,
    config: &mut SimConfig,
    status: &mut UiStatus,
) {
    help::hint(
        ui,
        "Drag into the area to place; click to edit; Delete (cursor on an entity) to remove.",
    );
    ui.separator();
    let mut started = None;
    let mut clicked = None;
    for (i, arch) in config.archetypes.iter().enumerate() {
        // The label is a LayoutJob so the Phosphor marker icons sit in the icon family
        // while the name stays in Inter — all in the archetype's colour. Marker: a
        // caret for the selected one, a dot otherwise; a sparkle suffix for captured
        // weights (cf. `Archetype::capture`).
        let color = archetype_color32(arch);
        let icon_fmt = egui::TextFormat {
            font_id: egui::FontId::new(14.0, fonts::phosphor()),
            color,
            valign: egui::Align::Center,
            ..Default::default()
        };
        let text_fmt = egui::TextFormat {
            font_id: egui::FontId::new(14.0, egui::FontFamily::Proportional),
            color,
            valign: egui::Align::Center,
            ..Default::default()
        };
        let mark = if palette.selected == Some(i) {
            icons::CARET_RIGHT
        } else {
            icons::CIRCLE
        };
        let suffix = if arch.is_sessile() { " · sessile" } else { "" };
        let mut label = egui::text::LayoutJob::default();
        label.append(&mark.to_string(), 0.0, icon_fmt.clone());
        label.append(&format!("  {}{suffix}", arch.name), 0.0, text_fmt);
        if arch.captured_brain.is_some() {
            label.append(&format!("  {}", icons::SPARKLE), 0.0, icon_fmt);
        }
        let mut resp = ui.add_sized(
            [ui.available_width(), 28.0],
            egui::Button::new(label).sense(egui::Sense::click_and_drag()),
        );
        if let Some(from) = &arch.captured_from {
            resp = resp.on_hover_text(format!("Weights captured from {from}"));
        }
        if resp.drag_started() {
            started = Some(i);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    if started.is_some() {
        palette.dragging = started;
    }
    if clicked.is_some() {
        palette.selected = clicked;
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(fonts::icon_label(icons::PLUS, "Agent")).clicked() {
            config
                .archetypes
                .push(Archetype::new_agent(config.archetypes.len()));
            palette.selected = Some(config.archetypes.len() - 1);
        }
        if ui.button(fonts::icon_label(icons::PLUS, "Food")).clicked() {
            config
                .archetypes
                .push(Archetype::new_food(config.archetypes.len()));
            palette.selected = Some(config.archetypes.len() - 1);
        }
    });

    // Operations on the **selected** archetype: duplicate and reorder (like the
    // deletion below, they act on the selection).
    if let Some(i) = palette.selected {
        let count = config.archetypes.len();
        ui.horizontal(|ui| {
            if ui
                .button(fonts::icon_label(icons::COPY, "Duplicate"))
                .on_hover_text(
                    "Clones the selected archetype at the end of the list (relations not copied).",
                )
                .clicked()
            {
                palette.selected = duplicate_archetype(config, i);
            }
            // Reorder = swap with the neighbor + transpose the relation indices.
            // Like deletion, the structural change fully takes effect at the world
            // (re)generation (⟲ of the bar); the relations, for their part, are fixed
            // right away (the saved scenario stays correct).
            if ui
                .add_enabled(
                    i > 0,
                    egui::Button::new(fonts::icon_label(icons::ARROW_UP, "Move up")),
                )
                .on_hover_text("Swaps with the previous archetype (remaps the relations).")
                .clicked()
            {
                swap_archetypes(config, i, i - 1);
                palette.selected = Some(i - 1);
            }
            if ui
                .add_enabled(
                    i + 1 < count,
                    egui::Button::new(fonts::icon_label(icons::ARROW_DOWN, "Move down")),
                )
                .on_hover_text("Swaps with the next archetype (remaps the relations).")
                .clicked()
            {
                swap_archetypes(config, i, i + 1);
                palette.selected = Some(i + 1);
            }
        });
    }

    if let Some(i) = palette.selected
        && config.archetypes.len() > 1
        && ui
            .button(fonts::icon_label(
                icons::TRASH,
                "Delete the selected archetype",
            ))
            .on_hover_text("Removes the archetype and remaps the relation table.")
            .clicked()
    {
        remove_archetype(config, i);
        palette.selected = None;
    }

    ui.separator();
    species_library_section(ui, palette, config, status);

    if palette.dragging.is_some() {
        ui.separator();
        ui.weak("Release above the area to drop.");
    }
}

/// Sync state of an imported archetype relative to its `source` file (the resync
/// indicator). `None` when the archetype has no source (it is local to the scenario).
enum SyncState {
    /// Body/brain/… already match the source (only the local count may differ).
    InSync,
    /// The source file defines something different — a resync would change it.
    Changed,
    /// The `source` file is gone (moved / deleted).
    Missing,
}

/// `true` if `arch` already matches the library `def`: what a resync would yield
/// ([`merge_species_def`]) equals `arch`, so a difference in the *count* (preserved by
/// resync) never reads as out of sync. The shared sync test, used both for an open
/// scenario's archetype and for a scenario's copy during the cross-scenario scan.
fn species_in_sync(arch: &Archetype, def: &Archetype) -> bool {
    let mut resynced = arch.clone();
    merge_species_def(
        &mut resynced,
        def.clone(),
        arch.source.clone().unwrap_or_default(),
    );
    resynced == *arch
}

/// Compares archetype `arch` against its `source` *file*: loads the definition and
/// asks [`species_in_sync`] what a resync would change.
fn species_sync_state(arch: &Archetype) -> Option<SyncState> {
    let src = arch.source.clone()?;
    Some(match Archetype::from_ron_file(&src) {
        Ok(loaded) if species_in_sync(arch, &loaded) => SyncState::InSync,
        Ok(_) => SyncState::Changed,
        Err(_) => SyncState::Missing,
    })
}

/// Display name of a RON path: its file stem (e.g. `species/hunter.ron` → `hunter`,
/// `scenarios/predator_prey.ron` → `predator_prey`).
fn display_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

/// **Species library** (item 4 + the §9 cross-scenario step): make a species reusable
/// across scenarios. Three layers, all on the **copy** model (a scenario stays
/// self-contained and reproducible — the deliberate fork kept over reference):
/// - the **selected archetype** — **Export** it to `species/<name>.ron`, and (if
///   imported) its **sync state** + a one-species **Resync**;
/// - **propagation into the open scenario** — **Update all from library** (bulk resync
///   of every imported species, each local count preserved);
/// - the **catalog** — every `species/*.ron` with a color swatch, brain, an **Import**
///   (copy), and its **cross-scenario usage** (how many scenarios import it, which, and
///   whether each copy is still in sync), cached by [`scan_library`].
fn species_library_section(
    ui: &mut egui::Ui,
    palette: &mut Palette,
    config: &mut SimConfig,
    status: &mut UiStatus,
) {
    let resp = egui::CollapsingHeader::new("Species library")
        .default_open(false)
        .show(ui, |ui| {
            help::hint(
                ui,
                "Reusable species saved as species/*.ron: export the selected archetype, \
                 import a COPY into this scenario, or resync imported ones from their source.",
            );

            // ── Selected archetype: export it, and (if imported) its sync state + resync.
            if let Some(i) = palette.selected.filter(|&i| i < config.archetypes.len()) {
                let state = species_sync_state(&config.archetypes[i]);
                ui.horizontal(|ui| {
                    if ui
                        .button(fonts::icon_label(icons::UPLOAD, "Export selection"))
                        .on_hover_text("Saves the archetype as a reusable species/*.ron.")
                        .clicked()
                    {
                        status.set(export_species(&config.archetypes[i]));
                        palette.library = scan_library();
                    }
                    if state.is_some()
                        && ui
                            .button(fonts::icon_label(icons::SYNC, "Resync"))
                            .on_hover_text(
                                "Reload the source's definition (keeps the local count).",
                            )
                            .clicked()
                    {
                        status.set(sync_species(config, i));
                        palette.library = scan_library();
                    }
                });
                // Provenance + sync indicator for an imported archetype.
                if let (Some(state), Some(src)) = (state, config.archetypes[i].source.clone()) {
                    let (msg, color) = match state {
                        SyncState::InSync => ("in sync", ui.visuals().weak_text_color()),
                        SyncState::Changed => (
                            "source changed — resync available",
                            ui.visuals().warn_fg_color,
                        ),
                        SyncState::Missing => ("source missing", ui.visuals().error_fg_color),
                    };
                    ui.small(egui::RichText::new(format!("↧ {src} · {msg}")).color(color));
                }
            } else {
                help::hint(ui, "Select an archetype to export or resync it.");
            }

            // ── Propagation INTO the open scenario: bulk resync every imported species.
            ui.separator();
            let imported = config
                .archetypes
                .iter()
                .filter(|a| a.source.is_some())
                .count();
            ui.add_enabled_ui(imported > 0, |ui| {
                if ui
                    .button(fonts::icon_label(icons::SYNC, "Update all from library"))
                    .on_hover_text(
                        "Resync every imported species in this scenario from its source \
                         file (each local count preserved).",
                    )
                    .clicked()
                {
                    status.set(sync_all_species(config));
                }
            });
            ui.small(format!("{imported} imported species in this scenario."));

            // ── Catalog: available species + cross-scenario usage. The list refreshes
            // itself when the section opens (see below) — no manual rescan button.
            ui.separator();
            ui.strong("Available species");
            if palette.library.is_empty() {
                help::hint(
                    ui,
                    "No species/*.ron yet — export an archetype to create one.",
                );
            }

            // Sources already imported in THIS scenario, to flag the catalog rows.
            let here: HashSet<String> = config
                .archetypes
                .iter()
                .filter_map(|a| a.source.clone())
                .collect();

            let mut to_import = None;
            for entry in &palette.library {
                ui.horizontal(|ui| {
                    if ui
                        .button(fonts::icon(icons::DOWNLOAD))
                        .on_hover_text("Import a COPY into the scenario")
                        .clicked()
                    {
                        to_import = Some(entry.path.clone());
                    }
                    match &entry.def {
                        Some(def) => {
                            // Color swatch + name + brain kind.
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 2.0, archetype_color32(def));
                            ui.label(display_name(&entry.path));
                            ui.weak(def.brain.name());
                        }
                        None => {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!("{} (unreadable)", display_name(&entry.path)),
                            );
                        }
                    }
                    // Right-aligned: "imported here" marker + cross-scenario usage badge.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if here.contains(&entry.path) {
                            ui.weak("· here");
                        }
                        if !entry.usage.is_empty() {
                            let outdated = entry.usage.iter().filter(|u| !u.in_sync).count();
                            if outdated > 0 {
                                ui.colored_label(ui.visuals().warn_fg_color, "⚠");
                            }
                            ui.weak(format!("used in {}", entry.usage.len()))
                                .on_hover_ui(|ui| {
                                    for u in &entry.usage {
                                        let mark = if u.in_sync { "in sync" } else { "outdated" };
                                        ui.label(format!("{} — {mark}", display_name(&u.scenario)));
                                    }
                                });
                        }
                    });
                });
            }
            if let Some(path) = to_import {
                status.set(import_species(config, &path));
                palette.selected = Some(config.archetypes.len().saturating_sub(1));
                palette.library = scan_library();
            }
        });

    // The catalog refreshes itself **when the section opens** (no manual reload button —
    // the same pattern as the Scenario menu, cf. `runs::scenario_section`): detect the
    // closed→open transition and rescan once. The scan reads every species and scenario
    // file, so it stays off the per-frame path.
    let open = resp.openness > 0.5;
    if open && !palette.library_was_open {
        palette.library = scan_library();
    }
    palette.library_was_open = open;
}

/// Removes archetype `i` and **remaps the relation table**: relations that
/// reference it (actor or target) are removed, and any higher index is decremented
/// — otherwise the archetype indices would point to the wrong species.
fn remove_archetype(config: &mut SimConfig, i: usize) {
    config.archetypes.remove(i);
    let removed = i as u16;
    config
        .relations
        .retain(|r| r.actor != removed && r.target != removed);
    for r in &mut config.relations {
        if r.actor > removed {
            r.actor -= 1;
        }
        if r.target > removed {
            r.target -= 1;
        }
    }
}

/// Duplicates archetype `i`: an **independent** clone added **at the end** of the
/// list — therefore without shifting the existing indices, and the relation table
/// stays intact (cf. [`swap_archetypes`] which, for its part, must remap). The
/// clone does **not** carry over the original's relations (to be rewired by hand);
/// its name gains " (copy)". Returns the clone's index to select it. `None` if `i`
/// is out of list.
fn duplicate_archetype(config: &mut SimConfig, i: usize) -> Option<usize> {
    let mut clone = config.archetypes.get(i)?.clone();
    clone.name = format!("{} (copy)", clone.name);
    config.archetypes.push(clone);
    Some(config.archetypes.len() - 1)
}

/// Swaps archetypes `i` and `j` (reordering) and **transposes** their indices in
/// the relation table: an archetype's index *is* its species identity
/// ([`Species`]), so swapping two archetypes without touching the relations would
/// make them point to the wrong species. The exact counterpart, for reordering, of
/// the remap [`remove_archetype`] does for deletion.
fn swap_archetypes(config: &mut SimConfig, i: usize, j: usize) {
    config.archetypes.swap(i, j);
    let (i, j) = (i as u16, j as u16);
    let transpose = |x: &mut u16| {
        if *x == i {
            *x = j;
        } else if *x == j {
            *x = i;
        }
    };
    for r in &mut config.relations {
        transpose(&mut r.actor);
        transpose(&mut r.target);
    }
}

/// Exports an archetype as a **reusable species** to `species/<name>.ron`. The
/// `source` field is cleared: the exported file *is* the source (no self-reference
/// if an imported species is re-exported). Returns a status message for the UI.
fn export_species(arch: &Archetype) -> String {
    let _ = std::fs::create_dir_all("species");
    let path = format!("species/{}.ron", sanitize_filename(&arch.name));
    let mut def = arch.clone();
    def.source = None;
    match def.save_ron_file(&path) {
        Ok(()) => format!("Species exported → {path}"),
        Err(e) => format!("Export failed: {e}"),
    }
}

/// Imports a species: a **copy** joins the scenario (which stays self-contained,
/// §9), retaining the file as `source` (for resyncing). Added at the end of the
/// list, hence without shifting the relation indices. Returns a status message.
fn import_species(config: &mut SimConfig, path: &str) -> String {
    match Archetype::from_ron_file(path) {
        Ok(mut arch) => {
            arch.source = Some(path.to_string());
            config.archetypes.push(arch);
            format!("Species imported (copy) ← {path}")
        }
        Err(e) => format!("Import failed: {e}"),
    }
}

/// Resyncs archetype `i` from its `source` file: reloads the definition and
/// reapplies it while **keeping the local count** (`count`). Returns a status message.
fn sync_species(config: &mut SimConfig, i: usize) -> String {
    let Some(src) = config.archetypes[i].source.clone() else {
        return "This archetype has no source to sync.".to_string();
    };
    match Archetype::from_ron_file(&src) {
        Ok(loaded) => {
            merge_species_def(&mut config.archetypes[i], loaded, src.clone());
            format!("Species resynced ← {src}")
        }
        Err(e) => format!("Sync failed: {e}"),
    }
}

/// Resyncs **every** imported archetype of the open scenario from its `source` file
/// (propagation *into* this scenario) — each local count preserved
/// ([`merge_species_def`]), already-in-sync species untouched. Returns a status
/// message summarizing how many were updated (and any missing source). The change is
/// in-memory: the user saves to persist it (the scenario stays self-contained).
fn sync_all_species(config: &mut SimConfig) -> String {
    let (mut updated, mut missing, mut linked) = (0usize, 0usize, 0usize);
    for i in 0..config.archetypes.len() {
        let Some(src) = config.archetypes[i].source.clone() else {
            continue;
        };
        linked += 1;
        match Archetype::from_ron_file(&src) {
            Ok(loaded) => {
                if !species_in_sync(&config.archetypes[i], &loaded) {
                    merge_species_def(&mut config.archetypes[i], loaded, src);
                    updated += 1;
                }
            }
            Err(_) => missing += 1,
        }
    }
    match (linked, updated, missing) {
        (0, _, _) => "No imported species to update.".to_string(),
        (_, 0, 0) => "All imported species already in sync.".to_string(),
        (_, n, 0) => format!("Updated {n} species from the library."),
        (_, n, m) => format!("Updated {n} species ({m} source missing)."),
    }
}

/// Reapplies a species definition `loaded` onto `target`, **preserving the count**
/// (`count`, specific to the scenario) and re-setting the `source` link. The rest
/// (body, brain, color, name, mutability) comes from the definition. Pure (no I/O)
/// → testable; [`sync_species`] only adds the file read to it.
fn merge_species_def(target: &mut Archetype, loaded: Archetype, source: String) {
    let count = target.count;
    *target = loaded;
    target.count = count;
    target.source = Some(source);
}

/// Cleans a species name into a safe filename: we keep letters/digits, `-` and
/// `_`, everything else becomes `_`; an empty name falls back to "species".
fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "species".to_string()
    } else {
        s
    }
}

/// "Editor" section: editing the selected archetype's genes. Rendered under the
/// selector. Makes the distinction explicit: **archetype** (the model edited here)
/// / **genome** (the copy inherited by each instance, which then mutates on its own).
pub(crate) fn editor_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    match palette.selected {
        Some(i) if i < config.archetypes.len() => {
            archetype_editor(ui, config, i, &mut palette.show_mutability)
        }
        _ => {
            ui.label("Click an archetype in the palette to edit it (or create one).");
        }
    }
    // Editor feedback (archetype capture, species import/export) now funnels into the
    // unified status line in the bottom bar (cf. `crate::status`), not here.
}

/// Editor of **archetype `i`**: common properties (name, color, count, size,
/// reserve) then genes + mutability (per species) + brain. Since Phase 3b there is
/// no more type branch: a food source is an archetype with a `Sessile` brain,
/// editable like the others. Writes *directly* into `config.archetypes[i]`
/// (persisted by "Save").
fn archetype_editor(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    i: usize,
    show_mutability: &mut bool,
) {
    // (Global) bounds captured before borrowing `config.archetypes` mutably.
    let trait_bounds: Vec<_> = TRAITS.iter().map(|t| (t.bounds)(config)).collect();
    // Brain type BEFORE editing: if the user changes the topology or the type while
    // weights had been captured, those weights no longer match → we clear them (cf.
    // end of function).
    let brain_before = config.archetypes[i].brain.clone();

    // BODY — identity (name, colour) then the spawn / physical parameters, in a framed
    // card. Laid out in a two-column grid so the labels line up.
    ui.group(|ui| {
        let arch = &mut config.archetypes[i];
        ui.strong("Body");
        egui::Grid::new("body_fields")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("name");
                ui.add(egui::TextEdit::singleline(&mut arch.name).desired_width(f32::INFINITY));
                ui.end_row();

                ui.label("colour");
                color_button(ui, &mut arch.color);
                ui.end_row();

                ui.label("count");
                fonts::value(ui, |ui| {
                    ui.add(egui::DragValue::new(&mut arch.count).range(0..=5000))
                        .on_hover_text("How many to spawn (applied on the next reset).")
                });
                ui.end_row();

                ui.label("body radius");
                fonts::value(ui, |ui| {
                    ui.add(egui::Slider::new(&mut arch.radius, 2.0..=30.0))
                });
                ui.end_row();

                ui.label("max reserve");
                fonts::value(ui, |ui| {
                    ui.add(egui::Slider::new(&mut arch.reserve_max, 10.0..=500.0))
                        .on_hover_text("Energy ceiling: the reserve is clamped to it.")
                });
                ui.end_row();
            });
        help::hint(
            ui,
            "Count, radius and reserve are baked at spawn, applied on the next reset (⟲).",
        );
    });

    // GENES — the founding genotype + per-species mutability, in a framed card. Every
    // archetype is an agent (Phase 3b); a *food source* is just one with a Sessile
    // brain, editable through these same controls.
    ui.group(|ui| {
        let arch = &mut config.archetypes[i];
        ui.strong("Genes");
        help::hint(
            ui,
            "Each placed agent receives a COPY of these genes — its genome — which then \
             mutates on its own.",
        );
        // Mutability toggle at the **top** of the panel (discoverable): when on, a
        // checkbox appears beside each gene to mark it MUTABLE.
        ui.checkbox(show_mutability, "Edit mutability")
            .on_hover_text(
                "Show a checkbox beside each gene: checked ⇒ MUTABLE (drifts at \
             reproduction, passed on with variation); unchecked ⇒ transmitted but \
             frozen at the founder's value.",
            );
        // An immobile entity (zero max speed, e.g. a flora / sessile source) neither
        // moves nor exploits vision: its locomotion and vision genes are inert, so we
        // do not expose them (cf. `TraitSpec::inert_when_immobile`). `max_speed`, for
        // its part, stays shown — it is the mobility switch.
        let immobile = arch.genotype.locomotion().is_immobile();
        if immobile {
            help::hint(
                ui,
                "Immobile: locomotion and vision genes hidden (no effect).",
            );
        }

        // The gene editor sits several nested cards deep (Entities › Archetype editor
        // › Genes › category); trim the per-category indent so the gene rows keep their
        // width (egui side panels don't grow to fit content — cf. `panels`).
        ui.spacing_mut().indent = 10.0;

        // Grouped by GeneCategory (taming the 17-gene wall): one CollapsingHeader per
        // category, in display order, filtering TRAITS by category. Still a single
        // pass over TRAITS — adding a gene only needs a category, no line here
        // (item 15). A category with no visible gene (e.g. Locomotion/Vision when
        // immobile) draws no header.
        for cat in GeneCategory::ALL {
            let mut visible: Vec<usize> = TRAITS
                .iter()
                .enumerate()
                .filter(|(_, t)| t.category == cat && !(immobile && t.inert_when_immobile))
                .map(|(idx, _)| idx)
                .collect();
            if visible.is_empty() {
                continue;
            }
            // Costs sort to the **bottom** of each category — a uniform reading order
            // (a stable sort keeps the TRAITS order within the cost / non-cost groups,
            // so it is robust to where a future gene lands in TRAITS).
            visible.sort_by_key(|&idx| TRAITS[idx].is_cost);
            egui::CollapsingHeader::new(cat.label())
                .default_open(cat.default_open(immobile))
                .show(ui, |ui| {
                    for &idx in &visible {
                        let t = &TRAITS[idx];
                        let bounds = &trait_bounds[idx];
                        // One row per gene: when "Edit mutability" is on, a compact
                        // checkbox **beside** the gene (an aligned column on the left),
                        // then the slider with its name fills the rest of the row.
                        ui.horizontal(|ui| {
                            if *show_mutability {
                                let mut m = (t.mutable)(&arch.mutable);
                                if ui
                                    .checkbox(&mut m, "")
                                    .on_hover_text(
                                        "Mutable: this gene drifts at reproduction (passed \
                                         on with variation). Unchecked: transmitted but \
                                         frozen at the founder's value.",
                                    )
                                    .changed()
                                {
                                    (t.set_mutable)(&mut arch.mutable, m);
                                }
                            }
                            let mut value = (t.get)(&arch.genotype);
                            // Slider value in Departure Mono; the gene name (label)
                            // stays Inter, after the slider.
                            let changed = fonts::value(ui, |ui| {
                                ui.add(egui::Slider::new(&mut value, bounds.min..=bounds.max))
                                    .changed()
                            });
                            if changed {
                                (t.set)(&mut arch.genotype, value);
                            }
                            ui.label(t.name);
                        });
                    }
                });
        }
    });

    // BRAIN — the decision's author + any captured weights, in a framed card.
    ui.group(|ui| {
        let arch = &mut config.archetypes[i];
        ui.strong("Brain");
        let rays = arch.genotype.ray_count();
        brain_kind_editor(ui, &mut arch.brain, rays);

        // Body ↔ brain coherence: a moving decider on an immobile body (or a Sessile
        // brain on a mobile body) is almost always a mistake — surface it (the
        // plant-aware theme of the gene editor) without forbidding it.
        let immobile = arch.genotype.locomotion().is_immobile();
        let moves = !matches!(arch.brain, BrainKind::Sessile);
        let warn = ui.visuals().warn_fg_color;
        if immobile && moves {
            ui.colored_label(
                warn,
                "The body can't move (max speed 0) — this decider won't move it. \
                 Pick Sessile, or give the body some speed.",
            );
        } else if !immobile && !moves {
            ui.colored_label(
                warn,
                "Sessile never acts — this mobile body will just sit still.",
            );
        }

        // Topology/type changed while weights were captured → they have become
        // inconsistent (fan-in/shape), we clear them rather than spawning mute brains.
        if arch.brain != brain_before && arch.captured_brain.is_some() {
            arch.captured_brain = None;
            arch.captured_from = None;
        }
        if let Some(from) = arch.captured_from.clone() {
            ui.separator();
            ui.weak(format!("Weights captured from {from}"));
            if ui
                .button("Clear the captured weights")
                .on_hover_text("The founders will restart from fresh weights (the brain recipe).")
                .clicked()
            {
                arch.captured_brain = None;
                arch.captured_from = None;
            }
        }
    });
}

/// Edits **a** [`BrainKind`]: type combo (selection by *kind*, so as not to reset
/// the current variant's parameters), variant-specific parameters, then its
/// functional description. The exhaustive `match` forces every future `Brain`
/// variant to expose its parameters here — the *heterogeneous* counterpart
/// (parameters specific to each brain) of the homogeneous `TRAITS` table.
///
/// `vision_rays` only serves the MLP, to display the (constrained) size of its
/// input layer.
fn brain_kind_editor(ui: &mut egui::Ui, kind: &mut BrainKind, vision_rays: usize) {
    ui.horizontal(|ui| {
        ui.label("decider");
        egui::ComboBox::from_id_salt("brain_kind")
            .selected_text(kind.name())
            .show_ui(ui, |ui| {
                let is_wander = matches!(kind, BrainKind::Wander { .. });
                if ui.selectable_label(is_wander, "Wander").clicked() && !is_wander {
                    *kind = BrainKind::default();
                }
                let is_hunter = matches!(kind, BrainKind::Hunter);
                if ui.selectable_label(is_hunter, "Hunter").clicked() && !is_hunter {
                    *kind = BrainKind::Hunter;
                }
                let is_sessile = matches!(kind, BrainKind::Sessile);
                if ui.selectable_label(is_sessile, "Sessile").clicked() && !is_sessile {
                    *kind = BrainKind::Sessile;
                }
                let is_mlp = matches!(kind, BrainKind::Mlp { .. });
                if ui.selectable_label(is_mlp, "Network (MLP)").clicked() && !is_mlp {
                    *kind = BrainKind::Mlp { hidden: vec![8] };
                }
            });
    });
    match kind {
        BrainKind::Wander { turn_rate } => {
            ui.horizontal(|ui| {
                fonts::value(ui, |ui| ui.add(egui::Slider::new(turn_rate, 0.0..=1.0)))
                    .on_hover_text("Max amplitude of the heading drift each tick (rad).");
                ui.label("turn responsiveness");
            });
        }
        BrainKind::Hunter | BrainKind::Sessile => {}
        BrainKind::Mlp { hidden } => mlp_architecture_editor(ui, hidden, vision_rays),
    }
    help::hint(ui, kind.description());
}

/// **Numeric** editing of an MLP's architecture (item 18b, core): the number of
/// hidden layers and the width of each. The input (`3 × rays`: vision, target,
/// threat) and the output (2) are *constrained* by the contract and only displayed.
fn mlp_architecture_editor(ui: &mut egui::Ui, hidden: &mut Vec<usize>, vision_rays: usize) {
    help::hint(
        ui,
        format!(
            "Input {} at the founder (= 3 × {vision_rays} rays: vision, target, threat) to \
             output {} (contract). The input layer then adapts to each individual's \
             visual precision (gene \"Rays\").",
            MlpBrain::input_size(vision_rays),
            MlpBrain::OUTPUTS,
        ),
    );
    let mut remove = None;
    for (i, n) in hidden.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("hidden layer {}", i + 1));
            fonts::value(ui, |ui| ui.add(egui::DragValue::new(n).suffix(" neurons")));
            *n = (*n).clamp(1, 64); // at least one neuron, reasonable ceiling.
            if ui
                .small_button(fonts::icon(icons::X))
                .on_hover_text("remove this layer")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        hidden.remove(i); // 0 hidden layers = a simple perceptron, valid.
    }
    if ui
        .button(fonts::icon_label(icons::PLUS, "Hidden layer"))
        .clicked()
    {
        hidden.push(8);
    }

    // Structural preview of the network (item 18b-viz): input → hidden → output. No
    // activations here (we edit a *type*, not a living brain) — it is the inspector
    // that shows the network in action.
    let mut sizes = vec![MlpBrain::input_size(vision_rays)];
    sizes.extend_from_slice(hidden);
    sizes.push(MlpBrain::OUTPUTS);
    draw_mlp_graph(ui, &sizes, None, None);
}

/// Color of a node by its activation `v`: cold (blue) for negative, warm (orange)
/// for positive, dark gray at rest — the natural scale of a `tanh` (∈ [-1, 1]; the
/// input ∈ [0, 1] falls on the warm side). `None` → neutral node (structural
/// preview, without activation).
fn activation_color(v: Option<f32>) -> egui::Color32 {
    let Some(v) = v else {
        return egui::Color32::from_gray(110);
    };
    let t = v.clamp(-1.0, 1.0).abs();
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    let base = 60; // resting gray
    if v >= 0.0 {
        egui::Color32::from_rgb(lerp(base, 240), lerp(base, 150), lerp(base, 40)) // warm
    } else {
        egui::Color32::from_rgb(lerp(base, 60), lerp(base, 140), lerp(base, 240)) // cold
    }
}

/// Draws an MLP as a **graph** (item 18b-viz): one column of nodes per layer (input
/// on the left, output on the right), edges between consecutive layers.
///
/// - `brain = Some(mlp)` (inspector): edges tinted by the **sign/intensity of the
///   weight** (blue negative, orange positive) — the real network. The nodes are
///   **sized by their |bias|** (normalized to the network's largest bias; the
///   input, without a bias, stays at the reference size). With `activations`
///   (computed on demand by [`MlpBrain::forward_activations`]), the nodes are
///   colored by their **current activation**: the network "in action".
/// - `brain = None` (editor): structural preview, neutral nodes and faint edges.
///
/// `sizes` gives the number of nodes per column (input included). Separating weights
/// (`brain`) and activations (`activations`) lets `MlpBrain::think` no longer carry
/// transient state: it is the inspector that replays the propagation.
pub(crate) fn draw_mlp_graph(
    ui: &mut egui::Ui,
    sizes: &[usize],
    brain: Option<&MlpBrain>,
    activations: Option<&[Vec<f32>]>,
) {
    use egui::{Pos2, Sense, Stroke, vec2};
    if sizes.len() < 2 {
        return;
    }
    let cols = sizes.len();
    let widest = *sizes.iter().max().unwrap_or(&1);
    // Height proportional to the widest column, bounded to stay compact.
    let height = (widest as f32 * 16.0).clamp(60.0, 220.0);
    let width = ui.available_width().max(180.0);
    let (resp, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    // Horizontal margins reserved for the input (left) and output (right) labels;
    // small vertical margins. The node columns live in `rect`.
    const LABEL_W: f32 = 42.0;
    let rect = egui::Rect::from_min_max(
        egui::pos2(resp.rect.left() + LABEL_W, resp.rect.top() + 8.0),
        egui::pos2(resp.rect.right() - LABEL_W, resp.rect.bottom() - 8.0),
    );

    // Position of node `node` (0-based) of column `col`.
    let pos = |col: usize, node: usize, n: usize| -> Pos2 {
        let x = if cols == 1 {
            rect.center().x
        } else {
            rect.left() + rect.width() * col as f32 / (cols - 1) as f32
        };
        let y = if n == 1 {
            rect.center().y
        } else {
            rect.top() + rect.height() * (node as f32 + 0.5) / n as f32
        };
        Pos2::new(x, y)
    };

    // Edges first (under the nodes). In live mode, tint/thickness by weight.
    for col in 0..cols - 1 {
        let (from_n, to_n) = (sizes[col], sizes[col + 1]);
        let weights = brain.and_then(|m| (col < m.weight_layers()).then(|| m.layer_weights(col)));
        for o in 0..to_n {
            for i in 0..from_n {
                let stroke = match weights {
                    Some((w, fan_in, _)) if i + o * fan_in < w.len() => {
                        let wt = w[o * fan_in + i];
                        let a = (wt.abs() * 0.9).clamp(0.04, 0.9);
                        let c = if wt >= 0.0 {
                            egui::Color32::from_rgb(230, 150, 60)
                        } else {
                            egui::Color32::from_rgb(70, 140, 230)
                        };
                        Stroke::new(1.0, c.gamma_multiply(a))
                    }
                    _ => Stroke::new(0.5, egui::Color32::from_gray(80)),
                };
                painter.line_segment([pos(col, i, from_n), pos(col + 1, o, to_n)], stroke);
            }
        }
    }

    // Nodes on top, colored by activation (live) or neutral (preview). Their
    // **size** encodes the neuron's |bias| (the only "weight" specific to a node):
    // we normalize by the network's largest |bias|, the most biased neuron taking
    // the reference radius and the others shrinking. The input column (without a
    // bias) keeps the reference radius. In preview (`brain = None`) or as long as
    // all biases are zero (founder network, biases initialized to 0), all nodes stay
    // at this reference.
    let base_radius = (rect.height() / (widest as f32 * 2.2)).clamp(2.5, 8.0);
    let max_bias = brain
        .map(|m| {
            (0..m.weight_layers())
                .flat_map(|l| m.layer_biases(l).iter().copied())
                .fold(0.0_f32, |acc, b| acc.max(b.abs()))
        })
        .filter(|&m| m > 1e-6);
    for (col, &n) in sizes.iter().enumerate() {
        // Biases of this column: column `col` is fed by layer `col-1` (column 0, the
        // input, has none).
        let biases = brain.and_then(|m| {
            (col >= 1 && col - 1 < m.weight_layers()).then(|| m.layer_biases(col - 1))
        });
        for node in 0..n {
            let act = activations
                .and_then(|a| a.get(col))
                .and_then(|layer| layer.get(node))
                .copied();
            let radius = match (biases, max_bias) {
                (Some(b), Some(mx)) => {
                    let t = (b.get(node).copied().unwrap_or(0.0).abs() / mx).clamp(0.0, 1.0);
                    base_radius * (0.35 + 0.65 * t)
                }
                _ => base_radius,
            };
            let center = pos(col, node, n);
            painter.circle_filled(center, radius, activation_color(act));
            painter.circle_stroke(
                center,
                radius,
                Stroke::new(0.6, egui::Color32::from_gray(25)),
            );
        }
    }

    // Labels of the input / output channels — "what it corresponds to", derived
    // from the MLP's I/O contract: the input concatenates the *vision* (obstacle),
    // *target* then *threat* channels (cf. `MlpBrain::input_vector`); the output is
    // the steering in body frame (forward, side). Drawn in the reserved margins on
    // either side, at the height of each relevant node.
    let font = egui::FontId::monospace(8.0);
    let ink = egui::Color32::from_gray(165);
    let n_in = sizes[0];
    let rays = n_in / 3; // input = 3 × rays (vision ++ target ++ threat)
    for node in 0..n_in {
        let text = if n_in.is_multiple_of(3) {
            match node / rays {
                0 => format!("vis {node}"),
                1 => format!("tgt {}", node - rays),
                _ => format!("thr {}", node - 2 * rays),
            }
        } else {
            format!("in {node}")
        };
        let p = pos(0, node, n_in);
        painter.text(
            egui::pos2(rect.left() - 4.0, p.y),
            egui::Align2::RIGHT_CENTER,
            text,
            font.clone(),
            ink,
        );
    }
    let last = cols - 1;
    let n_out = sizes[last];
    for node in 0..n_out {
        let text = match (n_out == MlpBrain::OUTPUTS).then_some(node) {
            Some(0) => "fwd",
            Some(1) => "side",
            _ => continue, // non-standard output: no guessable label.
        };
        let p = pos(last, node, n_out);
        painter.text(
            egui::pos2(rect.right() + 4.0, p.y),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            ink,
        );
    }
}

/// "Layers" section: toggle the view **calques** — the agents (main layer) and the
/// nutrient concentration **heatmaps** (background, off by default). Purely a
/// rendering concern ([`Layers`]), it never touches the scenario or the sim. The
/// nutrient layers share an opacity budget (`N` active ⇒ `1/N` each), so the label
/// states it for the user.
pub(crate) fn layers_section(ui: &mut egui::Ui, layers: &mut Layers) {
    ui.checkbox(&mut layers.agents, "Agents (main)");
    if !layers.nutrients.is_empty() {
        ui.separator();
        help::hint(ui, "Nutrient maps — background, shared opacity:");
        for (i, on) in layers.nutrients.iter_mut().enumerate() {
            ui.checkbox(on, format!("Nutrient {i}"));
        }
    }
    // The dismissable-help toggle lives in this View menu (a view concern, like the
    // layers) rather than the scenario data.
    ui.separator();
    help::toggle(ui);
}

/// "World" section: the **scenario** parameters (everything but the per-species
/// archetypes), as collapsible cards ordered by how often they're touched —
/// *Arena & generation* and *Relations* open, the rest (Nutrients, Gene bounds,
/// Appearance) collapsed. Direct read/write of the [`SimConfig`], hence persisted by
/// "Save". Some fields only take effect at the next Reset (⟲); relations act **live**.
pub(crate) fn world_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    // ARENA & GENERATION — size and RNG. Open by default. (The sim rate is a scenario
    // file parameter, not exposed here.)
    egui::CollapsingHeader::new("Arena & generation")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                fonts::value(ui, |ui| {
                    ui.add(egui::Slider::new(
                        &mut config.arena_half_extent,
                        100.0..=1000.0,
                    ))
                });
                ui.label("half-arena");
            });
            ui.horizontal(|ui| {
                ui.label("seed");
                fonts::value(ui, |ui| {
                    ui.add(egui::DragValue::new(&mut config.seed).speed(1.0))
                });
            });
            help::hint(
                ui,
                "Seed and arena walls apply on the next Reset (⟲). Population, bodies and \
                 brains live in the \"Archetypes\" panel.",
            );
        });

    // RELATIONS — the interaction table (acts live). Open by default.
    egui::CollapsingHeader::new("Relations")
        .default_open(true)
        .show(ui, |ui| relations_section(ui, config));

    // The advanced cards, collapsed by default (each carries its own header).
    nutrient_section(ui, config);
    gene_bounds_section(ui, config);

    // APPEARANCE — windowed-render backgrounds (read continuously by
    // `main::draw_play_area` → immediate preview, saved with the scenario).
    egui::CollapsingHeader::new("Appearance")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                color_button(ui, &mut config.play_area_color);
                ui.label("inner background (play area)");
            });
            ui.horizontal(|ui| {
                color_button(ui, &mut config.off_game_color);
                ui.label("outer background (off-game)");
            });
        });
}

/// The **nutrients** (T2 substrate): the concentration-field parameters and the
/// emission **sources**. Sources are a *separate category* (not archetypes, not
/// agents), so they live here in the World editor rather than in the archetype
/// palette. Everything here is **"(reset)"**: the field is rebuilt and the sources
/// respawned at the world (re)generation (⟲ of the bar, or "Reload into the world"),
/// the single passage point that also re-applies the field's resolution/diffusion
/// (cf. [`crate::controls::apply_reset`]). Collapsed by default (off for most
/// scenarios — no source ⇒ inert layer).
fn nutrient_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Nutrients")
        .default_open(false)
        .show(ui, |ui| {
            help::hint(
                ui,
                "A finite nutrient bounds REPRODUCTION (Liebig), decoupled from \
                 survival (the sun). The field is fed by the sources below and spread \
                 by diffusion into gradients. All (reset): applied at ⟲.",
            );
            ui.horizontal(|ui| {
                ui.label("grid resolution (reset)");
                fonts::value(ui, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut config.nutrient.resolution)
                            .range(8..=256)
                            .speed(1.0),
                    )
                    .on_hover_text("Cells per side of the nutrient field over the arena.")
                });
            });
            ui.horizontal(|ui| {
                fonts::value(ui, |ui| {
                    ui.add(egui::Slider::new(&mut config.nutrient.diffusion, 0.0..=1.0))
                        .on_hover_text(
                            "Per-tick spread toward neighbours (the local↔global knob); \
                             0 = no spread.",
                        )
                });
                ui.label("diffusion (reset)");
            });

            ui.separator();
            ui.strong("Sources (emit nutrient into the field)");
            help::hint(
                ui,
                "A fixed point emitting `rate`/s of nutrient at its position.",
            );
            let mut to_remove = None;
            for (i, src) in config.sources.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        color_button(ui, &mut src.color);
                        ui.label(format!("source {i}"));
                        if ui
                            .button(fonts::icon(icons::TRASH))
                            .on_hover_text("Remove this source")
                            .clicked()
                        {
                            to_remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("pos");
                        fonts::value(ui, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut src.pos[0])
                                    .speed(1.0)
                                    .prefix("x: "),
                            )
                        });
                        fonts::value(ui, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut src.pos[1])
                                    .speed(1.0)
                                    .prefix("y: "),
                            )
                        });
                    });
                    ui.horizontal(|ui| {
                        fonts::value(ui, |ui| {
                            ui.add(egui::Slider::new(&mut src.rate, 0.0..=100.0))
                        });
                        ui.label("rate/s");
                    });
                    ui.horizontal(|ui| {
                        fonts::value(ui, |ui| {
                            ui.add(egui::Slider::new(&mut src.radius, 1.0..=40.0))
                        });
                        ui.label("visual radius");
                    });
                });
            }
            if let Some(i) = to_remove {
                config.sources.remove(i);
            }
            if ui
                .button(fonts::icon_label(icons::PLUS, "Add a source"))
                .clicked()
            {
                // T2: a single nutrient (index 0). Sensible defaults at the center.
                config.sources.push(Source {
                    pos: [0.0, 0.0],
                    nutrient: 0,
                    rate: 10.0,
                    color: [1.0, 0.6, 0.2],
                    radius: 12.0,
                });
            }
        });
}

/// The **gene bounds** (`*_bounds` of the scenario), edited globally: min/max of
/// each characteristic. They bound the **mutation** (cf. [`Genotype::mutate`]) AND
/// the archetype editor's sliders. Loops over [`TRAITS`] via `bounds_mut`, so adding
/// a gene exposes it here without touching this section (item 15/3). Collapsed by
/// default (an advanced setting, rarely touched — hence "outside the UI" until now).
fn gene_bounds_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Gene bounds")
        .default_open(false)
        .show(ui, |ui| {
            help::hint(
                ui,
                "Min/max of each gene: bound the mutation and the archetype editor's \
                 sliders. Global (shared by all archetypes).",
            );
            egui::Grid::new("gene_bounds_grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("gene");
                    ui.strong("min");
                    ui.strong("max");
                    ui.end_row();
                    for t in &TRAITS {
                        // Drag step suited to the gene's scale (fine for agility,
                        // coarse for speed), via its display decimals.
                        let speed = 10f64.powi(-(t.decimals as i32));
                        let b = (t.bounds_mut)(config);
                        ui.label(t.name);
                        fonts::value(ui, |ui| {
                            ui.add(egui::DragValue::new(&mut b.min).speed(speed))
                        });
                        fonts::value(ui, |ui| {
                            ui.add(egui::DragValue::new(&mut b.max).speed(speed))
                        });
                        // Keep min ≤ max: a negative span would make the mutation's
                        // clamp panic (`f32::clamp` requires min ≤ max).
                        b.max = b.max.max(b.min);
                        ui.end_row();
                    }
                });
        });
}

/// The relation table, **addressed by archetype**: each actor/target is chosen in
/// an archetype menu (name + color). No more bare numbers nor possible collision
/// with the food — which is a full-fledged archetype, with its index.
fn relations_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    help::hint(
        ui,
        "An actor reduces a target's reserve within range — the gap between their \
         bodies, so range = 0 means contact. This is what makes an archetype a \
         TARGET (what Brain::Hunter pursues). transfer = predation (the actor gains \
         the energy); otherwise plain destruction.",
    );
    // Snapshot (name, color) of the archetypes for the menus — captured before
    // borrowing `config.relations` mutably.
    let archs: Vec<(String, egui::Color32)> = config
        .archetypes
        .iter()
        .map(|a| (a.name.clone(), archetype_color32(a)))
        .collect();
    if archs.len() < 2 {
        ui.weak("Create at least two archetypes to define a relation.");
        return;
    }
    let mut to_remove = None;
    for (i, rel) in config.relations.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                archetype_combo(ui, ("rel_actor", i), &mut rel.actor, &archs);
                ui.label(fonts::icon(icons::ARROW_RIGHT));
                archetype_combo(ui, ("rel_target", i), &mut rel.target, &archs);
                if ui
                    .button(fonts::icon(icons::TRASH))
                    .on_hover_text("Remove this relation")
                    .clicked()
                {
                    to_remove = Some(i);
                }
            });
            ui.checkbox(&mut rel.transfer, "transfer (predation)");
            ui.horizontal(|ui| {
                fonts::value(ui, |ui| {
                    ui.add(egui::Slider::new(&mut rel.rate, 0.0..=400.0))
                });
                ui.label("rate/s");
            });
            ui.horizontal(|ui| {
                fonts::value(ui, |ui| {
                    ui.add(egui::Slider::new(&mut rel.range, 0.0..=100.0))
                });
                ui.label("range (0 = contact)");
            });
        });
    }
    if let Some(i) = to_remove {
        config.relations.remove(i);
    }
    if ui
        .button(fonts::icon_label(icons::PLUS, "Add a relation"))
        .clicked()
    {
        // Default: the first mobile agent eats the first sessile source (common case).
        let actor = config
            .archetypes
            .iter()
            .position(|a| !a.is_sessile())
            .unwrap_or(0) as u16;
        let target = config
            .archetypes
            .iter()
            .position(|a| a.is_sessile())
            .unwrap_or(0) as u16;
        config.relations.push(Relation {
            actor,
            target,
            transfer: true,
            rate: 100.0,
            range: 0.0, // contact by default (surface-to-surface clearance).
        });
    }
}

/// A dropdown that selects an **archetype** (by `u16` index) among `archs`,
/// displaying its name in its color. `id` disambiguates the combos of a same frame.
fn archetype_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    value: &mut u16,
    archs: &[(String, egui::Color32)],
) {
    let cur = *value as usize;
    let selected_text = archs
        .get(cur)
        .map(|(n, _)| n.clone())
        .unwrap_or_else(|| format!("#{value}"));
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            for (i, (name, color)) in archs.iter().enumerate() {
                let label = egui::RichText::new(name).color(*color);
                if ui.selectable_label(cur == i, label).clicked() {
                    *value = i as u16;
                }
            }
        });
}

/// Live statistics, rendered in the **Analysis** (right) panel by
/// [`crate::panels::dock`]. Read-only over the world: observation for display, not sim
/// logic. A two-column `name : value` grid suited to the narrow side panel.
pub(crate) fn stats_section(
    ui: &mut egui::Ui,
    agents: &Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
) {
    // Computation shared with the native Bevy visualizer
    // ([`teemlab::metrics::live_stats`]) → same numbers in the egui panel and in the
    // video. Population and gene means cover only the mobile fauna; sessile sources
    // count only in `food` (otherwise their frozen genes would swamp the fauna's
    // drift).
    let stats = metrics::live_stats(agents);
    egui::Grid::new("live_stats")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            // Values in the monospace family (Departure Mono); labels stay Inter.
            ui.label("Population");
            ui.label(egui::RichText::new(stats.population.to_string()).monospace());
            ui.end_row();
            ui.label("Food");
            ui.label(egui::RichText::new(stats.food.to_string()).monospace());
            ui.end_row();
            ui.label("Mean reserve");
            ui.label(egui::RichText::new(format!("{:.0}", stats.mean_reserve)).monospace());
            ui.end_row();
            // One row per TRAITS characteristic (the gene means), without a hard-coded
            // field — adding a gene shows up here automatically.
            for (t, mean) in TRAITS.iter().zip(&stats.mean_traits) {
                ui.label(t.name);
                ui.label(
                    egui::RichText::new(format!("{:.*}", t.decimals as usize, mean)).monospace(),
                );
                ui.end_row();
            }
        });
}

/// Compiles archetype `i` into a living entity, placed at `world` (the archetype's
/// genotype/brain). Its `Species` is its **archetype index** — the identity the
/// relation table targets. Since Phase 3b, no more type branch: a food source is an
/// agent with a `Sessile` brain, compiled by the same `spawn_agent` (which gives it
/// an immobile pass-through body).
fn place(
    commands: &mut Commands,
    config: &SimConfig,
    palette: &mut Palette,
    i: usize,
    world: Vec2,
) {
    if config.archetypes.get(i).is_none() {
        return;
    }
    let species = i as u16;
    let seed = palette.next_seed;
    palette.next_seed = palette.next_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    spawn_agent(
        commands,
        config,
        config.genotype_of(species),
        Species(species),
        world,
        0.0,
        seed,
        config.reserve_max_of(species),
        0, // placed by hand: generation 0 (founder).
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: a relation `actor → target` (trivial rate/range, of no interest here).
    fn rel(actor: u16, target: u16) -> Relation {
        Relation {
            actor,
            target,
            transfer: true,
            rate: 1.0,
            range: 1.0,
        }
    }

    /// Reordering swaps two archetypes AND **transposes** their indices in the
    /// relations (the index IS the species identity). Swapping archetypes 0 and 1: a
    /// relation `0→2` becomes `1→2`, and `1→0` becomes `0→1`; a third index (2) does
    /// not move.
    #[test]
    fn swap_transposes_relation_indices() {
        let mut config = SimConfig {
            archetypes: vec![
                Archetype::new_agent(0),
                Archetype::new_agent(1),
                Archetype::new_food(2),
            ],
            relations: vec![rel(0, 2), rel(1, 0)],
            ..SimConfig::default()
        };
        swap_archetypes(&mut config, 0, 1);
        // The archetype formerly at 0 ("Species 0") is now at 1, and vice versa.
        assert_eq!(config.archetypes[0].name, "Species 1");
        assert_eq!(config.archetypes[1].name, "Species 0");
        // 0→2 ⇒ 1→2 (the third target 2 is intact); 1→0 ⇒ 0→1.
        assert_eq!(
            (config.relations[0].actor, config.relations[0].target),
            (1, 2)
        );
        assert_eq!(
            (config.relations[1].actor, config.relations[1].target),
            (0, 1)
        );
    }

    /// Duplicating adds a clone **at the end** (without shifting the existing indices
    /// → relations intact), with the same body as the original, named "… (copy)".
    #[test]
    fn duplicate_appends_a_clone_without_touching_relations() {
        let mut config = SimConfig {
            archetypes: vec![Archetype::new_agent(0), Archetype::new_food(1)],
            relations: vec![rel(0, 1)],
            ..SimConfig::default()
        };
        let new = duplicate_archetype(&mut config, 0).expect("clone of a valid index");
        assert_eq!(new, 2, "the clone is added at the end");
        assert_eq!(config.archetypes.len(), 3);
        assert_eq!(config.archetypes[2].name, "Species 0 (copy)");
        // Same body as the original (everything but the name).
        assert_eq!(config.archetypes[2].genotype, config.archetypes[0].genotype);
        assert_eq!(config.archetypes[2].brain, config.archetypes[0].brain);
        // Relations unchanged: the clone is at the end, no index slid.
        assert_eq!(config.relations.len(), 1);
        assert_eq!(
            (config.relations[0].actor, config.relations[0].target),
            (0, 1)
        );
    }

    /// An out-of-list index duplicates nothing.
    #[test]
    fn duplicate_out_of_range_is_none() {
        let mut config = SimConfig::default();
        let n = config.archetypes.len();
        assert_eq!(duplicate_archetype(&mut config, 99), None);
        assert_eq!(config.archetypes.len(), n, "nothing added");
    }

    /// Resyncing an imported species **preserves the local count** (`count`) and
    /// re-sets the `source` link; everything else (body, name…) comes from the
    /// definition.
    #[test]
    fn merge_species_preserves_local_count_and_relinks() {
        let mut target = Archetype::new_agent(0);
        target.count = 50; // count specific to THIS scenario
        target.name = "renamed locally".into();
        // The reloaded definition: other body (food), other name, other count.
        let mut loaded = Archetype::new_food(1);
        loaded.count = 7;
        loaded.name = "from the file".into();

        merge_species_def(&mut target, loaded, "species/x.ron".into());

        assert_eq!(target.count, 50, "the local count is preserved");
        assert_eq!(
            target.name, "from the file",
            "the rest comes from the definition"
        );
        assert!(target.is_sessile(), "the body comes from the definition");
        assert_eq!(
            target.source.as_deref(),
            Some("species/x.ron"),
            "the resync link is re-set"
        );
    }

    /// Sanitizing a species name into a filename: safe characters kept, the rest as
    /// `_`, an empty name falls back to a default.
    #[test]
    fn sanitize_filename_keeps_safe_chars() {
        assert_eq!(sanitize_filename("Gray wolf/2"), "Gray_wolf_2");
        assert_eq!(sanitize_filename("alpha-1_b"), "alpha-1_b");
        assert_eq!(sanitize_filename(""), "species");
    }

    /// The library/scenario row label is the file stem (no directory, no `.ron`).
    #[test]
    fn display_name_is_the_file_stem() {
        assert_eq!(display_name("species/hunter.ron"), "hunter");
        assert_eq!(display_name("scenarios/predator_prey.ron"), "predator_prey");
        assert_eq!(display_name("noext"), "noext");
    }

    /// `species_in_sync` ignores the local `count` (preserved by a resync) but flags a
    /// real body/brain divergence — the test behind both the per-species indicator and
    /// the cross-scenario usage state.
    #[test]
    fn species_in_sync_ignores_count_only() {
        let def = Archetype::new_agent(0);
        // A scenario copy: same body, different count, with the provenance link set.
        let mut copy = def.clone();
        copy.count = def.count + 25;
        copy.source = Some("species/x.ron".to_string());
        assert!(
            species_in_sync(&copy, &def),
            "only the count differs → in sync"
        );
        // A real divergence (the library brain changed) is caught.
        let mut changed = def.clone();
        changed.brain = teemlab::brain::BrainKind::Sessile;
        assert!(
            !species_in_sync(&copy, &changed),
            "a body/brain difference → out of sync"
        );
    }
}
