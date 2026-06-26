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
use teemlab::SimConfig;
use teemlab::components::{Agent, Wall};
use teemlab::ecology::SimRng;
use teemlab::metrics::History;
use teemlab::nutrients::{Emits, NutrientField};
use teemlab::spawn;

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

/// The simulation controls — pause / step / speed / reset. Only acts on
/// `Time<Virtual>` (pause/speed) or sets a flag (step, reset). Rendered **centered in
/// the top bar** (fixed dock) by [`crate::panels::dock`], which handles the panel;
/// this section only draws the button row.
pub(crate) fn controls_section(
    ui: &mut egui::Ui,
    controls: &mut SimControls,
    vtime: &mut Time<Virtual>,
) {
    let paused = vtime.is_paused();
    if ui
        .button(if paused { "▶ Play" } else { "⏸ Pause" })
        .clicked()
    {
        if paused {
            vtime.unpause();
        } else {
            vtime.pause();
        }
    }
    // Single-stepping only makes sense when stopped.
    ui.add_enabled_ui(paused, |ui| {
        if ui.button("⏭ Step").clicked() {
            controls.steps_pending += 1;
        }
    });

    ui.separator();
    // Logarithmic-scale slider: fine tuning from x0.1 to x10 on a single handle.
    if ui
        .add(
            egui::Slider::new(&mut controls.speed, 0.1..=10.0)
                .logarithmic(true)
                .text("Speed ×"),
        )
        .changed()
    {
        vtime.set_relative_speed(controls.speed);
    }

    ui.separator();
    if ui.button("⟲ Reset").clicked() {
        controls.reset_requested = true;
    }
    if paused {
        ui.separator();
        ui.weak("paused");
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
/// HUD. In `PreUpdate`: the commands apply before the fixed loop, so the frame
/// already restarts on the new world.
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
    mut nutrient_field: ResMut<NutrientField>,
    mut history: ResMut<History>,
    mut fixed: ResMut<Time<Fixed>>,
    simulated: Query<Entity, Or<(With<Agent>, With<Wall>, With<Emits>)>>,
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
    // Rebuild the nutrient field from the (possibly edited) config: this clears the
    // accumulated concentrations and re-applies the resolution and diffusion — the
    // "(reset)" counterpart of editing them in the World panel.
    *nutrient_field = NutrientField::new(
        config.nutrient.resolution,
        config.arena_half_extent,
        config.nutrient.diffusion,
    );
    history.clear();
}
