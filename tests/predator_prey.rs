//! Item 17 — the **driver** of the co-evolutionary predator-prey scenario.
//!
//! The item-17 falsification criterion has three parts: (1) **population band** —
//! the two lineages coexist without either going extinct or exploding, and that
//! **robustly** (over several seeds, not by luck); (2) **expected drift** — vision
//! is MAINTAINED (the hunter uses it), instead of collapsing as under wandering;
//! (3) **"scenario as data + one driver, zero edits to
//! `movement`/`interaction`/`ecology`"** — this test file IS that driver, and
//! these three engine systems have not moved a line (item 17's "per-species count"
//! addition lives in `config`/`spawn`).
//!
//! We run the *real* sim world (the same `SimPlugin` as both binaries), in manual
//! single-stepping (cf. headless throughput, §6), and sample the per-species
//! population over time, **for several seeds**: a single seed's coexistence would
//! be anecdotal, not a band.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::genotype::Genotype;

mod common;

/// The bundled scenario, loaded as-is: the driver measures WHAT the binaries
/// launch, not a test variant.
const SCENARIO: &str = include_str!("../scenarios/predator_prey.ron");

/// Experiment seeds (cf. §5: we replay a *config*, not bit-for-bit). Five
/// independent worlds: if coexistence holds for all of them, it is not luck but a
/// property of the calibrated economy.
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];

const SECONDS: usize = 120;

/// Trajectory of a run: counts (predators = species 0, prey = species 1) sampled
/// each sim second, + the final mean vision per species.
struct Run {
    traj: Vec<(usize, usize)>,
    pred_vision: f32,
    prey_vision: f32,
    /// Mean vision over **all** living agents at the end of the run (robust to a
    /// momentarily empty species, where the per-species mean would be NaN).
    all_vision: f32,
}

/// Runs the scenario for `SECONDS` seconds for a given seed.
fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid predator-prey scenario");
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
        let (mut pred, mut prey) = (0, 0);
        for s in q.iter(world) {
            match s.0 {
                0 => pred += 1,
                1 => prey += 1,
                _ => {}
            }
        }
        traj.push((pred, prey));
    }

    let world = app.world_mut();
    let mut q = world.query::<(&Species, &Genotype)>();
    let (mut pred_sum, mut pred_n) = (0.0f32, 0usize);
    let (mut prey_sum, mut prey_n) = (0.0f32, 0usize);
    for (s, g) in q.iter(world) {
        match s.0 {
            0 => {
                pred_sum += g.vision_range;
                pred_n += 1;
            }
            1 => {
                prey_sum += g.vision_range;
                prey_n += 1;
            }
            _ => {}
        }
    }
    let mean = |sum: f32, n: usize| if n == 0 { f32::NAN } else { sum / n as f32 };
    Run {
        traj,
        pred_vision: mean(pred_sum, pred_n),
        prey_vision: mean(prey_sum, prey_n),
        all_vision: mean(pred_sum + prey_sum, pred_n + prey_n),
    }
}

#[test]
#[ignore = "WIP: photosynthetic food now dies when grazed (Law 11 reorder); this scenario \
needs a nutrient-based density bound to be re-balanced — ROADMAP §9 nutrients"]
fn predator_prey_coexists_in_a_band_across_seeds() {
    let founder_vision = SimConfig::from_ron_str(SCENARIO)
        .unwrap()
        .genotype_of(0)
        .vision_range;

    let mut failures = Vec::new();
    eprintln!(
        "  seed       | pred(min..max) | prey(min..max) | vision pred/prey (founder {founder_vision:.0})"
    );
    for seed in SEEDS {
        let run = run_seed(seed);
        // We judge on the 2nd half: we let the initial transient pass (founders'
        // peak, first adjustment) and look at the steady state.
        let back = &run.traj[SECONDS / 2..];
        let pred_min = back.iter().map(|&(p, _)| p).min().unwrap();
        let pred_max = back.iter().map(|&(p, _)| p).max().unwrap();
        let prey_min = back.iter().map(|&(_, q)| q).min().unwrap();
        let prey_max = back.iter().map(|&(_, q)| q).max().unwrap();
        let peak = run.traj.iter().map(|&(p, q)| p + q).max().unwrap();
        eprintln!(
            "  {seed:#012x} | {pred_min:>4}..{pred_max:<4}    | {prey_min:>4}..{prey_max:<4}     | {:.0} / {:.0}",
            run.pred_vision, run.prey_vision
        );
        // Coarse trajectory (pred/prey every 20 s) — the shape of the regime.
        let sampled: Vec<String> = run
            .traj
            .iter()
            .step_by(20)
            .map(|&(p, q)| format!("{p}/{q}"))
            .collect();
        eprintln!("              t=0,20,..: {}", sampled.join("  "));

        // --- Part 1: population band (bounded coexistence), for THIS seed. ---
        if pred_min == 0 {
            failures.push(format!(
                "seed {seed:#x}: predators extinct (chain not sustained)"
            ));
        }
        if prey_min == 0 {
            failures.push(format!("seed {seed:#x}: prey extinct (overpredation)"));
        }
        if peak > 600 {
            failures.push(format!("seed {seed:#x}: explosion (peak {peak})"));
        }

        // --- Part 2: expected drift. Under a hunter, vision IS USED — it stays well
        // above the floor (lower bound 30) toward which wandering would melt it (cf.
        // evolution.ron). We do not aim at a precise value (stochastic drift in a
        // small population), only the qualitative contrast: it did not melt away.
        if run.all_vision < 90.0 {
            failures.push(format!(
                "seed {seed:#x}: vision collapsed ({:.0}) — a hunter should maintain it",
                run.all_vision
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "coexistence not robust:\n  {}",
        failures.join("\n  ")
    );
}
