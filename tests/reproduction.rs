//! Driver of **energy conservation at reproduction** (review #1).
//!
//! `ecology::reproduce` deducts `offspring_energy` from the parent and gives it to
//! the child: energy must be *conserved*, never created. But the reproduction
//! threshold and cost are two genes that drift independently — nothing guarantees
//! `threshold >= cost`. The `reserve >= offspring_energy` guard makes conservation
//! **unconditional**; these tests run the *real* `SimPlugin` (the same as the
//! binaries) and check that no energy appears, in the normal regime as in the
//! pathological case (cost > reserve) that the guard must neutralize.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Reserve};
use teemlab::{SimConfig, SimPlugin};

/// A *pure* reproduction world: a single agent, no metabolism nor food nor
/// interaction → the only thing that can move energy is reproduction. We thus
/// isolate the tested invariant. The capacity, threshold and reproduction cost are
/// parameterized (genes of the single archetype).
fn repro_world(reserve_max: f32, threshold: f32, offspring: f32) -> SimConfig {
    use teemlab::brain::BrainKind;
    use teemlab::config::{Archetype, Mutability};
    use teemlab::genotype::Genotype;
    let genotype = Genotype {
        reproduction_threshold: threshold,
        offspring_energy: offspring,
        mutation_rate: 0.0, // stable genes: we reason on exact values.
        base_metabolism: 0.0,
        move_cost: 0.0,
        // No drain at all (inert-world shortcut) so the reserve evolves by reproduction
        // only — the "living" Genotype::default now prices these, so pin them to 0.
        agility_cost: 0.0,
        brain_cost: 0.0,
        ..Genotype::default()
    };
    SimConfig {
        archetypes: vec![Archetype {
            name: "Agent".into(),
            color: Archetype::default_color(0),
            count: 1,
            radius: 8.0,
            reserve_max,
            genotype,
            brain: BrainKind::default(),
            mutable: Mutability::default(),
            source: None,
            captured_brain: None,
            captured_from: None,
        }],
        relations: Vec::new(),
        seed: 0x5EED,
        ..SimConfig::default()
    }
}

/// A manual single-stepping app (cf. the other drivers): one `update()` = one fixed tick.
fn stepping_app(config: SimConfig) -> App {
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config));
    // Avian inserts some resources in finish()/cleanup(): to be triggered ourselves
    // when we pump the loop by hand.
    app.finish();
    app.cleanup();
    app
}

/// Living population + total energy in reserve, at a given instant.
fn population_and_energy(app: &mut App) -> (usize, f32) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Reserve, With<Agent>>();
    let mut n = 0;
    let mut total = 0.0;
    for r in q.iter(world) {
        n += 1;
        total += r.current;
    }
    (n, total)
}

/// Normal regime (`threshold >= cost`, like the shipped scenarios): the founder,
/// born at full reserve, reproduces **once**; energy passes from parent to child
/// without creating anything, and the population stabilizes at two.
#[test]
fn reproduction_conserves_energy_in_the_normal_regime() {
    // reachable threshold (= starting reserve), cost < threshold: healthy regime.
    let config = repro_world(120.0, 95.0, 45.0);
    let initial = 120.0; // the single founder is born full.
    let mut app = stepping_app(config);

    // One sim second: more than enough for the (single) reproduction.
    for _ in 0..64 {
        app.update();
        let (_, energy) = population_and_energy(&mut app);
        assert!(
            energy <= initial + 1e-3,
            "the total energy must never exceed the initial input ({initial}), seen: {energy}"
        );
    }

    let (population, energy) = population_and_energy(&mut app);
    assert_eq!(population, 2, "the founder reproduced once");
    assert!(
        (energy - initial).abs() < 1e-3,
        "energy conserved: {energy} ≈ {initial}"
    );
}

/// Pathological case the guard neutralizes: `offspring_energy > reserve`. Without
/// the guard, the parent would pay more than it has (negative reserve → death), but
/// the child would carry the full `offspring_energy` → energy created. With the
/// guard, reproduction is simply refused: population and energy stay frozen.
#[test]
fn reproduction_is_refused_when_offspring_costs_more_than_reserve() {
    // threshold crossed (starting reserve = 50), cost > reserve: impossible to pay.
    let config = repro_world(50.0, 40.0, 80.0);
    let initial = 50.0;
    let mut app = stepping_app(config);

    for _ in 0..64 {
        app.update();
        let (population, energy) = population_and_energy(&mut app);
        // Never a child (unpayable cost), hence never any energy created.
        assert_eq!(
            population, 1,
            "no reproduction: the cost exceeds the reserve"
        );
        assert!(
            (energy - initial).abs() < 1e-3,
            "energy frozen at {initial} (nothing created, nothing made negative): {energy}"
        );
    }
}
