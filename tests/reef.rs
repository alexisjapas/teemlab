//! Reef scenario driver — the two physical/spatial levers (solid **rocks** + spring-anchored
//! **kelp**) composed into a living ecosystem (`scenarios/examples/10_reef.ron`). We assert
//! the reef is healthy over its coexistence window **and** that **turnover** happens: a
//! grazed kelp deposits **detritus** (the `emit_at_death` corpse) into its field — the
//! payoff of the mortality lever, made observable. The anchoring/rock *mechanics*
//! themselves are proven in `movement` (unit — the three tear-off branches) + `tests/anchor.rs`
//! / `tests/obstacle.rs`; here we prove they compose into a bounded, living scene.
//!
//! Note — under emergent targeting the grazer eats at a *reach* (it never rams the kelp)
//! and an anchored body is effectively pinned by its spring, so the tear-off does not fire
//! from ordinary grazing/crowding here (it did in the pre-refactor reef only because the
//! same-size grazer *couldn't* eat and shoved endlessly). Turnover is therefore
//! grazing-driven; the tear-off branches stay covered by the `movement` unit tests.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::nutrients::Fields;

mod common;

#[test]
fn reef_persists_and_turns_over() {
    let config = SimConfig::from_ron_file("scenarios/examples/10_reef.ron").expect("reef loads");
    let mut app = common::stepping_app(&config);

    // Peak detritus reached at any point over the coexistence window — proof that kelp died
    // and deposited a corpse (turnover), even though detritus decays away between events.
    let mut peak_detritus = 0.0f32;
    for _ in 0..1800 {
        app.update();
        let detritus = app
            .world()
            .resource::<Fields>()
            .0
            .get(1)
            .map(|f| f.total())
            .unwrap_or(0.0);
        peak_detritus = peak_detritus.max(detritus);
    }

    let world = app.world_mut();
    let mut q = world.query_filtered::<&Species, With<Agent>>();
    let (mut kelp, mut grazer) = (0usize, 0usize);
    for s in q.iter(world) {
        match s.0 {
            0 => kelp += 1,
            1 => grazer += 1,
            _ => {}
        }
    }

    // Kelp thrives but is **bounded** — the anchoring + grazing turnover holds it below the
    // founding 90 without wiping it out (a living reef: not a carpet, not a graveyard).
    assert!(
        (25..=90).contains(&kelp),
        "kelp population {kelp} outside the healthy reef window [25, 90]"
    );
    // The grazer coexists (the reef keeps its second trophic level).
    assert!(
        grazer > 0,
        "the grazers went extinct — the reef lost its second level"
    );
    // TURNOVER happened: detritus was deposited — the uprooting/grazing mortality lever's
    // payoff (corpses via `emit_at_death`), the thing the detritivore niche waits on.
    assert!(
        peak_detritus > 3.0,
        "no detritus accumulated (peak {peak_detritus:.1}) — turnover did not occur"
    );
}
