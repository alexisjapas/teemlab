//! Pheromones — component EMISSION + SENSING (Phase 3 of component emission).
//!
//! The forager MLPs of `scenarios/examples/18_pheromones.ron` emit a diffusing/decaying
//! "Pheromone" component (a `FieldRelation` `emit`) and sense its local concentration (a
//! `sense` input channel → [`teemlab::components::Perception::field_state`]). This driver
//! checks the SUBSTRATE works end-to-end: the population persists on the oasis (the honest
//! §7 target — emergent *communication* is not claimed; the falsifiable wiring proof is
//! the unit `brain::tests::mlp_reads_field_state_channel`), AND the pheromone field is
//! actually written (the agent→environment emission reaches the field).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Species};
use teemlab::nutrients::Fields;
use teemlab::{SimConfig, SimPlugin};

const SCENARIO: &str = include_str!("../scenarios/examples/18_pheromones.ron");

#[test]
fn pheromone_substrate_runs_and_writes() {
    const SEEDS: [u64; 3] = [0x00C0_FFEE, 0x1234, 0xBEEF];
    const SECONDS: usize = 45;

    for seed in SEEDS {
        let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid pheromone scenario");
        config.seed = seed;
        let tick_hz = config.tick_hz as usize;
        // Guard the point of the scenario: a forager that both emits AND senses a component.
        assert!(
            config.field_relations.iter().any(|f| f.emit > 0.0),
            "the showcase must have an emitter"
        );
        assert!(
            config.field_relations.iter().any(|f| f.sense),
            "the showcase must have a senser"
        );

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

        let (mlp, flora) = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Species, With<Agent>>();
            let (mut m, mut f) = (0usize, 0usize);
            for s in q.iter(world) {
                match s.0 {
                    0 => m += 1,
                    1 => f += 1,
                    _ => {}
                }
            }
            (m, f)
        };
        assert!(
            mlp > 0,
            "seed {seed:#x}: the forager population collapsed at {SECONDS}s"
        );
        assert!(
            flora > 0,
            "seed {seed:#x}: the flora collapsed at {SECONDS}s"
        );

        // The pheromone field (component 1) must hold concentration — the emission
        // actually wrote into the environment (agent → field, the point of Phase 3).
        let pheromone = app.world().resource::<Fields>()[1].total();
        assert!(
            pheromone > 0.0,
            "seed {seed:#x}: the pheromone field is empty — emission did not write"
        );
    }
}
