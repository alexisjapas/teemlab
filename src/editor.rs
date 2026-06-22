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
use teemlab::SimConfig;
use teemlab::brain::{Brain, BrainKind, MlpBrain};
use teemlab::components::{Agent, Reserve, Species};
use teemlab::config::{Archetype, Relation};
use teemlab::genotype::{Genotype, TRAITS};
use teemlab::metrics;
use teemlab::spawn::spawn_agent;

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
    /// Last status message (archetype capture, species import/export).
    pub status: String,
    /// Species library: `species/*.ron` files found at the last scan.
    pub species_files: Vec<String>,
    /// Index, in [`species_files`](Self::species_files), of the species chosen for import.
    pub species_selected: Option<usize>,
}

/// egui color of an archetype, from its stored color (`[r, g, b]` ∈ [0, 1]).
fn archetype_color32(a: &Archetype) -> egui::Color32 {
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(a.color[0]), q(a.color[1]), q(a.color[2]))
}

/// Builds the palette at `Startup`, after [`SimConfig`] is inserted by the sim
/// plugin.
pub fn build_palette(mut commands: Commands, config: Res<SimConfig>) {
    commands.insert_resource(Palette {
        dragging: None,
        selected: None,
        next_seed: config.seed ^ 0xED17,
        status: String::new(),
        species_files: scan_species(),
        species_selected: None,
    });
}

/// Lists the library's species (`species/*.ron`), sorted. Mirror of the scenario
/// scan ([`crate::runs`]); a missing directory simply gives an empty list.
fn scan_species() -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir("species") {
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

/// Resolution of an archetype's drag-and-drop into the play area. A **distinct**
/// system, ordered after all the egui panels: `is_pointer_over_area` then knows
/// all the edges, otherwise a drop over a panel (bottom or left) would place an
/// entity hidden under the UI. `viewport_to_world_2d` accounts for the viewport's
/// offset (centered sim, cf. `set_sim_camera`) → the window cursor remains the
/// correct input.
pub fn resolve_drag(
    mut contexts: EguiContexts,
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
        if !ctx.is_pointer_over_area()
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
pub(crate) fn selector_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    ui.label("Drag into the area to place; click to edit; Delete (cursor on an entity) to remove.");
    ui.separator();
    let mut started = None;
    let mut clicked = None;
    for (i, arch) in config.archetypes.iter().enumerate() {
        let mark = if palette.selected == Some(i) {
            "▶ "
        } else {
            "⬤ "
        };
        let suffix = if arch.is_sessile() { " · sessile" } else { "" };
        // ✦: this archetype carries **captured weights** (cf. `Archetype::capture`).
        let captured = if arch.captured_brain.is_some() {
            " ✦"
        } else {
            ""
        };
        let label = egui::RichText::new(format!("{mark}{}{suffix}{captured}", arch.name))
            .color(archetype_color32(arch));
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
        if ui.button("＋ Agent").clicked() {
            config
                .archetypes
                .push(Archetype::new_agent(config.archetypes.len()));
            palette.selected = Some(config.archetypes.len() - 1);
        }
        if ui.button("＋ Food").clicked() {
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
                .button("⧉ Duplicate")
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
                .add_enabled(i > 0, egui::Button::new("▲ Move up"))
                .on_hover_text("Swaps with the previous archetype (remaps the relations).")
                .clicked()
            {
                swap_archetypes(config, i, i - 1);
                palette.selected = Some(i - 1);
            }
            if ui
                .add_enabled(i + 1 < count, egui::Button::new("▼ Move down"))
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
            .button("🗑 Delete the selected archetype")
            .on_hover_text("Removes the archetype and remaps the relation table.")
            .clicked()
    {
        remove_archetype(config, i);
        palette.selected = None;
    }

    ui.separator();
    species_library_section(ui, palette, config);

    if palette.dragging.is_some() {
        ui.separator();
        ui.weak("Release above the area to drop.");
    }
}

/// **Species library** (item 4): make a species reusable outside the scenario.
///
/// - **Export** the selected archetype to `species/<name>.ron` (`source` cleared:
///   the file *is* the source).
/// - **Import** a species: a **copy** joins the scenario (which stays
///   self-contained, §9), retaining the file as `source`.
/// - **Sync** an imported archetype: reloads its definition from `source`, keeping
///   the local count. The "copy + resync link" choice of item 4.
fn species_library_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Species library")
        .default_open(false)
        .show(ui, |ui| {
            // Export / sync of the selected archetype.
            if let Some(i) = palette.selected.filter(|&i| i < config.archetypes.len()) {
                if ui
                    .button("📤 Export the selection → species/")
                    .on_hover_text("Saves the archetype as a reusable species.")
                    .clicked()
                {
                    palette.status = export_species(&config.archetypes[i]);
                    palette.species_files = scan_species();
                }
                if let Some(src) = config.archetypes[i].source.clone()
                    && ui
                        .button("↻ Sync from the source")
                        .on_hover_text(format!(
                            "Reloads the definition from {src} (keeps the local count)."
                        ))
                        .clicked()
                {
                    palette.status = sync_species(config, i);
                }
            } else {
                ui.weak("Select an archetype to export / sync it.");
            }

            ui.separator();
            // Import: combo of the `species/*.ron`. We work on local copies so as not
            // to borrow `palette` both for reading (list) and writing (selection) in
            // the combo's closure (cf. `crate::runs`).
            let files = palette.species_files.clone();
            let mut sel = palette.species_selected;
            let mut rescan = false;
            let mut to_import = None;
            ui.horizontal(|ui| {
                let label = sel
                    .and_then(|j| files.get(j))
                    .map(String::as_str)
                    .unwrap_or("(choose a species…)");
                egui::ComboBox::from_id_salt("species_import")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (j, path) in files.iter().enumerate() {
                            ui.selectable_value(&mut sel, Some(j), path);
                        }
                    });
                if ui.button("↻").on_hover_text("Rescan species/").clicked() {
                    rescan = true;
                }
            });
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("📥 Import (copy)"))
                .on_hover_text("Adds a COPY of the species to the scenario (re-import to resync).")
                .clicked()
                && let Some(path) = sel.and_then(|j| files.get(j))
            {
                to_import = Some(path.clone());
            }

            palette.species_selected = sel;
            if rescan {
                palette.species_files = scan_species();
                palette.species_selected = None;
            }
            if let Some(path) = to_import {
                palette.status = import_species(config, &path);
                palette.selected = Some(config.archetypes.len().saturating_sub(1));
            }

            if !palette.status.is_empty() {
                ui.weak(&palette.status);
            }
        });
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
        Some(i) if i < config.archetypes.len() => archetype_editor(ui, config, i),
        _ => {
            ui.label("Click an archetype in the palette to edit it (or create one).");
        }
    }

    // Editor messages (archetype capture, species import/export); scenario IO
    // (save/load a .ron) now lives in the "Scenario" strip ([`crate::runs`]), not
    // here.
    if !palette.status.is_empty() {
        ui.separator();
        ui.weak(&palette.status);
    }
}

/// Editor of **archetype `i`**: common properties (name, color, count, size,
/// reserve) then genes + mutability (per species) + brain. Since Phase 3b there is
/// no more type branch: a food source is an archetype with a `Sessile` brain,
/// editable like the others. Writes *directly* into `config.archetypes[i]`
/// (persisted by "Save").
fn archetype_editor(ui: &mut egui::Ui, config: &mut SimConfig, i: usize) {
    // (Global) bounds captured before borrowing `config.archetypes` mutably.
    let trait_bounds: Vec<_> = TRAITS.iter().map(|t| (t.bounds)(config)).collect();
    // Brain type BEFORE editing: if the user changes the topology or the type while
    // weights had been captured, those weights no longer match → we clear them (cf.
    // end of function).
    let brain_before = config.archetypes[i].brain.clone();
    let arch = &mut config.archetypes[i];

    ui.horizontal(|ui| {
        ui.label("name:");
        ui.text_edit_singleline(&mut arch.name);
    });
    ui.horizontal(|ui| {
        ui.label("color:");
        ui.color_edit_button_rgb(&mut arch.color);
    });
    ui.add(
        egui::DragValue::new(&mut arch.count)
            .range(0..=5000)
            .prefix("count at spawn (reset): "),
    );
    ui.add(egui::Slider::new(&mut arch.radius, 2.0..=30.0).text("body radius"));
    ui.add(egui::Slider::new(&mut arch.reserve_max, 10.0..=500.0).text("max reserve"));

    // Every archetype is an agent (Phase 3b): genes + brain, no type branch. A *food
    // source* is just an archetype with a Sessile brain, living on photosynthesis —
    // editable like any other via these same controls.
    let Archetype {
        genotype,
        brain,
        mutable,
        ..
    } = arch;
    ui.separator();
    ui.strong("Genes (the archetype)");
    ui.small(
        "Each placed agent receives a COPY of these genes — its genome — which then \
         mutates on its own. The \"mutable\" checkbox governs, PER SPECIES, the right \
         to mutate.",
    );
    // An immobile entity (zero max speed, e.g. a flora / sessile source) neither
    // moves nor exploits vision: its locomotion and vision genes are inert, so we do
    // not expose them (cf. `TraitSpec::inert_when_immobile`). `max_speed`, for its
    // part, stays shown — it is the mobility switch.
    let immobile = genotype.locomotion().is_immobile();
    if immobile {
        ui.weak("Immobile: locomotion and vision genes hidden (no effect).");
    }
    // A single loop over TRAITS: slider (value, bounds) + "mutable" checkbox per
    // gene. Adding a trait does not add a line here (item 15).
    for (t, bounds) in TRAITS.iter().zip(&trait_bounds) {
        if immobile && t.inert_when_immobile {
            continue;
        }
        let mut value = (t.get)(genotype);
        if ui
            .add(egui::Slider::new(&mut value, bounds.min..=bounds.max).text(t.name))
            .changed()
        {
            (t.set)(genotype, value);
        }
        let mut m = (t.mutable)(mutable);
        if ui
            .checkbox(&mut m, "mutable")
            .on_hover_text(
                "Checked: this gene mutates at reproduction (it drifts and is passed \
                 on with variation). Unchecked: it is still transmitted, but frozen \
                 at the founder's value.",
            )
            .changed()
        {
            (t.set_mutable)(mutable, m);
        }
    }
    ui.separator();
    ui.strong("Brain (the decision's author)");
    brain_kind_editor(ui, brain, genotype.ray_count());

    // Captured weights ("capture as archetype" item): status + removal. Re-borrow of
    // the archetype after the brain editor (the re-borrows above are no longer used).
    let arch = &mut config.archetypes[i];
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
    egui::ComboBox::from_label("type")
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
    match kind {
        BrainKind::Wander { turn_rate } => {
            ui.add(egui::Slider::new(turn_rate, 0.0..=1.0).text("turn responsiveness"))
                .on_hover_text("Max amplitude of the heading drift each tick (rad).");
        }
        BrainKind::Hunter | BrainKind::Sessile => {}
        BrainKind::Mlp { hidden } => mlp_architecture_editor(ui, hidden, vision_rays),
    }
    ui.weak(kind.description());
}

/// **Numeric** editing of an MLP's architecture (item 18b, core): the number of
/// hidden layers and the width of each. The input (`3 × rays`: vision, target,
/// threat) and the output (2) are *constrained* by the contract and only displayed.
fn mlp_architecture_editor(ui: &mut egui::Ui, hidden: &mut Vec<usize>, vision_rays: usize) {
    ui.small(format!(
        "Input {} at the founder (= 3 × {vision_rays} rays: vision, target, threat) → \
         output {} (contract). The input layer then adapts to each individual's \
         visual precision (gene \"Rays\").",
        MlpBrain::input_size(vision_rays),
        MlpBrain::OUTPUTS,
    ));
    let mut remove = None;
    for (i, n) in hidden.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("hidden {i}"));
            ui.add(egui::DragValue::new(n));
            *n = (*n).clamp(1, 64); // at least one neuron, reasonable ceiling.
            if ui
                .small_button("✕")
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
    if ui.button("+ hidden layer").clicked() {
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

/// "World" section: the **scenario** parameters — arena, population, food economy,
/// interaction table — as opposed to the **archetype** editor (a species'
/// genotype). Direct read/write of the [`SimConfig`], hence persisted by "Save".
/// Some fields only take effect at the world (re)generation (annotated *reset*);
/// the others — maintained food, relations — are read continuously by the sim and
/// act **live**.
pub(crate) fn world_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    ui.small(
        "Global parameters. (reset) = at the world (re)generation (⟲ of the bar); \
         the relations act live. Population, bodies and brains live in the archetypes \
         (\"Archetypes\" panel).",
    );

    ui.separator();
    ui.strong("World");
    ui.add(egui::Slider::new(&mut config.arena_half_extent, 100.0..=1000.0).text("half-arena"));
    ui.add(
        egui::DragValue::new(&mut config.tick_hz)
            .range(8.0..=240.0)
            .speed(1.0)
            .prefix("sim rate (Hz, reset): "),
    )
    .on_hover_text("Fixed step of the simulation. Takes effect at the world (re)generation (⟲).");
    ui.add(
        egui::DragValue::new(&mut config.seed)
            .speed(1.0)
            .prefix("seed (reset): "),
    );

    // Windowed-render backgrounds: the color of the play area (inside of the arena)
    // and that of the off-game area (beyond the walls). Presentation settings, read
    // continuously by `main::draw_play_area` → immediate preview, and saved with the
    // scenario.
    ui.horizontal(|ui| {
        ui.color_edit_button_rgb(&mut config.play_area_color);
        ui.label("inner background (play area)");
    });
    ui.horizontal(|ui| {
        ui.color_edit_button_rgb(&mut config.off_game_color);
        ui.label("outer background (off-game)");
    });

    ui.separator();
    gene_bounds_section(ui, config);

    ui.separator();
    relations_section(ui, config);
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
            ui.small(
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
                        ui.add(egui::DragValue::new(&mut b.min).speed(speed));
                        ui.add(egui::DragValue::new(&mut b.max).speed(speed));
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
    ui.strong("Relations (who acts on whom)");
    ui.small(
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
        ui.separator();
        ui.horizontal(|ui| {
            archetype_combo(ui, ("rel_actor", i), &mut rel.actor, &archs);
            ui.label("→");
            archetype_combo(ui, ("rel_target", i), &mut rel.target, &archs);
            if ui
                .button("🗑")
                .on_hover_text("Remove this relation")
                .clicked()
            {
                to_remove = Some(i);
            }
        });
        ui.checkbox(&mut rel.transfer, "transfer (predation)");
        ui.add(egui::Slider::new(&mut rel.rate, 0.0..=400.0).text("rate/s"));
        ui.add(egui::Slider::new(&mut rel.range, 0.0..=100.0).text("range (0 = contact)"));
    }
    if let Some(i) = to_remove {
        config.relations.remove(i);
    }
    if ui.button("＋ Add a relation").clicked() {
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

/// Live statistics, rendered **on the right of the top bar** (fixed dock) by
/// [`crate::panels::top_bar`]. Read-only over the world: observation for display,
/// not sim logic. In `horizontal_wrapped` to wrap rather than overflow when the
/// window is narrow.
pub(crate) fn stats_section(
    ui: &mut egui::Ui,
    agents: &Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
) {
    // Computation shared with the native Bevy visualizer
    // ([`teemlab::metrics::live_stats`]) → same numbers in the egui bar and in the
    // video. Population and gene means cover only the mobile fauna; sessile sources
    // count only in `food` (otherwise their frozen genes would swamp the fauna's
    // drift).
    let stats = metrics::live_stats(agents);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Population: {}", stats.population));
        ui.separator();
        ui.label(format!("Food: {}", stats.food));
        ui.separator();
        ui.label(format!("Mean reserve: {:.0}", stats.mean_reserve));
        ui.separator();
        ui.label("Mean genes —");
        // One mean per TRAITS characteristic, without a hard-coded field.
        for (t, mean) in TRAITS.iter().zip(&stats.mean_traits) {
            ui.label(format!("{} {:.*}", t.name, t.decimals as usize, mean));
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
}
