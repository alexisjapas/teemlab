//! Driver for the generational (`run → score → breed`) orchestrator (P5, §4 axis A).
//!
//! Tests the orchestrator's **mechanism** — that it runs the right cohort, and that
//! selection **re-seeds elites** while no-selection does not — on a cheap, self-sufficient
//! reproducer (a photosynthetic `Wander`: no food, no nutrients, no costly raycasting →
//! fast and low-chaos). It deliberately does **not** assert emergent fitness *improvement*:
//! designing a fitness landscape that climbs is scenario-design research (neuroevolution on
//! living food plateaus at parity — `tests/mlp.rs`, ROADMAP §7; and `BestEvolved` selection
//! on a *free* reproducer is even perverse — it rewards reproduce-to-collapse). The
//! emergent payoff is the `breed` bin's job on `13_mlp_breed.ron` — a generator, like the
//! `train` bin, which is likewise not in CI. The fitness **scoring** itself is unit-tested
//! in `breeding::tests` (no `App`).

use teemlab::brain::BrainKind;
use teemlab::breeding::Orchestrator;
use teemlab::config::{Archetype, BatchConfig, Fitness, SimConfig};
use teemlab::genotype::Genotype;

/// A scenario whose single species is a **self-sufficient** reproducer — a `Wander` that
/// lives on photosynthesis (no food to forage, no nutrient field) and reproduces fast, so
/// a short match reliably reaches `generation ≥ 1`. Cheap to run (no raycasting against
/// targets, a tiny founder count). `survivors` / `generations` parametrize the regime.
fn breeder_config(survivors: usize, generations: usize) -> SimConfig {
    let mut breeder = Archetype::new_agent(0);
    breeder.name = "Breeder".into();
    breeder.count = 10;
    breeder.reserve_max = 60.0;
    breeder.brain = BrainKind::Wander { turn_rate: 0.25 };
    // Founders spawn at `reserve_max` (≥ threshold → they reproduce at once, so
    // `generation ≥ 1` is reliable). The net energy is kept **small** (photosynthesis
    // barely above metabolism + locomotion) so the population grows slowly to ~30 rather
    // than exploding exponentially (a high net would mature children within the window →
    // a chain reaction → thousands of bodies → a slow physics step).
    breeder.genotype = Genotype {
        max_speed: 60.0,
        reproduction_threshold: 35.0,
        offspring_energy: 20.0,
        mutation_rate: 0.12,
        base_metabolism: 2.0,
        move_cost: 1.5,
        vision_rays: 1.0,
        photosynthesis: 5.0,
        ..Genotype::default()
    };
    SimConfig {
        arena_half_extent: 220.0,
        archetypes: vec![breeder],
        batch: Some(BatchConfig {
            generations,
            matches_per_gen: 2,
            match_ticks: 600,
            scored_species: vec![0],
            fitness: Fitness::BestEvolved,
            survivors,
            seed_base: 1,
        }),
        seed: 1,
        ..SimConfig::default()
    }
}

/// `Orchestrator::new` requires a `batch` regime, and reflects its parameters.
#[test]
fn new_requires_a_batch_regime() {
    let mut continuous = breeder_config(1, 2);
    continuous.batch = None;
    assert!(
        Orchestrator::new(continuous).is_none(),
        "a continuous scenario has nothing to breed"
    );

    let orch = Orchestrator::new(breeder_config(1, 4)).expect("batch present");
    assert_eq!(orch.generations(), 4);
    assert_eq!(orch.scored_species().to_vec(), vec![0]);
    assert!(
        orch.survivors().is_empty(),
        "no elite before the first step"
    );
    assert!(!orch.is_done());
}

/// The orchestrator runs exactly `generations` generations, each reporting one score per
/// cohort match, then signals `is_done`.
#[test]
fn runs_every_generation_and_reports_the_cohort() {
    let mut orch = Orchestrator::new(breeder_config(1, 3)).expect("batch present");
    let mut seen = 0;
    while !orch.is_done() {
        let report = orch.step();
        assert_eq!(report.generation, seen, "reports come in order");
        assert_eq!(
            report.match_scores.len(),
            2,
            "one fitness score per match in the cohort"
        );
        seen += 1;
    }
    assert_eq!(seen, 3, "ran exactly `generations` generations");
    assert!(orch.is_done());
}

/// The **core breeding contrast**: with selection the orchestrator carries an evolved
/// elite forward (re-seeding the next cohort); with `survivors: 0` it carries nothing —
/// the falsifiable "selection re-seeds, no-selection does not".
#[test]
fn selection_carries_an_elite_forward_no_selection_does_not() {
    // Selection on: after a generation, one EVOLVED elite (it reproduced in-match, so
    // generation ≥ 1) is held for re-seeding.
    let mut sel = Orchestrator::new(breeder_config(1, 2)).expect("batch present");
    let report = sel.step();
    assert!(
        report.best.is_some(),
        "the scored species produced a best genome"
    );
    assert_eq!(sel.survivors().len(), 1, "one elite carried forward");
    assert!(
        sel.survivors()[0].generation >= 1,
        "the carried elite is an EVOLVED individual (reproduced in-match), not a founder"
    );

    // Selection off (`survivors: 0`): a best still surfaces (for display / final capture),
    // but **nothing** is carried — the next cohort would restart from the scenario's
    // founders. This is the breeding mechanism switched OFF.
    let mut nosel = Orchestrator::new(breeder_config(0, 2)).expect("batch present");
    let report = nosel.step();
    assert!(
        report.best.is_some(),
        "a best still surfaces even with no selection"
    );
    assert!(
        nosel.survivors().is_empty(),
        "no-selection re-seeds nothing — the breeding mechanism is OFF"
    );
}
