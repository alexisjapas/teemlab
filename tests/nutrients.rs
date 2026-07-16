//! Nutrient metabolism (driver) — the nutrient gates SURVIVAL (Liebig).
//!
//! Since the metabolisation change (`CostLaw::metabolic_cost`), turning light into energy
//! CONSUMES a finite nutrient, so the nutrient is now a **survival** resource, not just a
//! reproduction gate. It is emitted by point sources and diffused into halos, so:
//!
//! - **with sources** — the population **grows** from its founders and settles at a
//!   **bounded carrying capacity** (Liebig's law of the minimum: the crowd around a source
//!   draws the field down, and the excess starves), **persisting** (oscillating around that
//!   capacity, births at the fringe vs starvation deaths in the centre) rather than carpeting;
//! - **without sources** — the *same* plants **COLLAPSE**: with no nutrient to metabolise,
//!   photosynthesis throttles to zero and they starve. This contrast is the proof that the
//!   nutrient now gates **survival** — the density-dependent turnover that replaced the
//!   jostle-cost artefact.
//!
//! We run the *real* sim world (same `SimPlugin` as the binaries), single-stepping.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

/// The bundled scenario, loaded as-is.
const SCENARIO: &str = include_str!("../scenarios/examples/02_meadow.ron");

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

        // (a) GREW clearly above its founders → the nutrient feeds a standing crop.
        if peak < founders + 20 {
            failures.push(format!(
                "with sources, seed {seed:#x}: growth too weak (peak {peak}, founders {founders})"
            ));
        }
        // (b) bounded by the nutrient (Liebig, `metabolic_cost`) — a dense oasis, never a
        //     carpet filling the map.
        if peak > 1500 {
            failures.push(format!(
                "with sources, seed {seed:#x}: reproduction not bounded (peak {peak})"
            ));
        }
        // (c) PERSISTS — it oscillates around its carrying capacity, so the 2nd-half PEAK
        //     stays healthy (it does not die out where the nutrient reaches).
        if hi < founders {
            failures.push(format!(
                "with sources, seed {seed:#x}: not sustained (2nd-half peak {hi} < founders {founders})"
            ));
        }
    }

    eprintln!("  WITHOUT sources (founders={founders}):");
    for seed in SEEDS {
        let traj = population_trajectory(&no_sources, seed);
        let peak = *traj.iter().max().unwrap();
        let last = *traj.last().unwrap();
        eprintln!("    {seed:#012x} | peak {peak:>4} | final {last:>4}");

        // No nutrient → no metabolism → the plants STARVE: the population collapses well
        // below its founders. This is the falsifiable proof that the nutrient now gates
        // SURVIVAL (with sources it persisted at a carrying capacity; without, it dies).
        if last >= founders / 2 {
            failures.push(format!(
                "without sources, seed {seed:#x}: did not collapse without nutrient \
                 (final {last}, founders {founders}) — the nutrient should gate survival"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "T2 nutrient gating not robust:\n  {}",
        failures.join("\n  ")
    );
}
