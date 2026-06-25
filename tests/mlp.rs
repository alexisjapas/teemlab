//! Item 18b (core) — the **driver** of the evolved MLP brain.
//!
//! The §4 falsification through the most robust experiment: **cohabitation** (cf.
//! `tests/cohabitation.rs`). Two species with the same body and the same economy,
//! shared and limited food, differ only by their brain — species 0 = MLP (learned,
//! started from random weights), species 1 = wander (naive control). With a scarce
//! resource, the better forager excludes the other: if neuroevolution learns
//! anything useful, the MLP gains the upper hand over the wanderer. We check it
//! over several seeds (a single one would be anecdotal).
//!
//! We run the *real* sim world (the same `SimPlugin` as the binaries), in manual
//! single-stepping (cf. §6).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::genotype::Genotype;

mod common;

const SCENARIO: &str = include_str!("../scenarios/mlp_brain.ron");
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];
const SECONDS: usize = 200;

/// Species 0 = MLP (learned), species 1 = wander (naive control).
const MLP: u16 = 0;
const WANDER: u16 = 1;

/// Trajectory of a run: counts (MLP, wander) per second + final mean vision per
/// species (vision is maintained if the brain uses it).
struct Run {
    traj: Vec<(usize, usize)>,
    mlp_vision: f32,
    wander_vision: f32,
}

fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid MLP scenario");
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
        let (mut mlp, mut wander) = (0, 0);
        for s in q.iter(world) {
            match s.0 {
                MLP => mlp += 1,
                WANDER => wander += 1,
                _ => {}
            }
        }
        traj.push((mlp, wander));
    }

    let world = app.world_mut();
    let mut q = world.query::<(&Species, &Genotype)>();
    let (mut ms, mut mn, mut ws, mut wn) = (0.0f32, 0usize, 0.0f32, 0usize);
    for (s, g) in q.iter(world) {
        match s.0 {
            MLP => {
                ms += g.vision_range;
                mn += 1;
            }
            WANDER => {
                ws += g.vision_range;
                wn += 1;
            }
            _ => {}
        }
    }
    let mean = |sum: f32, n: usize| if n == 0 { f32::NAN } else { sum / n as f32 };
    Run {
        traj,
        mlp_vision: mean(ms, mn),
        wander_vision: mean(ws, wn),
    }
}

#[test]
#[ignore = "WIP: photosynthetic food now dies when grazed (Law 11 reorder); this scenario \
needs a nutrient-based density bound to be re-balanced — ROADMAP §9 nutrients"]
fn mlp_outforages_wanderer_across_seeds() {
    let mut failures = Vec::new();
    eprintln!("  seed         | MLP(2nd-half mean) | wander(2nd-half mean) | vision MLP/wander");
    for seed in SEEDS {
        let run = run_seed(seed);
        // 2nd half: we let the transient pass (growth, the first generations while
        // neuroevolution gets going).
        let back = &run.traj[SECONDS / 2..];
        let mean = |f: &dyn Fn(&(usize, usize)) -> usize| -> f32 {
            back.iter().map(f).sum::<usize>() as f32 / back.len() as f32
        };
        let mlp_mean = mean(&|&(m, _)| m);
        let wander_mean = mean(&|&(_, w)| w);
        let sampled: Vec<String> = run
            .traj
            .iter()
            .step_by(25)
            .map(|&(m, w)| format!("{m}/{w}"))
            .collect();
        eprintln!(
            "  {seed:#012x} | {mlp_mean:>6.1}             | {wander_mean:>6.1}                 | {:.0} / {:.0}",
            run.mlp_vision, run.wander_vision
        );
        eprintln!("               t=0,25,..: {}", sampled.join("  "));

        // The learned MLP must DOMINATE the wanderer (forage far better): with an
        // equal start and an identical body, this is the proof that it learned (§4).
        // We require a clear domination (≥ 2×), not a hair — and that the MLP itself
        // thrives (otherwise "both collapsed" would pass wrongly).
        if mlp_mean < 50.0 {
            failures.push(format!(
                "seed {seed:#x}: the MLP does not thrive ({mlp_mean:.1}) — it did not learn to forage"
            ));
        } else if mlp_mean <= 2.0 * wander_mean {
            failures.push(format!(
                "seed {seed:#x}: the MLP does not dominate the wanderer ({mlp_mean:.1} vs {wander_mean:.1}) — insufficient learning"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "the MLP failed to beat the wander control:\n  {}",
        failures.join("\n  ")
    );
}
