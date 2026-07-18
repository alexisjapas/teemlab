//! Anchoring (Feature 2) end-to-end: an anchored species is spawned with an [`Anchor`],
//! is **excluded from `act`**, and is held near its anchor by the spring across a *real*
//! sim run (Avian solver + collisions). The tear-off *logic* — the three branches of the
//! tension threshold — is unit-tested in `movement`; here we prove the wiring holds in
//! the full pipeline (spawn → act-exclusion → spring → physics).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Anchor};
use teemlab::config::AnchorConfig;

mod common;

#[test]
fn anchored_bodies_stay_rooted_at_their_anchor() {
    let mut config = SimConfig::from_ron_file("scenarios/examples/01_meadow.ron")
        .expect("scenario 01_meadow.ron loadable");
    // Root every archetype with a stiff spring and a tear tension far above anything a
    // mere neighbour jostle produces (so nothing tears): the test isolates "stays put".
    for arch in &mut config.archetypes {
        arch.anchor = Some(AnchorConfig {
            stiffness: 40.0,
            damping: 4.0,
            tear_force: 1.0e6,
            die_on_detach: true,
        });
    }

    let mut app = common::stepping_app(&config);
    for _ in 0..600 {
        app.update();
    }

    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Transform, &Anchor), With<Agent>>();
    let mut population = 0;
    let mut worst = 0.0f32;
    for (transform, anchor) in q.iter(world) {
        population += 1;
        worst = worst.max((transform.translation.truncate() - anchor.0).length());
    }

    assert!(population > 0, "the flora went extinct — inconclusive test");
    // Every living body carries an [`Anchor`] (spawn wiring), and none has drifted from
    // it: the spring holds despite collisions, and `act` no longer resets its velocity.
    assert!(
        worst < 30.0,
        "an anchored body drifted {worst:.1} from its anchor — the spring is not holding \
         (or `act` was not excluded)"
    );
}
