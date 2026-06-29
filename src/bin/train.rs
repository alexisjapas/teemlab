//! `train` — generate the trained-MLP showcase from a training scenario.
//!
//! Step 3's *generator* in the MLP learning story (mlp_brain = naive baseline;
//! mlp_train = the training ground; mlp_evolved = the trained variant in action). It
//! runs the training scenario **headless**, captures the best-evolved MLP (the highest
//! generation, tie-broken by current reserve), and writes:
//!   - `species/examples/mlp_trained.ron` — the reusable catalog **variant** (the
//!     evolved genotype + the frozen `captured_brain`), as if exported from the
//!     inspector's "Save as variant";
//!   - `scenarios/examples/09_mlp_evolved.ron` — a **self-contained** showcase: the
//!     trained MLP vs a WANDER control on the same oasis flora (import = copy, so the
//!     trained brain is embedded inline).
//!
//! A one-off generator, not part of the test suite. Re-run to regenerate the artifacts.
//! Usage: `cargo run --bin train -- [train_scenario.ron] [ticks] [seed]`.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::SimPlugin;
use teemlab::brain::{Brain, BrainKind};
use teemlab::components::{Agent, Generation, Reserve, Species};
use teemlab::config::{Relation, SimConfig, SpeciesEntry};
use teemlab::genotype::Genotype;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scenario = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "scenarios/examples/08_mlp_train.ron".into());
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6000);
    let seed: Option<u64> = args.get(3).and_then(|s| s.parse().ok());

    let mut config = SimConfig::from_ron_file(&scenario).expect("load training scenario");
    if let Some(s) = seed {
        config.seed = s;
    }
    let base = config.archetypes[0].clone();
    let flora = config.archetypes[1].clone();

    // Run the training ground headless (one update() = one fixed tick).
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config.clone()));
    app.finish();
    app.cleanup();
    for _ in 0..ticks {
        app.update();
    }

    // Capture the best-evolved MLP: highest generation, tie-broken by reserve.
    let world = app.world_mut();
    let mut q =
        world.query_filtered::<(&Species, &Generation, &Reserve, &Genotype, &Brain), With<Agent>>();
    let mut best: Option<(u32, f32, Genotype, Brain)> = None;
    for (species, generation, reserve, genotype, brain) in q.iter(world) {
        if species.0 != 0 || !matches!(brain, Brain::Mlp(_)) {
            continue;
        }
        let key = (generation.0, reserve.current);
        if best.as_ref().is_none_or(|b| (b.0, b.1) < key) {
            best = Some((generation.0, reserve.current, *genotype, brain.clone()));
        }
    }
    let (generation, reserve, genotype, brain) = best.expect(
        "no MLP survived the training run — re-run with a different seed or fewer ticks \
         (the population fades on living food; capture before it does)",
    );
    println!("captured MLP: generation {generation}, reserve {reserve:.1}");

    // The evolved archetype (frozen brain). Used as both the catalog variant and the
    // showcase's species 0.
    let captured = base.capture(genotype, brain, generation);

    // (1) Catalog variant.
    let entry = SpeciesEntry::variant(
        captured.clone(),
        base.name.clone(),
        format!("trained-{generation}"),
    );
    std::fs::write(
        "species/examples/mlp_trained.ron",
        entry.to_ron_string().expect("serialize variant"),
    )
    .expect("write mlp_trained.ron");

    // (2) Self-contained showcase: trained MLP (sp0) vs a WANDER control (sp1) — same
    // evolved body, naive brain — on the oasis flora (sp2). Only the brain differs.
    let mut mlp = captured;
    mlp.name = "Trained MLP".into();
    mlp.count = 14;
    let mut wander = mlp.clone();
    wander.name = "Wanderer".into();
    wander.color = [0.95, 0.8, 0.3];
    wander.brain = BrainKind::Wander { turn_rate: 0.25 };
    wander.captured_brain = None;
    wander.captured_from = None;

    let mut evolved = config.clone();
    evolved.archetypes = vec![mlp, wander, flora];
    evolved.relations = vec![
        Relation {
            actor: 0,
            target: 2,
            transfer: true,
            rate: 45.0,
            range: 16.0,
        },
        Relation {
            actor: 1,
            target: 2,
            transfer: true,
            rate: 45.0,
            range: 16.0,
        },
    ];
    let evolved_header = "\
// MLP evolved — a TRAINED learned brain in action (the payoff of the learning story:
// mlp_brain = naive baseline, mlp_train = the training ground, this = the trained
// variant reused). Species 0 carries a frozen `captured_brain` evolved in mlp_train;
// it forages the oasis flora on PAR with the wander control — a far cry from the naive
// MLP of mlp_brain, which the wanderer out-forages. (Parity, not domination: see
// ROADMAP — neuroevolution in the living-food window plateaus at parity.)
// GENERATED by `cargo run --bin train` — do not hand-edit; re-run to regenerate.\n";
    std::fs::write(
        "scenarios/examples/09_mlp_evolved.ron",
        format!(
            "{evolved_header}{}",
            evolved.to_ron_string().expect("serialize evolved scenario")
        ),
    )
    .expect("write mlp_evolved.ron");

    // (3) The NAIVE baseline: the same showcase, but species 0 is a FRESH MLP (random
    // weights from the seed, no `captured_brain`) — what the trained variant started
    // from. Same economy as mlp_evolved, so the only difference is naive vs trained.
    let mut naive = evolved.clone();
    naive.archetypes[0].name = "MLP".into();
    naive.archetypes[0].color = [0.8, 0.45, 1.0];
    naive.archetypes[0].captured_brain = None;
    naive.archetypes[0].captured_from = None;
    let naive_header = "\
// MLP brain — the NAIVE learned brain: a from-random MLP vs a wander control on the
// oasis flora (step 1 of the MLP learning story). With random weights the MLP forages
// no better than chance — the wanderer out-forages it. Train it (mlp_train) and reuse
// the evolved variant (mlp_evolved) to see it reach parity. Inspect the MLP network in
// action (activations) in the inspector.
// GENERATED by `cargo run --bin train` — do not hand-edit; re-run to regenerate.\n";
    std::fs::write(
        "scenarios/examples/07_mlp_brain.ron",
        format!(
            "{naive_header}{}",
            naive.to_ron_string().expect("serialize naive scenario")
        ),
    )
    .expect("write mlp_brain.ron");

    println!(
        "wrote species/examples/mlp_trained.ron + scenarios/examples/09_mlp_evolved.ron + \
         scenarios/examples/07_mlp_brain.ron"
    );
}
