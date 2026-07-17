//! teemlab — evolutionary simulation engine.
//!
//! A *single* engine interprets data; each simulation is just a scenario. The
//! loop is always **perceive → decide → act**.
//!
//! This crate exposes the *render-agnostic* core ([`SimPlugin`]) shared by the
//! two entry points (windowed and headless), so that they advance exactly the
//! same world.

// Bevy queries (component tuples + filters) trigger `type_complexity` by their
// very nature; that is the idiomatic shape of an ECS system, not debt. We allow
// it at the crate level rather than sprinkling `#[allow]` or inventing aliases
// that would hide what a system actually reads.
#![allow(clippy::type_complexity)]

pub mod brain;
pub mod breeding;
pub mod components;
pub mod config;
pub mod dataviz;
pub mod decor;
pub mod ecology;
pub mod genotype;
pub mod interaction;
pub mod metrics;
pub mod movement;
pub mod rng;
pub mod selection;
pub mod spawn;
pub mod substrate;
pub mod visuals;

use avian2d::prelude::*;
use bevy::prelude::*;

pub use config::SimConfig;

/// The heart of the simulation: everything that advances the world.
///
/// **Absolute rule: no sim logic in `Update`.** Agency lives in [`FixedUpdate`]
/// and Avian physics in [`FixedPostUpdate`]. `Update` is reserved for the
/// rendering / UI of the windowed binary. This way the headless build and the
/// windowed build run the *same* world, identically.
#[derive(Default)]
pub struct SimPlugin {
    pub config: SimConfig,
}

impl SimPlugin {
    pub fn new(config: SimConfig) -> Self {
        Self { config }
    }
}

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            // Physics placed explicitly in FixedPostUpdate.
            .add_plugins(PhysicsPlugins::new(FixedPostUpdate))
            // Top-down view: no gravity (inserted after the plugin so it
            // overrides its default).
            .insert_resource(Gravity(Vec2::ZERO))
            // Constant sim rate (64 Hz by default), independent of rendering.
            .insert_resource(Time::<Fixed>::from_hz(self.config.tick_hz))
            // The sim's random stream (seeding, mutations, …), seeded separately
            // from population so the two are not correlated.
            .insert_resource(ecology::SimRng::from_config(&self.config))
            // The component fields (the substrate): one concentration grid per
            // declared component, sized from the scenario. Empty (no component) or
            // inert (no source, diffusion/decay 0) → existing scenarios byte-identical.
            .insert_resource(substrate::Fields::from_config(&self.config))
            .add_systems(Startup, spawn::setup_world)
            // perceive → decide → act, strictly within FixedUpdate.
            // `interact` extends "act" (eat/attack); then the energy economy:
            // die, metabolize (photosynthesis included), age, reproduce. The order
            // interact → **reap** → metabolize means an entity drained to zero by
            // grazing dies *before* its metabolism could refill it — so a flora
            // grazed empty dies like a fauna starved empty, the **uniform** death
            // rule of SIM Law 11 (no schedule ordering tuned to exempt a kind).
            // (Pre-Phase-3b this was interact → metabolize → reap, which let a
            // photosynthetic source regain `photosynthesis·dt` before the death
            // check and thus never die — the immortal-patch behavior we dropped.)
            // Since T3, `reap` also **recycles**: a dying body returns its accumulated
            // nutrient to the field (link 2 — the conserving loop), inert (the field
            // untouched) when the store is empty → existing scenarios byte-identical.
            //
            // `anchor_spring` (Feature 2) sits between `interact` and `reap`: a rooted body
            // pulled past its tear tension is marked dead (reserve 0) there, so `reap` turns
            // it into a corpse the *same* tick — placed **after** `interact` so a lethal
            // uprooting cannot be undone by eating the energy back. No anchored body → no-op.
            //
            // The **component** sub-pipeline sits after metabolize and before
            // reproduce, so the store is filled before reproduction reads it: sources
            // emit → the fields diffuse and decay → agents absorb into their store;
            // reproduce then gates a child on `offspring_nutrient`. All early-return
            // when inert (no source / diffusion & decay 0 / no absorber) → byte-identical.
            .add_systems(
                FixedUpdate,
                (
                    movement::perceive,
                    movement::decide,
                    movement::act,
                    interaction::interact,
                    movement::anchor_spring,
                    ecology::reap,
                    ecology::metabolize,
                    substrate::emit_sources,
                    substrate::emit_components,
                    substrate::diffuse_fields,
                    substrate::decay_fields,
                    substrate::absorb_components,
                    substrate::affect_agents,
                    ecology::age_agents,
                    ecology::reproduce,
                )
                    .chain(),
            );
        // SINGLE-THREADED executors for the sim schedules. Our systems are fully
        // `.chain()`ed (a total order — determinism), so system-level parallelism has
        // nothing to win here; the multi-threaded executor still paid its cross-thread
        // task-queue traffic on every one of the dozens of systems per tick (profiled
        // at ~5-8% of a tick under the bench, plus idle worker churn — worse for the
        // breeding orchestrator, whose per-match worlds all contend on the one global
        // pool). Byte-identical: the same total order runs either way (the
        // chaos-sensitive suite is the tripwire). Avian's inner schedules get the same
        // treatment; its heavy phases keep their *internal* parallelism (they fan work
        // out through the ComputeTaskPool inside single systems).
        use bevy::ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
        for label in [
            FixedUpdate.intern(),
            FixedPostUpdate.intern(),
            avian2d::schedule::PhysicsSchedule.intern(),
            avian2d::dynamics::solver::schedule::SubstepSchedule.intern(),
        ] {
            app.edit_schedule(label, |schedule| {
                schedule.set_executor(SingleThreadedExecutor::new());
            });
        }
    }
}
