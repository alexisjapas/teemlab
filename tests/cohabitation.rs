//! Item 18a — the **driver** of the control/learned cohabitation.
//!
//! The "brain per species" seam + brain inheritance at reproduction, falsified
//! with the TWO existing deterministic brains (hunter vs wander), before the MLP
//! (18b) arrives. Two species share the same body and the same economy, differ
//! ONLY by their brain, and graze the same food. The criterion has three parts,
//! judged over several seeds (a single one's success would be anecdotal):
//!
//! 1. **Inheritance invariant** — every living agent of species 0 carries
//!    `Brain::Hunter`, every agent of species 1 carries `Brain::Wander`. If
//!    reproduction rebuilt the brain from the global `config` instead of inheriting
//!    from the parent, this part would break: it is the direct falsification of the
//!    seam.
//! 2. **Effective reproduction** — the hunter population grows beyond its founders:
//!    without it, inheritance would not be exercised.
//! 3. **Domination of the competent control** (§4) — with a shared and limited
//!    resource, the hunter wins over the wanderer (population). It is the contrast
//!    that a learned brain will, in 18b, have to at least match.
//!
//! We run the *real* sim world (the same `SimPlugin` as both binaries), in manual
//! single-stepping (cf. headless throughput, §6).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Agent, Species};

mod common;

/// The bundled scenario, loaded as-is: the driver measures WHAT the binaries
/// launch, not a test variant.
const SCENARIO: &str = include_str!("../scenarios/examples/cohabitation.ron");

/// Experiment seeds (cf. §5: we replay a *config*, not bit-for-bit). Five
/// independent worlds: if the domination holds for all of them, it is not luck but
/// a property of the brain.
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];

// The coexistence window: the living food is a reproducing flora (a fixed immortal
// patch now dies when grazed — Law 11), so this is a Lotka-Volterra system that
// coexists for a while then the foragers fade. We judge the domination over the 2nd
// half of this window, where the hunter has pulled clearly ahead of the wanderer.
const SECONDS: usize = 60;

/// Species 0 = hunter (competent control), species 1 = wander (naive control).
const HUNTER: u16 = 0;
const WANDER: u16 = 1;

/// Trajectory of a run: counts (hunters, wanderers) per sim second, + an
/// end-of-run tally for the inheritance invariant.
struct Run {
    traj: Vec<(usize, usize)>,
    /// Number of living agents whose brain does NOT match their species (hunter
    /// expected for 0, wander for 1) — must stay zero (part 1).
    brain_mismatches: usize,
    /// Peak of hunters over the whole run (part 2: growth > founders).
    hunter_peak: usize,
}

/// Runs the scenario for `SECONDS` seconds for a given seed.
fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid cohabitation scenario");
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
        let (mut hunters, mut wanderers) = (0, 0);
        for s in q.iter(world) {
            match s.0 {
                HUNTER => hunters += 1,
                WANDER => wanderers += 1,
                _ => {}
            }
        }
        traj.push((hunters, wanderers));
    }

    // Inheritance tally: does each living agent's brain match its species?
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Brain), With<Agent>>();
    let brain_mismatches = q
        .iter(world)
        .filter(|(s, brain)| match s.0 {
            HUNTER => !matches!(brain, Brain::Hunter(_)),
            WANDER => !matches!(brain, Brain::Wander(_)),
            _ => false,
        })
        .count();

    let hunter_peak = traj.iter().map(|&(h, _)| h).max().unwrap_or(0);
    Run {
        traj,
        brain_mismatches,
        hunter_peak,
    }
}

#[test]
fn hunter_outforages_wanderer_across_seeds() {
    let hunter_founders =
        SimConfig::from_ron_str(SCENARIO).unwrap().archetypes[HUNTER as usize].count;

    let mut failures = Vec::new();
    eprintln!(
        "  seed         | hunter(2nd-half mean) | wander(2nd-half mean) | hunter peak (founders {hunter_founders})"
    );
    for seed in SEEDS {
        let run = run_seed(seed);
        // We judge the domination on the **last third** of the coexistence window:
        // on living (mortal, reproducing) food the wanderer survives the abundant
        // early phase by chance, and the competitive exclusion only becomes clear once
        // the food has been drawn down — exactly where the competent forager pulls
        // away. (Pre-Law-11 immortal food showed it across the whole 2nd half.)
        let back = &run.traj[SECONDS * 2 / 3..];
        let mean = |f: &dyn Fn(&(usize, usize)) -> usize| -> f32 {
            back.iter().map(f).sum::<usize>() as f32 / back.len() as f32
        };
        let hunter_mean = mean(&|&(h, _)| h);
        let wander_mean = mean(&|&(_, w)| w);
        let sampled: Vec<String> = run
            .traj
            .iter()
            .step_by(20)
            .map(|&(h, w)| format!("{h}/{w}"))
            .collect();
        eprintln!(
            "  {seed:#012x} | {hunter_mean:>6.1}                 | {wander_mean:>6.1}                | {}",
            run.hunter_peak
        );
        eprintln!("               t=0,20,..: {}", sampled.join("  "));

        // --- Part 1: inheritance invariant (the per-species seam). ---
        if run.brain_mismatches > 0 {
            failures.push(format!(
                "seed {seed:#x}: {} agent(s) with a brain inconsistent with their species \
                 (brain inheritance failed)",
                run.brain_mismatches
            ));
        }

        // --- Part 2: effective reproduction (the hunters spread). ---
        if run.hunter_peak <= hunter_founders {
            failures.push(format!(
                "seed {seed:#x}: the hunters did not grow beyond the founders \
                 (peak {} ≤ {hunter_founders}) — inheritance not exercised",
                run.hunter_peak
            ));
        }

        // --- Part 3: domination of the competent control (§4). With a shared and
        // limited resource, the hunter (which finds the food) must win CLEARLY over
        // the wanderer (which only crosses it by chance). ---
        if hunter_mean <= wander_mean * 1.3 {
            failures.push(format!(
                "seed {seed:#x}: the hunter does not dominate the wanderer ({hunter_mean:.1} vs \
                 {wander_mean:.1}) — the competent control should forage far better"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "cohabitation inconclusive:\n  {}",
        failures.join("\n  ")
    );
}
