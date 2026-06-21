//! Evolutionary flora (Phase 3) — driver: a population of sessile plants **self-regulates**.
//!
//! Falsification of the item's core: a *flora* ([`Brain::Sessile`] brain, energy
//! from photosynthesis, local seeding) is a full-fledged entity that (a) **grows**
//! strongly from a few founders (photosynthesis + seeding work), (b) stays
//! **bounded well below the physical saturation** of the arena — intraspecific
//! competition (Plant→Plant relation, §3 interaction primitive: contested
//! light/space) slows the growth into a spatial wave instead of filling the arena
//! —, and (c) **persists** at a sustained count, all of this robustly over several
//! seeds.
//!
//! It is a **negative feedback** (high density → competition drain → less seeding /
//! mortality), hence robust — not the oscillating *knife-edge* coupling of
//! predator-prey. We run the *real* sim world (same `SimPlugin` as the binaries),
//! in single-stepping.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

/// The bundled scenario, loaded as-is.
const SCENARIO: &str = include_str!("../scenarios/flora.ron");

/// Four independent worlds: a band that holds for all of them is not luck.
const SEEDS: [u64; 4] = [0x00C0_FFEE, 0x1234, 0x9999, 0xBEEF];

const SECONDS: usize = 120;

/// Plant count sampled each sim second, for a seed.
fn population_trajectory(seed: u64) -> Vec<usize> {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid flora scenario");
    config.seed = seed;
    let tick_hz = config.tick_hz as usize;

    let mut app = common::stepping_app(&config);
    let mut traj = Vec::with_capacity(SECONDS);
    for _ in 0..SECONDS {
        for _ in 0..tick_hz {
            app.update();
        }
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Species, With<Agent>>();
        traj.push(q.iter(world).count());
    }
    traj
}

#[test]
fn flora_grows_and_self_regulates_across_seeds() {
    let founders = SimConfig::from_ron_str(SCENARIO).unwrap().archetypes[0].count;

    let mut failures = Vec::new();
    eprintln!("  seed         | founders={founders} | peak | 2nd-half band (min..max)");
    for seed in SEEDS {
        let traj = population_trajectory(seed);
        let peak = *traj.iter().max().unwrap();
        let back = &traj[SECONDS / 2..];
        let lo = *back.iter().min().unwrap();
        let hi = *back.iter().max().unwrap();
        let sampled: Vec<String> = traj.iter().step_by(20).map(|n| n.to_string()).collect();
        eprintln!(
            "  {seed:#012x} | peak {peak:>4} | {lo:>4}..{hi:<4}  t=0,20,..: {}",
            sampled.join("  ")
        );

        // (a) GREW strongly (≫ founders) → photosynthesis + seeding work.
        if peak < 200 {
            failures.push(format!(
                "seed {seed:#x}: growth too weak (peak {peak}, founders {founders})"
            ));
        }
        // (b) bounded FAR from the arena's physical saturation (~4500 bodies for
        //     radius 6, half-arena 360) → competition slows it, the arena does not fill.
        if peak > 2000 {
            failures.push(format!(
                "seed {seed:#x}: competition does not bound (peak {peak})"
            ));
        }
        // (c) PERSISTS at a sustained count over the 2nd half (no collapse).
        if lo < 100 {
            failures.push(format!("seed {seed:#x}: count not sustained (trough {lo})"));
        }
    }

    assert!(
        failures.is_empty(),
        "self-regulation not robust:\n  {}",
        failures.join("\n  ")
    );
}
