//! Item 18b — the MLP **learning story**, told from one scenario.
//!
//! A from-random MLP is a poor forager, and on living (mortal, reproducing) food the
//! coexistence window is too short for neuroevolution to evolve a *dominant* forager
//! (the high-variance wall of item 18b: the original domination needed a long, stable
//! selection window — immortal food + many generations). So instead of forcing a
//! single "MLP dominates wander" scenario, we tell the story honestly, deriving both ends
//! from the ONE showcase (`deferred/learning.ron`, the trained MLP vs a wander control):
//!
//!   - the **naive** end — the same world with species 0's `captured_brain` stripped, so
//!     it is a from-RANDOM MLP: the wanderer **out-forages** it;
//!   - the **trained** end — the showcase as shipped (the captured, evolved brain): it
//!     reaches **parity** (it is no longer out-foraged).
//!
//! This driver guards the honest living-food outcome: the trained capture LOADS and yields
//! a **viable forager that holds PARITY** with the wander control across several seeds
//! (neither driven extinct nor crushed) — the regression that would fire if the captured
//! brain were broken (wrong input size, corrupt weights → extinction). It does NOT assert a
//! per-seed "training beats random": on this economy, neuroevolution plateaus at parity and
//! the seed-to-seed variance swamps the small skill difference (ROADMAP §7 — the trained
//! brain is the `train` bin's product, and its wiring is proven by the unit
//! `brain::tests::mlp_reads_*_channel`). The naive baseline is measured and printed for
//! contrast, not asserted.
//!
//! We run the *real* sim world (the same `SimPlugin` as the binaries), single-stepping.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

const SCENARIO: &str = include_str!("../scenarios/deferred/learning.ron");
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];
const SECONDS: usize = 45;

/// Species 0 = the MLP (naive or trained), species 1 = the wander control.
const MLP: u16 = 0;
const WANDER: u16 = 1;

/// Mean (MLP, wander) counts over the **last third** of the coexistence window for one run
/// under `seed`. With `naive`, species 0's captured brain is stripped so it spawns from
/// RANDOM weights (the baseline); otherwise the shipped trained brain stands.
fn forager_means(seed: u64, naive: bool) -> (f32, f32) {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid MLP scenario");
    config.seed = seed;
    if naive {
        // Strip the evolved weights → a fresh, random MLP compiled from the seed (spawn
        // falls back to a from-scratch brain when there is no capture).
        config.archetypes[MLP as usize].captured_brain = None;
        config.archetypes[MLP as usize].captured_from = None;
    }
    let tick_hz = config.tick_hz as usize;

    let mut app = common::stepping_app(&config);
    let mut traj = Vec::with_capacity(SECONDS);
    for _ in 0..SECONDS {
        for _ in 0..tick_hz {
            app.update();
        }
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Species, With<Agent>>();
        let (mut mlp, mut wander) = (0usize, 0usize);
        for s in q.iter(world) {
            match s.0 {
                MLP => mlp += 1,
                WANDER => wander += 1,
                _ => {}
            }
        }
        traj.push((mlp, wander));
    }

    let back = &traj[SECONDS * 2 / 3..];
    let n = back.len() as f32;
    let mlp = back.iter().map(|&(m, _)| m).sum::<usize>() as f32 / n;
    let wander = back.iter().map(|&(_, w)| w).sum::<usize>() as f32 / n;
    (mlp, wander)
}

/// The MLP/wander ratio over the window — how the MLP fares **relative to the control**.
/// `wander == 0` (control extinct) ⇒ the MLP is unboundedly ahead, reported as a large
/// finite number so the comparison stays well-defined.
fn ratio(mlp: f32, wander: f32) -> f32 {
    if wander <= 0.0 {
        if mlp > 0.0 { 1000.0 } else { 0.0 }
    } else {
        mlp / wander
    }
}

#[test]
fn trained_mlp_is_a_viable_parity_forager() {
    let mut failures = Vec::new();
    let mut trained_ratios = Vec::new();
    eprintln!("  seed         | naive MLP/wander | trained MLP/wander");
    for seed in SEEDS {
        let (naive_mlp, naive_wander) = forager_means(seed, true);
        let (trained_mlp, trained_wander) = forager_means(seed, false);
        let naive_ratio = ratio(naive_mlp, naive_wander);
        let trained_ratio = ratio(trained_mlp, trained_wander);
        trained_ratios.push(trained_ratio);
        eprintln!(
            "  {seed:#012x} | {naive_mlp:>4.1}/{naive_wander:<4.1} ({naive_ratio:.2}) | \
             {trained_mlp:>4.1}/{trained_wander:<4.1} ({trained_ratio:.2})"
        );

        // (1) The trained capture yields a VIABLE forager — it coexists, is not driven
        // extinct. This is what a broken capture (wrong input size / corrupt weights) would
        // fail: the regression this tripwire guards.
        if trained_mlp <= 0.0 {
            failures.push(format!(
                "seed {seed:#x}: the TRAINED MLP went extinct — the capture is not a viable forager"
            ));
        }
        // (2) …and it is not CRUSHED by the control: it holds within a parity band (≥ 40 %
        // of the wander's standing crop). Domination is not the living-food outcome (§7).
        if trained_ratio < 0.4 {
            failures.push(format!(
                "seed {seed:#x}: the trained MLP was crushed by the control \
                 (ratio {trained_ratio:.2}) — it should hold parity, not be out-foraged away"
            ));
        }
    }

    // Aggregate parity: across the seeds the trained MLP holds its own with the control
    // (mean ratio near 1), robust to the per-seed Lotka-Volterra variance.
    let mean_ratio = trained_ratios.iter().sum::<f32>() / trained_ratios.len() as f32;
    if mean_ratio < 0.5 {
        failures.push(format!(
            "the trained MLP does not reach parity on average (mean ratio {mean_ratio:.2})"
        ));
    }

    assert!(
        failures.is_empty(),
        "the trained MLP is not a viable parity forager:\n  {}",
        failures.join("\n  ")
    );
}
