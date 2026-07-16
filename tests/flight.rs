//! Active flight — the prey sees and **flees** its predator.
//!
//! The exact mirror of `tests/hunter.rs` (item 16), on the repulsion side: we test
//! end-to-end the perception's "threat" channel + [`Brain::Hunter`]'s flight. We
//! place a prey at the origin, heading +X, and a predator (a species that can act
//! *on* it, via the relation table) straight ahead, within its vision range; we run
//! the *real* sim world and check (1) that the predator registers in the prey's
//! "threat" channel, and (2) that it moves **away** from it clearly — the proof
//! that the same `Hunter` brain, read by the prey species via the *inverse*
//! relation, produces a FLIGHT (the counterpart of the "target" attraction).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::{Agent, Perception, Species};
use teemlab::config::{Archetype, ComponentConfig, CostLaw, FieldRelation, Mutability, Predation};
use teemlab::genotype::Genotype;
use teemlab::spawn::spawn_agent;

mod common;

#[test]
fn prey_sees_and_flees_its_predator() {
    // Bare world: no auto population (we place everything by hand), no metabolism (the
    // prey does not die during the test). Emergent targeting drives the threat: the
    // predator DOMINATES the prey in size (12 vs 8) AND `need`s the component the prey
    // `holds`, so it *can eat* the prey — which is exactly what lights the prey's THREAT
    // channel (the inverse of the target rule). A ZERO bite rate makes it a **stable
    // scarecrow**: threatening, never eating. It is also IMMOBILE (max_speed 0): a fixed
    // threat point, just as the food is a fixed bait in `tests/hunter.rs`.
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![
            // Species 0: the prey (hunter). Here it has no target — only the threat
            // drives it → pure flight. Vision raised to 260 to see the scarecrow.
            Archetype {
                name: "Prey".into(),
                color: Archetype::default_color(0),
                count: 0,
                radius: 8.0,
                reserve_max: 100.0,
                genotype: Genotype {
                    vision_fov_deg: 120.0,
                    vision_range: 260.0,
                    // Inert + non-reproducing + non-mutating: a short determinism
                    // test, predating the "living" Genotype::default (which now prices
                    // the costs and breeds), so we pin the changed genes to 0.
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
                anchor: None,
                spawn_zone: None,
            },
            // Species 1: the predator, immobile (max_speed 0) — the scarecrow. Bigger than
            // the prey so it dominates it (the size half of `can_eat`).
            Archetype {
                name: "Predator".into(),
                color: Archetype::default_color(1),
                count: 0,
                radius: 12.0,
                reserve_max: 100.0,
                genotype: Genotype {
                    max_speed: 0.0,
                    // Inert + non-reproducing (cf. the prey above).
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
                anchor: None,
                spawn_zone: None,
            },
        ],
        // The prey (species 0) holds a "Flesh" component the predator (species 1) needs,
        // so the predator CAN eat the prey → the prey perceives it as a THREAT (the
        // *inverse* of the target rule). A zero bite rate → it only threatens.
        components: vec![ComponentConfig {
            name: "Flesh".into(),
            diffusion: 0.0,
            decay: 0.0,
        }],
        field_relations: vec![
            FieldRelation {
                species: 0,
                component: 0,
                capacity: 50.0,
                ..default()
            },
            FieldRelation {
                species: 1,
                component: 0,
                need: 1.0,
                ..default()
            },
        ],
        predation: Predation {
            size_margin: 0.1,
            range: 20.0,
            rate: 0.0, // a stable scarecrow: threatening, never eating
        },
        cost_law: CostLaw::inert(),
        ..SimConfig::default()
    };

    // Exactly one fixed tick per `update()` (cf. `common::stepping_app`).
    let mut app = common::stepping_app(&config);

    // Prey at the origin (heading +X), predator straight ahead at 120 u: within
    // range (260) and CLOSE enough to cross the flight threshold (proximity ≈ 0.54 > 0.35).
    let predator_x = 120.0_f32;
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            // Prey (species 0).
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max_of(0),
                0, // founder: generation 0.
                0, // lineage: inert here (no breeding scoring)
            );
            // Immobile predator (species 1), straight ahead.
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(1),
                Species(1),
                Vec2::new(predator_x, 0.0),
                0.0,
                1,
                config.reserve_max_of(1),
                0,
                0, // lineage: inert here (no breeding scoring)
            );
        })
        .expect("one-off spawn");

    // Avian's broad-phase needs a few ticks to integrate the predator; meanwhile the
    // prey already starts turning away. We therefore sample the "threat" channel
    // over the first ticks: it must light up AT LEAST once (the window where the
    // predator is integrated AND still in the forward vision cone).
    let mut ever_saw_threat = false;
    for _ in 0..12 {
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Species, &Perception), With<Agent>>();
        for (species, perception) in q.iter(world) {
            if species.0 == 0 && perception.threat.iter().any(|&v| v > 0.0) {
                ever_saw_threat = true;
            }
        }
    }
    assert!(
        ever_saw_threat,
        "the predator straight ahead must appear in the prey's \"threat\" channel"
    );

    // We let it run: the prey must have moved AWAY from the predator — flight toward
    // negative x, opposite to the scarecrow placed at +X.
    for _ in 0..80 {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Transform), With<Agent>>();
    let prey_x = q
        .iter(world)
        .find(|(s, _)| s.0 == 0)
        .map(|(_, t)| t.translation.x)
        .expect("the prey still exists");
    assert!(
        prey_x < -50.0,
        "the prey must have fled its threat (x={prey_x:.1}, start 0, predator at {predator_x})"
    );
}
