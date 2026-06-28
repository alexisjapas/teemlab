//! Nutrient recycling — **a dying body returns its nutrient to the field**.
//!
//! Link 2 of the nutrient food web (ROADMAP §9 "T3"), the counterpart of the
//! trophic transfer (link 1): link 1 lets the nutrient flow *up* the chain into an
//! agent's [`Nutrients`] store, so a death must **return** that store to the
//! substrate — otherwise eating would slowly **destroy** the nutrient. `reap` now
//! deposits a dead body's store into the [`NutrientField`] at its cell, closing the
//! conservation loop (Law 9 in spirit: matter is moved, not created or destroyed).
//!
//! We falsify it on a **static, deterministic** world: a single immobile agent that
//! holds a known nutrient store and is starved to death (no photosynthesis, no
//! source, no absorption — the field is the *only* place the nutrient can come
//! from or go to). We check the field gains **exactly** what the body held; and, as
//! the contrast, that an **empty** body deposits **nothing** (recycling returns the
//! store, it does not conjure nutrient from death).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::{Agent, Species};
use teemlab::config::{Archetype, Mutability};
use teemlab::genotype::Genotype;
use teemlab::nutrients::{NutrientField, Nutrients};
use teemlab::spawn::spawn_agent;

mod common;

/// A genotype that **starves to death** quickly and does nothing else: immobile,
/// no photosynthesis (so the drain is never refilled → it dies), no reproduction, no
/// mutation, no absorption (the only nutrient it has is the one we hand it). A roomy
/// store capacity so the seeded amount fits.
fn dying_genotype() -> Genotype {
    Genotype {
        max_speed: 0.0,        // immobile: stays on its cell
        base_metabolism: 60.0, // a steep drain → death within a tick or two
        move_cost: 0.0,
        agility_cost: 0.0,
        brain_cost: 0.0,
        photosynthesis: 0.0,         // nothing refills the reserve → it dies
        reproduction_threshold: 0.0, // does not reproduce
        mutation_rate: 0.0,
        nutrient_absorption: 0.0, // cannot pull from the field
        nutrient_capacity: 100.0,
        offspring_nutrient: 0.0,
        ..Genotype::default()
    }
}

fn one_agent_config() -> SimConfig {
    SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![Archetype {
            name: "Body".into(),
            color: Archetype::default_color(0),
            count: 0,
            radius: 8.0,
            reserve_max: 100.0,
            genotype: dying_genotype(),
            brain: BrainKind::Sessile,
            mutable: Mutability::default(),
            source: None,
            captured_brain: None,
            captured_from: None,
        }],
        // No relations, no sources: the field is inert except for what recycling
        // deposits. Diffusion stays 0 (default) → the deposit stays put, `total()`
        // is exact.
        relations: vec![],
        ..SimConfig::default()
    }
}

/// Spawn one agent at the origin with a low reserve (so it starves fast), then set
/// its nutrient store to `stored`. Returns the stepping app.
fn world_with_one_body(config: &SimConfig, stored: f32) -> App {
    let mut app = common::stepping_app(config);
    app.world_mut()
        .run_system_once(|mut commands: Commands, config: Res<SimConfig>| {
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                1.0, // a sliver of energy → starves within a tick or two
                0,
            );
        })
        .expect("one-off spawn");
    app.world_mut()
        .run_system_once(move |mut q: Query<&mut Nutrients, With<Agent>>| {
            for mut store in &mut q {
                store.current = stored;
            }
        })
        .expect("seed the body's nutrient store");
    app
}

/// Count the living agents in the world.
fn agent_count(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(), With<Agent>>();
    q.iter(world).count()
}

#[test]
fn a_dead_body_recycles_its_nutrient_to_the_field() {
    let config = one_agent_config();
    let stored = 30.0_f32;
    let mut app = world_with_one_body(&config, stored);

    // Before death: the body holds all the nutrient, the field is empty.
    assert_eq!(
        app.world().resource::<NutrientField>().total(),
        0.0,
        "the field starts empty"
    );

    // Starve it to death (a few ticks: metabolize drains, then reap collects it).
    for _ in 0..10 {
        app.update();
    }

    // The body is gone…
    assert_eq!(agent_count(&mut app), 0, "the starved body must have died");
    // …and the field holds **exactly** the nutrient it carried — conservation
    // across death (no creation, no destruction; diffusion 0 → the deposit is exact).
    let field_total = app.world().resource::<NutrientField>().total();
    assert!(
        (field_total - stored).abs() < 1e-3,
        "the field must recover the body's whole store ({stored}), got {field_total:.3}"
    );
}

#[test]
fn an_empty_body_creates_no_nutrient() {
    // The falsifiable contrast: recycling **returns** the store, it does not invent
    // nutrient out of death. A body that held nothing deposits nothing.
    let config = one_agent_config();
    let mut app = world_with_one_body(&config, 0.0);

    for _ in 0..10 {
        app.update();
    }

    assert_eq!(agent_count(&mut app), 0, "the starved body must have died");
    assert_eq!(
        app.world().resource::<NutrientField>().total(),
        0.0,
        "an empty body must leave the field empty (no nutrient conjured from death)"
    );
}
