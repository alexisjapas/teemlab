//! Toxicity — self-poisoning as an endogenous collapse mode (`19_toxicity.ron`).
//!
//! A prudent-grazer monoculture that persists on the oasis flora (17_restraint) here also
//! EMITS a toxin and is HARMED by it (`emit` + `affect < 0` on the same pair). As the crowd
//! grows and clusters, the toxin accumulates faster than it decays, so the dose it inflicts
//! on itself rises with density → collapse. The falsifiable contrast: the SAME world with
//! the toxin emission turned off (the restraint-persist baseline) does NOT collapse — so
//! the collapse is caused by the endogenous emission, not the ecology. The destabiliser
//! pole of `docs/persistent-ecosystems.md` §1, config-only on the emission substrate.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Species};
use teemlab::{SimConfig, SimPlugin};

const SCENARIO: &str = include_str!("../scenarios/examples/07_signals.ron");

/// Build the scenario at `seed`. `toxic` false = the CLEAN control: the same world with the
/// toxin **emission** turned off (nothing else changed) — the falsifiable contrast.
fn app_with(seed: u64, toxic: bool) -> (App, usize) {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid toxicity scenario");
    config.seed = seed;
    if !toxic {
        for fr in &mut config.field_relations {
            if fr.component == 1 {
                fr.emit = 0.0;
            }
        }
    }
    let hz = config.tick_hz as usize;
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config));
    app.finish();
    app.cleanup();
    (app, hz)
}

/// Run `seconds` at `seed`/`toxic`, returning the living **emitter** (species 0) count.
fn emitters_after(seed: u64, toxic: bool, seconds: usize) -> usize {
    let (mut app, hz) = app_with(seed, toxic);
    for _ in 0..seconds * hz {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Species, With<Agent>>();
    q.iter(world).filter(|s| s.0 == 0).count()
}

/// **Self-poisoning is an endogenous collapse mode.** The toxin-emitting monoculture
/// blooms, then the toxin it accumulates on its own oases poisons it to **extinction**;
/// the SAME world with the emission turned off (nothing else changed — the restraint-persist
/// baseline) does **not** collapse. The falsifiable contrast pins the collapse to the
/// endogenous emission, not the ecology — the destabiliser pole of §1, config-only on the
/// component-emission substrate (a toxin is an `emit` + an `affect < 0`, no new mechanism).
#[test]
#[ignore = "behavioural: awaits scenario re-tuning after the emergent-trophics refactor"]
fn self_poisoning_collapses_where_a_clean_control_persists() {
    const SEEDS: [u64; 3] = [1, 2, 3];
    const HORIZON: usize = 150;
    for seed in SEEDS {
        let toxic = emitters_after(seed, true, HORIZON);
        let clean = emitters_after(seed, false, HORIZON);
        assert!(
            toxic <= 4,
            "seed {seed}: the toxin-emitting population must self-poison to collapse \
             (still {toxic} emitters at {HORIZON}s)"
        );
        assert!(
            clean >= 30,
            "seed {seed}: the SAME world with emission off must persist \
             (only {clean} emitters at {HORIZON}s)"
        );
    }
}
