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
use teemlab::config::{Archetype, BatchConfig, Fitness, Relation, Source, SpeciesEntry};
use teemlab::genotype::{GeneCategory, Genotype, TRAITS};
use teemlab::metrics;
use teemlab::spawn::spawn_agent;
use teemlab::visuals::Layers;

use crate::files::ron_files;
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
    /// Species catalog: every library form (`species/<lib>/*.ron`) grouped by species
    /// into a **base + its variants**, with **cross-scenario usage**, cached by
    /// [`scan_library`] — refreshed when the library section opens (it reads every
    /// species and scenario file, so never per frame), not by a manual reload button.
    pub catalog: Vec<CatalogSpecies>,
    /// Per-species selected form in the catalog (key = species name → index into its
    /// [`CatalogSpecies::forms`], 0 = base): the dropdown's choice, what Import copies.
    pub catalog_pick: HashMap<String, usize>,
    /// Catalog search box: filters species by name or variant id (case-insensitive).
    pub catalog_search: String,
    /// Working buffer for the inspector's "Export as variant" name field.
    pub variant_name: String,
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

/// A framed **card** spanning the panel's full available width. `ui.group` otherwise
/// shrink-wraps its frame to the content, so sibling cards would differ in width by
/// whatever each happens to hold (a slider vs a progress bar vs a label); pinning the
/// inner width to `available_width` — already net of the parent's padding and any
/// scrollbar — keeps cards at the same level aligned to one width. The single card
/// primitive shared by the archetype editor, the world sub-sections and the inspector.
pub(crate) fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        add(ui)
    })
    .inner
}

/// Builds the palette at `Startup`, after [`SimConfig`] is inserted by the sim
/// plugin.
pub fn build_palette(mut commands: Commands, config: Res<SimConfig>) {
    commands.insert_resource(Palette {
        dragging: None,
        selected: None,
        next_seed: config.seed ^ 0xED17,
        catalog: scan_library(),
        catalog_pick: HashMap::new(),
        catalog_search: String::new(),
        variant_name: String::new(),
        library_was_open: false,
        show_mutability: false,
    });
}

/// The committed, curated archetype library (read-only from the editor — hand-managed).
const EXAMPLES_LIB: &str = "species/examples";
/// The local (gitignored) library — where the editor *writes* exports and variants.
const SAVED_LIB: &str = "species/saved";
/// The libraries the catalog reads (display name, directory). Only `examples` is
/// committed (cf. `.gitignore`); the editor only ever writes to `saved`.
const LIBRARIES: [(&str, &str); 2] = [("examples", EXAMPLES_LIB), ("saved", SAVED_LIB)];

/// One reusable **form** in the catalog — a base or an evolved variant — with its
/// file, library, loaded archetype, and which scenarios import *this file*.
pub struct CatalogForm {
    /// File path, e.g. `species/saved/hunter--predator_prey-1.ron`.
    pub path: String,
    /// Library display name ("examples" / "saved").
    pub library: &'static str,
    /// The loaded archetype (a variant carries the evolved genotype + frozen brain).
    pub archetype: Archetype,
    /// Variant id `"<scenario>-<n>"`; `None` for the base form.
    pub variant_id: Option<String>,
    /// Scenario files importing this form (by `source`) — informational usage tracking
    /// (import is a one-time copy: no sync state, since there is no resync to act on it).
    pub usage: Vec<String>,
}

/// A catalog species: the **base** form (standard archetype) and its evolved
/// **variants**, grouped by name across libraries. Cached by [`scan_library`].
pub struct CatalogSpecies {
    /// Species name (the grouping key, shared by the base and its variants).
    pub name: String,
    /// The base form (the standard archetype), if one exists in any library.
    pub base: Option<CatalogForm>,
    /// Evolved variants of this species (snapshots from runs).
    pub variants: Vec<CatalogForm>,
}

impl CatalogSpecies {
    fn new(name: String) -> Self {
        Self {
            name,
            base: None,
            variants: Vec::new(),
        }
    }

    /// All selectable forms: the base (if any) first, then the variants.
    fn forms(&self) -> Vec<&CatalogForm> {
        self.base.iter().chain(self.variants.iter()).collect()
    }
}

/// Builds the species catalog: every library form (`species/<lib>/*.ron`) loaded and
/// **grouped by species name** into a base + its variants (across libraries), each form
/// cross-referenced against every scenario — **examples and saved** — to find which
/// import it (and whether each copy is still in sync). One pass over the scenarios
/// (parsed once), so it scales with the file count, not their product. A manual rescan —
/// never per frame.
fn scan_library() -> Vec<CatalogSpecies> {
    // Index every scenario's imported archetypes by their `source` path in one pass,
    // across both scenario categories.
    let mut imports: HashMap<String, Vec<String>> = HashMap::new();
    let scenarios = ron_files(crate::runs::EXAMPLES_DIR)
        .into_iter()
        .chain(ron_files(crate::runs::SAVED_DIR));
    for scenario in scenarios {
        let Ok(cfg) = SimConfig::from_ron_file(&scenario) else {
            continue;
        };
        for arch in cfg.archetypes {
            if let Some(src) = arch.source {
                imports.entry(src).or_default().push(scenario.clone());
            }
        }
    }

    // Load every library's forms and group them by species name (a base keeps its
    // variants together even when the base is committed and the variants are local).
    let mut species: std::collections::BTreeMap<String, CatalogSpecies> =
        std::collections::BTreeMap::new();
    for (lib_name, dir) in LIBRARIES {
        for path in ron_files(dir) {
            let Ok(entry) = SpeciesEntry::from_ron_file(&path) else {
                continue;
            };
            let form = CatalogForm {
                library: lib_name,
                usage: imports.get(&path).cloned().unwrap_or_default(),
                variant_id: entry.variant_id.clone(),
                archetype: entry.archetype.clone(),
                path,
            };
            match entry.variant_of {
                // A base: keyed by its own name.
                None => {
                    species
                        .entry(entry.archetype.name.clone())
                        .or_insert_with(|| CatalogSpecies::new(entry.archetype.name.clone()))
                        .base = Some(form);
                }
                // A variant: attaches to its base name (group created if the base is
                // missing — an orphan variant still appears).
                Some(base) => species
                    .entry(base.clone())
                    .or_insert_with(|| CatalogSpecies::new(base))
                    .variants
                    .push(form),
            }
        }
    }
    species.into_values().collect()
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
        if ui
            .button(fonts::icon_label(icons::PLUS, "Agent"))
            .on_hover_text("Add a mobile agent archetype to the scenario.")
            .clicked()
        {
            config
                .archetypes
                .push(Archetype::new_agent(config.archetypes.len()));
            palette.selected = Some(config.archetypes.len() - 1);
        }
        if ui
            .button(fonts::icon_label(icons::PLUS, "Food"))
            .on_hover_text("Add a sessile food-source archetype to the scenario.")
            .clicked()
        {
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
            .button(fonts::icon_label(icons::TRASH, "Delete"))
            .on_hover_text("Removes the selected archetype and remaps the relation table.")
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

/// Display name of a RON path: its file stem (e.g. `species/hunter.ron` → `hunter`,
/// `scenarios/examples/10_predator_prey.ron` → `10_predator_prey`).
fn display_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

/// **Species library** (item 4 + the §9 cross-scenario step), on the **copy** model
/// (a scenario stays self-contained and reproducible). **Import is a one-time copy** —
/// there is no resync; to update an imported species, re-import it. Two parts:
/// - **Export to catalog** — write the selected scenario archetype into `species/saved/`
///   as a reusable base;
/// - the **catalog** — species grouped into a **base + a variant dropdown**, with a
///   color swatch, brain, an **Import** of the selected form, informational
///   **cross-scenario usage**, and a name/id **search**, cached by [`scan_library`]
///   (refreshed on open).
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
                "Reusable species (species/examples committed · species/saved local). Save the \
                 selected archetype to the catalog, or import a one-time COPY (base or variant).",
            );
            // Save the selected scenario archetype into the catalog (scenario → catalog).
            if let Some(i) = palette.selected.filter(|&i| i < config.archetypes.len()) {
                if ui
                    .button(fonts::icon_label(icons::UPLOAD, "Export to catalog"))
                    .on_hover_text(
                        "Exports the selected archetype as a reusable base in species/saved/.",
                    )
                    .clicked()
                {
                    status.set(export_species(&config.archetypes[i]));
                    palette.catalog = scan_library();
                }
            } else {
                help::hint(ui, "Select an archetype to save it to the catalog.");
            }
            ui.separator();
            catalog_section(ui, palette, config, status);
        });

    // The catalog refreshes itself **when the section opens** (no manual reload button —
    // the same pattern as the Scenario menu, cf. `runs::scenario_section`): detect the
    // closed→open transition and rescan once. The scan reads every species and scenario
    // file, so it stays off the per-frame path.
    let open = resp.openness > 0.5;
    if open && !palette.library_was_open {
        palette.catalog = scan_library();
    }
    palette.library_was_open = open;
}

/// Dropdown label of a catalog form: `Base`, or `name (id)` for a variant.
fn form_label(form: &CatalogForm) -> String {
    match &form.variant_id {
        None => "Base".to_string(),
        Some(id) => format!("{} ({id})", form.archetype.name),
    }
}

/// The **catalog**: species grouped into a base + a variant dropdown, with a search box,
/// a color swatch, the Import of the selected form, and its cross-scenario usage.
fn catalog_section(
    ui: &mut egui::Ui,
    palette: &mut Palette,
    config: &mut SimConfig,
    status: &mut UiStatus,
) {
    ui.horizontal(|ui| {
        ui.strong("Catalog");
        // Fill the rest of the row rather than leaving dead space in the fixed-width
        // panel (cf. the panel comment on why the side columns can't shrink to content).
        ui.add(
            egui::TextEdit::singleline(&mut palette.catalog_search)
                .hint_text("search name / id")
                .desired_width(f32::INFINITY),
        );
    });
    if palette.catalog.is_empty() {
        help::hint(
            ui,
            "No species yet — export an archetype to species/saved/.",
        );
        return;
    }

    // Sources already imported in THIS scenario, to flag the catalog rows.
    let here: HashSet<String> = config
        .archetypes
        .iter()
        .filter_map(|a| a.source.clone())
        .collect();
    let needle = palette.catalog_search.trim().to_lowercase();

    // Split borrows: iterate the catalog (read) while mutating the per-species pick map.
    let Palette {
        catalog,
        catalog_pick,
        ..
    } = &mut *palette;
    let mut to_import: Option<String> = None;

    for species in catalog.iter() {
        let forms = species.forms();
        if forms.is_empty() {
            continue;
        }
        // Search: match the species name, or any form's name / variant id.
        if !needle.is_empty() {
            let hit = species.name.to_lowercase().contains(&needle)
                || forms.iter().any(|f| {
                    f.archetype.name.to_lowercase().contains(&needle)
                        || f.variant_id
                            .as_deref()
                            .is_some_and(|id| id.to_lowercase().contains(&needle))
                });
            if !hit {
                continue;
            }
        }

        let pick = catalog_pick.entry(species.name.clone()).or_insert(0);
        if *pick >= forms.len() {
            *pick = 0;
        }
        let form = forms[*pick];

        ui.horizontal(|ui| {
            if ui
                .button(fonts::icon(icons::DOWNLOAD))
                .on_hover_text("Import a COPY of the selected form into the scenario")
                .clicked()
            {
                to_import = Some(form.path.clone());
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 2.0, archetype_color32(&form.archetype));
            ui.label(&species.name);
            ui.weak(form.archetype.brain.name());
            egui::ComboBox::from_id_salt(("catalog_form", &species.name))
                .selected_text(form_label(form))
                .show_ui(ui, |ui| {
                    for (i, f) in forms.iter().enumerate() {
                        ui.selectable_value(pick, i, form_label(f));
                    }
                });
        });

        // Selected form details: library, "imported here", cross-scenario usage.
        let form = forms[*pick]; // re-read: the dropdown may have changed `pick`.
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.weak(form.library);
            if here.contains(&form.path) {
                ui.weak("· imported here");
            }
            if !form.usage.is_empty() {
                ui.weak(format!("· used in {}", form.usage.len()))
                    .on_hover_ui(|ui| {
                        for scenario in &form.usage {
                            ui.label(display_name(scenario));
                        }
                    });
            }
        });
    }

    if let Some(path) = to_import {
        status.set(import_species(config, &path));
        palette.selected = Some(config.archetypes.len().saturating_sub(1));
        palette.catalog = scan_library();
    }
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

/// Exports an archetype as a **reusable base** to `species/saved/<name>.ron` (the local
/// library; the editor never writes to the committed `examples`). The `source`/capture
/// links are cleared — the exported file *is* the source. Returns a status message.
fn export_species(arch: &Archetype) -> String {
    let mut base = arch.clone();
    base.source = None;
    base.captured_brain = None;
    base.captured_from = None;
    let path = format!("{SAVED_LIB}/{}.ron", sanitize_filename(&arch.name));
    match SpeciesEntry::base(base).save_ron_file(&path) {
        Ok(()) => format!("Species exported → {path}"),
        Err(e) => format!("Export failed: {e}"),
    }
}

/// Imports a catalog form (base or variant): a **one-time copy** of its archetype joins
/// the scenario (which stays self-contained, §9). The originating file is kept as
/// `source` — **provenance only** (cross-scenario usage tracking), not a live link: there
/// is no resync. Added at the end of the list, hence without shifting the relation
/// indices. Returns a status message.
fn import_species(config: &mut SimConfig, path: &str) -> String {
    match SpeciesEntry::from_ron_file(path) {
        Ok(entry) => {
            let mut arch = entry.archetype;
            arch.source = Some(path.to_string());
            config.archetypes.push(arch);
            format!("Imported (copy) ← {path}")
        }
        Err(e) => format!("Import failed: {e}"),
    }
}

/// Next variant number for `species` from `scenario`: `max(existing) + 1` over the
/// cached catalog, so ids stay `"<scenario>-<n>"` and don't collide after a deletion.
fn next_variant_number(catalog: &[CatalogSpecies], species: &str, scenario: &str) -> u32 {
    let prefix = format!("{scenario}-");
    catalog
        .iter()
        .filter(|s| s.name == species)
        .flat_map(|s| s.variants.iter())
        .filter_map(|v| v.variant_id.as_deref())
        .filter_map(|id| id.strip_prefix(&prefix))
        .filter_map(|n| n.parse::<u32>().ok())
        .max()
        .map_or(1, |m| m + 1)
}

/// Saves an **evolved variant** to `species/saved/` (the inspector's "Export as variant").
/// `variant` is the captured snapshot (evolved genotype + frozen brain) with its display
/// name already set; `species` is the scenario archetype index it derives from (its name
/// is the base it attaches to). If no base exists for that name in the catalog, the
/// **standard form is auto-exported** as a base alongside it (cf. §9, your #4). The id is
/// `"<scenario>-<n>"`. Returns a status message and rescans the catalog.
pub(crate) fn save_variant(
    palette: &mut Palette,
    config: &SimConfig,
    species: usize,
    mut variant: Archetype,
    scenario: &str,
) -> String {
    let Some(base_arch) = config.archetypes.get(species) else {
        return "No such species to vary.".to_string();
    };
    let base_name = base_arch.name.clone();

    // Auto-export the base (standard form) if the catalog has none for this name.
    let has_base = palette
        .catalog
        .iter()
        .any(|s| s.name == base_name && s.base.is_some());
    if !has_base {
        let msg = export_species(base_arch);
        if msg.starts_with("Export failed") {
            return msg;
        }
        palette.catalog = scan_library();
    }

    let id = format!(
        "{scenario}-{}",
        next_variant_number(&palette.catalog, &base_name, scenario)
    );
    variant.source = None;
    let entry = SpeciesEntry::variant(variant, base_name.clone(), id.clone());
    // File named after the BASE (groups variants visually): `<base>--<scenario>-<n>.ron`.
    let path = format!("{SAVED_LIB}/{}--{id}.ron", sanitize_filename(&base_name));
    let result = match entry.save_ron_file(&path) {
        Ok(()) => format!("Variant saved → {path}"),
        Err(e) => format!("Variant save failed: {e}"),
    };
    palette.catalog = scan_library();
    result
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
    card(ui, |ui| {
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
    card(ui, |ui| {
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
    card(ui, |ui| {
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
                .small_button(fonts::icon(icons::TRASH))
                .on_hover_text("Remove this layer")
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
    // Channel names from the MLP contract (`MlpBrain::CHANNEL_LABELS`): the input is one
    // block of `rays` per channel (vision ++ target ++ threat).
    let channels = MlpBrain::CHANNEL_LABELS;
    let rays = n_in / channels.len();
    for node in 0..n_in {
        let text = if n_in.is_multiple_of(channels.len()) {
            let ch = node / rays;
            format!("{} {}", channels[ch], node - ch * rays)
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
            Some(o) if o < MlpBrain::OUTPUT_LABELS.len() => MlpBrain::OUTPUT_LABELS[o],
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
/// archetypes), as framed **cards** (the same idiom as the archetype editor's
/// Body/Genes/Brain) — *Arena & generation*, *Relations*, *Nutrients*, *Gene bounds*,
/// *Appearance*. Each card carries a **collapsible** header (open by default for the
/// short, frequent ones; closed for the heavy *Nutrients* / *Gene bounds*). Direct
/// read/write of the [`SimConfig`], hence persisted by "Save". Some fields only take
/// effect at the next Reset (⟲); relations act **live**.
pub(crate) fn world_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    // ARENA & GENERATION — size and RNG. Open by default. (The sim rate is a scenario
    // file parameter, not exposed here.)
    card(ui, |ui| {
        egui::CollapsingHeader::new("Arena & generation")
            .default_open(true)
            .show(ui, |ui| {
                // Label-left two-column grid (the convention shared with the Body card),
                // so the parameter labels align and read label → value.
                egui::Grid::new("arena_fields")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("half-arena");
                        fonts::value(ui, |ui| {
                            ui.add(egui::Slider::new(
                                &mut config.arena_half_extent,
                                100.0..=1000.0,
                            ))
                        });
                        ui.end_row();

                        ui.label("seed");
                        fonts::value(ui, |ui| {
                            ui.add(egui::DragValue::new(&mut config.seed).speed(1.0))
                        });
                        ui.end_row();
                    });
                help::hint(
                    ui,
                    "Seed and arena walls apply on the next Reset (⟲). Population, bodies and \
                 brains live in the \"Archetypes\" panel.",
                );
            });
    });

    // RELATIONS — the interaction table (acts live).
    card(ui, |ui| {
        egui::CollapsingHeader::new("Relations")
            .default_open(true)
            .show(ui, |ui| relations_section(ui, config));
    });

    // BREEDING — the generational regime (P5). Authored here (saved with the scenario);
    // the run itself happens in the Breeding window / the `breed` bin.
    batch_section(ui, config);

    // Nutrients and gene bounds each frame their own card (below).
    nutrient_section(ui, config);
    gene_bounds_section(ui, config);

    // APPEARANCE — windowed-render backgrounds (read continuously by
    // `main::draw_play_area` → immediate preview, saved with the scenario).
    card(ui, |ui| {
        egui::CollapsingHeader::new("Appearance")
            .default_open(true)
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
    });
}

/// The **nutrients** (T2 substrate): the concentration-field parameters and the
/// emission **sources**. Sources are a *separate category* (not archetypes, not
/// agents), so they live here in the World editor rather than in the archetype
/// palette. Everything here is **"(reset)"**: the field is rebuilt and the sources
/// respawned at the world (re)generation (⟲ of the bar, or "Reload into the world"),
/// the single passage point that also re-applies the field's resolution/diffusion
/// (cf. [`crate::controls::apply_reset`]). A framed card like the other World sections,
/// but with a **collapsible** header inside it (default closed): with its field grid
/// plus a card per source it is the other heavy section, and inert for most scenarios
/// (no source ⇒ inert layer), so it folds away while keeping the sibling card frame.
fn nutrient_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    card(ui, |ui| {
        egui::CollapsingHeader::new("Nutrients")
            .default_open(false)
            .show(ui, |ui| {
                help::hint(
                    ui,
                    "A finite nutrient bounds REPRODUCTION (Liebig), decoupled from \
                 survival (the sun). The field is fed by the sources below and spread \
                 by diffusion into gradients. All (reset): applied at ⟲.",
                );
                egui::Grid::new("nutrient_fields")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("grid resolution (reset)");
                        fonts::value(ui, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut config.nutrient.resolution)
                                    .range(8..=256)
                                    .speed(1.0),
                            )
                            .on_hover_text("Cells per side of the nutrient field over the arena.")
                        });
                        ui.end_row();

                        ui.label("diffusion (reset)");
                        fonts::value(ui, |ui| {
                            ui.add(egui::Slider::new(&mut config.nutrient.diffusion, 0.0..=1.0))
                                .on_hover_text(
                                    "Per-tick spread toward neighbours (the local↔global knob); \
                                 0 = no spread.",
                                )
                        });
                        ui.end_row();
                    });

                ui.separator();
                ui.strong("Sources (emit nutrient into the field)");
                help::hint(
                    ui,
                    "A fixed point emitting `rate`/s of nutrient at its position.",
                );
                let mut to_remove = None;
                for (i, src) in config.sources.iter_mut().enumerate() {
                    card(ui, |ui| {
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
                        egui::Grid::new(("source_fields", i))
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("pos");
                                ui.horizontal(|ui| {
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
                                ui.end_row();

                                ui.label("rate/s");
                                fonts::value(ui, |ui| {
                                    ui.add(egui::Slider::new(&mut src.rate, 0.0..=100.0))
                                });
                                ui.end_row();

                                ui.label("visual radius");
                                fonts::value(ui, |ui| {
                                    ui.add(egui::Slider::new(&mut src.radius, 1.0..=40.0))
                                });
                                ui.end_row();
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
    });
}

/// The **gene bounds** (`*_bounds` of the scenario), edited globally: min/max of
/// each characteristic. They bound the **mutation** (cf. [`Genotype::mutate`]) AND
/// the archetype editor's sliders. Loops over [`TRAITS`] via `bounds_mut`, so adding
/// a gene exposes it here without touching this section (item 15/3). A framed card,
/// like the other World sections — but with a **collapsible** header inside it
/// (default closed): this grid is the one heavy section (one row per gene), rarely
/// touched, so it folds away while keeping the card frame of its siblings.
fn gene_bounds_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    card(ui, |ui| {
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
        card(ui, |ui| {
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
            egui::Grid::new(("relation_fields", i))
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("rate/s");
                    fonts::value(ui, |ui| {
                        ui.add(egui::Slider::new(&mut rel.rate, 0.0..=400.0))
                    });
                    ui.end_row();

                    ui.label("range (0 = contact)");
                    fonts::value(ui, |ui| {
                        ui.add(egui::Slider::new(&mut rel.range, 0.0..=100.0))
                    });
                    ui.end_row();
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

/// The **generational (breeding) regime** (P5): toggle a `batch` on the scenario and edit
/// its parameters. The run itself happens in the **Breeding** window (the dashboard) or
/// the `breed` bin headless; this card only *authors* the [`BatchConfig`] (saved with the
/// scenario, like the relations / nutrients). A framed card with a collapsible header
/// (default closed — most scenarios are continuous).
fn batch_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    card(ui, |ui| {
        egui::CollapsingHeader::new("Breeding (generational)")
            .default_open(false)
            .show(ui, |ui| {
                let mut enabled = config.batch.is_some();
                if ui
                    .checkbox(&mut enabled, "generational regime")
                    .on_hover_text(
                        "Run → score → breed across generations (the Breeding window, or \
                         the `breed` bin headless).",
                    )
                    .changed()
                {
                    // Enabling installs sensible defaults; disabling drops the regime
                    // (the scenario goes back to continuous — the field leaves the RON).
                    config.batch = enabled.then(BatchConfig::default);
                }
                // (name, color) snapshot for the scored-species menu, before the mutable
                // borrow of `config.batch`.
                let archs: Vec<(String, egui::Color32)> = config
                    .archetypes
                    .iter()
                    .map(|a| (a.name.clone(), archetype_color32(a)))
                    .collect();
                if let Some(batch) = config.batch.as_mut() {
                    egui::Grid::new("batch_fields")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("scored species");
                            archetype_combo(ui, "batch_scored", &mut batch.scored_species, &archs);
                            ui.end_row();

                            ui.label("fitness");
                            fitness_combo(ui, &mut batch.fitness);
                            ui.end_row();

                            ui.label("generations");
                            fonts::value(ui, |ui| {
                                ui.add(egui::DragValue::new(&mut batch.generations).range(1..=200))
                            });
                            ui.end_row();

                            ui.label("matches / gen");
                            fonts::value(ui, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut batch.matches_per_gen).range(1..=64),
                                )
                            });
                            ui.end_row();

                            ui.label("match ticks");
                            fonts::value(ui, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut batch.match_ticks)
                                        .speed(50.0)
                                        .range(100..=200_000),
                                )
                            });
                            ui.end_row();

                            ui.label("survivors");
                            fonts::value(ui, |ui| {
                                ui.add(egui::DragValue::new(&mut batch.survivors).range(0..=64))
                            });
                            ui.end_row();

                            ui.label("seed base");
                            fonts::value(ui, |ui| {
                                ui.add(egui::DragValue::new(&mut batch.seed_base).speed(1.0))
                            });
                            ui.end_row();
                        });
                    help::hint(
                        ui,
                        "survivors = elites carried to the next generation (0 = no \
                         selection). Run it from the Breeding window.",
                    );
                }
            });
    });
}

/// A dropdown over the [`Fitness`] menu — the exhaustive `match` keeps the labels in
/// sync with the enum (a new variant must be handled here).
fn fitness_combo(ui: &mut egui::Ui, value: &mut Fitness) {
    let text = match value {
        Fitness::BestEvolved => "best evolved",
        Fitness::Population => "population",
    };
    egui::ComboBox::from_id_salt("batch_fitness")
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, Fitness::BestEvolved, "best evolved");
            ui.selectable_value(value, Fitness::Population, "population");
        });
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
        assert_eq!(display_name("species/examples/hunter.ron"), "hunter");
        assert_eq!(
            display_name("scenarios/examples/10_predator_prey.ron"),
            "10_predator_prey"
        );
        assert_eq!(display_name("noext"), "noext");
    }
}
