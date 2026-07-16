//! Affect verb — a component's local concentration EDITS an agent's [`Reserve`]
//! (`nutrients::affect_agents`): `affect < 0` drains it (a **toxin**), `affect > 0` feeds
//! it (a **boon**), `affect == 0` leaves it (Law 11 — a toxin and a nutrient differ only
//! by the verb pointed at the field). This is a **direct, deterministic** proof of the
//! mechanism, decoupled from any example scenario: a single Source floods the small arena
//! with one component, a fixed cohort of sessile agents sits in it, and we read their
//! reserves back after a fixed horizon.
//!
//! It stands in for the retired `06_signals` toxin scenario: a *robust inter-species*
//! toxin (one species poisoning another) is deferred — the current engine can only give a
//! stationary, co-located victim if both are producers, and producers compete for the one
//! hardcoded metabolic nutrient (`metabolize` → `take(0)`) and so spatially segregate; a
//! mobile victim self-selects out of the toxic patches at no cost. The affect *verb* itself
//! works and is kept covered here for when we return to it (see the Phase C "signals" note
//! in ROADMAP §0).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Reserve};

mod common;

const HORIZON: usize = 300;

/// A minimal world: a central Source floods component 0 across the arena (high diffusion),
/// and 40 sessile agents are `affect`-ed by it at `affect`. A gentle size-independent
/// maintenance drain (`size_exponent 0`) makes the baseline reserve ebb from full, so the
/// boon has room to show above it; no reproduction (huge threshold) and no death within the
/// horizon, so the cohort is fixed and the ONLY thing else moving the reserve is `affect`.
fn scenario(affect: f32) -> SimConfig {
    let ron = format!(
        "(
            tick_hz: 64.0,
            arena_half_extent: 160.0,
            archetypes: [(
                name: \"Subject\", color: (0.6, 0.6, 0.6), count: 40, radius: 5.0,
                reserve_max: 1000.0,
                genotype: (max_speed: 0.0, vision_range: 20.0, vision_rays: 1.0,
                    reproduction_threshold: 100000.0, offspring_energy: 10.0,
                    mutation_rate: 0.0, photosynthesis: 0.0, seed_dispersal: 0.0),
                brain: Sessile,
                mutable: (max_speed: false, agility: false, vision_range: false, vision_fov: false,
                    reproduction_threshold: false, offspring_energy: false, mutation_rate: false,
                    vision_rays: false, photosynthesis: false, seed_dispersal: false,
                    brain_cost: false, act_cost: false),
            )],
            cost_law: (size_exponent: 0.0, maintenance: 20.0, locomotion: 0.0, maneuver: 0.0, metabolic_cost: 0.0),
            field_resolution: 64,
            components: [ (name: \"Haze\", diffusion: 0.5, decay: 0.05) ],
            sources: [ (pos: (0.0, 0.0), component: 0, rate: 3000.0, color: (1.0, 0.3, 0.3), radius: 20.0) ],
            field_relations: [ (species: 0, component: 0, affect: {affect}) ],
            seed: 7,
        )"
    );
    SimConfig::from_ron_str(&ron).expect("valid affect scenario")
}

/// Total reserve of the living cohort after [`HORIZON`] ticks, and the head-count (to prove
/// the cohort is fixed — the contrast is a reserve edit, not a population artefact).
fn reserve_after(affect: f32) -> (f32, usize) {
    let mut app = common::stepping_app(&scenario(affect));
    for _ in 0..HORIZON {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Reserve, With<Agent>>();
    let mut total = 0.0f32;
    let mut n = 0usize;
    for r in q.iter(world) {
        total += r.current;
        n += 1;
    }
    (total, n)
}

#[test]
fn affect_drains_feeds_or_leaves_the_reserve() {
    let (toxin, n_tox) = reserve_after(-10.0);
    let (neutral, n_neu) = reserve_after(0.0);
    let (boon, n_boon) = reserve_after(10.0);

    eprintln!(
        "  toxin={toxin:.0} ({n_tox})  neutral={neutral:.0} ({n_neu})  boon={boon:.0} ({n_boon})"
    );

    // The cohort is fixed (nobody bred, nobody starved within the horizon) — so the three
    // worlds compare the SAME 40 agents, and the only difference is the affect verb.
    assert!(
        n_tox == 40 && n_neu == 40 && n_boon == 40,
        "cohort not fixed (toxin {n_tox}, neutral {n_neu}, boon {n_boon}) — retune the horizon"
    );
    // The verb edits the reserve directionally: a toxin (`affect < 0`) drains it below the
    // untouched control, a boon (`affect > 0`) holds it above (clamped at the reserve max).
    assert!(
        toxin < neutral,
        "a toxin (affect < 0) must drain the reserve below the control (toxin {toxin:.0} vs neutral {neutral:.0})"
    );
    assert!(
        boon > neutral,
        "a boon (affect > 0) must hold the reserve above the control (boon {boon:.0} vs neutral {neutral:.0})"
    );
}
