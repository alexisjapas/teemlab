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
use teemlab::nutrients::Nutrients;
use teemlab::selection::{AutoSelect, Selection, SelectionRoll};
use teemlab::visuals::BlurCamera;

use crate::editor::{Palette, card, draw_mlp_graph};
use crate::fonts::{self, icons};

/// **World** position of the cursor in the play area (single camera and window),
/// if it exists, plus the world size of a **~6-pixel screen slack** — the picking
/// tolerance that keeps a small body clickable at any zoom. Shared by the
/// inspector's picking and the deletion: the `viewport_to_world_2d` accounts for
/// the centered sim's offset (cf. `main::set_sim_camera`), so the window cursor
/// remains the correct input.
fn pointer_world(
    cameras: &Query<(&Camera, &GlobalTransform), Without<BlurCamera>>,
    windows: &Query<&Window>,
) -> Option<(Vec2, f32)> {
    let (camera, cam_tf) = cameras.single().ok()?;
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let world = camera.viewport_to_world_2d(cam_tf, cursor).ok()?;
    // A probe 6 px to the side gives the world units per 6 px (the camera is 2D
    // orthographic: the scale is uniform, one probe is enough).
    let side = camera
        .viewport_to_world_2d(cam_tf, cursor + Vec2::X * 6.0)
        .ok()?;
    Some((world, world.distance(side)))
}

/// The nearest entity (body) whose radius — plus `slack`, the screen-space picking
/// tolerance in world units (cf. [`pointer_world`]) — **contains** `world`, if any.
/// Same criterion for selecting (inspector) and deleting — hence the sharing.
/// `None` = cursor in the void.
fn body_at<'a>(
    world: Vec2,
    slack: f32,
    bodies: impl IntoIterator<Item = (Entity, &'a Transform, &'a Radius)>,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, transform, radius) in bodies {
        let d = transform.translation.truncate().distance(world);
        if d <= radius.0 + slack && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((entity, d));
        }
    }
    best.map(|(entity, _)| entity)
}

/// Selects the agent under the cursor on a click in the play area. A click in the
/// void deselects; a click on an egui panel or during an archetype drag is
/// ignored (the editor handles the latter).
#[allow(clippy::too_many_arguments)]
pub fn pick_agent(
    mut contexts: EguiContexts,
    central: Res<crate::panels::CentralRect>,
    mut selection: ResMut<Selection>,
    mut auto: ResMut<AutoSelect>,
    palette: Res<Palette>,
    cameras: Query<(&Camera, &GlobalTransform), Without<BlurCamera>>,
    windows: Query<&Window>,
    agents: Query<(Entity, &Transform, &Radius), With<Agent>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // We do not pick during an archetype drag-and-drop (editor), nor when the
    // pointer targets an egui panel.
    if palette.dragging.is_some() || crate::panels::pointer_over_ui(ctx, central.0) {
        return Ok(());
    }
    let Some((world, slack)) = pointer_world(&cameras, &windows) else {
        return Ok(());
    };
    // The nearest agent whose body contains the cursor. Hover feedback first: a
    // clickable body shows a pointing hand *before* the click.
    let hovered = body_at(world, slack, agents);
    if hovered.is_some() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if ctx.input(|i| i.pointer.any_click()) {
        // Click on a body → select it; in the void → deselect. Picking an agent by
        // hand switches the Follow selector to **Manual** (`Off`), so the auto-follow
        // stops overriding the choice the observer just made (it would otherwise hold
        // the click only until that agent dies, then resume rolling).
        selection.0 = hovered;
        if hovered.is_some() {
            auto.roll = SelectionRoll::Off;
        }
    }
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
    cameras: Query<(&Camera, &GlobalTransform), Without<BlurCamera>>,
    windows: Query<&Window>,
    bodies: Query<(Entity, &Transform, &Radius)>,
) -> Result {
    if !crate::keymap::pressed(&keys, crate::keymap::UiAction::DeleteUnderCursor) {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    // Not while a text field holds keyboard focus (Backspace is then editing text,
    // not the world — the same gate as `main::keyboard_shortcuts`), nor during an
    // archetype drag, nor when the cursor targets an egui panel.
    if ctx.egui_wants_keyboard_input()
        || palette.dragging.is_some()
        || crate::panels::pointer_over_ui(ctx, central.0)
    {
        return Ok(());
    }
    let Some((world, slack)) = pointer_world(&cameras, &windows) else {
        return Ok(());
    };
    // The nearest body whose radius contains the cursor (same criterion as the
    // inspector's picking).
    if let Some(entity) = body_at(world, slack, bodies) {
        commands.entity(entity).despawn();
        if selection.0 == Some(entity) {
            selection.0 = None; // do not keep a phantom selection.
        }
    }
    Ok(())
}

/// The **follow-mode** combo box (a [`SelectionRoll`] picker), shared by the live
/// *Observation* overlay and the **Record menu**'s video sub-options. The two are
/// **different settings** (what the live view follows vs. what the render follows), so
/// only the widget is shared — each caller keeps its own label and interval control.
pub(crate) fn follow_combo(ui: &mut egui::Ui, id_salt: &str, roll: &mut SelectionRoll) {
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(roll.label())
        .show_ui(ui, |ui| {
            for mode in SelectionRoll::ALL {
                ui.selectable_value(roll, mode, mode.label())
                    .on_hover_text(mode.hint());
            }
        });
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
/// world**: we never write into the sim. The "Capture" / "Export as variant" buttons are
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
            &Nutrients,
        ),
        With<Agent>,
    >,
) -> Option<InspectorAction> {
    // Possible action request (cf. doc): set by the Capture menu, returned to the caller
    // who applies it (add to the scenario, or save a library variant). Named `request`
    // to avoid shadowing the `action` (`&Action`) component below.
    let mut request: Option<InspectorAction> = None;

    // Resolve the inspected agent up front — its data feeds the header's Capture menu. A
    // tuple of shared refs is `Copy`, so we can read it in the header and again in the body.
    let selected = selection.0.and_then(|e| agents.get(e).ok());

    // HEADER — the panel title, with the **Capture** menu pinned right (only when an
    // agent is inspected). Capture freezes this agent (evolved genome + concrete weights):
    // **To scenario** adds it to the current scenario (opens Studio); **Save as library
    // variant** writes it to the catalog under a name. We never touch the sim — we build
    // the derived archetype (`Archetype::capture`) and **return** the request; the caller
    // applies it. A **sticky** menu (closes only on a click outside), so the name field
    // and the items behave.
    ui.horizontal(|ui| {
        ui.strong("Agent inspector");
        if let Some((species, _, genotype, _, _, _, brain, generation, _, _)) = selected {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cap_label =
                    fonts::icon_label_tinted(icons::SPARKLE, "Capture", crate::theme::ACCENT);
                let cap_button = egui::Button::new(cap_label)
                    .fill(crate::theme::soft(crate::theme::ACCENT))
                    .stroke(egui::Stroke::new(
                        1.0,
                        crate::theme::line(crate::theme::ACCENT),
                    ));
                crate::theme::sticky_menu(ui, cap_button, |ui| {
                    // Cap the width: egui menus lay out justified (stretch to a wide
                    // default), so only a `max_width` tightens them.
                    ui.set_max_width(150.0);
                    if ui
                        .button(fonts::icon_label(icons::SPARKLE, "To scenario"))
                        .on_hover_text(
                            "Freeze this agent's evolved genome AND weights into a new \
                             archetype of the current scenario (opens Studio). The original \
                             species stays intact.",
                        )
                        .clicked()
                    {
                        request = config.archetypes.get(species.0 as usize).map(|src| {
                            InspectorAction::Capture(src.capture(
                                *genotype,
                                brain.clone(),
                                generation.0,
                            ))
                        });
                        ui.close();
                    }
                    ui.separator();
                    // Save as library variant — the same snapshot, written to the catalog
                    // as a NAMED variant of this species (species/saved/), with a
                    // "<scenario>-<n>" id the caller resolves. Name field on its own line,
                    // the button below it.
                    crate::theme::caption(ui, "Save as variant");
                    ui.add(
                        egui::TextEdit::singleline(variant_name)
                            .hint_text("variant name")
                            .desired_width(f32::INFINITY),
                    );
                    let named = !variant_name.trim().is_empty();
                    if ui
                        .add_enabled(
                            named,
                            egui::Button::new(fonts::icon_label(icons::FLOPPY, "Save"))
                                .min_size(egui::vec2(ui.available_width(), 30.0)),
                        )
                        .on_hover_text(
                            "Save this evolved agent as a named variant of its species in \
                             the library (species/saved/), reusable in any scenario.",
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
                        ui.close();
                    }
                });
            });
        }
    });
    ui.separator();

    // Body — the inspected agent, or a hint when nothing (valid) is selected.
    let Some((
        species,
        reserve,
        genotype,
        vision,
        perception,
        action,
        brain,
        generation,
        age,
        nutrients,
    )) = selected
    else {
        if selection.0.is_some() {
            // A selection that no longer resolves → the agent died.
            ui.colored_label(
                crate::theme::ERROR,
                "The selected agent no longer exists (dead?).",
            );
            ui.weak("Click another agent, or in the void to deselect.");
        } else {
            ui.weak("Click an agent in the area to inspect it.");
        }
        return request;
    };

    // An immobile entity (flora / sessile source) neither moves nor exploits
    // vision: we then hide the inert genes (locomotion, vision) and the perception
    // section — characteristics without effect, that would have nothing to show.
    let immobile = genotype.locomotion().is_immobile();

    // IDENTITY — the comp's 2×2 mini-cards: a faint label over a mono value. All four
    // cards share one height (a fixed content height) and the value **truncates**
    // instead of wrapping, so a long name can never make one card taller than its row.
    crate::theme::caption(ui, "Identity");
    let identity_cell = |ui: &mut egui::Ui, label: &str, value: String| {
        card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(30.0);
            ui.label(
                egui::RichText::new(label)
                    .size(11.0)
                    .color(crate::theme::INK_FAINT),
            );
            ui.add(egui::Label::new(egui::RichText::new(value).monospace().size(14.0)).truncate());
        });
    };
    // The species **name** only — the numeric id is shown nowhere else in the card and
    // adds no information the inspector needs (review).
    let species_name = config
        .archetypes
        .get(species.0 as usize)
        .map(|a| a.name.clone())
        .unwrap_or_default();
    ui.columns(2, |cols| {
        identity_cell(&mut cols[0], "Species", species_name);
        identity_cell(&mut cols[1], "Generation", generation.0.to_string());
    });
    ui.columns(2, |cols| {
        identity_cell(&mut cols[0], "Age", format!("{:.1} s", age.0));
        identity_cell(&mut cols[1], "Brain", brain.name().to_string());
    });
    ui.add_space(10.0);

    // ENERGY — comp gauge: caption + mono read-out over a slim amber bar.
    crate::theme::caption_value(
        ui,
        "Energy / reserve",
        &format!("{:.1} / {:.0}", reserve.current, reserve.max),
    );
    crate::theme::gauge(ui, reserve.fraction(), crate::theme::AMBER);
    ui.add_space(10.0);

    // NUTRIENT STORE (T3 — the second reservoir). Energy (sun/food) governs survival,
    // the nutrient governs reproduction. Shown whenever the **scenario** uses the
    // nutrient axis (a source emits, or some archetype has a capacity) — so it is
    // visible in a nutrient world even on an entity that carries none — but hidden in
    // the scenarios that ignore nutrients entirely (no "0 / 0" noise on plain fauna).
    let scenario_uses_nutrients = !config.sources.is_empty()
        || config
            .field_relations
            .iter()
            .any(|f| f.capacity > 0.0 || f.absorb > 0.0 || f.repro_cost > 0.0);
    if nutrients.capacity(0) > 0.0 || scenario_uses_nutrients {
        if nutrients.capacity(0) > 0.0 {
            crate::theme::caption_value(
                ui,
                "Nutrient store",
                &format!("{:.1} / {:.0}", nutrients.current(0), nutrients.capacity(0)),
            );
            // The comp's nutrient blue (#6aa6ff) — a channel encoding local to the
            // inspector, like TARGET / THREAT.
            crate::theme::gauge(
                ui,
                nutrients.fraction(),
                egui::Color32::from_rgb(106, 166, 255),
            )
            .on_hover_text("Absorbed from the field / eaten; spent to reproduce.");
        } else {
            // A nutrient world, but this entity is off the axis (capacity 0).
            crate::theme::caption(ui, "Nutrient store");
            ui.weak("Not on the nutrient axis (capacity 0).");
        }
        ui.add_space(10.0);
    }

    // GENOTYPE. One row per gene — the comp's dense list: a muted name on the left, a
    // mono value **pinned right**, both at 12.5 pt (the comp's genotype size; the old
    // 14 pt read oversized against the tight inspector column).
    crate::theme::caption(ui, "Genotype (inherited genes)");
    let gene_row = |ui: &mut egui::Ui, name: &str, value: String| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(name)
                    .size(12.5)
                    .color(crate::theme::INK_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(value)
                        .monospace()
                        .size(12.5)
                        .color(crate::theme::INK),
                );
            });
        });
    };
    card(ui, |ui| {
        ui.set_min_width(ui.available_width());
        if immobile {
            // A state explanation of *absent* content: stays visible (nothing to hover).
            ui.weak("Immobile — locomotion and vision genes hidden (no effect).");
        }
        // One row per TRAITS characteristic: adding a trait displays it here without
        // touching the inspector. On an immobile entity, we skip the inert genes
        // (locomotion, vision).
        for t in &TRAITS {
            if immobile && t.inert_when_immobile {
                continue;
            }
            gene_row(
                ui,
                t.name,
                format!("{:.*}", t.decimals as usize, (t.get)(genotype)),
            );
        }
        // The vision cost only makes sense for an entity that sees (rays > 0).
        if !immobile {
            gene_row(
                ui,
                "vision cost/s",
                format!("{:.3}", vision.metabolic_cost()),
            );
        }
    });

    // ACTION — heading read-out + the comp's accent throttle gauge.
    ui.add_space(10.0);
    crate::theme::caption(ui, "Action (brain output)");
    card(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let throttle = action.throttle;
        let heading_deg = if action.dir.length_squared() > 1e-6 {
            action.dir.to_angle().to_degrees()
        } else {
            0.0
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("desired heading")
                    .size(12.5)
                    .color(crate::theme::INK_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!("{heading_deg:+.0}°"));
            });
        });
        crate::theme::caption_value(ui, "throttle", &format!("{throttle:.2}"));
        crate::theme::gauge(ui, throttle, crate::theme::ACCENT);
    });

    // (Capture + Save-as-variant moved under the panel title — review.)

    // MLP brain: the network in action (item 18b-viz). Nodes colored by their
    // current activation (the last `think`), edges by sign/weight — the learned
    // decision made readable. The other brains have no graph.
    if let Brain::Mlp(mlp) = brain {
        card(ui, |ui| {
            ui.strong("MLP brain (activations)").on_hover_text(
                "input (vision/target) → hidden layers → steering · color = activation \
                 (cold < 0 < warm) · size = |bias|",
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
        // Proprioception summary in the header read-out (comp); one slim gauge per
        // ray (obstacle proximity, gray) with two fixed swatches whose **opacity**
        // encodes the target / threat channels.
        ui.add_space(10.0);
        let [nrg, nut, spd] = perception.self_state;
        // Caption and proprioception read-out on **separate lines**: side by side (a
        // `caption_value`) the two collided in the narrow inspector column — the
        // "PERCEPTION · N RAYS" title overran the "nrg … nut … spd …" figures.
        crate::theme::caption(ui, &format!("Perception · {} rays", vision.ray_count));
        ui.label(
            egui::RichText::new(format!("nrg {nrg:.2} · nut {nut:.2} · spd {spd:.2}"))
                .monospace()
                .size(11.0)
                .color(crate::theme::INK_MUTED),
        );
        card(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // Tighten the rays into one dense block: **no** inter-row gap AND a reduced
            // row height (each row is as tall as its tallest item — the `r{i}` label — so
            // shrinking the label is what actually pulls the bars together).
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, &proximity) in perception.vision.iter().enumerate() {
                let target = perception.target.get(i).copied().unwrap_or(0.0);
                let threat = perception.threat.get(i).copied().unwrap_or(0.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("r{i}"))
                            .monospace()
                            .size(9.0)
                            .line_height(Some(9.0))
                            .color(crate::theme::INK_FAINT),
                    );
                    // The proximity gauge takes what the two 12 px swatches leave.
                    let gap = ui.spacing().item_spacing.x;
                    let bar_w = (ui.available_width() - 2.0 * (12.0 + gap) - 0.5).max(1.0);
                    crate::theme::gauge_sized(ui, proximity, crate::theme::INK_MUTED, bar_w, 9.0)
                        .on_hover_text(format!("obstacle {proximity:.2}"));
                    for (v, c, ch) in [
                        (target, crate::theme::TARGET, "target"),
                        (threat, crate::theme::THREAT, "threat"),
                    ] {
                        let (r, resp) =
                            ui.allocate_exact_size(egui::vec2(12.0, 9.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            r,
                            3.0,
                            c.gamma_multiply(0.2 + 0.8 * v.clamp(0.0, 1.0)),
                        );
                        resp.on_hover_text(format!("{ch} {v:.2} — 0 = nothing, 1 = in contact"));
                    }
                });
            }
        });
    }

    request
}
