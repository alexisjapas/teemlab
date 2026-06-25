//! T2 nutrients layer (driver) — reproduction gated by a finite nutrient.
//!
//! Falsification of the whole T2 design (ROADMAP §9; `docs/nutrients-t2-plan.md`):
//! a plant **lives on the sun** (energy → survival) but **reproduces only where it
//! can absorb a finite nutrient** (the second axis). The nutrient is emitted by
//! point sources and diffused into halos, so:
//!
//! - **with sources** — the population (a) **grows** strongly from its founders
//!   (reproduction is fed where the nutrient reaches), (b) stays **bounded** (no
//!   carpet — the finite nutrient throttles the reproduction *rate*; a true
//!   standing-crop cap awaits turnover, a deferred sub-phase), (c) **persists** (no
//!   collapse: the T1 death spiral of `minerals.ron` is gone);
//! - **without sources** — the *same* plants do **not** grow (no nutrient → no
//!   reproduction) yet do **not** collapse either (sun-fed survival). This contrast
//!   is the proof that the nutrient gates **only** reproduction, never survival.
//!
//! We run the *real* sim world (same `SimPlugin` as the binaries), single-stepping.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

/// The bundled scenario, loaded as-is.
const SCENARIO: &str = include_str!("../scenarios/nutrients.ron");

/// Four independent worlds: a behavior that holds for all of them is not luck.
const SEEDS: [u64; 4] = [0x00C0_FFEE, 0x1234, 0x9999, 0xBEEF];

const SECONDS: usize = 120;

/// Plant count sampled each sim second, for a given config + seed.
fn population_trajectory(config: &SimConfig, seed: u64) -> Vec<usize> {
    let mut config = config.clone();
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
fn nutrient_gates_reproduction_without_a_death_spiral() {
    let base = SimConfig::from_ron_str(SCENARIO).expect("valid nutrients scenario");
    let founders = base.archetypes[0].count;
    assert!(!base.sources.is_empty(), "the scenario must ship sources");

    // The contrast world: the very same scenario with the nutrient sources removed.
    let mut no_sources = base.clone();
    no_sources.sources.clear();

    let mut failures = Vec::new();

    eprintln!("  WITH sources (founders={founders}):");
    for seed in SEEDS {
        let traj = population_trajectory(&base, seed);
        let peak = *traj.iter().max().unwrap();
        let back = &traj[SECONDS / 2..];
        let lo = *back.iter().min().unwrap();
        let hi = *back.iter().max().unwrap();
        let sampled: Vec<String> = traj.iter().step_by(20).map(|n| n.to_string()).collect();
        eprintln!(
            "    {seed:#012x} | peak {peak:>4} | 2nd-half {lo:>4}..{hi:<4} | t=0,20..: {}",
            sampled.join("  ")
        );

        // (a) GREW strongly (≫ founders) → the nutrient feeds reproduction.
        if peak < 2 * founders {
            failures.push(format!(
                "with sources, seed {seed:#x}: growth too weak (peak {peak}, founders {founders})"
            ));
        }
        // (b) bounded FAR from the arena's physical saturation (~2500 bodies for
        //     radius 6, half-arena 300): the finite nutrient throttles the
        //     reproduction *rate* (≈ emission / offspring_nutrient), so the
        //     population grows slowly and does not carpet within the run. (A true
        //     standing-crop carrying capacity needs turnover — recycling / mortality
        //     — which are deferred sub-phases; T2 establishes the gating + the
        //     spiral-free persistence, not yet the closed loop.)
        if peak > 1500 {
            failures.push(format!(
                "with sources, seed {seed:#x}: reproduction not bounded (peak {peak})"
            ));
        }
        // (c) PERSISTS over the 2nd half (no collapse — the T1 spiral is gone).
        if lo < founders {
            failures.push(format!(
                "with sources, seed {seed:#x}: not sustained (trough {lo} < founders {founders})"
            ));
        }
    }

    eprintln!("  WITHOUT sources (founders={founders}):");
    for seed in SEEDS {
        let traj = population_trajectory(&no_sources, seed);
        let peak = *traj.iter().max().unwrap();
        let last = *traj.last().unwrap();
        eprintln!("    {seed:#012x} | peak {peak:>4} | final {last:>4}");

        // No nutrient → no reproduction: the population does not grow appreciably.
        if peak > founders + founders / 4 {
            failures.push(format!(
                "without sources, seed {seed:#x}: grew without nutrient (peak {peak}, founders {founders})"
            ));
        }
        // ...but sun-fed survival means it does NOT collapse either.
        if last < founders {
            failures.push(format!(
                "without sources, seed {seed:#x}: collapsed without nutrient (final {last} < founders {founders})"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "T2 nutrient gating not robust:\n  {}",
        failures.join("\n  ")
    );
}
