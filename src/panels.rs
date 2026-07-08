//! **Docked** layout of the windowed build: resizable egui panels around the central
//! simulation area, assembled by a **single** system ([`dock`]). The side panels
//! resize within a range that always reserves the sim's minimum width, and the
//! archetype editor folds into the left panel on a narrow window (cf. [`crate::layout`]).
//!
//! A module of the windowed *binary* only. We invent nothing: each panel calls the
//! reusable `*_section(ui, …)` already exposed by its tool module (`controls`,
//! `editor`, `runs`, `hud`, `recorder`, `inspector`). The role of this system is
//! purely **layout** — reserving the edges of the egui screen.
//!
//! **Semantic** split (master/detail): the **world** on the left (the *World* scenario
//! params + the *Archetypes* list / library) — the scenario as a whole; the **archetype
//! editor** in a second left column that opens only when an archetype is selected — the
//! one species you are editing; **Analysis** on the right (live *stats* + the agent
//! *inspector*) — the current state you read; the evolution *curves* (a time series)
//! at the bottom, spanning only the **central width** the side panels leave free;
//! *scenario IO + transport controls + View menu + Export* in
//! the top strip (controls centered). View layers live in the top-bar **View** menu and
//! video export in a floating window from the **Export** button — both out of the
//! always-on panels.
//!
//! **One root viewport `Ui`, `show_inside`.** Following bevy_egui 0.40
//! (`examples/ui.rs`): we build a single background-layer `Ui` covering
//! `ctx.viewport_rect()`, then add every panel into it with
//! `Panel::show_inside(&mut root, …)`. No deprecated top-level `Panel::show(ctx, …)`
//! anymore (egui 0.34 deprecates it), and the central region left free is read from
//! the root `Ui` with `available_rect_before_wrap()` — the non-deprecated successor
//! of `ctx.available_rect()`. We stash it in [`CentralRect`] so `main::set_sim_camera`
//! (which runs right after this system) frames the sim there.
//!
//! No `CentralPanel`: the center stays "transparent" and lets the Bevy rendering
//! show through, so the simulation is always **centered and fully visible**, whatever
//! the panels' size. With the curves moved to the lone bottom panel and stats/inspector
//! to the right, the central sim now gets the **full height** between the top strip and
//! the bottom curves.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use teemlab::genotype::Genotype;
use teemlab::metrics::History;
use teemlab::nutrients::Nutrients;
use teemlab::selection::{AutoSelect, Selection};
use teemlab::visuals::Layers;

use crate::controls::{self, SimControls};
use crate::dashboard::{self, BreedingSession};
use crate::editor::{self, Palette};
use crate::fonts::{self, icons};
use crate::help;
use crate::hud;
use crate::inspector;
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};
use crate::status::UiStatus;

/// Last frame's measured panel widths and the left-region mode — the inputs the
/// [`crate::layout`] rules need this frame (a one-frame lag, harmless for sizing).
/// A single [`Local`] so [`dock`] adds no system parameter.
#[derive(Default)]
pub struct DockLayout {
    /// Left panel (`left_tools`) width, feeding the right panel's range.
    left_w: f32,
    /// Right panel width, feeding the left panels' ranges.
    right_w: f32,
    /// Archetype-editor column width (two-column mode), a `left_mode` input.
    editor_w: f32,
    /// Left-region mode (master/detail vs single column) — the hysteresis carrier.
    mode: crate::layout::LeftMode,
    /// Measured width of the centered transport controls (for centering — see below).
    ctrl_width: f32,
}

/// Visibility of the **user-toggleable floating surfaces** — the one convention for
/// "what's open": a bool per surface on a single resource. (egui memory holds only
/// per-widget presentation state like collapsing headers; `Option`-presence stays
/// reserved for genuine *selection*, e.g. `palette.selected`, never for visibility;
/// a scenario's `batch` being set is a data *precondition* for the breeding window,
/// not its toggle.)
#[derive(Resource)]
pub struct UiWindows {
    /// The video **Export** window (top-bar Export button).
    pub export: bool,
    /// The **Breeding** dashboard — also gated on `config.batch.is_some()` (its data
    /// precondition); default open so it still appears with a batch, as before, but
    /// now dismissable and re-openable from the top bar.
    pub breeding: bool,
    /// The keyboard-shortcuts **cheatsheet** (`?` / Help menu).
    pub shortcuts: bool,
}

impl Default for UiWindows {
    fn default() -> Self {
        Self {
            export: false,
            breeding: true,
            shortcuts: false,
        }
    }
}

/// UI **preferences** (not scenario data): the source of truth for the dismissable
/// inline help. `dock` mirrors it into egui memory each frame so `help::hint` keeps
/// its zero-threading ergonomics (cf. `help`).
#[derive(Resource)]
pub struct UiPrefs {
    /// Show the explanatory hints in the panels (default on — discoverable).
    pub inline_help: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self { inline_help: true }
    }
}

/// Cross-panel resources [`dock`] writes, bundled into one [`SystemParam`] so the
/// system stays within Bevy's 16-parameter limit (like [`ObsParams`]): the scenario
/// document model, the recorder settings, the unified status line, the window toggles
/// and the UI preferences.
#[derive(SystemParam)]
pub struct DockState<'w> {
    pub runs_panel: ResMut<'w, RunsPanel>,
    pub recorder_panel: ResMut<'w, RecorderPanel>,
    pub ui_status: ResMut<'w, UiStatus>,
    pub windows: ResMut<'w, UiWindows>,
    pub prefs: ResMut<'w, UiPrefs>,
    /// The breeding session (P5) — the docked breeding panel (the bottom panel's left
    /// half, beside the curves, when the Breeding toggle is on) reads/drives it.
    /// Bundled here so `dock` stays within Bevy's 16-parameter limit.
    pub breeding: ResMut<'w, BreedingSession>,
    /// The config the running world was built from — the transport's Reset accents
    /// itself when the live config diverges from it (cf. `controls::world_diverged`).
    pub world_baseline: Res<'w, controls::WorldBaseline>,
}

/// **Observation** state of the right panel, bundled into one [`SystemParam`] so
/// [`dock`] stays within Bevy's 16-parameter limit: the current [`Selection`]
/// (read), the auto-follow mode ([`AutoSelect`]) and the sim view's pan/zoom
/// ([`crate::ViewControl`]) — all written/read by `inspector::observation_section`
/// and the inspector.
#[derive(SystemParam)]
pub struct ObsParams<'w> {
    pub selection: Res<'w, Selection>,
    pub auto_select: ResMut<'w, AutoSelect>,
    pub view: ResMut<'w, crate::ViewControl>,
}

/// The central region left free by the docked panels (egui points), computed by
/// [`dock`] from the root `Ui` and consumed by `main::set_sim_camera` to frame the
/// simulation. Replaces the deprecated `ctx.available_rect()`.
#[derive(Resource)]
pub struct CentralRect(pub egui::Rect);

impl Default for CentralRect {
    fn default() -> Self {
        Self(egui::Rect::ZERO)
    }
}

/// True if the pointer is over the egui UI (a docked panel or a floating window), as
/// opposed to the central simulation area — the gate the interaction systems use to
/// avoid acting on the sim through the UI.
///
/// Replaces `ctx.is_pointer_over_egui()`, which egui 0.34 only wires up for its own
/// closure-based `run_ui` flow: it relies on `root_ui_available_rect`, left **unset**
/// under bevy_egui + `show_inside` (and unsettable — `pass_state_mut` is crate-private),
/// so the built-in falls back to a legacy `unused_rect` that `show_inside` never
/// shrinks → it would report "not over UI" everywhere, and clicks on a panel would
/// fall through to the sim. We reproduce its **modern** logic against our
/// [`CentralRect`] (which *is* what `root_ui_available_rect` would hold): a floating
/// window (non-background layer) always counts as UI; on the background layer, the
/// pointer is over a panel iff it falls **outside** the central rect.
pub fn pointer_over_ui(ctx: &egui::Context, central: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.interact_pos())
        .is_some_and(|pos| pointer_over_ui_at(ctx, pos, central))
}

/// [`pointer_over_ui`] at an arbitrary position — the **gesture-origin** variant: a
/// drag belongs to where it *started*, so `camera_navigation` tests the press origin
/// here rather than the pointer's current position (a pan begun on the sim survives
/// crossing a panel; one begun on a panel never pans the view).
pub fn pointer_over_ui_at(ctx: &egui::Context, pos: egui::Pos2, central: egui::Rect) -> bool {
    match ctx.layer_id_at(pos) {
        // A window / menu / popup floating over the sim: always UI.
        Some(layer) if layer.order != egui::Order::Background => true,
        // Background layer (panels live here): UI iff outside the sim's central rect.
        _ => !central.contains(pos),
    }
}

/// The archetype-editor **detail** view: a header then the editor itself, shared by
/// the two-column second panel and the single-column in-place swap. Returns `true` if
/// the user asked to close (deselect the archetype).
///
/// `with_switcher` (single-column layout, where the master list is not visible beside
/// it) adds a **back** button to the list and a **combo** to jump between archetypes
/// without going back; otherwise the header is a plain "Archetype editor" title. Both
/// carry a close `X`.
fn archetype_detail(
    ui: &mut egui::Ui,
    palette: &mut Palette,
    config: &mut SimConfig,
    with_switcher: bool,
) -> bool {
    let mut deselect = false;
    ui.horizontal(|ui| {
        if with_switcher {
            if ui
                .button("‹  Archetypes")
                .on_hover_text("Back to the archetypes list")
                .clicked()
            {
                deselect = true;
            }
            let current = palette
                .selected
                .and_then(|i| config.archetypes.get(i))
                .map(|a| a.name.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("archetype_switcher")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, a) in config.archetypes.iter().enumerate() {
                        ui.selectable_value(&mut palette.selected, Some(i), &a.name);
                    }
                });
        } else {
            ui.strong("Archetype editor");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(fonts::icon(icons::X))
                .on_hover_text("Close (deselect the archetype)")
                .clicked()
            {
                deselect = true;
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("archetype_detail_scroll")
        .show(ui, |ui| editor::editor_section(ui, palette, config));
    deselect
}

/// The keyboard-shortcuts cheatsheet body: the keyboard [`crate::keymap::BINDINGS`]
/// then the [`crate::keymap::MOUSE`] gestures, as two-column grids. Reads the same
/// tables the tooltips do, so it can never drift from the real controls.
fn shortcuts_cheatsheet(ui: &mut egui::Ui) {
    use crate::keymap::{BINDINGS, MOUSE};
    egui::Grid::new("keyboard_shortcuts")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for b in BINDINGS {
                ui.strong(b.keys_text);
                if b.when.is_empty() {
                    ui.label(b.label);
                } else {
                    ui.label(format!("{}  ({})", b.label, b.when));
                }
                ui.end_row();
            }
        });
    ui.separator();
    ui.weak("Mouse");
    egui::Grid::new("mouse_gestures")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for (gesture, label) in MOUSE {
                ui.strong(*gesture);
                ui.label(*label);
                ui.end_row();
            }
        });
}

/// Builds the whole docked layout in one pass: one background-layer root `Ui`, then
/// each panel `show_inside` it. Chained **before** the interaction systems
/// (`pick_agent`, `resolve_drag`, …) and `set_sim_camera`, all of which read the free
/// central rect it records in [`CentralRect`] (the camera to frame the sim, the
/// interactions via [`pointer_over_ui`] to tell a click on the sim from one on a panel).
#[allow(clippy::too_many_arguments)]
pub fn dock(
    mut contexts: EguiContexts,
    mut central: ResMut<CentralRect>,
    mut state: DockState,
    mut config: ResMut<SimConfig>,
    mut layers: ResMut<Layers>,
    mut palette: ResMut<Palette>,
    mut sim_controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    mut history: ResMut<History>,
    // Gate: don't render until the UI fonts are live (cf. `fonts`), so an icon is never
    // drawn before its Phosphor family is bound (egui binds fonts only next-pass).
    fonts_ready: Res<crate::fonts::FontsReady>,
    // Real (unpausable) time — stamps the status line so info/success messages expire
    // (presentation only, never the sim clock).
    time: Res<Time<Real>>,
    // Last frame's panel widths + left-region mode + transport width (cf. [`DockLayout`]).
    mut layout: Local<DockLayout>,
    // Observation: selection + auto-follow mode + the sim view's pan/zoom (bundled to
    // keep `dock` within the 16-param system limit — cf. [`ObsParams`]).
    mut obs: ObsParams,
    stats_agents: Query<(&Reserve, &Genotype, &Species), With<Agent>>,
    inspector_agents: Query<
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
            &Nutrients,
        ),
        With<Agent>,
    >,
) -> Result {
    // Skip the first pass (before the fonts are bound): the icons would panic, and a
    // blank first frame on the paused startup screen is imperceptible.
    if !fonts_ready.0 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    // Mirror the inline-help preference into egui memory so `help::hint` reads it
    // without every panel threading the flag (cf. `help`). A one-frame lag on a toggle
    // is imperceptible.
    ctx.data_mut(|d| d.insert_temp(crate::help::id(), state.prefs.inline_help));
    // Stamp a freshly-set status message with the current real time so it can expire.
    let now = time.elapsed_secs_f64();
    state.ui_status.stamp(now);
    // A single root viewport `Ui` on the background layer, shared by every panel
    // (bevy_egui 0.40 `examples/ui.rs`). `show_inside` then docks each panel into it.
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("teemlab_dock"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    // Top strip, **a single line** — the app's command strip: scenario IO (the
    // Scenario menu) pinned **left**; the **transport controls** (play / step / speed /
    // reset) **centered**; the **View** / **Help** menus, the **Breeding** toggle and
    // the **Export** button pinned **right**. Video recording lives in a floating
    // window opened by the Export button (below).
    egui::Panel::top("top_bar")
        .resizable(false)
        .show_inside(&mut root, |ui| {
            let row_h = ui.spacing().interact_size.y;
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let full_w = ui.available_width();
                    // LEFT: scenario IO.
                    ui.push_id("scenario_bar", |ui| {
                        runs::scenario_section(
                            ui,
                            &mut state.runs_panel,
                            &mut config,
                            &mut state.ui_status,
                        );
                    });
                    // CENTER: the transport controls, centered on the **whole bar**. egui
                    // can't center a *group* along the main axis in immediate mode (it only
                    // learns the group's width after laying it out), so we pad by the width
                    // measured last frame (`ctrl_width`, 1-frame lag, clamped so it never
                    // collides with the scenario group). `scope` measures this frame's width.
                    let left_w = full_w - ui.available_width();
                    let pad = (full_w * 0.5 - layout.ctrl_width * 0.5 - left_w).max(8.0);
                    ui.add_space(pad);
                    let measured = ui
                        .scope(|ui| {
                            controls::controls_section(
                                ui,
                                &mut sim_controls,
                                &mut vtime,
                                &config,
                                &state.world_baseline,
                            )
                        })
                        .response
                        .rect
                        .width();
                    layout.ctrl_width = measured;
                    // RIGHT (emitted right→left, so reading order is View · Help ·
                    // [Breeding] · Export): Export rightmost, then the Breeding toggle
                    // (only with a batch regime), the Help menu, and the View menu.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(fonts::icon_label(icons::RECORD, "Export…"))
                            .on_hover_text(
                                "Render the current scenario to a video (opens the export panel).",
                            )
                            .clicked()
                        {
                            state.windows.export = !state.windows.export;
                        }
                        // Breeding dashboard toggle — shown only when the scenario carries a
                        // batch regime (the window's data precondition).
                        if config.batch.is_some() {
                            let on = state.windows.breeding;
                            if ui
                                .selectable_label(on, fonts::icon_label(icons::SPARKLE, "Breeding"))
                                .on_hover_text(
                                    "Dock the breeding dashboard in the bottom panel, beside \
                                     the curves.",
                                )
                                .clicked()
                            {
                                state.windows.breeding = !on;
                            }
                        }
                        ui.menu_button(fonts::icon_label(icons::CARET_DOWN, "Help"), |ui| {
                            help::toggle(ui, &mut state.prefs.inline_help);
                            if ui
                                .button(crate::keymap::tooltip(
                                    "Keyboard shortcuts…",
                                    crate::keymap::UiAction::ToggleShortcuts,
                                ))
                                .clicked()
                            {
                                state.windows.shortcuts = !state.windows.shortcuts;
                                ui.close();
                            }
                        })
                        .response
                        .on_hover_text("Inline help and the keyboard-shortcuts cheatsheet.");
                        ui.menu_button(fonts::icon_label(icons::CARET_DOWN, "View"), |ui| {
                            editor::layers_section(ui, &mut layers, &config)
                        })
                        .response
                        .on_hover_text("Toggle view layers (agents, nutrient maps).");
                    });
                },
            );
        });

    // Floating "Export video" window, toggled by the Export button. Driven through a
    // local `open` (the window's [x]) so it does not alias the `&mut recorder_panel`
    // the section needs — same pattern as the scenario "save as" dialog.
    if state.windows.export {
        let mut open = true;
        egui::Window::new("Export video")
            .collapsible(true)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 36.0))
            .open(&mut open)
            .show(root.ctx(), |ui| {
                recorder::recorder_section(ui, &mut state.recorder_panel);
            });
        if !open {
            state.windows.export = false;
        }
    }

    // Keyboard-shortcuts cheatsheet (toggled by `?` / F1 / Help menu). A floating
    // window over [`crate::keymap::BINDINGS`] + [`crate::keymap::MOUSE`] — the same
    // table the tooltips read, so it cannot drift from the actual controls.
    if state.windows.shortcuts {
        let mut open = true;
        egui::Window::new(fonts::icon_label(icons::CARET_DOWN, "Keyboard shortcuts"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(root.ctx(), shortcuts_cheatsheet);
        if !open {
            state.windows.shortcuts = false;
        }
    }

    // Whether an archetype is selected → the editor's **detail** half is shown. On a
    // wide window it opens a second left column ([`layout::LeftMode::TwoColumn`]); on a
    // narrow one it folds into the left panel in place ([`layout::LeftMode::SingleColumn`]),
    // so the sim never drops below [`layout::CENTRAL_MIN`] (cf. `layout`).
    let editor_open = palette
        .selected
        .is_some_and(|i| i < config.archetypes.len());

    // Left-region mode, from last frame's widths (a `SIDE_DEFAULT` fallback before a
    // panel has rendered, so a freshly opened editor picks its mode without a one-frame
    // flash). Only meaningful while `editor_open`, but tracked every frame for hysteresis.
    let viewport_w = root.ctx().viewport_rect().width();
    let est = |w: f32| {
        if w > 1.0 {
            w
        } else {
            crate::layout::SIDE_DEFAULT
        }
    };
    let mode = crate::layout::left_mode(
        layout.mode,
        viewport_w,
        est(layout.right_w),
        est(layout.left_w),
        est(layout.editor_w),
    );
    layout.mode = mode;
    let detail_in_left = editor_open && mode == crate::layout::LeftMode::SingleColumn;
    let two_column_editor = editor_open && mode == crate::layout::LeftMode::TwoColumn;

    // Right column — **Analysis** of the current state: live *stats* (means) then the
    // agent *inspector*, with *Observation* pinned above the scroll. Resizable within a
    // range that always reserves [`layout::CENTRAL_MIN`] for the sim (its "other side" is
    // last frame's left width — a harmless one-frame lag on a drag clamp). Rendered
    // before the left panels so their ranges can read this frame's fresh right width.
    // (The breeding dashboard docks in the bottom panel's left half — see below.)
    let mut deselect = false;
    let breeding_active = config.batch.is_some() && state.windows.breeding;
    let right_w = egui::Panel::right("right_panel")
        .default_size(crate::layout::SIDE_DEFAULT)
        .resizable(true)
        .size_range(crate::layout::side_range(viewport_w, layout.left_w))
        .show_inside(&mut root, |ui| {
            // Observation (small: follow mode + view reset) stays pinned; only the tall
            // sections below scroll, so each working surface keeps its own scroll offset.
            egui::CollapsingHeader::new("Observation")
                .default_open(true)
                .show(ui, |ui| {
                    inspector::observation_section(ui, &mut obs.auto_select, &mut obs.view)
                });
            egui::ScrollArea::vertical()
                .id_salt("analysis_scroll")
                .show(ui, |ui| {
                    egui::CollapsingHeader::new("Live stats")
                        .default_open(false)
                        .show(ui, |ui| editor::stats_section(ui, &stats_agents, &config));
                    // `inspector_section` **returns** any capture request (a derived
                    // archetype); `body_returned` is `Some` only while the header is
                    // expanded, so `flatten` maps the collapsed case to `None`. Applied
                    // *after* the call (it borrows `config` shared) → the mutable borrow
                    // is then allowed.
                    let inspector_action = egui::CollapsingHeader::new("Agent inspector")
                        .default_open(true)
                        .show(ui, |ui| {
                            inspector::inspector_section(
                                ui,
                                &obs.selection,
                                &config,
                                &mut palette.variant_name,
                                &inspector_agents,
                            )
                        })
                        .body_returned
                        .flatten();
                    match inspector_action {
                        // Capture → add the derived archetype to the current scenario.
                        Some(inspector::InspectorAction::Capture(arch)) => {
                            let from = arch.captured_from.clone().unwrap_or_default();
                            config.archetypes.push(arch);
                            palette.selected = Some(config.archetypes.len() - 1);
                            state
                                .ui_status
                                .set(format!("Captured to scenario (from {from})."));
                        }
                        // Save variant → write it to the library (species/saved/).
                        Some(inspector::InspectorAction::SaveVariant { species, variant }) => {
                            let scenario = state.runs_panel.origin_label();
                            let msg = editor::save_variant(
                                &mut palette,
                                &config,
                                species as usize,
                                variant,
                                &scenario,
                            );
                            palette.variant_name.clear();
                            state.ui_status.set_result(msg);
                        }
                        None => {}
                    }
                });
        })
        .response
        .rect
        .width();

    // Left column — **the world** (scenario params + the *Archetypes* list/library). On a
    // narrow window with an archetype selected, its content becomes the archetype editor
    // in place (single column); otherwise it stays the master list and the editor gets its
    // own column below. Resizable, reserving the sim's minimum against this frame's right
    // width. Drops its right separator in two-column mode so the world and the editor read
    // as one contiguous surface.
    let left_w = egui::Panel::left("left_tools")
        .default_size(crate::layout::SIDE_DEFAULT)
        .resizable(true)
        .size_range(crate::layout::side_range(viewport_w, right_w))
        .show_separator_line(!two_column_editor)
        .show_inside(&mut root, |ui| {
            if detail_in_left {
                // Detail view swapped in place — under its own id scope so its widgets
                // never share auto-ids with the master content (stable ids on the swap).
                ui.push_id("detail", |ui| {
                    if archetype_detail(ui, &mut palette, &mut config, true) {
                        deselect = true;
                    }
                });
            } else {
                ui.push_id("master", |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::CollapsingHeader::new("World")
                            .default_open(true)
                            .show(ui, |ui| editor::world_section(ui, &mut config));
                        egui::CollapsingHeader::new("Archetypes")
                            .default_open(true)
                            .show(ui, |ui| {
                                editor::selector_section(
                                    ui,
                                    &mut palette,
                                    &mut config,
                                    &mut state.ui_status,
                                )
                            });
                    });
                });
            }
        })
        .response
        .rect
        .width();

    // Bottom panel reserved **after** the side columns so it spans only the **central
    // width** they leave free. The evolution **curves** with the unified **status line**.
    // Created **before** the conditional `archetype_editor` so toggling that panel never
    // shifts this one's egui ids, and so the editor docks above these curves (which keep
    // the full central width). Height-**resizable** now: `hud_section` fills whatever
    // height the panel gets between the two plots (cf. `hud`). The floor is set so the
    // two plots at their minimum height plus the labels/legends still fit (no clipping).
    // A taller ceiling while breeding is docked here: the dashboard (navigator + curve +
    // metrics + leaderboard + network) is tall, so give the user room to drag it open.
    let bottom_max = if breeding_active { 760.0 } else { 520.0 };
    egui::Panel::bottom("bottom_panel")
        .resizable(true)
        .default_size(if breeding_active { 360.0 } else { 300.0 })
        .size_range(260.0..=bottom_max)
        .show_inside(&mut root, |ui| {
            // The status line, coloured by kind and shown only while unexpired (info /
            // success fade after a few seconds; errors persist — cf. `status`). Full width
            // above any split.
            if state.ui_status.visible(now) {
                let color = match state.ui_status.kind {
                    crate::status::StatusKind::Success => crate::theme::SUCCESS,
                    crate::status::StatusKind::Error => crate::theme::ERROR,
                    crate::status::StatusKind::Info => crate::theme::INK_MUTED,
                };
                ui.colored_label(color, &state.ui_status.message);
                ui.separator();
            }
            // Framed like the other sections (Body/Genes/Brain, the World cards): the
            // curves live in their own card. The curves are the closure the breeding split
            // and the full-width case share.
            let curves = |ui: &mut egui::Ui, history: &mut History, config: &SimConfig| {
                editor::card(ui, |ui| {
                    ui.strong("Evolution — curves");
                    hud::hud_section(ui, history, config);
                });
            };
            if breeding_active {
                // **Breeding dashboard** in the bottom panel's LEFT HALF, side by side with
                // the curves (P5): docked in the layout (not a floating popup over the sim),
                // it runs the generational loop + browses/replays generations while the
                // curves keep the right half. Config lives in the left World panel.
                ui.columns(2, |cols| {
                    egui::ScrollArea::vertical()
                        .id_salt("breeding_scroll")
                        .show(&mut cols[0], |ui| {
                            editor::card(ui, |ui| {
                                ui.strong("Breeding (generational)");
                                if let Some(act) = dashboard::breeding_panel(
                                    ui,
                                    &mut state.breeding,
                                    &config,
                                    &mut vtime,
                                    &mut state.ui_status,
                                ) {
                                    dashboard::apply_action(
                                        act,
                                        &mut config,
                                        &mut palette,
                                        &state.runs_panel,
                                        &mut state.ui_status,
                                        &mut sim_controls,
                                        &mut vtime,
                                    );
                                }
                            });
                        });
                    curves(&mut cols[1], &mut history, &config);
                });
            } else {
                curves(ui, &mut history, &config);
            }
        });

    // Archetype editor — the **detail** half as a second left column, **two-column mode
    // only** (in single column the detail lives in `left_tools` above). Created **last**,
    // after every unconditional panel: an egui child panel's id mixes in the parent's
    // running auto-id counter ([`egui::Ui::new_child`]), so a *conditional* panel inserted
    // earlier would shift the *later* panels' widget ids each time it toggles → egui's
    // "changed id between passes" warnings. Created last, the others keep stable ids.
    let mut editor_w = 0.0;
    if two_column_editor {
        // Zero left inner margin: the editor's content butts against the world panel's
        // (separator-less) right edge, so the two columns share a single ~8 px seam.
        let editor_frame = egui::Frame::side_top_panel(root.style()).inner_margin(egui::Margin {
            left: 0,
            right: 8,
            top: 2,
            bottom: 2,
        });
        editor_w = egui::Panel::left("archetype_editor")
            .default_size(crate::layout::SIDE_DEFAULT)
            .resizable(true)
            .size_range(crate::layout::side_range(viewport_w, right_w + left_w))
            .frame(editor_frame)
            .show_inside(&mut root, |ui| {
                if archetype_detail(ui, &mut palette, &mut config, false) {
                    deselect = true;
                }
            })
            .response
            .rect
            .width();
    }
    if deselect {
        palette.selected = None;
    }

    // Remember this frame's widths + mode for next frame's ranges and mode decision.
    layout.left_w = left_w;
    layout.right_w = right_w;
    layout.editor_w = editor_w;

    // The region left free by the panels: the central area where the sim is framed.
    // Non-deprecated successor of `ctx.available_rect()`.
    central.0 = root.available_rect_before_wrap();

    // Sim-state overlay over that central area (egui composites over the Bevy sim):
    // the run time (+ speed when not ×1), a paused chip, and a first-steps hint on an
    // empty arena. The run time comes from the history's latest sample (resets with the
    // world). Themed, so it matches the rest of the UI (cf. `theme`).
    let painter = root.painter().with_clip_rect(central.0);
    central_overlay(
        &painter,
        central.0,
        history.latest_time(),
        sim_controls.speed,
        vtime.is_paused(),
        stats_agents.is_empty(),
        !config.archetypes.is_empty(),
    );
    Ok(())
}

/// The run-time / speed read-out shown at the top of the sim area. The speed suffix is
/// dropped at ×1 (the default is noise). Pure, so it is unit-tested.
fn overlay_label(t: f32, speed: f32) -> String {
    if (speed - 1.0).abs() < 1e-3 {
        format!("t = {t:.1} s")
    } else {
        format!("t = {t:.1} s   ·   ×{speed:.1}")
    }
}

/// Paints the sim-area overlay: the [`overlay_label`] read-out, an accent **paused
/// chip** (which doubles as a "Space to run" affordance), and — on an empty arena — a
/// discreet hint on how to begin.
fn central_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    run_time: f32,
    speed: f32,
    paused: bool,
    agents_empty: bool,
    has_archetypes: bool,
) {
    let cx = rect.center().x;
    painter.text(
        egui::pos2(cx, rect.top() + 6.0),
        egui::Align2::CENTER_TOP,
        overlay_label(run_time, speed),
        egui::FontId::monospace(11.0),
        crate::theme::INK_MUTED,
    );
    if paused {
        let text = "Paused — Space to run";
        let font = egui::FontId::proportional(14.0);
        let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), crate::theme::ACCENT);
        let top = rect.top() + 26.0;
        let chip = egui::Rect::from_center_size(
            egui::pos2(cx, top + galley.size().y * 0.5),
            galley.size() + egui::vec2(20.0, 8.0),
        );
        painter.rect_filled(chip, 6.0, crate::theme::ACCENT.gamma_multiply(0.15));
        painter.rect_stroke(
            chip,
            6.0,
            egui::Stroke::new(1.0, crate::theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(cx, top),
            egui::Align2::CENTER_TOP,
            text,
            font,
            crate::theme::ACCENT,
        );
    }
    if agents_empty {
        let hint = if has_archetypes {
            "Drag a species from Archetypes into the arena"
        } else {
            "Scenario ▸ Open, or add an archetype to begin"
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(13.0),
            crate::theme::INK_FAINT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_windows_defaults() {
        let w = UiWindows::default();
        assert!(!w.export, "Export starts closed");
        assert!(
            w.breeding,
            "Breeding starts open (appears with a batch, as before)"
        );
        assert!(!w.shortcuts, "the cheatsheet starts closed");
        assert!(UiPrefs::default().inline_help, "inline help starts on");
    }

    #[test]
    fn overlay_label_hides_unit_speed() {
        // At ×1 (the default) the speed suffix is dropped as noise.
        let at_one = overlay_label(12.34, 1.0);
        assert_eq!(at_one, "t = 12.3 s");
        assert!(!at_one.contains('×'));
        // Off-default speed is shown.
        assert!(overlay_label(12.0, 2.0).contains("×2.0"));
    }
}
