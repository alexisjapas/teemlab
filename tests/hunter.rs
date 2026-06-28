//! Item 16 — the hunter sees and pursues its target.
//!
//! End-to-end test of the perception's "target" channel + the [`Brain::Hunter`]
//! reflex: we place a hunter at the origin, heading +X, and food straight ahead,
//! within its vision range; we run the *real* sim world and check (1) that the
//! target registers in its perception channel, and (2) that it moves clearly
//! closer to it — proof that perceive→decide→act has become MEANINGFUL (and that
//! the per-scenario brain selection works).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::{Agent, Perception, Species};
use teemlab::config::{Archetype, Mutability, Relation};
use teemlab::genotype::Genotype;
use teemlab::spawn::spawn_agent;

mod common;

#[test]
fn hunter_sees_and_chases_its_target() {
    // Bare world: no auto population (we place everything by hand), no metabolism
    // (the hunter does not die during the test), a ZERO-rate relation — the food
    // stays a stable bait: targeted (hence "target"), never consumed. It is `brain:
    // Hunter` that we put to the test.
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![
            Archetype {
                name: "Hunter".into(),
                color: Archetype::default_color(0),
                count: 0,
                radius: 8.0,
                reserve_max: 100.0,
                genotype: Genotype {
                    vision_fov_deg: 120.0,
                    vision_range: 260.0,
                    // Inert + non-reproducing + non-mutating: a short determinism
                    // test, predating the "living" Genotype::default.
                    base_metabolism: 0.0,
                    move_cost: 0.0,
                    agility_cost: 0.0,
                    brain_cost: 0.0,
                    reproduction_threshold: 0.0,
                    mutation_rate: 0.0,
                    ..Genotype::default()
                },
                brain: BrainKind::Hunter,
                mutable: Mutability::default(),
                source: None,
                captured_brain: None,
                captured_from: None,
            },
            // The bait: a sessile source (Phase 3b) — immobile, never consumed
            // (zero-rate relation); the hunter must see it as a "target".
            Archetype {
                name: "Food".into(),
                color: Archetype::default_color(1),
                count: 0,
                radius: 6.0,
                reserve_max: 50.0,
                genotype: Genotype {
                    max_speed: 0.0,
                    // Inert scenery: no metabolism so the bait never starves over the
                    // test (the "living" Genotype::default would drain it).
                    base_metabolism: 0.0,
                    move_cost: 0.0,
                    agility_cost: 0.0,
                    brain_cost: 0.0,
                    reproduction_threshold: 0.0,
                    mutation_rate: 0.0,
                    ..Genotype::default()
                },
                brain: BrainKind::Sessile,
                mutable: Mutability::all_fixed(),
                source: None,
                captured_brain: None,
                captured_from: None,
            },
        ],
        relations: vec![Relation {
            actor: 0,
            target: 1,
            transfer: true,
            rate: 0.0,
            range: 10.0,
        }],
        ..SimConfig::default()
    };

    // Exactly one fixed tick per `update()` (cf. `common::stepping_app`).
    let mut app = common::stepping_app(&config);

    // Hunter at the origin (heading +X), food straight ahead at 200 u (< range).
    let food_x = 200.0_f32;
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            let genotype = config.genotype_of(0);
            spawn_agent(
                &mut commands,
                &config,
                genotype,
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max_of(0),
                0, // founder: generation 0.
            );
            // The sessile bait, placed via the same `spawn_agent` (pass-through body).
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(1),
                Species(1),
                Vec2::new(food_x, 0.0),
                0.0,
                1,
                config.reserve_max_of(1),
                0,
            );
        })
        .expect("one-off spawn");

    // A few ticks for Avian's broad-phase to integrate the food, then we check that
    // it registers in the hunter's "target" channel.
    for _ in 0..10 {
        app.update();
    }
    let world = app.world_mut();
    let mut perceptions = world.query_filtered::<&Perception, With<Agent>>();
    let saw_target = perceptions
        .iter(world)
        .any(|p| p.target.iter().any(|&v| v > 0.0));
    assert!(
        saw_target,
        "the food straight ahead must appear in the \"target\" channel"
    );

    // We let it run: the hunter must move clearly closer to its target.
    for _ in 0..80 {
        app.update();
    }
    let world = app.world_mut();
    let mut transforms = world.query_filtered::<&Transform, With<Agent>>();
    let x = transforms
        .iter(world)
        .next()
        .expect("the hunter still exists")
        .translation
        .x;
    assert!(
        x > 100.0,
        "the hunter must have charged toward its target (x={x:.1}, start 0, target {food_x})"
    );
}
