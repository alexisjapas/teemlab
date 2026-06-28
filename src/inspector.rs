//! Agent inspector of the windowed build: **click an agent → see its state**
//! (item 12).
//!
//! A module of the windowed *binary* only (like [`crate::editor`],
//! [`crate::hud`], [`crate::controls`]). It is the behavior debugging tool — the
//! guardrail of the deterministic control group: we read the genotype, energy,
//! perception and current action of a living agent.
//!
//! Read-only: we never write into the sim. The selection (an `Entity`) and its
//! rendering (a gizmo ring) live in the windowed binary; the cardinal invariant
//! holds.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::brain::Brain;
use teemlab::components::{
    Action, Age, Agent, Generation, Perception, Radius, Reserve, Species, Vision,
};
use teemlab::config::{Archetype, SimConfig};
use teemlab::genotype::{Genotype, TRAITS};
use teemlab::selection::{AutoSelect, Selection, SelectionRoll};

use crate::editor::{Palette, card, draw_mlp_graph};
use crate::fonts::{self, icons};
use crate::help;

/// **World** position of the cursor in the play area (single camera and window),
/// if it exists. Shared by the inspector's picking and the deletion: the
/// `viewport_to_world_2d` accounts for the centered sim's offset (cf.
/// `main::set_sim_camera`), so the window cursor remains the correct input.
fn pointer_world(
    cameras: &Query<(&Camera, &GlobalTransform)>,
    windows: &Query<&Window>,
) -> Option<Vec2> {
    let (camera, cam_tf) = cameras.single().ok()?;
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    camera.viewport_to_world_2d(cam_tf, cursor).ok()
}

/// The nearest entity (body) whose radius **contains** `world`, if any. Same
/// criterion for selecting (inspector) and deleting — hence the sharing. `None` =
/// cursor in the void.
fn body_at<'a>(
    world: Vec2,
    bodies: impl IntoIterator<Item = (Entity, &'a Transform, &'a Radius)>,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, transform, radius) in bodies {
        let d = transform.translation.truncate().distance(world);
        if d <= radius.0 && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((entity, d));
        }
    }
    best.map(|(entity, _)| entity)
}

/// Selects the agent under the cursor on a click in the play area. A click in the
/// void deselects; a click on an egui panel or during an archetype drag is
/// ignored (the editor handles the latter).
pub fn pick_agent(
    mut contexts: EguiContexts,
    central: Res<crate::panels::CentralRect>,
    mut selection: ResMut<Selection>,
    palette: Res<Palette>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    agents: Query<(Entity, &Transform, &Radius), With<Agent>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // We do not pick during an archetype drag-and-drop (editor), nor when the
    // click targets an egui panel, nor if there is no click at all.
    if palette.dragging.is_some()
        || !ctx.input(|i| i.pointer.any_click())
        || crate::panels::pointer_over_ui(ctx, central.0)
    {
        return Ok(());
    }
    let Some(world) = pointer_world(&cameras, &windows) else {
        return Ok(());
    };
    // The nearest agent whose body contains the click; otherwise (void) → None.
    selection.0 = body_at(world, agents);
    Ok(())
}

/// Manual deletion (Delete / Backspace): removes the entity **under the cursor**
/// — agent OR food (any body with a [`Radius`]; walls, which have no `Radius`, are
/// spared). Manual editing triggered by the user, like the editor's placement →
/// lives outside `FixedUpdate`, and remains allowed even when not paused, for
/// consistency with placement. No undo in v1: an entity is re-placed from the
/// palette (the world is an experiment sandbox, not precious data).
///
/// Like [`pick_agent`] and `resolve_drag`, it must run **after** `panels::dock` so
/// that the central rect it feeds [`crate::panels::pointer_over_ui`] is current
/// (otherwise a Delete over a panel would target the entity hidden beneath it).
#[allow(clippy::too_many_arguments)]
pub fn delete_under_cursor(
    mut contexts: EguiContexts,
    central: Res<crate::panels::CentralRect>,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<Palette>,
    mut selection: ResMut<Selection>,
    mut commands: Commands,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    bodies: Query<(Entity, &Transform, &Radius)>,
) -> Result {
    if !(keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace)) {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    // Not during an archetype drag, nor when the cursor targets an egui panel.
    if palette.dragging.is_some() || crate::panels::pointer_over_ui(ctx, central.0) {
        return Ok(());
    }
    let Some(world) = pointer_world(&cameras, &windows) else {
        return Ok(());
    };
    // The nearest body whose radius contains the cursor (same criterion as the
    // inspector's picking).
    if let Some(entity) = body_at(world, bodies) {
        commands.entity(entity).despawn();
        if selection.0 == Some(entity) {
            selection.0 = None; // do not keep a phantom selection.
        }
    }
    Ok(())
}

/// **Observation controls** (windowed): the *follow* mode — the **same options as
/// the video recorder** ([`SelectionRoll`]) — plus the camera **Reset view**. All
/// rendering-side: it writes the auto-follow mode and the [`crate::ViewControl`],
/// never the sim. `None` leaves selection to **manual mouse picking** (click an
/// agent); the other modes auto-follow as in a video. A manual click still works in
/// any mode — the driver then *holds* the clicked agent until it dies (cf.
/// [`teemlab::selection`]).
pub(crate) fn observation_section(
    ui: &mut egui::Ui,
    auto: &mut AutoSelect,
    view: &mut crate::ViewControl,
) {
    ui.horizontal(|ui| {
        ui.label("Follow:");
        egui::ComboBox::from_id_salt("follow_mode")
            .selected_text(auto.roll.label())
            .show_ui(ui, |ui| {
                for mode in SelectionRoll::ALL {
                    ui.selectable_value(&mut auto.roll, mode, mode.label());
                }
            });
        if ui
            .button(fonts::icon_label(icons::RESET, "Reset view"))
            .on_hover_text("Recenter on the whole arena (pan / zoom)  ·  Home")
            .clicked()
        {
            *view = crate::ViewControl::default();
        }
    });
    // The interval only matters for the "timer" modes (cycle / active / species tour).
    if auto.roll.rolls() {
        ui.add(egui::Slider::new(&mut auto.interval, 0.5..=20.0).text("interval (s)"));
    }
    help::hint(
        ui,
        "None = manual: click an agent. Other modes auto-follow like the video. \
         Scroll = zoom · middle/right-drag = pan · Home = reset view.",
    );
}

/// What the inspector asks the caller to do this frame (it never writes the sim/config
/// itself — the editor does). `None` = nothing this tick.
pub(crate) enum InspectorAction {
    /// Add the captured archetype to the **current scenario** (evolved genome + weights).
    Capture(Archetype),
    /// Save the captured archetype as a library **variant** of scenario species `species`.
    SaveVariant {
        /// Scenario archetype index the variant derives from (its base form).
        species: u16,
        /// The captured variant archetype (display name already set by the user).
        variant: Archetype,
    },
}

/// The agent inspector — genotype, energy, perception, action (+ MLP graph) of
/// the selected agent. Rendered in the bottom panel (on the right, dock item). If
/// the selected agent has disappeared (died), we report it. **Read-only over the
/// world**: we never write into the sim. The "Capture" / "Save as variant" buttons are
/// no exception — they *read* the agent and **return** an [`InspectorAction`] the caller
/// applies (the editor writes the config / the library, not the sim). `variant_name` is
/// the live buffer of the variant-name field. `None` when the user does nothing this tick.
pub(crate) fn inspector_section(
    ui: &mut egui::Ui,
    selection: &Selection,
    config: &SimConfig,
    variant_name: &mut String,
    agents: &Query<
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
) -> Option<InspectorAction> {
    let Some(entity) = selection.0 else {
        ui.weak("Click an agent in the area to inspect it.");
        return None;
    };
    let Ok((species, reserve, genotype, vision, perception, action, brain, generation, age)) =
        agents.get(entity)
    else {
        ui.colored_label(
            egui::Color32::from_rgb(255, 140, 120),
            "The selected agent no longer exists (dead?).",
        );
        ui.weak("Click another agent, or in the void to deselect.");
        return None;
    };

    // Possible action request (cf. doc): set by the buttons below, returned to the
    // caller who applies it (add to the scenario, or save a library variant). Named
    // `request` to avoid shadowing the `action` (`&Action`) component above.
    let mut request: Option<InspectorAction> = None;

    // An immobile entity (flora / sessile source) neither moves nor exploits
    // vision: we then hide the inert genes (locomotion, vision) and the perception
    // section — characteristics without effect, that would have nothing to show.
    let immobile = genotype.locomotion().is_immobile();

    // A scroll area avoids clipping the list of vision rays when the panel is
    // reduced.
    egui::ScrollArea::vertical().show(ui, |ui| {
        // IDENTITY.
        card(ui, |ui| {
            ui.strong("Identity");
            ui.label(format!("Species: {}", species.0));
            ui.label(format!("Brain: {}", brain.name()));
            ui.label(format!("Generation: {}", generation.0));
            ui.label(format!("Age: {:.1} s", age.0));
        });

        // ENERGY.
        card(ui, |ui| {
            ui.strong("Energy / reserve");
            ui.add(
                egui::ProgressBar::new(reserve.fraction())
                    .text(format!("{:.1} / {:.0}", reserve.current, reserve.max)),
            );
        });

        // GENOTYPE.
        card(ui, |ui| {
            ui.strong("Genotype (inherited genes)");
            if immobile {
                help::hint(ui, "Immobile: locomotion and vision genes hidden (no effect).");
            }
            egui::Grid::new("genes").num_columns(2).show(ui, |ui| {
                // One row per TRAITS characteristic: adding a trait displays it here
                // without touching the inspector. On an immobile entity, we skip the
                // inert genes (locomotion, vision).
                for t in &TRAITS {
                    if immobile && t.inert_when_immobile {
                        continue;
                    }
                    ui.label(t.name);
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.*}",
                            t.decimals as usize,
                            (t.get)(genotype)
                        ))
                        .monospace(),
                    );
                    ui.end_row();
                }
                // The vision cost only makes sense for an entity that sees (rays > 0).
                if !immobile {
                    ui.label("vision cost/s");
                    ui.label(egui::RichText::new(format!("{:.3}", vision.metabolic_cost())).monospace());
                    ui.end_row();
                }
            });
        });

        // ACTION.
        card(ui, |ui| {
            ui.strong("Action (brain output)");
            let throttle = action.throttle;
            let heading_deg = if action.dir.length_squared() > 1e-6 {
                action.dir.to_angle().to_degrees()
            } else {
                0.0
            };
            ui.label(format!("desired heading: {heading_deg:+.0}°"));
            ui.add(egui::ProgressBar::new(throttle).text(format!("throttle {throttle:.2}")));
        });

        // CAPTURE (a standalone action): freeze this agent (evolved genome + concrete
        // weights) into a new reusable archetype. We do not touch the sim: we build the
        // derived archetype (a clone of the original species, cf. `Archetype::capture`)
        // and return it — the caller will add it to the config.
        if ui
            .button(fonts::icon_label(icons::FLOPPY, "Capture as archetype"))
            .on_hover_text(
                "Creates a new archetype freezing this agent's evolved genome AND weights \
                 (to reuse trained weights). The original species stays intact.",
            )
            .clicked()
        {
            request = config
                .archetypes
                .get(species.0 as usize)
                .map(|src| InspectorAction::Capture(src.capture(*genotype, brain.clone(), generation.0)));
        }

        // SAVE AS LIBRARY VARIANT: the same snapshot (evolved genome + frozen weights),
        // but written to the catalog as a NAMED variant of this species (species/saved/),
        // with a "<scenario>-<n>" id — the caller resolves the base + id (cf. editor).
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(variant_name)
                    .hint_text("variant name")
                    .desired_width(120.0),
            );
            let named = !variant_name.trim().is_empty();
            if ui
                .add_enabled(
                    named,
                    egui::Button::new(fonts::icon_label(icons::UPLOAD, "Save as variant")),
                )
                .on_hover_text(
                    "Save this evolved agent as a named variant in the species library \
                     (species/saved/), reusable in any scenario.",
                )
                .clicked()
                && let Some(src) = config.archetypes.get(species.0 as usize)
            {
                let mut variant = src.capture(*genotype, brain.clone(), generation.0);
                variant.name = variant_name.trim().to_string();
                request = Some(InspectorAction::SaveVariant {
                    species: species.0,
                    variant,
                });
            }
        });

        // MLP brain: the network in action (item 18b-viz). Nodes colored by their
        // current activation (the last `think`), edges by sign/weight — the learned
        // decision made readable. The other brains have no graph.
        if let Brain::Mlp(mlp) = brain {
            card(ui, |ui| {
                ui.strong("MLP brain (activations)");
                help::hint(
                    ui,
                    "input (vision/target) → hidden layers → steering · color = activation (cold<0<warm) · size = |bias|",
                );
                // The activations are recomputed here, on demand, for the single
                // inspected agent (the sim core's `think` no longer memorizes them).
                let activations = mlp.forward_activations(perception);
                draw_mlp_graph(ui, &mlp.layer_sizes(), Some(mlp), Some(&activations));
            });
        }

        // Perception section reserved for entities that see: a flora (immobile,
        // without a ray) has no channel to show.
        if !immobile {
            card(ui, |ui| {
                ui.strong(format!("Perception — vision ({} rays)", vision.ray_count));
                help::hint(
                    ui,
                    "obstacle (gray) · edible target (orange) · threat (red) — 0 = nothing, 1 = in contact",
                );
                for (i, &proximity) in perception.vision.iter().enumerate() {
                    let target = perception.target.get(i).copied().unwrap_or(0.0);
                    let threat = perception.threat.get(i).copied().unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(proximity)
                                .desired_width(95.0)
                                .text(format!("r{i} · {proximity:.2}")),
                        );
                        ui.add(
                            egui::ProgressBar::new(target)
                                .desired_width(85.0)
                                .fill(egui::Color32::from_rgb(220, 130, 40))
                                .text(format!("{target:.2}")),
                        );
                        ui.add(
                            egui::ProgressBar::new(threat)
                                .desired_width(85.0)
                                .fill(egui::Color32::from_rgb(210, 60, 60))
                                .text(format!("{threat:.2}")),
                        );
                    });
                }
            });
        }
    });

    request
}
