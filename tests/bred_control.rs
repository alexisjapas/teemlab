//! Breeding control experiment — the "decisive control" of `docs/p5-breeding-plan.md`
//! ("RESUME HERE"): does the individual captured from the 2026-07-06 `breed` run
//! forage better than a naive founder, **relative to the same wander control**?
//!
//! The bred archetype (`species/saved/mlp_bred.ron`, generation 6) is transplanted
//! into the standard MLP arena (`scenarios/saved/mlp_bred_control.ron`,
//! generated) and measured with the exact `tests/mlp.rs` protocol: mean populations
//! over the last third of 45 simulated seconds, subject/wander ratio, 5 seeds. The
//! naive (07) and trained (09) scenarios run in the same binary as the two poles of
//! the comparison.
//!
//! **Experiment driver, not a regression gate** — it reads local, gitignored files,
//! so it is `#[ignore]`d. Run it explicitly:
//! `cargo test --test bred_control -- --ignored --nocapture`

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

const NAIVE: &str = include_str!("../scenarios/deferred/learning.ron");
const TRAINED: &str = include_str!("../scenarios/deferred/learning.ron");
/// The bred archetype (`species/saved/mlp_bred.ron`) spliced over archetype 0 of
/// `deferred/learning.ron` (count set back to 14); gitignored local state, hence read at
/// runtime and not `include_str!`ed.
const BRED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scenarios/saved/mlp_bred_control.ron"
);
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];
const SECONDS: usize = 45;

/// Species 0 = the subject (naive / trained / bred MLP), species 1 = the wander control.
const SUBJECT: u16 = 0;
const WANDER: u16 = 1;

/// Mean (subject, wander) counts over the **last third** of the coexistence window for
/// one run of `scenario` under `seed` (sampled once per simulated second).
fn forager_means(scenario: &str, seed: u64) -> (f32, f32) {
    let mut config = SimConfig::from_ron_str(scenario).expect("valid scenario");
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
        let (mut subject, mut wander) = (0usize, 0usize);
        for s in q.iter(world) {
            match s.0 {
                SUBJECT => subject += 1,
                WANDER => wander += 1,
                _ => {}
            }
        }
        traj.push((subject, wander));
    }

    let back = &traj[SECONDS * 2 / 3..];
    let n = back.len() as f32;
    let subject = back.iter().map(|&(s, _)| s).sum::<usize>() as f32 / n;
    let wander = back.iter().map(|&(_, w)| w).sum::<usize>() as f32 / n;
    (subject, wander)
}

/// The subject/wander ratio over the window (cf. `tests/mlp.rs`).
fn ratio(subject: f32, wander: f32) -> f32 {
    if wander <= 0.0 {
        if subject > 0.0 { 1000.0 } else { 0.0 }
    } else {
        subject / wander
    }
}

#[test]
#[ignore = "experiment driver reading local gitignored files; run with --ignored --nocapture"]
fn bred_vs_naive_against_the_wander_control() {
    let bred = std::fs::read_to_string(BRED_PATH).expect(
        "scenarios/saved/mlp_bred_control.ron missing — rebuild it by splicing the \
             species/saved/mlp_bred.ron archetype over archetype 0 of deferred/learning.ron \
             (count 14); cf. docs/p5-breeding-plan.md \"RESUME HERE\"",
    );

    eprintln!("  seed         | naive MLP/wander | trained MLP/wander | bred MLP/wander");
    let (mut naive_sum, mut trained_sum, mut bred_sum) = (0.0f32, 0.0f32, 0.0f32);
    for seed in SEEDS {
        let (naive_s, naive_w) = forager_means(NAIVE, seed);
        let (trained_s, trained_w) = forager_means(TRAINED, seed);
        let (bred_s, bred_w) = forager_means(&bred, seed);
        let (naive_r, trained_r, bred_r) = (
            ratio(naive_s, naive_w),
            ratio(trained_s, trained_w),
            ratio(bred_s, bred_w),
        );
        eprintln!(
            "  {seed:#012x} | {naive_s:>4.1}/{naive_w:<4.1} ({naive_r:.2}) | \
             {trained_s:>4.1}/{trained_w:<4.1} ({trained_r:.2}) | \
             {bred_s:>4.1}/{bred_w:<4.1} ({bred_r:.2})"
        );
        naive_sum += naive_r;
        trained_sum += trained_r;
        bred_sum += bred_r;
    }

    let n = SEEDS.len() as f32;
    let (naive, trained, bred) = (naive_sum / n, trained_sum / n, bred_sum / n);
    eprintln!("  mean ratio   | naive {naive:.2} | trained {trained:.2} | bred {bred:.2}");
    eprintln!(
        "  verdict      | bred vs naive: {:.2}x | bred vs trained: {:.2}x",
        if naive > 0.0 {
            bred / naive
        } else {
            f32::INFINITY
        },
        if trained > 0.0 {
            bred / trained
        } else {
            f32::INFINITY
        },
    );
}
