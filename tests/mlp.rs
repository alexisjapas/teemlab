//! Item 18b — the MLP **learning story**, told across two scenarios.
//!
//! A from-random MLP is a poor forager, and on living (mortal, reproducing) food the
//! coexistence window is too short for neuroevolution to evolve a *dominant* forager
//! (the high-variance wall of item 18b: the original domination needed a long, stable
//! selection window — immortal food + many generations). So instead of forcing a
//! single "MLP dominates wander" scenario, we tell the story honestly:
//!
//!   - `mlp_brain` — the **naive** MLP (random weights) vs a wander control: the
//!     wanderer **out-forages** it.
//!   - `mlp_train` — MLPs train ALONE on the oasis flora (a generator, see the `train`
//!     bin), from which an evolved variant is captured.
//!   - `mlp_evolved` — the **trained** variant vs the same control: it reaches
//!     **parity** (it is no longer out-foraged).
//!
//! This driver falsifies the *learning* claim: the trained MLP fares **better relative
//! to the control** than the naive MLP — training closed the gap — across several seeds.
//! (Parity, not domination, is the honest outcome of the living-food regime.)
//!
//! We run the *real* sim world (the same `SimPlugin` as the binaries), single-stepping.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

const NAIVE: &str = include_str!("../scenarios/examples/mlp_brain.ron");
const TRAINED: &str = include_str!("../scenarios/examples/mlp_evolved.ron");
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];
const SECONDS: usize = 45;

/// Species 0 = the MLP (naive or trained), species 1 = the wander control.
const MLP: u16 = 0;
const WANDER: u16 = 1;

/// Mean (MLP, wander) counts over the **last third** of the coexistence window for one
/// run of `scenario` under `seed` (sampled once per simulated second).
fn forager_means(scenario: &str, seed: u64) -> (f32, f32) {
    let mut config = SimConfig::from_ron_str(scenario).expect("valid MLP scenario");
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
fn training_improves_the_mlp_against_the_control() {
    let mut failures = Vec::new();
    eprintln!("  seed         | naive MLP/wander | trained MLP/wander");
    for seed in SEEDS {
        let (naive_mlp, naive_wander) = forager_means(NAIVE, seed);
        let (trained_mlp, trained_wander) = forager_means(TRAINED, seed);
        let naive_ratio = ratio(naive_mlp, naive_wander);
        let trained_ratio = ratio(trained_mlp, trained_wander);
        eprintln!(
            "  {seed:#012x} | {naive_mlp:>4.1}/{naive_wander:<4.1} ({naive_ratio:.2}) | \
             {trained_mlp:>4.1}/{trained_wander:<4.1} ({trained_ratio:.2})"
        );

        // (1) The naive MLP is out-foraged by the wander control (the baseline).
        if naive_ratio >= 1.0 {
            failures.push(format!(
                "seed {seed:#x}: the NAIVE MLP was not out-foraged by the wanderer \
                 ({naive_mlp:.1} vs {naive_wander:.1}) — the baseline should show it losing"
            ));
        }
        // (2) The trained MLP coexists (it is a viable forager, not driven extinct).
        if trained_mlp <= 0.0 {
            failures.push(format!(
                "seed {seed:#x}: the TRAINED MLP went extinct — training did not yield a forager"
            ));
        }
        // (3) Training closed the gap: the trained MLP fares clearly better **relative
        // to the control** than the naive one did (≥ 1.5×). This is the learning claim
        // (parity, not domination — the honest living-food outcome).
        if trained_ratio < naive_ratio * 1.5 {
            failures.push(format!(
                "seed {seed:#x}: training did not improve the MLP's standing \
                 (naive ratio {naive_ratio:.2} → trained {trained_ratio:.2})"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "the MLP learning story did not hold:\n  {}",
        failures.join("\n  ")
    );
}
