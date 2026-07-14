//! Trophic nutrient transfer — **eating carries the nutrient up the food chain**.
//!
//! The first slice of the nutrient food web (ROADMAP §9 "T3"): until now the
//! nutrient ([`Nutrients`]) only entered an entity by **absorption** from the field
//! (plants on a substrate); fauna never acquired any. Now the *single interaction
//! primitive* (§3) carries it: when an actor eats a prey (`transfer: true`), it
//! receives the share of the prey's nutrient store proportional to the fraction of
//! biomass it consumed — the prerequisite to recycling and to emergent targeting.
//!
//! We falsify it on a **static, deterministic** world (no movement, no metabolism,
//! no reproduction — the mechanism in isolation): a forager that **cannot absorb**
//! (`nutrient_absorption = 0`, and there is no field source at all) sits in range
//! of a nutrient-rich plant. Its store can therefore rise **only** by eating. We
//! check (1) it gains nutrient, (2) the plant loses it, and (3) the total is
//! **conserved** (nothing created, nothing destroyed — the forager's capacity
//! exceeds what it receives, so no clamping loss).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::{Agent, Species};
use teemlab::config::{Archetype, CostLaw, FieldRelation, Mutability};
use teemlab::genotype::Genotype;
use teemlab::nutrients::Nutrients;
use teemlab::spawn::spawn_agent;

mod common;

/// A genotype that is **inert** on every axis but the one under test: it neither
/// moves, metabolizes, reproduces nor mutates. Both species share it (only their
/// nutrient store differs), so the world is fully static — the nutrient transfer is
/// the *only* thing that changes across the run.
fn inert_genotype() -> Genotype {
    Genotype {
        max_speed: 0.0, // immobile: nothing moves, the two stay exactly in place
        brain_cost: 0.0,
        photosynthesis: 0.0,
        reproduction_threshold: 0.0, // does not reproduce
        mutation_rate: 0.0,
        ..Genotype::default()
    }
}

#[test]
fn eating_carries_the_nutrient_from_prey_to_predator() {
    // Species 0 = forager (eats species 1), species 1 = a nutrient-rich plant. Both
    // immobile and inert; a single predation relation 0 → 1 with a generous range so
    // they interact from the first tick, without relying on any movement.
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![
            Archetype {
                name: "Forager".into(),
                color: Archetype::default_color(0),
                count: 0,
                radius: 10.0, // dominates the plant in size (emergent predation)
                reserve_max: 1000.0,
                genotype: inert_genotype(),
                brain: BrainKind::Sessile,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
                anchor: None,
            },
            Archetype {
                name: "Plant".into(),
                color: Archetype::default_color(1),
                count: 0,
                radius: 6.0, // smaller → the forager can prey on it
                reserve_max: 1000.0,
                genotype: inert_genotype(),
                brain: BrainKind::Sessile,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
                anchor: None,
            },
        ],
        // Forager (0) eats the plant (1): predation (transfer) at a steady rate, in
        // contact range. No source/field exists → absorption is impossible.
        // The forager (species 0) can hold nutrient (capacity 100) — the store it eats
        // into, without clamping. Declared in the field-relations table, not a gene.
        field_relations: vec![
            FieldRelation {
                species: 0,
                component: 0,
                capacity: 100.0,
                need: 1.0, // needs the nutrient → the plant (which holds it) is digestible
                ..default()
            },
            FieldRelation {
                species: 1,
                component: 0,
                capacity: 100.0,
                ..default()
            },
        ],
        cost_law: CostLaw::inert(),
        ..SimConfig::default()
    };

    let mut app = common::stepping_app(&config);

    // The forager starts with no nutrient; the plant carries a known store. This is
    // the only nutrient in the world.
    let plant_nutrient0 = 50.0_f32;
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            // Forager at the origin.
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max_of(0),
                0,
            );
            // Plant within reach (gap 4 < the relation's reach).
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(1),
                Species(1),
                Vec2::new(20.0, 0.0),
                0.0,
                1,
                config.reserve_max_of(1),
                0,
            );
        })
        .expect("one-off spawn");

    // Endow the plant's store (founders are born empty): the nutrient we will watch
    // flow up the chain.
    app.world_mut()
        .run_system_once(
            move |mut q: Query<(&Species, &mut Nutrients), With<Agent>>| {
                for (species, mut store) in &mut q {
                    if species.0 == 1 {
                        store.set(0, plant_nutrient0);
                    }
                }
            },
        )
        .expect("seed the plant's nutrient store");

    // Let the forager graze for a while.
    for _ in 0..40 {
        app.update();
    }

    // Read the two stores back.
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Nutrients), With<Agent>>();
    let mut forager = None;
    let mut plant = None;
    for (species, store) in q.iter(world) {
        match species.0 {
            0 => forager = Some(store.current(0)),
            1 => plant = Some(store.current(0)),
            _ => {}
        }
    }
    let forager = forager.expect("the forager still exists");
    let plant = plant.expect("the plant still exists");

    // (1) The forager gained nutrient — and, since it cannot absorb and there is no
    //     field, **only eating** can explain it.
    assert!(
        forager > 1.0,
        "the forager must acquire nutrient by eating (got {forager:.3})"
    );
    // (2) The plant lost the nutrient the forager gained.
    assert!(
        plant < plant_nutrient0,
        "the plant must lose the nutrient it was grazed of (still {plant:.3})"
    );
    // (3) Conservation: nothing created or destroyed (capacity 100 > 50 → no clamp).
    assert!(
        (forager + plant - plant_nutrient0).abs() < 0.1,
        "nutrient must be conserved: forager {forager:.3} + plant {plant:.3} \
         should equal {plant_nutrient0} (Δ {:.3})",
        (forager + plant - plant_nutrient0).abs()
    );
}
