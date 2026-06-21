//! Driver: an archetype carrying a **captured brain** makes its founders born with
//! THESE exact weights (reuse of trained weights), via the *real* SimPlugin — the
//! same world as the binaries. The end-to-end ECS counterpart of the unit tests of
//! `Archetype::capture` / RON round-trip (cf. `config.rs`).

mod common;

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::{Brain, BrainKind, MlpBrain};
use teemlab::components::Agent;
use teemlab::config::{Archetype, Mutability};
use teemlab::genotype::Genotype;

/// A world with a single MLP archetype, `count` founders, stable genes (no
/// mutation, no metabolism, no reproduction → the population and the brains stay
/// frozen at the first tick). `captured` injects (or not) a concrete captured brain.
fn world(captured: Option<Brain>, count: usize) -> SimConfig {
    let genotype = Genotype {
        mutation_rate: 0.0,
        base_metabolism: 0.0,
        move_cost: 0.0,
        ..Genotype::default()
    };
    let arch = Archetype {
        count,
        genotype,
        brain: BrainKind::Mlp { hidden: vec![6] }, // topology consistent with the captured brain
        mutable: Mutability::default(),
        captured_brain: captured,
        ..Archetype::new_agent(0)
    };
    SimConfig {
        archetypes: vec![arch],
        relations: Vec::new(),
        seed: 0x5EED,
        ..SimConfig::default()
    }
}

/// An MLP brain whose input layer matches the default visual precision (what a
/// founder with the default genotype receives).
fn mlp_brain(seed: u64) -> Brain {
    let rays = Genotype::default().ray_count();
    Brain::Mlp(MlpBrain::random(seed, MlpBrain::input_size(rays), &[6]))
}

/// All living agents' brains, after population.
fn agent_brains(app: &mut App) -> Vec<Brain> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Brain, With<Agent>>();
    q.iter(world).cloned().collect()
}

/// With a captured brain, **each founder** is born with these exact weights
/// (clone), instead of a fresh random brain — it is the seam that reuses trained
/// weights.
#[test]
fn founders_are_born_with_the_captured_weights() {
    let captured = mlp_brain(99);
    let config = world(Some(captured.clone()), 3);
    let mut app = common::stepping_app(&config);
    app.update(); // Startup populates the founders.

    let brains = agent_brains(&mut app);
    assert_eq!(brains.len(), 3, "three founders populated");
    for b in &brains {
        assert_eq!(*b, captured, "a founder is born with the captured weights");
    }
}

/// Counter-check: **without** a capture (same topology), each founder receives a
/// fresh brain seeded distinctly → their weights differ. This is what makes the
/// previous test's equality meaningful (they are indeed the captured weights, not a
/// construction artifact).
#[test]
fn without_capture_founders_get_distinct_fresh_brains() {
    let config = world(None, 2);
    let mut app = common::stepping_app(&config);
    app.update();

    let brains = agent_brains(&mut app);
    assert_eq!(brains.len(), 2, "two founders populated");
    assert_ne!(
        brains[0], brains[1],
        "without a capture, two founders have distinct fresh weights"
    );
}
