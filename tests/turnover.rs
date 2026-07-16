//! Turnover — **a dying body leaves a corpse** (the `emit_at_death` verb).
//!
//! The agent→environment write AT death (`docs/component-emission-plan.md` §3): a
//! `FieldRelation` with `emit_at_death > 0` deposits that fixed biomass of a component
//! into its field when the agent dies — the detritus/carrion a decomposer or scavenger
//! lives on, and the matter a closed loop returns (Law 11: no per-kind death code).
//!
//! We falsify it on a **static, deterministic** world: one immobile agent that either
//! starves to death (→ leaves a corpse) or lives on (→ leaves none). The corpse is a
//! fixed amount deposited exactly at death, independent of any store.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::Species;
use teemlab::config::{Archetype, ComponentConfig, CostLaw, FieldRelation, Mutability};
use teemlab::genotype::Genotype;
use teemlab::nutrients::Fields;
use teemlab::spawn::spawn_agent;

mod common;

const CORPSE: f32 = 10.0;

/// One "Body" archetype that leaves a `CORPSE` of the Carrion component (index 0) at
/// death, and a single Carrion field for it to fall into. `mortal` picks a genotype that
/// starves to death (steep metabolism, no photosynthesis) or one that lives on (net
/// positive photosynthesis).
fn config(mortal: bool) -> SimConfig {
    let genotype = Genotype {
        max_speed: 0.0, // immobile: stays on its cell
        brain_cost: 0.0,
        vision_rays: 0.0,            // blind (sessile): no vision cost
        reproduction_threshold: 0.0, // does not reproduce
        mutation_rate: 0.0,
        photosynthesis: if mortal { 0.0 } else { 5.0 }, // starves vs lives on the sun
        ..Genotype::default()
    };
    SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![Archetype {
            name: "Body".into(),
            color: Archetype::default_color(0),
            count: 0,
            radius: 8.0,
            reserve_max: 100.0,
            genotype,
            brain: BrainKind::Sessile,
            mutable: Mutability::default(),
            source: None,
            captured_brain: None,
            captured_from: None,
            anchor: None,
        }],
        components: vec![ComponentConfig {
            name: "Carrion".into(),
            diffusion: 0.0, // stays put → total() is exact
            decay: 0.0,
        }],
        // The body leaves a fixed CORPSE of carrion (component 0) at death.
        field_relations: vec![FieldRelation {
            species: 0,
            component: 0,
            emit_at_death: CORPSE,
            ..default()
        }],
        // Mortal: a large maintenance so the sliver-energy body starves in a tick (the
        // drain that was `base_metabolism` is now the allometric `CostLaw`). Living: a
        // cost-free world, so photosynthesis keeps it alive.
        cost_law: if mortal {
            CostLaw {
                maintenance: 50.0,
                ..CostLaw::inert()
            }
        } else {
            CostLaw::inert()
        },
        ..SimConfig::default()
    }
}

/// Spawn one Body at the origin with `energy`, then step `ticks`, returning the Carrion
/// field's total.
fn carrion_after(mortal: bool, energy: f32, ticks: usize) -> f32 {
    let config = config(mortal);
    let mut app = common::stepping_app(&config);
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                energy,
                0,
                0, // lineage: inert here (no breeding scoring)
            );
        })
        .expect("one-off spawn");
    for _ in 0..ticks {
        app.update();
    }
    app.world().resource::<Fields>()[0].total()
}

/// **A dead body leaves a corpse.** A body starved to death deposits exactly `CORPSE`
/// of carrion into the field at its cell — the `emit_at_death` write.
#[test]
fn a_dead_body_leaves_a_corpse() {
    // A sliver of energy → it starves within a tick or two, then reap deposits the corpse.
    let carrion = carrion_after(true, 1.0, 10);
    assert!(
        (carrion - CORPSE).abs() < 1e-3,
        "a dead body must leave exactly one corpse ({CORPSE}); got {carrion:.3}"
    );
}

/// **A living body leaves none.** The corpse is deposited *at death*, not during life: a
/// body that lives on the sun over the same span leaves the carrion field empty.
#[test]
fn a_living_body_leaves_no_corpse() {
    let carrion = carrion_after(false, 100.0, 10);
    assert!(
        carrion.abs() < 1e-3,
        "a living body must leave no corpse; got {carrion:.3}"
    );
}
