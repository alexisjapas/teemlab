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
use teemlab::config::{Archetype, Mutability, Relation};
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
        base_metabolism: 0.0,
        move_cost: 0.0,
        agility_cost: 0.0,
        brain_cost: 0.0,
        photosynthesis: 0.0,
        reproduction_threshold: 0.0, // does not reproduce
        mutation_rate: 0.0,
        // The forager cannot pull nutrient from the substrate — the falsifiable
        // distinction: any nutrient it ends up with came from **eating**.
        nutrient_absorption: 0.0,
        nutrient_capacity: 100.0, // room to receive without clamping
        offspring_nutrient: 0.0,
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
                radius: 8.0,
                reserve_max: 1000.0,
                genotype: inert_genotype(),
                brain: BrainKind::Sessile,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
            },
            Archetype {
                name: "Plant".into(),
                color: Archetype::default_color(1),
                count: 0,
                radius: 8.0,
                reserve_max: 1000.0,
                genotype: inert_genotype(),
                brain: BrainKind::Sessile,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
            },
        ],
        // Forager (0) eats the plant (1): predation (transfer) at a steady rate, in
        // contact range. No source/field exists → absorption is impossible.
        relations: vec![Relation {
            actor: 0,
            target: 1,
            transfer: true,
            rate: 100.0,
            range: 30.0,
        }],
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
                        store.current = plant_nutrient0;
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
            0 => forager = Some(store.current),
            1 => plant = Some(store.current),
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

/// The falsifiable **contrast**: a relation that does **not** transfer (combat —
/// the reserve is destroyed, not eaten) moves **no** nutrient. This pins the
/// transfer to *predation* specifically, not to mere proximity or contact.
#[test]
fn destruction_without_transfer_moves_no_nutrient() {
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![
            Archetype {
                name: "Attacker".into(),
                color: Archetype::default_color(0),
                count: 0,
                radius: 8.0,
                reserve_max: 1000.0,
                genotype: inert_genotype(),
                brain: BrainKind::Sessile,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
            },
            Archetype {
                name: "Victim".into(),
                color: Archetype::default_color(1),
                count: 0,
                radius: 8.0,
                reserve_max: 1000.0,
                genotype: inert_genotype(),
                brain: BrainKind::Sessile,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
            },
        ],
        // transfer: false → combat: the victim's reserve is destroyed without the
        // attacker gaining it. The nutrient must not move either.
        relations: vec![Relation {
            actor: 0,
            target: 1,
            transfer: false,
            rate: 100.0,
            range: 30.0,
        }],
        ..SimConfig::default()
    };

    let mut app = common::stepping_app(&config);
    let victim_nutrient0 = 50.0_f32;
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
                config.reserve_max_of(0),
                0,
            );
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
    app.world_mut()
        .run_system_once(
            move |mut q: Query<(&Species, &mut Nutrients), With<Agent>>| {
                for (species, mut store) in &mut q {
                    if species.0 == 1 {
                        store.current = victim_nutrient0;
                    }
                }
            },
        )
        .expect("seed the victim's nutrient store");

    for _ in 0..40 {
        app.update();
    }

    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Nutrients), With<Agent>>();
    let attacker = q
        .iter(world)
        .find(|(s, _)| s.0 == 0)
        .map(|(_, n)| n.current)
        .expect("the attacker still exists");
    assert_eq!(
        attacker, 0.0,
        "combat (transfer: false) must move no nutrient (attacker has {attacker:.3})"
    );
}
