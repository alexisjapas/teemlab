//! Solid sources — **rocks / obstacles** (the tangible half of the substrate-feature
//! category). A `Source { solid: true, .. }` spawns a **static circle collider**, so it
//! blocks dynamic bodies and carves spatial refugia / winding zones; `solid: false` (the
//! historical vent) stays intangible. It remains a non-`Agent` entity either way, so the
//! life machinery ignores it (Law 11 untouched).
//!
//! We prove tangibility **behaviorally**, with a falsifiable contrast: a single central
//! feature (emitting nothing — a *pure* obstacle), and the closest any body's center ever
//! gets to it over a run. Solid ⇒ the collider keeps every center out of the disc;
//! intangible ⇒ wandering bodies cross straight through it.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::Agent;
use teemlab::config::Source;

mod common;

const ROCK_R: f32 = 40.0;

/// The grazing scenario (mobile foragers + sessile food) with a single central feature
/// of radius [`ROCK_R`] that **emits nothing** (`rate 0`) — so the only thing that can
/// keep a body out of its disc is the collider, present iff `solid`.
fn evolution_with_central_rock(solid: bool) -> SimConfig {
    let mut config = SimConfig::from_ron_file("scenarios/examples/04_grazing.ron")
        .expect("scenario 04_grazing.ron loadable");
    config.sources = vec![Source {
        pos: [0.0, 0.0],
        component: 0,
        rate: 0.0,
        color: [0.5, 0.5, 0.55],
        radius: ROCK_R,
        solid,
    }];
    config
}

/// Smallest distance from the origin reached by **any** agent over the whole run (the
/// running minimum, so a single visit to the disc is caught even if the visitor later
/// leaves or dies).
fn min_center_distance_over_run(solid: bool, ticks: usize) -> (f32, usize) {
    let config = evolution_with_central_rock(solid);
    let mut app = common::stepping_app(&config);
    let mut closest = f32::INFINITY;
    for _ in 0..ticks {
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Transform, With<Agent>>();
        for t in q.iter(world) {
            closest = closest.min(t.translation.truncate().length());
        }
    }
    let world = app.world_mut();
    let population = world
        .query_filtered::<Entity, With<Agent>>()
        .iter(world)
        .count();
    (closest, population)
}

#[test]
fn solid_rock_excludes_bodies_from_its_disc() {
    // Solid: the static collider (circle vs circle) keeps every body center at ≥
    // rock_r + agent_r apart, so — barring a couple of pixels of solver penetration —
    // no center ever enters the disc of radius `ROCK_R`.
    let (solid_closest, solid_pop) = min_center_distance_over_run(true, 1200);
    // Intangible control: the *same* world, feature emitting nothing and carrying no
    // collider — wandering bodies cross the origin freely.
    let (porous_closest, porous_pop) = min_center_distance_over_run(false, 1200);

    assert!(
        solid_pop > 0 && porous_pop > 0,
        "a population went extinct — inconclusive (solid {solid_pop}, porous {porous_pop})"
    );
    assert!(
        solid_closest >= ROCK_R - 2.0,
        "a body entered the solid rock's disc (closest center {solid_closest:.1} < {ROCK_R})"
    );
    assert!(
        porous_closest < ROCK_R,
        "control failed: no body reached the disc without a collider \
         (closest center {porous_closest:.1} ≥ {ROCK_R}) — the test cannot falsify"
    );
}
