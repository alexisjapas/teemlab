//! **Docked** layout of the windowed build: fixed egui panels around the central
//! simulation area, assembled by a **single** system ([`dock`]).
//!
//! A module of the windowed *binary* only. We invent nothing: each panel calls the
//! reusable `*_section(ui, …)` already exposed by its tool module (`controls`,
//! `editor`, `runs`, `hud`, `recorder`, `inspector`). The role of this system is
//! purely **layout** — reserving the edges of the egui screen.
//!
//! **Semantic** split: *world* (scenario data) on the left, *entities* on the right,
//! *scenario IO + View menu + Export* in the top strip, *controls + stats + status*
//! then *curves + inspector* at the bottom. View layers live in the top-bar **View**
//! menu and video export in a floating window opened from the **Export** button — both
//! out of the always-on panels.
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
//! the panels' size.
//!
//! Panel **creation order matters**: the two bottom panels are added
//! `bottom_panel` *then* `bottom_bar`, so the curves/inspector panel takes the very
//! bottom edge and the controls/stats bar sits just under the sim.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use teemlab::config::Archetype;
use teemlab::genotype::Genotype;
use teemlab::metrics::History;
use teemlab::selection::Selection;
use teemlab::visuals::Layers;

use crate::controls::{self, SimControls};
use crate::editor::{self, Palette};
use crate::hud;
use crate::inspector;
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};
use crate::status::UiStatus;

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
    let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else {
        return false;
    };
    match ctx.layer_id_at(pos) {
        // A window / menu / popup floating over the sim: always UI.
        Some(layer) if layer.order != egui::Order::Background => true,
        // Background layer (panels live here): UI iff outside the sim's central rect.
        _ => !central.contains(pos),
    }
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
    mut runs_panel: ResMut<RunsPanel>,
    mut recorder_panel: ResMut<RecorderPanel>,
    mut config: ResMut<SimConfig>,
    mut layers: ResMut<Layers>,
    mut palette: ResMut<Palette>,
    mut sim_controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    mut history: ResMut<History>,
    mut ui_status: ResMut<UiStatus>,
    selection: Res<Selection>,
    stats_agents: Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
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
        ),
        With<Agent>,
    >,
) -> Result {
    /// Minimum width of the central panel below which we no longer try to split it
    /// into two columns. `egui::Ui::columns` computes a negative column width — and
    /// **panics** — when the available space is nearly zero, which happens as soon as
    /// the side columns eat almost the whole window. Below it, we stack the two
    /// sections instead of crashing (and taking down the whole egui pass).
    const MIN_TWO_COLUMN_WIDTH: f32 = 160.0;

    let ctx = contexts.ctx_mut()?;
    // A single root viewport `Ui` on the background layer, shared by every panel
    // (bevy_egui 0.40 `examples/ui.rs`). `show_inside` then docks each panel into it.
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("teemlab_dock"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    // Top strip, **a single line** — the app's command strip: scenario IO (choose /
    // reload / save-load) pinned left; the **View** menu (view layers) and the
    // **Export** toggle pinned right. Video recording is no longer crammed here — it
    // lives in a floating window opened by the Export button (below), so the old
    // reverse-order `right_to_left` recorder hack is gone.
    egui::Panel::top("top_bar").show_inside(&mut root, |ui| {
        let row_h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.push_id("scenario_bar", |ui| {
                    runs::scenario_section(ui, &mut runs_panel, &mut config, &mut ui_status);
                });
                // Pinned right (emitted right→left): Export first (rightmost), then the
                // View menu to its left, so reading order is View · Export.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("⏺ Export…")
                        .on_hover_text(
                            "Render the current scenario to a video (opens the export panel).",
                        )
                        .clicked()
                    {
                        recorder_panel.open = !recorder_panel.open;
                    }
                    ui.menu_button("View ▾", |ui| editor::layers_section(ui, &mut layers))
                        .response
                        .on_hover_text("Toggle view layers (agents, nutrient maps).");
                });
            },
        );
    });

    // Floating "Export video" window, toggled by the Export button. Driven through a
    // local `open` (the window's [x]) so it does not alias the `&mut recorder_panel`
    // the section needs — same pattern as the scenario "save as" dialog.
    if recorder_panel.open {
        let mut open = true;
        egui::Window::new("Export video")
            .collapsible(true)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 36.0))
            .open(&mut open)
            .show(root.ctx(), |ui| {
                recorder::recorder_section(ui, &mut recorder_panel);
            });
        if !open {
            recorder_panel.open = false;
        }
    }

    // Left column, resizable: the **world** — the global scenario parameters (arena,
    // rate, seed, backgrounds), gene bounds, nutrients and relations. Purely scenario
    // data now (the view layers moved to the top-bar "View" menu), so its scroll area
    // wraps the whole content, heading included.
    egui::Panel::left("left_tools")
        .resizable(true)
        .default_size(280.0)
        .show_inside(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("World");
                editor::world_section(ui, &mut config);
            });
        });

    // Right column, resizable: the **entities** — archetype palette (selector +
    // species library) and editor of the selected archetype.
    egui::Panel::right("right_panel")
        .resizable(true)
        .default_size(320.0)
        .show_inside(&mut root, |ui| {
            ui.heading("Entities");
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::CollapsingHeader::new("Archetypes")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::selector_section(ui, &mut palette, &mut config, &mut ui_status)
                    });
                egui::CollapsingHeader::new("Archetype editor")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::editor_section(ui, &mut palette, &mut config)
                    });
            });
        });

    // Bottom panel, resizable: evolution curves (left) and agent inspector (right),
    // in two columns. **Created before `bottom_bar`** so it takes the very bottom edge.
    egui::Panel::bottom("bottom_panel")
        .resizable(true)
        .default_size(220.0)
        .show_inside(&mut root, |ui| {
            // Curves: we wrap them in a scroll area (they have none).
            let mut curves = |ui: &mut egui::Ui| {
                egui::ScrollArea::vertical()
                    .id_salt("curves")
                    .show(ui, |ui| {
                        ui.strong("Evolution — curves");
                        hud::hud_section(ui, &mut history, &config);
                    });
            };
            // Inspector: `inspector_section` already carries its own `ScrollArea`. If
            // there is a capture request, it **returns** the derived archetype, which
            // we add to the config after the closures (so as not to borrow `config`
            // mutably while the curves read it).
            let mut capture_request: Option<Archetype> = None;
            let mut inspector = |ui: &mut egui::Ui| {
                ui.strong("Agent inspector");
                if let Some(arch) =
                    inspector::inspector_section(ui, &selection, &config, &inspector_agents)
                {
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
                ui_status.set(format!("Archetype captured from {from}."));
            }
        });

    // Bottom bar (just below the sim): controls (pause/step/speed/reset) on the left,
    // global stats on the right, same line — then the **unified status line** (scenario
    // / species / capture / recording feedback, cf. `crate::status`).
    egui::Panel::bottom("bottom_bar").show_inside(&mut root, |ui| {
        ui.horizontal_wrapped(|ui| {
            controls::controls_section(ui, &mut sim_controls, &mut vtime);
            ui.separator();
            editor::stats_section(ui, &stats_agents);
        });
        if !ui_status.message.is_empty() {
            ui.separator();
            ui.weak(&ui_status.message);
        }
    });

    // The region left free by the panels: the central area where the sim is framed.
    // Non-deprecated successor of `ctx.available_rect()`.
    central.0 = root.available_rect_before_wrap();
    Ok(())
}
