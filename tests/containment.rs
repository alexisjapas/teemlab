//! Containment: no agent must leave the arena.
//!
//! Guardrail of the **half-space** walls (infinite planes): we run a real sim
//! world — movement *and* reproduction (which places newborns near the edges) —
//! and check that no agent crosses the boundary. This test catches the two
//! possible regressions: a return to thin walls (where an agent can tunnel or be
//! born outside) **and** an inverted normal (which would conversely eject everyone
//! *out of* the arena).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::Agent;

mod common;

#[test]
fn agents_stay_within_arena() {
    // The evolution scenario: it moves at full speed and reproduces, so it presses
    // the edges in every useful way.
    let config = SimConfig::from_ron_file("scenarios/examples/evolution.ron")
        .expect("scenario evolution.ron loadable");

    // Each `update()` advances by exactly one fixed tick (cf. `common::stepping_app`).
    let mut app = common::stepping_app(&config);

    // ~30 s of simulated time: plenty to reach the walls and chain generations near
    // the edges.
    for _ in 0..2000 {
        app.update();
    }

    let h = config.arena_half_extent;
    // Tolerance: an agent may sink a few pixels into the half-space before the
    // solver pushes it back. A real escape, by contrast, is measured in
    // tens/hundreds of units — so this tight margin suffices to detect it.
    let margin = config.agent_radius_of(0) + 2.0;

    let world = app.world_mut();
    let mut query = world.query_filtered::<(Entity, &Transform), With<Agent>>();
    let mut escaped = Vec::new();
    for (entity, transform) in query.iter(world) {
        let p = transform.translation.truncate();
        if p.x.abs() > h + margin || p.y.abs() > h + margin {
            escaped.push((entity, p));
        }
    }
    let population = query.iter(world).count();
    assert!(
        population > 0,
        "the population went extinct — inconclusive test"
    );
    assert!(
        escaped.is_empty(),
        "{} agent(s) outside the arena (|x|,|y| ≤ {:.0} expected): {:?}",
        escaped.len(),
        h + margin,
        escaped,
    );
}
