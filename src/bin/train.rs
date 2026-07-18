//! `train` — generate the trained-MLP showcase (`deferred/learning`) from a training scenario.
//!
//! The *generator* in the MLP learning story: it runs a training scenario **headless**,
//! captures the best-evolved MLP seen **over the whole run** (highest generation,
//! tie-broken by current reserve — sampled periodically, so the peak-generation lineage
//! is caught before the living-food population fades, not the dying remnant at the final
//! tick), and writes:
//!   - `species/examples/mlp_trained.ron` — the reusable catalog **variant** (the
//!     evolved genotype + the frozen `captured_brain`), as if exported from the
//!     inspector's "Save as variant";
//!   - `scenarios/deferred/learning.ron` — a **self-contained** showcase: the trained
//!     MLP (species 0) vs a WANDER control (species 1) on the oasis flora (species 2),
//!     the trained brain embedded inline (the diet/absorption relations are remapped to
//!     the 3-species layout).
//!
//! A one-off generator, not part of the test suite. Re-run to regenerate the artifacts.
//! Usage: `cargo run --bin train -- [train_scenario.ron] [ticks] [seed]`.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::SimPlugin;
use teemlab::brain::{Brain, BrainKind};
use teemlab::components::{Agent, Generation, Reserve, Species};
use teemlab::config::{SimConfig, SpeciesEntry};
use teemlab::genotype::Genotype;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scenario = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "scenarios/deferred/breeding.ron".into());
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12000);
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
    // Capture the **best MLP seen over the whole run** — highest generation,
    // tie-broken by reserve — sampled periodically rather than only at the final
    // tick. On living food the population peaks then fades (Lotka–Volterra), so the
    // final-tick survivors are a *dying remnant* (a poor forager to embody); the
    // peak-generation lineage, which existed mid-run, is the strongest evolved
    // brain. Tracking the best-ever lets us run past the peak (to reach a higher
    // generation) without capturing the collapse.
    let mut best: Option<(u32, f32, Genotype, Brain)> = None;
    for tick in 0..ticks {
        app.update();
        // Sampling every 50 ticks (≈ 0.8 s) is ample — a generation spans many ticks
        // — and keeps the scan cost negligible against the sim.
        if tick % 50 == 0 {
            scan_best(app.world_mut(), &mut best);
        }
    }
    scan_best(app.world_mut(), &mut best);
    let (generation, reserve, genotype, brain) = best.expect(
        "no MLP ever lived during the training run — re-run with a different seed \
         (the population must survive long enough to evolve a forager)",
    );
    println!("captured MLP: generation {generation}, reserve {reserve:.1}");

    // Quality floor. A *fluke* early-generation brain (few rounds of selection) is a weak
    // forager: rather than silently commit one on a regeneration, fail **loudly** so the
    // seed is re-picked. This guards the generator's output quality, not the sim.
    const MIN_CAPTURE_GENERATION: u32 = 3;
    assert!(
        generation >= MIN_CAPTURE_GENERATION,
        "captured generation {generation} < floor {MIN_CAPTURE_GENERATION}: the population \
         did not evolve far enough for a robust forager — re-run with a different seed / more \
         ticks (do NOT commit this weak capture; it will flake tests/mlp)",
    );

    // The evolved archetype (frozen brain). Used as both the catalog variant and the
    // showcase's species 0.
    let captured = base.capture(genotype, brain, generation);

    // (1) Catalog variant (the reusable, evolved species — as if exported from the
    // inspector's "Save as variant"). A nice library artifact independent of the scenario.
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

    // (2) Self-contained showcase `deferred/learning.ron`: the TRAINED MLP (sp0, a frozen
    // `captured_brain`) vs a WANDER control (sp1) — same evolved body, naive brain — on
    // the oasis flora (sp2). Only the brain differs, so the scene is the payoff of the
    // learning story: the evolved network forages on par with (or better than) the
    // coin-flip baseline it started level with.
    let mut mlp = captured;
    mlp.name = "Trained MLP".into();
    mlp.count = 6;
    let mut wander = mlp.clone();
    wander.name = "Wanderer".into();
    wander.color = [0.95, 0.8, 0.3];
    wander.brain = BrainKind::Wander { turn_rate: 0.25 };
    wander.captured_brain = None;
    wander.captured_from = None;

    let mut evolved = config.clone();
    evolved.archetypes = vec![mlp, wander, flora];
    // A representative default view seed for the showcase (independent of the training
    // seed): one on which the trained MLP holds a healthy parity with the wander control.
    evolved.seed = 0;
    // Remap the field relations for the 3-species showcase. The training ground had
    // species 0 = forager, 1 = flora; inserting the WANDER control at index 1 shifts the
    // flora to index 2, so the diet/absorption rows must move with them or the flora
    // stops being digestible (foragers starve amid a carpet). The wander shares the
    // forager's diet (only its brain differs).
    evolved.field_relations = config
        .field_relations
        .iter()
        .flat_map(|fr| match fr.species {
            0 => {
                let mut wander_diet = fr.clone();
                wander_diet.species = 1;
                vec![fr.clone(), wander_diet]
            }
            1 => {
                let mut flora_row = fr.clone();
                flora_row.species = 2;
                vec![flora_row]
            }
            _ => vec![fr.clone()],
        })
        .collect();
    let evolved_header = "\
// 06 · Learning — an evolved brain, and the control it must beat.
//
// Every forager so far ran a HAND-WRITTEN brain (Wander, Hunter, Grazer). Species 0's
// decider is instead a neural network whose weights were DELIVERED BY EVOLUTION: an
// `Mlp` brain that, from RANDOM weights, mutated and was selected by the ordinary
// continuous economy (neuroevolution — no gradient, no labels, just who eats and
// breeds) on the training ground, until a competent forager emerged. That evolved
// network is frozen here as a `captured_brain` and its founders are born with it. Its
// I/O is fixed by the body (SIM Law 4 — vision × rays + target/threat + proprioception
// in, steering + eat intent out); its neurons are PRICED (`brain_cost`).
//
// A WANDER control (species 1) shares the same oasis flora — the honest baseline of
// §4.2: a learned brain that cannot out-forage a coin-flip has learned nothing. From
// random the two start level (a fresh MLP even loses — its random `act` output often
// never eats); the TRAINED brain here forages on par with or ahead of the wanderer.
// Watch the network graph (click a Trained MLP → brain view) and compare the two
// populations. (Living-food neuroevolution plateaus near parity — the honest ROADMAP §7
// target; a longer, offline search is the `breed` regime, 09.)
//
// GENERATED by `cargo run --bin train` — do not hand-edit; re-run to regenerate.\n";
    std::fs::write(
        "scenarios/deferred/learning.ron",
        format!(
            "{evolved_header}{}",
            evolved.to_ron_string().expect("serialize evolved scenario")
        ),
    )
    .expect("write deferred/learning.ron");

    println!(
        "wrote species/examples/mlp_trained.ron + scenarios/deferred/learning.ron \
         (captured generation {generation})"
    );
}

/// Update `best` with the strongest MLP of species 0 currently alive — highest
/// [`Generation`], tie-broken by current [`Reserve`] — cloning its genotype + brain
/// when it improves on the running best. Called periodically so the *peak*-generation
/// lineage is captured even if the population later fades (cf. the loop above).
fn scan_best(world: &mut World, best: &mut Option<(u32, f32, Genotype, Brain)>) {
    let mut q =
        world.query_filtered::<(&Species, &Generation, &Reserve, &Genotype, &Brain), With<Agent>>();
    for (species, generation, reserve, genotype, brain) in q.iter(world) {
        if species.0 != 0 || !matches!(brain, Brain::Mlp(_)) {
            continue;
        }
        let key = (generation.0, reserve.current);
        if best.as_ref().is_none_or(|b| (b.0, b.1) < key) {
            *best = Some((generation.0, reserve.current, *genotype, brain.clone()));
        }
    }
}
