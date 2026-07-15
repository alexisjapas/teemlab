//! Simulation controls of the windowed build: **pause / speed / single-step /
//! reset** (item 11).
//!
//! A module of the windowed *binary* only (like [`crate::editor`] and
//! [`crate::hud`]). Time control goes through `Time<Virtual>` — the fixed clock
//! follows it (§6), so the pause freezes the sim *and* the HUD while rendering
//! continues, and the fast-forward changes the evolution rate without touching
//! rendering.
//!
//! Cardinal invariant respected: we never touch the sim *logic*, we only set its
//! clock or, for the reset, **rebuild the world** from the `SimConfig` — the
//! equivalent of a new `Startup`, triggered by hand (like the editor's
//! placement, it is editing, not sim).

use bevy::prelude::*;
use bevy_egui::egui;

use crate::fonts::{self, icons};
use crate::keymap::{self, UiAction};
use teemlab::SimConfig;
use teemlab::components::{Agent, Wall};
use teemlab::config::Archetype;
use teemlab::ecology::SimRng;
use teemlab::metrics::History;
use teemlab::nutrients::{Emits, Fields};
use teemlab::selection::Selection;
use teemlab::spawn;
use teemlab::visuals::NutrientLayer;

/// Controls state: chosen speed, pending steps, requested reset. The buttons (in
/// `EguiPrimaryContextPass`, too late for the frame's fixed loop) only write
/// here; it is [`drive_steps`] and [`apply_reset`], in `PreUpdate`, that act
/// **before** the fixed loop runs.
#[derive(Resource)]
pub struct SimControls {
    /// Active relative speed (applied to `Time<Virtual>` when not paused).
    pub speed: f32,
    /// Number of fixed ticks to play one by one while paused.
    pub steps_pending: u32,
    /// Reset requested this frame.
    pub reset_requested: bool,
}

impl Default for SimControls {
    fn default() -> Self {
        Self {
            speed: 1.0,
            steps_pending: 0,
            reset_requested: false,
        }
    }
}

/// `Startup`: the sim starts **paused**, so one can place/edit and prepare a run
/// before it runs. We only freeze the clock (`Time<Virtual>`) — the fixed clock
/// follows it (§6); rendering, meanwhile, continues.
pub fn pause_at_launch(mut vtime: ResMut<Time<Virtual>>) {
    vtime.pause();
}

/// The config the **running world** was built from (startup, then every reset). The
/// transport's Reset button compares the live [`SimConfig`] against it
/// ([`world_diverged`]) to show, in accent, that edits are waiting for a ⟲ — the
/// reset-vs-live application timing of the World / archetype editors, made visible
/// even with the inline hints turned off.
#[derive(Resource, Default)]
pub struct WorldBaseline(pub SimConfig);

/// `Startup`: capture the config the initial world was populated from (the CLI
/// scenario or the empty canvas) as the first [`WorldBaseline`].
pub fn init_world_baseline(config: Res<SimConfig>, mut baseline: ResMut<WorldBaseline>) {
    baseline.0 = config.clone();
}

/// Whether the running world (built from `world` at the last reset) no longer matches
/// `config` on the **reset-bound** fields — those only applied by a rebuild.
/// Live-applied fields (relations, field relations, gene bounds, colors) and fields
/// outside the live world (`batch`) are ignored. The struct is destructured without
/// `..` so a new `SimConfig` field forces a decision here: reset-bound (compare) or
/// live (bind to `_`).
pub fn world_diverged(config: &SimConfig, world: &SimConfig) -> bool {
    let SimConfig {
        tick_hz,
        arena_half_extent,
        archetypes,
        field_resolution,
        components,
        sources,
        seed,
        founder_pools,
        // Live-applied: read from the config every tick/frame, never stale in the world.
        field_relations: _,
        speed_bounds: _,
        agility_bounds: _,
        vision_range_bounds: _,
        vision_fov_bounds: _,
        reproduction_threshold_bounds: _,
        offspring_energy_bounds: _,
        mutation_rate_bounds: _,
        vision_rays_bounds: _,
        photosynthesis_bounds: _,
        seed_dispersal_bounds: _,
        brain_cost_bounds: _,
        act_cost_bounds: _,
        cost_law: _,
        predation: _,
        play_area_color: _,
        off_game_color: _,
        decor: _,
        // Outside the live world (the breeding orchestrator runs its own copies).
        batch: _,
    } = config;
    *tick_hz != world.tick_hz
        || *arena_half_extent != world.arena_half_extent
        || *field_resolution != world.field_resolution
        || *seed != world.seed
        || *components != world.components
        || *sources != world.sources
        || *founder_pools != world.founder_pools
        || archetypes.len() != world.archetypes.len()
        || archetypes
            .iter()
            .zip(&world.archetypes)
            .any(|(a, b)| archetype_diverged(a, b))
}

/// The archetype half of [`world_diverged`]: everything **baked at spawn** (bodies,
/// brains, genomes, counts). `name` is display-only and `color` is re-read every
/// frame by the reserve shading (cf. `visuals::shade_by_reserve`) — both live,
/// excluded; so are the provenance labels.
fn archetype_diverged(a: &Archetype, b: &Archetype) -> bool {
    let Archetype {
        count,
        radius,
        reserve_max,
        genotype,
        brain,
        mutable,
        captured_brain,
        anchor,
        // Live or display-only.
        name: _,
        color: _,
        source: _,
        captured_from: _,
    } = a;
    *count != b.count
        || *radius != b.radius
        || *reserve_max != b.reserve_max
        || *genotype != b.genotype
        || *brain != b.brain
        || *mutable != b.mutable
        || *captured_brain != b.captured_brain
        || *anchor != b.anchor
}

/// The simulation controls — pause / step / speed / reset. Only acts on
/// `Time<Virtual>` (pause/speed) or sets a flag (step, reset). Rendered **centered in
/// the top bar** (fixed dock) by [`crate::panels::dock`], which handles the panel;
/// this section only draws the button row.
pub(crate) fn controls_section(
    ui: &mut egui::Ui,
    controls: &mut SimControls,
    vtime: &mut Time<Virtual>,
    config: &SimConfig,
    world: &WorldBaseline,
) {
    // Play/Pause and Step are **icon-only, fixed-size** buttons: their width no longer
    // changes with the label ("Play" ↔ "Pause"), so the whole group's width is constant
    // and the top-bar centering (which pads by the last-frame width) never visibly shifts.
    let paused = vtime.is_paused();
    let play_glyph = if paused { icons::PLAY } else { icons::PAUSE };
    // Play/pause + step share one card-filled cluster (the comp's transport group:
    // 4 pt padding, 11 pt radius). Play is the transport's **primary** control — teal
    // (the comp's accent2) with the **filled** glyph weight, 38×34 like the comp.
    egui::Frame::new()
        .fill(crate::theme::CARD)
        .corner_radius(egui::CornerRadius::same(11))
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui
                .add(
                    egui::Button::new(fonts::icon_fill(play_glyph).color(crate::theme::ON_ACCENT2))
                        .fill(crate::theme::ACCENT2)
                        .min_size(egui::vec2(38.0, 34.0))
                        .corner_radius(egui::CornerRadius::same(8)),
                )
                .on_hover_text(keymap::tooltip("Play / pause", UiAction::PlayPause))
                .clicked()
            {
                if paused {
                    vtime.unpause();
                } else {
                    vtime.pause();
                }
            }
            // Single-stepping only makes sense when stopped. Same footprint as Play
            // (the group's width stays constant), quiet fill.
            ui.add_enabled_ui(paused, |ui| {
                if ui
                    .add(
                        egui::Button::new(fonts::icon(icons::STEP))
                            .min_size(egui::vec2(38.0, 34.0))
                            .corner_radius(egui::CornerRadius::same(8)),
                    )
                    .on_hover_text(keymap::tooltip(
                        "Advance one tick (when paused)",
                        UiAction::StepOnce,
                    ))
                    .clicked()
                {
                    controls.steps_pending += 1;
                }
            });
        });

    ui.add_space(8.0);
    // The comp's speed block: a faint mono "SPEED" caption, the log slider (an
    // addition kept from the previous transport — fine tuning ×0.1–×10 on one
    // handle), then the presets as the comp's segmented control.
    crate::theme::caption(ui, "Speed");
    ui.spacing_mut().slider_width = 120.0;
    if ui
        .add(egui::Slider::new(&mut controls.speed, 0.1..=10.0).logarithmic(true))
        .changed()
    {
        vtime.set_relative_speed(controls.speed);
    }
    // Quick presets: exact ×1 / ×2 / ×5 / ×10 are hard to land on a logarithmic
    // slider, and "compare runs at ×5" is a real use. Rendered as the comp's
    // segmented control: a card-filled group (4 pt pad, 11 pt radius), 40×30
    // segments at 7 pt, the active one on the raised-2 tone, mono digits.
    egui::Frame::new()
        .fill(crate::theme::CARD)
        .corner_radius(egui::CornerRadius::same(11))
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for &s in &[1.0f32, 2.0, 5.0, 10.0] {
                let active = (controls.speed - s).abs() < 1e-3;
                let (fill, ink) = if active {
                    (crate::theme::RAISED_2, crate::theme::INK)
                } else {
                    (egui::Color32::TRANSPARENT, crate::theme::INK_MUTED)
                };
                let text = egui::RichText::new(format!("×{s:.0}"))
                    .monospace()
                    .size(12.5)
                    .color(ink);
                if ui
                    .add(
                        egui::Button::new(text)
                            .fill(fill)
                            .min_size(egui::vec2(40.0, 30.0))
                            .corner_radius(egui::CornerRadius::same(7)),
                    )
                    .on_hover_text(format!("Set the speed to ×{s:.0}"))
                    .clicked()
                {
                    controls.speed = s;
                    vtime.set_relative_speed(s);
                }
            }
        });

    ui.add_space(8.0);
    // Accent the Reset while the running world no longer matches the config on the
    // reset-bound fields (arena, seed, bodies, brains…): those edits are waiting for
    // a ⟲, and the inline hints saying so may be turned off.
    let diverged = world_diverged(config, &world.0);
    let label = if diverged {
        fonts::icon_label_tinted(icons::RESET, "Reset", crate::theme::ACCENT)
    } else {
        fonts::icon_label(icons::RESET, "Reset")
    };
    let tip = if diverged {
        "Rebuild the world from the current config — edits are waiting to be applied"
    } else {
        "Rebuild the world from the current config"
    };
    if ui
        .button(label)
        .on_hover_text(keymap::tooltip(tip, UiAction::ResetWorld))
        .clicked()
    {
        controls.reset_requested = true;
    }
}

/// Single-step: while paused, advance `Time<Virtual>` by **exactly one
/// `timestep`** per requested step. Runs in `PreUpdate` (after the time update,
/// before the fixed loop) so that a single fixed tick is played this frame. When
/// not paused, pending steps are dropped (the normal flow resumes).
pub fn drive_steps(
    mut controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    fixed: Res<Time<Fixed>>,
) {
    if !vtime.is_paused() {
        controls.steps_pending = 0;
        return;
    }
    if controls.steps_pending == 0 {
        return;
    }
    // A timestep injected by hand: the fixed loop will accumulate it and execute
    // exactly one tick. (`advance_by` writes the delta even on a paused clock —
    // the pause only sets the delta computed by Bevy to zero.)
    vtime.advance_by(fixed.timestep());
    controls.steps_pending -= 1;
}

/// Hot reset: rebuild the world from the `SimConfig`. Despawn everything that is
/// simulated (agents, walls, **and the nutrient sources** — non-`Agent` substrate
/// entities, which `populate` would otherwise re-add on top, duplicating them),
/// re-populate, and reset the sim resources (RNG, **the nutrient field**) and the
/// HUD. The despawn also sweeps the **nutrient heatmap layers**
/// ([`NutrientLayer`](teemlab::visuals::NutrientLayer)): a pure render artifact keyed
/// by field index, it would otherwise linger frozen when the new scenario declares
/// **fewer** fields (or none) — `render_nutrient_layers` only repaints indices that
/// still exist, never the orphans. In `PreUpdate`: the commands apply before the
/// fixed loop, so the frame already restarts on the new world.
///
/// This is also **the single passage point** where we re-apply the sim rate
/// `tick_hz` (cf. [`SimPlugin`](teemlab::SimPlugin), which only sets it at
/// build): the reset being triggered also by the scenario reload
/// ([`crate::runs::apply_scenario_load`]), a rate change (editor or another
/// `.ron`) takes effect here, like the arena and the seed — a "(reset)"
/// parameter.
#[allow(clippy::too_many_arguments)]
pub fn apply_reset(
    mut controls: ResMut<SimControls>,
    mut commands: Commands,
    config: Res<SimConfig>,
    mut sim_rng: ResMut<SimRng>,
    mut fields: ResMut<Fields>,
    mut history: ResMut<History>,
    mut fixed: ResMut<Time<Fixed>>,
    mut baseline: ResMut<WorldBaseline>,
    mut selection: ResMut<Selection>,
    simulated: Query<Entity, Or<(With<Agent>, With<Wall>, With<Emits>, With<NutrientLayer>)>>,
) {
    if !controls.reset_requested {
        return;
    }
    controls.reset_requested = false;

    for entity in &simulated {
        commands.entity(entity).despawn();
    }
    spawn::populate(&mut commands, &config);
    // Re-apply the fixed rate from the config (the plugin build set it once; a
    // new world may want a different rate).
    fixed.set_timestep_hz(config.tick_hz);

    *sim_rng = SimRng::from_config(&config);
    // Rebuild the component fields from the (possibly edited) config: this clears the
    // accumulated concentrations and re-applies the resolution / diffusion / decay —
    // the "(reset)" counterpart of editing them in the World panel.
    *fields = Fields::from_config(&config);
    history.clear();
    // The rebuilt world now matches the config (the Reset accent clears)…
    baseline.0 = config.clone();
    // …and the previous world's selection could only point at a despawned entity —
    // clear it instead of letting the inspector report a spurious death.
    selection.0 = None;
}
