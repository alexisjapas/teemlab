//! **Docked** layout of the windowed build: fixed egui panels around the central
//! simulation area, assembled by a **single** system ([`dock`]).
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
//! full-width at the bottom; *scenario IO + transport controls + View menu + Export* in
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

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use teemlab::genotype::Genotype;
use teemlab::metrics::History;
use teemlab::selection::Selection;
use teemlab::visuals::Layers;

use crate::controls::{self, SimControls};
use crate::editor::{self, Palette};
use crate::fonts::{self, icons};
use crate::hud;
use crate::inspector;
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};
use crate::status::UiStatus;

/// Fixed width (egui points) of **both** side panels (Edit / Analysis). They are
/// **non-resizable**: an egui side panel cannot shrink-wrap its content's width
/// (sliders and scroll areas fill whatever width they are given — there is no natural
/// width to collapse to, unlike the bottom panel's content-driven *height*), so we
/// pick a width that comfortably fits the densest content (the gene editor) rather
/// than offering a drag handle. Kept **equal** left and right so the centered sim
/// stays centered.
const SIDE_PANEL_WIDTH: f32 = 370.0;

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
    // Gate: don't render until the UI fonts are live (cf. `fonts`), so an icon is never
    // drawn before its Phosphor family is bound (egui binds fonts only next-pass).
    fonts_ready: Res<crate::fonts::FontsReady>,
    // Last frame's measured width of the centered transport controls (for centering).
    mut ctrl_width: Local<f32>,
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
    // Skip the first pass (before the fonts are bound): the icons would panic, and a
    // blank first frame on the paused startup screen is imperceptible.
    if !fonts_ready.0 {
        return Ok(());
    }
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

    // Top strip, **a single line** — the app's command strip: scenario IO (the
    // Scenario menu) pinned **left**; the **transport controls** (play / step / speed /
    // reset) **centered**; the **View** menu and the **Export** toggle pinned **right**.
    // Video recording lives in a floating window opened by the Export button (below).
    egui::Panel::top("top_bar").show_inside(&mut root, |ui| {
        let row_h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let full_w = ui.available_width();
                // LEFT: scenario IO.
                ui.push_id("scenario_bar", |ui| {
                    runs::scenario_section(ui, &mut runs_panel, &mut config, &mut ui_status);
                });
                // CENTER: the transport controls, centered on the **whole bar**. egui
                // can't center a *group* along the main axis in immediate mode (it only
                // learns the group's width after laying it out), so we pad by the width
                // measured last frame (`ctrl_width`, 1-frame lag, clamped so it never
                // collides with the scenario group). `scope` measures this frame's width.
                let left_w = full_w - ui.available_width();
                let pad = (full_w * 0.5 - *ctrl_width * 0.5 - left_w).max(8.0);
                ui.add_space(pad);
                let measured = ui
                    .scope(|ui| controls::controls_section(ui, &mut sim_controls, &mut vtime))
                    .response
                    .rect
                    .width();
                *ctrl_width = measured;
                // RIGHT (emitted right→left): Export rightmost, then the View menu, so
                // reading order is View · Export.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(fonts::icon_label(icons::RECORD, "Export…"))
                        .on_hover_text(
                            "Render the current scenario to a video (opens the export panel).",
                        )
                        .clicked()
                    {
                        recorder_panel.open = !recorder_panel.open;
                    }
                    ui.menu_button(fonts::icon_label(icons::CARET_DOWN, "View"), |ui| {
                        editor::layers_section(ui, &mut layers)
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

    // Bottom panel reserved **first** so it spans the **full width**: the evolution
    // **curves** (a time series) with the unified **status line** folded in. Reserving
    // it before the side columns is what puts them — and the archetype editor — *above*
    // the curves (the order of `show_inside` is the layout). Non-resizable and not
    // wrapped in a `ScrollArea`, so it sizes to exactly the content's height.
    egui::Panel::bottom("bottom_panel").show_inside(&mut root, |ui| {
        if !ui_status.message.is_empty() {
            ui.weak(&ui_status.message);
            ui.separator();
        }
        ui.strong("Evolution — curves");
        hud::hud_section(ui, &mut history, &config);
    });

    // Left column, **fixed width, non-resizable** (width via [`SIDE_PANEL_WIDTH`] — egui
    // side panels can't fit their width to content, so no drag handle): **the world** —
    // the scenario parameters (*World*) and the entities list (*Archetypes*, with the
    // species library). Editing *one* archetype lives in its own panel (below), opened on
    // click — a master/detail split that keeps this panel about the scenario as a whole.
    egui::Panel::left("left_tools")
        .exact_size(SIDE_PANEL_WIDTH)
        .show_inside(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::CollapsingHeader::new("World")
                    .default_open(true)
                    .show(ui, |ui| editor::world_section(ui, &mut config));
                egui::CollapsingHeader::new("Archetypes")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::selector_section(ui, &mut palette, &mut config, &mut ui_status)
                    });
            });
        });

    // Right column, **fixed width, non-resizable** (same [`SIDE_PANEL_WIDTH`] as the
    // left, so the sim stays centered): **Analysis** of the current state — live
    // *stats* (means) on top, then the agent *inspector*. (The evolution curves — a
    // time series — stay at the bottom.)
    egui::Panel::right("right_panel")
        .exact_size(SIDE_PANEL_WIDTH)
        .show_inside(&mut root, |ui| {
            egui::CollapsingHeader::new("Live stats")
                .default_open(false)
                .show(ui, |ui| editor::stats_section(ui, &stats_agents));
            ui.separator();
            ui.strong("Agent inspector");
            // `inspector_section` carries its own `ScrollArea` and **returns** any
            // capture request (a derived archetype): we add it to the config *after*
            // the call (it borrows `config` shared) → mutable borrow then allowed.
            if let Some(arch) =
                inspector::inspector_section(ui, &selection, &config, &inspector_agents)
            {
                let from = arch.captured_from.clone().unwrap_or_default();
                config.archetypes.push(arch);
                palette.selected = Some(config.archetypes.len() - 1);
                ui_status.set(format!("Archetype captured from {from}."));
            }
        });

    // Archetype editor — the **detail** half: a second left column that docks to the
    // left of the central area (i.e. right of the world panel, above the full-width
    // curves) and opens **only when an archetype is selected** (clicked in the list).
    // Created **last**, after every unconditional panel: an egui child panel's id mixes
    // in the parent's running auto-id counter ([`egui::Ui::new_child`]), so a
    // *conditional* panel inserted earlier would shift the *later* panels' widget ids
    // each time it toggles → egui's "changed id between passes" warnings (and lost
    // widget state). Created last, the others keep stable ids; only this panel's own
    // widgets come and go, which is expected. Its left-docking position is unchanged by
    // the order (it takes the left edge of whatever rect the other panels leave free).
    let mut deselect = false;
    if palette
        .selected
        .is_some_and(|i| i < config.archetypes.len())
    {
        egui::Panel::left("archetype_editor")
            .exact_size(SIDE_PANEL_WIDTH)
            .show_inside(&mut root, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Archetype editor");
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    editor::editor_section(ui, &mut palette, &mut config)
                });
            });
    }
    if deselect {
        palette.selected = None;
    }

    // The region left free by the panels: the central area where the sim is framed.
    // Non-deprecated successor of `ctx.available_rect()`.
    central.0 = root.available_rect_before_wrap();

    // Sim-state overlay over that central area (egui composites over the Bevy sim):
    // the run time and speed always, a prominent PAUSED banner when frozen. The run
    // time comes from the history's latest sample (so it resets with the world).
    let painter = root.painter().with_clip_rect(central.0);
    let cx = central.0.center().x;
    let run_time = history.latest_time();
    painter.text(
        egui::pos2(cx, central.0.top() + 6.0),
        egui::Align2::CENTER_TOP,
        format!("t = {run_time:.1} s   ·   ×{:.1}", sim_controls.speed),
        egui::FontId::monospace(12.0),
        egui::Color32::from_gray(140),
    );
    if vtime.is_paused() {
        painter.text(
            egui::pos2(cx, central.0.top() + 24.0),
            egui::Align2::CENTER_TOP,
            "PAUSED",
            egui::FontId::proportional(20.0),
            egui::Color32::from_rgb(240, 180, 80),
        );
    }
    Ok(())
}
