//! Deliberate eating — the interaction primitive becomes a **brain-driven, costed**
//! action (ROADMAP §9 "Eating/attacking as a deliberate, costed action"; SIM Law 8).
//!
//! Until now `interaction::interact` fired on *every* actor with a valid target in
//! range — predation was a reflex. Now the brain decides (an `Action::act` output,
//! gated in `interact`) and holding the intent **costs** energy (gene `act_cost`,
//! charged in `metabolize`, SIM Law 7) — the substrate that makes behavioural
//! **restraint** expressible (`docs/persistent-ecosystems.md` §2). The hand-written
//! brains hold `act = 1.0` (reflex) so every non-MLP scenario stays byte-identical;
//! these drivers falsify the two new halves in isolation, on a deterministic world.
//!
//! To flip the intent deterministically (no hand-written brain outputs `≤ 0`), a
//! one-off system forces every agent's `Action::act` to `0` **after** `movement::act`
//! and **before** `interaction::interact`/`metabolize` — the "gated" world — versus the
//! reflex world where the brain's `1.0` stands.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::{Action, Agent, Reserve, Species};
use teemlab::config::{Archetype, Mutability, Relation};
use teemlab::genotype::Genotype;
use teemlab::spawn::spawn_agent;

mod common;

/// A genotype inert on every axis but the one under test — it neither moves,
/// metabolizes, reproduces nor mutates (nor even casts vision: `vision_rays = 0`, so
/// [`teemlab::components::Vision::metabolic_cost`] is zero) — save the `act_cost`
/// passed in. So the *only* energy change is the act cost, and the *only* reserve
/// change on a target is what an actor eats.
fn inert_genotype(act_cost: f32) -> Genotype {
    Genotype {
        max_speed: 0.0, // immobile: bodies stay exactly where spawned
        base_metabolism: 0.0,
        move_cost: 0.0,
        agility_cost: 0.0,
        brain_cost: 0.0,
        photosynthesis: 0.0,
        vision_rays: 0.0, // blind → zero vision cost, so act_cost is the sole drain
        reproduction_threshold: 0.0, // does not reproduce
        mutation_rate: 0.0,
        nutrient_absorption: 0.0,
        nutrient_capacity: 0.0,
        offspring_nutrient: 0.0,
        act_cost,
        ..Genotype::default()
    }
}

/// A sessile archetype carrying `genotype` — the reflex brain holds `act = 1.0`, so
/// without the gating system an actor eats and an agent pays its act cost.
fn sessile(name: &str, idx: usize, genotype: Genotype) -> Archetype {
    Archetype {
        name: name.into(),
        color: Archetype::default_color(idx),
        count: 0,
        radius: 8.0,
        reserve_max: 1000.0,
        genotype,
        brain: BrainKind::Sessile,
        mutable: Mutability::default(),
        source: None,
        captured_brain: None,
        captured_from: None,
    }
}

/// The gating system used by the "no intent" runs: force every agent to abstain,
/// **after** the brain has decided (`movement::act`) and **before** the primitive
/// reads the intent (`interaction::interact`) or the cost does (`ecology::metabolize`).
fn force_no_intent(mut q: Query<&mut Action, With<Agent>>) {
    for mut action in &mut q {
        action.act = 0.0;
    }
}

/// Grazes an actor (species 0) on a plant (species 1) for `ticks`, returning the
/// **plant's** remaining reserve. With `gate_off`, the actor's intent is forced to 0
/// each tick (it should eat nothing); otherwise its reflex `1.0` stands (it eats).
fn plant_reserve_after_grazing(gate_off: bool, ticks: usize) -> f32 {
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![
            sessile("Forager", 0, inert_genotype(0.0)),
            sessile("Plant", 1, inert_genotype(0.0)),
        ],
        // Forager (0) eats the plant (1): predation (transfer) at a steady rate, in a
        // generous contact range so they interact from the first tick without movement.
        relations: vec![Relation {
            actor: 0,
            target: 1,
            transfer: true,
            rate: 100.0,
            range: 30.0,
        }],
        ..SimConfig::default()
    };

    let mut app = common::stepping_app(&config);
    if gate_off {
        app.add_systems(
            FixedUpdate,
            force_no_intent
                .after(teemlab::movement::act)
                .before(teemlab::interaction::interact),
        );
    }
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max_of(0),
                0,
            );
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(1),
                Species(1),
                Vec2::new(20.0, 0.0),
                0.0,
                1,
                config.reserve_max_of(1),
                0,
            );
        })
        .expect("one-off spawn");

    for _ in 0..ticks {
        app.update();
    }

    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Reserve), With<Agent>>();
    q.iter(world)
        .find(|(s, _)| s.0 == 1)
        .map(|(_, r)| r.current)
        .expect("the plant still exists")
}

/// **The gate.** An actor eats only when its brain wills it: with the intent held
/// (reflex `1.0`) the plant is grazed; with the intent forced off the plant is
/// **untouched** — the falsifiable proof that `interact` is gated on `Action::act`.
#[test]
fn intent_gates_eating() {
    let start = 1000.0;
    let grazed = plant_reserve_after_grazing(false, 20);
    let spared = plant_reserve_after_grazing(true, 20);

    assert!(
        grazed < start - 10.0,
        "with intent, the actor eats: the plant should have lost reserve (still {grazed:.3})"
    );
    assert!(
        (spared - start).abs() < 1e-3,
        "without intent, the actor abstains: the plant should be untouched (got {spared:.3})"
    );
}

/// Runs one immobile agent (act cost `act_cost`) for `ticks`, returning its remaining
/// reserve. With `gate_off` its intent is forced to 0 (it should pay nothing);
/// otherwise its reflex `1.0` stands (it pays `act_cost` per second).
fn reserve_after_holding(act_cost: f32, gate_off: bool, ticks: usize) -> (f32, f32) {
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![sessile("Eater", 0, inert_genotype(act_cost))],
        relations: vec![],
        ..SimConfig::default()
    };
    let start = config.reserve_max_of(0);

    let mut app = common::stepping_app(&config);
    if gate_off {
        app.add_systems(
            FixedUpdate,
            force_no_intent
                .after(teemlab::movement::act)
                .before(teemlab::ecology::metabolize),
        );
    }
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max_of(0),
                0,
            );
        })
        .expect("one-off spawn");

    for _ in 0..ticks {
        app.update();
    }

    let dt = (1.0 / config.tick_hz) as f32;
    let expected_drop = act_cost * ticks as f32 * dt;

    let world = app.world_mut();
    let mut q = world.query_filtered::<&Reserve, With<Agent>>();
    let reserve = q.iter(world).next().map(|r| r.current).expect("the eater");
    (start - reserve, expected_drop)
}

/// **The cost.** Holding the eat/attack intent bleeds energy at `act_cost` per second
/// (SIM Law 7 — the act is priced): the reserve drops by exactly `act_cost · time`
/// while the intent is held, and by **nothing** when it is not. This is the pressure
/// that makes indiscriminate eating wasteful, so restraint can pay.
#[test]
fn holding_intent_costs_energy() {
    let ticks = 40;
    let cost = 5.0;

    let (drop_held, expected) = reserve_after_holding(cost, false, ticks);
    assert!(
        (drop_held - expected).abs() < 0.2,
        "holding the intent must drain act_cost·time: dropped {drop_held:.3}, expected {expected:.3}"
    );

    let (drop_off, _) = reserve_after_holding(cost, true, ticks);
    assert!(
        drop_off.abs() < 1e-3,
        "with the intent off, act_cost must charge nothing (dropped {drop_off:.3})"
    );
}

/// The **showcase** (`16_deliberate_eating.ron`) is a playable, non-collapsing example:
/// with eating priced (`act_cost > 0`) the MLP population evolves to *gate* its eating
/// (the `act` output) and **persists** on the oasis flora across seeds over the
/// observation window. The honest §7 target — persistence/coexistence over the window,
/// not domination (on living food the long-horizon outcome is Lotka–Volterra).
#[test]
fn showcase_population_persists() {
    const SCENARIO: &str = include_str!("../scenarios/examples/16_deliberate_eating.ron");
    const SEEDS: [u64; 3] = [0x00C0_FFEE, 0x1234, 0xBEEF];
    const SECONDS: usize = 60;

    for seed in SEEDS {
        let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid showcase scenario");
        config.seed = seed;
        let tick_hz = config.tick_hz as usize;
        // The scenario is vacuous if eating is free — the whole point is a priced act.
        assert!(
            config.archetypes[0].genotype.act_cost > 0.0,
            "the showcase must price eating (act_cost > 0)"
        );

        let mut app = common::stepping_app(&config);
        for _ in 0..SECONDS {
            for _ in 0..tick_hz {
                app.update();
            }
        }

        let world = app.world_mut();
        let mut q = world.query_filtered::<&Species, With<Agent>>();
        let (mut mlp, mut flora) = (0usize, 0usize);
        for s in q.iter(world) {
            match s.0 {
                0 => mlp += 1,
                1 => flora += 1,
                _ => {}
            }
        }
        assert!(
            mlp > 0,
            "seed {seed:#x}: the deliberate-eater population collapsed (extinct at {SECONDS}s)"
        );
        assert!(
            flora > 0,
            "seed {seed:#x}: the flora collapsed at {SECONDS}s"
        );
    }
}
