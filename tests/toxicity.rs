//! Toxicity — self-poisoning as an endogenous density regulator (`07_signals.ron`).
//!
//! A photosynthetic bloom (species 0) EMITS a toxin and is HARMED by it (`emit` + `affect
//! < 0` on the same pair, component 1). As the colony grows and clusters, the toxin
//! accumulates faster than it decays, so the dose it inflicts on itself rises with density
//! → it holds its own standing crop down. The falsifiable contrast: the SAME world with the
//! toxin emission turned off grows to roughly TWICE the standing crop — so the difference is
//! caused by the endogenous emission, not the ecology. The destabiliser pole of
//! `docs/persistent-ecosystems.md` §1, config-only on the emission substrate.

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

/// **Self-poisoning is an endogenous density regulator.** The toxin-emitting bloom is held
/// well below the standing crop the SAME world reaches with the emission turned off (nothing
/// else changed): the toxin, accumulating with density, caps the colony. The falsifiable
/// contrast pins the difference to the endogenous emission, not the ecology — the
/// destabiliser pole of §1, config-only on the component-emission substrate (a toxin is an
/// `emit` + an `affect < 0`, no new mechanism). It suppresses rather than extinguishes: the
/// dose is density-dependent, so it eases as the crop thins, settling at a lower level.
#[test]
fn self_poisoning_suppresses_below_a_clean_control() {
    const SEEDS: [u64; 3] = [1, 2, 3];
    const HORIZON: usize = 150;
    for seed in SEEDS {
        let toxic = emitters_after(seed, true, HORIZON);
        let clean = emitters_after(seed, false, HORIZON);
        // The emission clearly caps the bloom: at least a third below the clean control.
        assert!(
            (toxic as f32) < 0.66 * clean as f32,
            "seed {seed}: the toxin must hold the bloom well below the clean control \
             (toxic {toxic} vs clean {clean} at {HORIZON}s)"
        );
        // Both are alive (the toxin regulates, it does not sterilise; the control thrives).
        assert!(
            toxic > 20 && clean > 120,
            "seed {seed}: both populations must be alive (toxic {toxic}, clean {clean})"
        );
    }
}
