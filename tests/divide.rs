//! Divide driver — trophic dependence, shown in a single frame
//! (`scenarios/examples/02_divide.ron`, a split arena).
//!
//! An impassable rock wall splits the arena. The SAME grazer runs on both halves, but only
//! the LEFT half has producers + nutrient vents; the RIGHT half is bare. So:
//!   • the LEFT (fed, species 0) grazers eat the producers and PERSIST;
//!   • the RIGHT (starved, species 1) grazers have nothing to eat and, paying the allometric
//!     cost law every second (SIM Law 1/7), go EXTINCT.
//!
//! The falsifiable point: a consumer is not self-sufficient — strip its producers and it
//! dies, however capable. The wall keeps the two herds apart (and seed-dispersal cannot hop
//! it), so the contrast is a clean side-by-side control in one run. Single-stepping, the
//! same world as the binaries.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

const SEEDS: [u64; 3] = [1, 2, 3];
/// The bare-side herd starves out within ~40 s; this leaves margin for every seed.
const HORIZON: usize = 70;

/// Living (fed grazer, starved grazer, producer) counts — species 0, 1, 2 — after
/// `seconds` at `seed`.
fn counts(seed: u64, seconds: usize) -> (usize, usize, usize) {
    let mut config = SimConfig::from_ron_file("scenarios/examples/02_divide.ron")
        .expect("scenario 02_divide.ron loadable");
    config.seed = seed;
    let hz = config.tick_hz as usize;
    let mut app = common::stepping_app(&config);
    for _ in 0..seconds * hz {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Species, With<Agent>>();
    let (mut fed, mut starved, mut producer) = (0usize, 0usize, 0usize);
    for s in q.iter(world) {
        match s.0 {
            0 => fed += 1,
            1 => starved += 1,
            2 => producer += 1,
            _ => {}
        }
    }
    (fed, starved, producer)
}

#[test]
fn producer_side_persists_bare_side_starves() {
    for seed in SEEDS {
        let (fed, starved, producer) = counts(seed, HORIZON);
        // The bare half's herd has died out — no food, only the metabolic floor.
        assert_eq!(
            starved, 0,
            "seed {seed}: the bare-side grazers must starve out (still {starved} at {HORIZON}s)"
        );
        // The producer half sustains its herd, on a living food base.
        assert!(
            fed >= 4 && producer > 0,
            "seed {seed}: the producer-side herd must persist on its food base \
             (fed {fed}, producers {producer} at {HORIZON}s)"
        );
    }
}
