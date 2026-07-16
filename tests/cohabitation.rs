//! Item 18a — the **driver** of the per-species brain seam (brain inheritance).
//!
//! Two species share the same body and the same economy and graze the same food, differing
//! ONLY by their brain — a LEARNED `Mlp` (species 0) and a naive `Wander` (species 1). This
//! driver falsifies the "brain per species + inheritance at reproduction" seam, judged over
//! several seeds (a single one's success would be anecdotal):
//!
//! 1. **Inheritance invariant** — every living agent of species 0 carries `Brain::Mlp`,
//!    every agent of species 1 carries `Brain::Wander`. If reproduction rebuilt the brain
//!    from the global `config` instead of inheriting from the parent, a child could still
//!    match; but a child of a *captured*-brain founder that mutated its weights would only
//!    stay an `Mlp` if the enum is inherited — this is the direct falsification of the seam.
//! 2. **Effective reproduction** — the populations grow beyond their founders: without it,
//!    inheritance would not be exercised. (The *comparison* of the two brains — the honest
//!    parity of a living-food neuroevolution — is `tests/mlp`'s job, not this seam test.)
//!
//! We run the *real* sim world (the same `SimPlugin` as both binaries), in manual
//! single-stepping (cf. headless throughput, §6).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Agent, Species};

mod common;

/// The bundled scenario, loaded as-is: the driver measures WHAT the binaries launch.
const SCENARIO: &str = include_str!("../scenarios/examples/08_learning.ron");

/// Experiment seeds (cf. §5: we replay a *config*, not bit-for-bit).
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];

/// The coexistence window (living, reproducing flora → a Lotka-Volterra system): long
/// enough that both lineages reproduce and are inherited.
const SECONDS: usize = 60;

/// Species 0 = learned brain (`Mlp`), species 1 = naive `Wander`.
const LEARNED: u16 = 0;
const WANDER: u16 = 1;

/// End-of-run tallies for a seed.
struct Run {
    /// Living agents whose brain does NOT match their species — must stay zero (part 1).
    brain_mismatches: usize,
    /// Peak combined forager population over the run (part 2: growth > founders).
    forager_peak: usize,
}

/// Runs the scenario for `SECONDS` seconds for a given seed.
fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid cohabitation scenario");
    config.seed = seed;
    let tick_hz = config.tick_hz as usize;

    let mut app = common::stepping_app(&config);

    let mut forager_peak = 0usize;
    for _ in 0..SECONDS {
        for _ in 0..tick_hz {
            app.update();
        }
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Species, With<Agent>>();
        let foragers = q
            .iter(world)
            .filter(|s| s.0 == LEARNED || s.0 == WANDER)
            .count();
        forager_peak = forager_peak.max(foragers);
    }

    // Inheritance tally: does each living agent's brain match its species?
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Brain), With<Agent>>();
    let brain_mismatches = q
        .iter(world)
        .filter(|(s, brain)| match s.0 {
            LEARNED => !matches!(brain, Brain::Mlp(_)),
            WANDER => !matches!(brain, Brain::Wander(_)),
            _ => false,
        })
        .count();

    Run {
        brain_mismatches,
        forager_peak,
    }
}

#[test]
fn brains_are_inherited_per_species_across_seeds() {
    let founders = {
        let cfg = SimConfig::from_ron_str(SCENARIO).unwrap();
        cfg.archetypes[LEARNED as usize].count + cfg.archetypes[WANDER as usize].count
    };

    let mut failures = Vec::new();
    for seed in SEEDS {
        let run = run_seed(seed);

        // --- Part 1: inheritance invariant (the per-species seam). ---
        if run.brain_mismatches > 0 {
            failures.push(format!(
                "seed {seed:#x}: {} agent(s) with a brain inconsistent with their species \
                 (brain inheritance failed)",
                run.brain_mismatches
            ));
        }

        // --- Part 2: effective reproduction (the lineages spread → inheritance exercised). ---
        if run.forager_peak <= founders {
            failures.push(format!(
                "seed {seed:#x}: the foragers did not grow beyond the founders \
                 (peak {} ≤ {founders}) — inheritance not exercised",
                run.forager_peak
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "cohabitation inconclusive:\n  {}",
        failures.join("\n  ")
    );
}
