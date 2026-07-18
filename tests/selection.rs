//! Selection driver — a priced, UNUSED trait decays only where it is allowed to
//! (`scenarios/examples/03_selection.ron`, the split control).
//!
//! Both halves run the SAME far-sighted wanderers (vision range 300, 13 rays) that never
//! steer on their eyes, foraging the same flora. Vision is pure overhead (a real
//! `Vision::metabolic_cost`, SIM Law 7), so once the flora is grazed to a limiting level a
//! wanderer that spends less on it breeds faster. The two halves differ ONLY in whether
//! vision may mutate:
//!   • MUTABLE (species 0) → selection melts the eyes down (mean `vision_range` slides well
//!     below the founding 300);
//!   • FROZEN (species 1) → the CONTROL: same economy, same drift on every OTHER gene, but
//!     vision is non-mutable, so it stays exactly at the founding 300 / 13.
//!
//! The falsifiable contrast: a gene falling ONLY on the side where it can mutate is
//! selection, not noise or crowding. Living food (an example to watch, ROADMAP §7): the
//! flora oscillates hard, but both halves sustain a breeding population well past the point
//! where the decay is plain. Single-stepping, the same world as the binaries.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::genotype::Genotype;

mod common;

const SEEDS: [u64; 3] = [1, 2, 3];
/// Long enough that food-limited selection has plainly pruned the mutable side, while both
/// halves still hold a population (the decay precedes the eventual §7 wind-down).
const HORIZON: usize = 300;

/// After `seconds` at `seed`, the mean `vision_range` and living count of the MUTABLE
/// (species 0) and FROZEN (species 1) halves — from a SINGLE run (read both, don't re-sim).
fn vision_by_half(seed: u64, seconds: usize) -> [(f32, usize); 2] {
    let mut config = SimConfig::from_ron_file("scenarios/examples/03_selection.ron")
        .expect("scenario 03_selection.ron loadable");
    config.seed = seed;
    let hz = config.tick_hz as usize;
    let mut app = common::stepping_app(&config);
    for _ in 0..seconds * hz {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Genotype), With<Agent>>();
    let mut sum = [0.0f32; 2];
    let mut n = [0usize; 2];
    for (s, g) in q.iter(world) {
        let i = s.0 as usize;
        if i < 2 {
            sum[i] += g.vision_range;
            n[i] += 1;
        }
    }
    [0, 1].map(|i| (if n[i] > 0 { sum[i] / n[i] as f32 } else { 0.0 }, n[i]))
}

#[test]
fn mutable_vision_decays_frozen_control_holds() {
    for seed in SEEDS {
        let [(mutable_range, mutable_n), (frozen_range, frozen_n)] = vision_by_half(seed, HORIZON);

        assert!(
            mutable_n > 0 && frozen_n > 0,
            "seed {seed}: both halves must persist to be conclusive \
             (mutable n{mutable_n}, frozen n{frozen_n})"
        );
        // The control never moves: vision is non-mutable, so every survivor still carries
        // the founding 300 exactly.
        assert!(
            (frozen_range - 300.0).abs() < 1.0,
            "seed {seed}: the frozen control's vision must stay at the founding 300 \
             (got {frozen_range:.1})"
        );
        // The mutable half has been pruned well below it — selection at work.
        assert!(
            mutable_range < 290.0,
            "seed {seed}: the mutable half's vision must decay below the frozen control \
             (mutable {mutable_range:.1} vs frozen {frozen_range:.1} at {HORIZON}s)"
        );
    }
}
