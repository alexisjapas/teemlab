//! Reef driver — **size-selective refugia** (`scenarios/deferred/reef.ron`). Solid rocks
//! (`Collider::circle(radius)`) ringed around each vent leave ~18 px gaps, so a disc of
//! radius r threads a gap only when G ≥ 2·r: the small HERBIVORE (r 6, needs 12 px) crosses
//! into the sheltered kelp gardens, the large OMNIVORE (r 12, needs 24 px) is WALLED OUT.
//! No behaviour rule draws the line — the geometry does. We prove it by COUNTING CROSSINGS
//! (an agent going from outside a ring to inside it): herbivores cross, the omnivore never
//! does. Counting crossings (not mere occupancy) makes the test robust to the one artefact
//! the engine can produce — an omnivore whose random spawn lands inside a ring — since a
//! spawn-inside body never *crossed* from outside; here the rings are also too tight for an
//! r 12 body to occupy, so that case doesn't even arise.
//!
//! We also assert the reef is a **living, bounded** scene: kelp holds a nutrient-bounded
//! stand (not a carpet), the herbivores persist in their gardens, and **turnover** happens —
//! grazed/uprooted kelp deposits DETRITUS (the `emit_at_death` corpse, component 1). The
//! anchoring/rock mechanics themselves are proven in `movement` (the tear-off branches) and
//! `tests/anchor.rs` / `tests/obstacle.rs`; here we prove they compose into a size-structured,
//! bounded, living reef. (The omnivore is a *transient* — the refuge is so effective it
//! starves the excluded predator; ROADMAP §7 — so we only require it present early, long
//! enough to be excluded, not to persist.)

use std::collections::HashMap;

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::substrate::Fields;

mod common;

/// The four refuge-ring centres (must match the scenario's sheltered vents).
const REFUGES: [Vec2; 4] = [
    Vec2::new(-150.0, 130.0),
    Vec2::new(150.0, 130.0),
    Vec2::new(-150.0, -130.0),
    Vec2::new(150.0, -130.0),
];
/// "Inside a ring": within this of a centre — clearly past the rock ring (inner surface at
/// ~29 px), so reaching it means a body actually threaded a gap.
const INSIDE: f32 = 24.0;

/// Independent worlds: a size barrier that holds across all of them is not luck.
const SEEDS: [u64; 4] = [0, 1, 2, 3];
const SECONDS: usize = 130;

fn is_inside(p: Vec2) -> bool {
    REFUGES.iter().any(|c| p.distance(*c) < INSIDE)
}

/// Per-seed tallies.
struct Run {
    herb_crossings: u64,
    omni_crossings: u64,
    omni_ever_present: bool,
    omni_ever_inside: bool,
    peak_detritus: f32,
    kelp_end: usize,
    herb_end: usize,
}

fn run(seed: u64) -> Run {
    let mut cfg = SimConfig::from_ron_file("scenarios/deferred/reef.ron").expect("reef loads");
    cfg.seed = seed;
    let hz = cfg.tick_hz as usize;
    let mut app = common::stepping_app(&cfg);

    // Per-entity inside-state; a false→true flip is a CROSSING (entered from outside). The
    // first sighting only records the state (so a spawn-inside body is never a crossing).
    let mut state: HashMap<Entity, bool> = HashMap::new();
    let mut r = Run {
        herb_crossings: 0,
        omni_crossings: 0,
        omni_ever_present: false,
        omni_ever_inside: false,
        peak_detritus: 0.0,
        kelp_end: 0,
        herb_end: 0,
    };
    for _ in 0..SECONDS * hz {
        app.update();
        let det = app
            .world()
            .resource::<Fields>()
            .0
            .get(1)
            .map(|f| f.total())
            .unwrap_or(0.0);
        r.peak_detritus = r.peak_detritus.max(det);
        let w = app.world_mut();
        let mut q = w.query_filtered::<(Entity, &Transform, &Species), With<Agent>>();
        for (e, t, s) in q.iter(w) {
            let inside = is_inside(t.translation.truncate());
            if s.0 == 2 {
                r.omni_ever_present = true;
                r.omni_ever_inside |= inside;
            }
            match state.get(&e) {
                None => {
                    state.insert(e, inside);
                }
                Some(&was) => {
                    if !was && inside {
                        match s.0 {
                            1 => r.herb_crossings += 1,
                            2 => r.omni_crossings += 1,
                            _ => {}
                        }
                    }
                    state.insert(e, inside);
                }
            }
        }
    }
    let w = app.world_mut();
    let mut q = w.query_filtered::<&Species, With<Agent>>();
    for s in q.iter(w) {
        match s.0 {
            0 => r.kelp_end += 1,
            1 => r.herb_end += 1,
            _ => {}
        }
    }
    r
}

#[test]
fn reef_sorts_bodies_by_size_and_stays_alive() {
    let (mut herb_cross, mut omni_cross) = (0u64, 0u64);
    let mut omni_present_seeds = 0usize;
    let mut omni_inside_seeds = 0usize;
    let mut peak_detritus_any = 0.0f32;

    eprintln!("  seed | herb-cross | omni-cross | kelp | herb | detritus");
    for seed in SEEDS {
        let r = run(seed);
        eprintln!(
            "  {seed:>4} | {:>10} | {:>10} | {:>4} | {:>4} | {:.1}",
            r.herb_crossings, r.omni_crossings, r.kelp_end, r.herb_end, r.peak_detritus
        );
        herb_cross += r.herb_crossings;
        omni_cross += r.omni_crossings;
        omni_present_seeds += r.omni_ever_present as usize;
        omni_inside_seeds += r.omni_ever_inside as usize;
        peak_detritus_any = peak_detritus_any.max(r.peak_detritus);

        // Per-seed liveness: the reef is a living, bounded scene in every world.
        assert!(
            (60..=1400).contains(&r.kelp_end),
            "seed {seed}: kelp {} outside the living-but-bounded band [60, 1400]",
            r.kelp_end
        );
        assert!(
            r.herb_end > 0,
            "seed {seed}: the herbivores went extinct — the gardens emptied"
        );
    }

    // THE SIZE BARRIER. Small bodies thread the gates, the large one never does: aggregated
    // over seeds, herbivores cross many times and the omnivore crosses ZERO times.
    assert!(
        herb_cross >= 8 && omni_cross == 0,
        "size barrier failed: herbivore crossings {herb_cross} (want ≥ 8), omnivore crossings \
         {omni_cross} (want 0)"
    );
    // The exclusion is REAL: the omnivore was actually present (to be excluded) in most
    // seeds, and never got inside a ring.
    assert!(
        omni_present_seeds >= SEEDS.len() - 1,
        "the omnivore was barely present ({omni_present_seeds}/{} seeds) — nothing to exclude",
        SEEDS.len()
    );
    assert_eq!(
        omni_inside_seeds, 0,
        "an omnivore was found inside a ring in {omni_inside_seeds} seed(s) — the wall leaked"
    );
    // TURNOVER: grazed/uprooted kelp deposited detritus (the mortality lever's payoff).
    assert!(
        peak_detritus_any > 3.0,
        "no detritus accumulated (peak {peak_detritus_any:.1}) — turnover did not occur"
    );
}
