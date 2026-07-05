//! Reef scenario driver — the two physical/spatial levers (solid **rocks** + spring-anchored
//! **kelp**) composed into a living ecosystem (`scenarios/examples/20_reef.ron`). We assert
//! the reef is healthy over its coexistence window **and** that **turnover** happens: an
//! uprooted or grazed kelp deposits **detritus** (the `emit_at_death` corpse) into its field
//! — the payoff of the anchoring mortality lever, made observable. The anchoring/rock
//! *mechanics* themselves are proven in `movement` (unit) + `tests/anchor.rs` /
//! `tests/obstacle.rs`; here we prove they compose into a bounded, living scene.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Anchor, Species};
use teemlab::nutrients::Fields;

mod common;

/// Flip `die_on_detach` off so an uprooted kelp **survives, freed** (drops its `Anchor`)
/// instead of dying — which lets us **count** uprooting events directly: a Species-0 body
/// that has lost its `Anchor` was torn from the substrate. (With the shipped
/// `die_on_detach: true`, those same tears are deaths → detritus.) This proves the reef
/// actually exercises the anchor tear-off, not merely ordinary grazing mortality.
#[test]
fn reef_uproots_kelp() {
    let mut config =
        SimConfig::from_ron_file("scenarios/examples/20_reef.ron").expect("reef loads");
    config
        .archetypes
        .get_mut(0)
        .and_then(|a| a.anchor.as_mut())
        .expect("kelp is anchored")
        .die_on_detach = false;

    let mut app = common::stepping_app(&config);
    let mut peak_uprooted = 0usize;
    for _ in 0..1800 {
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Species, Option<&Anchor>), With<Agent>>();
        let freed = q
            .iter(world)
            .filter(|(s, anchor)| s.0 == 0 && anchor.is_none())
            .count();
        peak_uprooted = peak_uprooted.max(freed);
    }
    println!("reef: peak uprooted-and-freed kelp = {peak_uprooted}");
    assert!(
        peak_uprooted >= 3,
        "kelp is never uprooted (peak {peak_uprooted}) — the reef does not exercise the tear-off"
    );
}

#[test]
fn reef_persists_and_turns_over() {
    let config = SimConfig::from_ron_file("scenarios/examples/20_reef.ron").expect("reef loads");
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
