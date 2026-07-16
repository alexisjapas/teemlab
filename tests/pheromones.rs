//! Pheromones — component EMISSION + SENSING (Phase 3 of component emission).
//!
//! The bloom of `scenarios/examples/06_signals.ron` emits a diffusing/decaying "Scent"
//! component (a `FieldRelation` `emit`) and senses its local concentration (a `sense` input
//! channel → [`teemlab::components::Perception::field_state`]). This driver checks the
//! SUBSTRATE works end-to-end: the population persists (the honest §7 target — emergent
//! *communication* is not claimed; the falsifiable wiring proof is the unit
//! `brain::tests::mlp_reads_field_state_channel`), AND the sensed field is actually written
//! (the agent→environment emission reaches the very field the `sense` verb reads back).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Species};
use teemlab::nutrients::Fields;
use teemlab::{SimConfig, SimPlugin};

const SCENARIO: &str = include_str!("../scenarios/examples/06_signals.ron");

#[test]
#[ignore = "06_signals redesign pending (inter-species toxin) + metabolism re-tune"]
fn pheromone_substrate_runs_and_writes() {
    const SEEDS: [u64; 3] = [0x00C0_FFEE, 0x1234, 0xBEEF];
    const SECONDS: usize = 45;

    for seed in SEEDS {
        let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid pheromone scenario");
        config.seed = seed;
        let tick_hz = config.tick_hz as usize;
        // Guard the point of the scenario: a species that both emits AND senses a component —
        // and the SENSED one (the pheromone) is the field we then prove is written.
        assert!(
            config.field_relations.iter().any(|f| f.emit > 0.0),
            "the showcase must have an emitter"
        );
        let scent = config
            .field_relations
            .iter()
            .find(|f| f.sense && f.emit > 0.0)
            .map(|f| f.component)
            .expect("the showcase must emit AND sense one component (the pheromone)");

        let mut app = App::new();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / config.tick_hz,
        )));
        app.add_plugins(MinimalPlugins);
        app.add_plugins(SimPlugin::new(config));
        app.finish();
        app.cleanup();
        for _ in 0..SECONDS * tick_hz {
            app.update();
        }

        let population = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Species, With<Agent>>();
            q.iter(world).filter(|s| s.0 == 0).count()
        };
        assert!(
            population > 0,
            "seed {seed:#x}: the population collapsed at {SECONDS}s"
        );

        // The sensed pheromone field must hold concentration — the emission actually wrote
        // into the very field the `sense` verb reads back (agent → field, the point of Phase 3).
        let pheromone = app.world().resource::<Fields>()[scent].total();
        assert!(
            pheromone > 0.0,
            "seed {seed:#x}: the pheromone field is empty — emission did not write"
        );
    }
}
