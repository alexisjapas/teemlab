//! Selection driver — a priced, UNUSED trait decays only in the species that is allowed to
//! mutate it, and it does so FAST: the mutable lineage's advantage over the frozen control
//! is plain **inside two minutes** (`scenarios/examples/03_selection.ron`, the in-arena
//! control).
//!
//! Both species are the SAME far-sighted wanderers (vision range 300, 21 rays) that never
//! steer on their eyes, sharing one open arena and the same flora. Vision is pure overhead
//! (a real `Vision::metabolic_cost`, SIM Law 7, the dominant drain), so with the flora grazed
//! to a limiting level a wanderer that spends less on it breeds faster. The two species differ
//! ONLY in whether vision may mutate:
//!   • MUTABLE (species 0) → selection melts the eyes down (mean `vision_range` slides from
//!     300 to ~210 within 2 min) and, paying far less overhead, out-breeds the control;
//!   • FROZEN (species 1) → the CONTROL: same economy, same drift on every OTHER gene, but
//!     vision is non-mutable, so it stays exactly at the founding 300 and dwindles.
//!
//! The falsifiable contrast: a gene falling ONLY in the species that can mutate it is
//! selection, not noise or crowding. The founders are LARGE cohorts (40 each) so the outcome
//! is selection, not a coin-flip — the cheaper mutant wins every seed. Single-stepping, the
//! same world as the binaries.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::genotype::Genotype;

mod common;

const SEEDS: [u64; 3] = [1, 2, 3];
/// The demo window: the mutable lineage's advantage — decayed vision AND a clear population
/// lead over the frozen control — must be plain by here (the user's "under two minutes").
const HORIZON: usize = 120;

/// After `seconds` at `seed`, the mean `vision_range` and living count of the MUTABLE
/// (species 0) and FROZEN (species 1) lineages — from a SINGLE run (read both, don't re-sim).
fn vision_by_species(seed: u64, seconds: usize) -> [(f32, usize); 2] {
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
fn mutable_advantage_is_plain_within_two_minutes() {
    for seed in SEEDS {
        let [(mutable_range, mutable_n), (frozen_range, frozen_n)] =
            vision_by_species(seed, HORIZON);

        // The frozen control is still present (a flat 300 line to read against) — it dwindles
        // but is not gone at two minutes.
        assert!(
            mutable_n > 0 && frozen_n > 0,
            "seed {seed}: both lineages must persist at {HORIZON}s to be conclusive \
             (mutable n{mutable_n}, frozen n{frozen_n})"
        );
        // The control never moves: vision is non-mutable, so every survivor still carries the
        // founding 300 exactly.
        assert!(
            (frozen_range - 300.0).abs() < 1.0,
            "seed {seed}: the frozen control's vision must stay at the founding 300 \
             (got {frozen_range:.1})"
        );
        // Mechanism: the mutable lineage's eyes have plainly melted down (observed ~204–228
        // by 120s; a comfortable margin below).
        assert!(
            mutable_range < 265.0,
            "seed {seed}: the mutable lineage's vision must have decayed clearly by {HORIZON}s \
             (mutable {mutable_range:.1} vs frozen {frozen_range:.1})"
        );
        // Outcome: paying less overhead, the mutable lineage out-breeds the control by a clear
        // margin (observed ~2–3×; require ≥1.4× to leave room across seeds).
        assert!(
            mutable_n as f32 >= 1.4 * frozen_n as f32,
            "seed {seed}: the mutable lineage must lead the frozen control by {HORIZON}s \
             (mutable n{mutable_n} vs frozen n{frozen_n})"
        );
    }
}
