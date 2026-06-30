//! `breed` — run the generational (`run → score → breed`) loop headless and capture the
//! best-evolved genome (P5, §4 axis A — batched reproduction × explicit fitness).
//!
//! The headless face of the windowed *breeding dashboard* (deferred to a later step): it
//! drives a [`teemlab::breeding::Orchestrator`] over the scenario's `batch` regime,
//! prints the fitness per generation (best / mean over the cohort), and writes the
//! best-evolved genome as a reusable catalog **variant** into `species/saved/` — the
//! multi-generation extension of the single-run `train` bin (which is the `generations: 1`
//! special case). See `docs/p5-breeding-plan.md`.
//!
//! Usage: `cargo run --bin breed -- <scenario.ron> [generations]`. The scenario must
//! carry a `batch` [`BatchConfig`](teemlab::config::BatchConfig).

use teemlab::breeding::{Individual, Orchestrator};
use teemlab::config::{SimConfig, SpeciesEntry};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!(
            "usage: breed <scenario.ron> [generations]\n  \
             the scenario must carry a `batch` BatchConfig (see docs/p5-breeding-plan.md)"
        );
        std::process::exit(1);
    };
    let mut config = SimConfig::from_ron_file(path).unwrap_or_else(|err| {
        eprintln!("breed: scenario \"{path}\" unreadable: {err}");
        std::process::exit(1);
    });

    // Optional generations override (arg 2).
    if let (Some(g), Some(batch)) = (
        args.get(1).and_then(|s| s.parse::<usize>().ok()),
        config.batch.as_mut(),
    ) {
        batch.generations = g;
    }

    // The bin captures the **first** scored faction's champion (the report's view). With
    // several factions (co-evolution) the others co-evolve too; their capture is a refinement.
    let Some(scored) = config
        .batch
        .as_ref()
        .and_then(|b| b.scored_species.first().copied())
    else {
        eprintln!(
            "breed: scenario \"{path}\" has no `batch` regime (or no scored faction) — add a \
             BatchConfig (see docs/p5-breeding-plan.md)"
        );
        std::process::exit(1);
    };
    let Some(base_arch) = config.archetypes.get(scored as usize).cloned() else {
        eprintln!("breed: scored species {scored} is out of range");
        std::process::exit(1);
    };

    let mut orch = Orchestrator::new(config).expect("batch present (checked above)");
    eprintln!(
        "breed: {} generations on \"{path}\" — scored species: {} (#{scored})",
        orch.generations(),
        base_arch.name,
    );
    println!("=== fitness per generation (best / mean over the cohort) ===");

    let mut best_overall: Option<Individual> = None;
    while !orch.is_done() {
        let report = orch.step();
        let cohort = report
            .match_scores
            .iter()
            .map(|s| format!("{s:.0}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "gen {:>3}: best={:.2}  mean={:.2}  [{cohort}]",
            report.generation, report.best_fitness, report.mean_fitness,
        );
        if let Some(best) = report.best {
            let better = best_overall
                .as_ref()
                .is_none_or(|b| (b.generation, b.reserve) < (best.generation, best.reserve));
            if better {
                best_overall = Some(best);
            }
        }
    }

    let Some(best) = best_overall else {
        eprintln!(
            "breed: the scored species died out in every match — nothing captured \
             (try more match_ticks, a gentler economy, or another seed_base)"
        );
        std::process::exit(1);
    };

    // Capture the best genome as a catalog variant (the `train`-bin path): the evolved
    // genotype + the frozen `captured_brain`, under the base species.
    let (generation, reserve) = (best.generation, best.reserve);
    let captured = base_arch.capture(best.genotype, best.brain, generation);
    let variant_id = format!("bred-{generation}");
    let entry = SpeciesEntry::variant(captured, base_arch.name.clone(), variant_id.clone());
    let out = format!(
        "species/saved/{}_bred.ron",
        base_arch.name.to_lowercase().replace(' ', "_"),
    );
    entry.save_ron_file(&out).unwrap_or_else(|err| {
        eprintln!("breed: cannot write \"{out}\": {err}");
        std::process::exit(1);
    });
    println!(
        "\ncaptured best (generation {generation}, reserve {reserve:.1}) → {out} [variant {variant_id}]"
    );
}
