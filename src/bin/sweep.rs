//! **Sweep**: run a scenario many times and score each final world by its
//! **biodiversity**, to search for the runs (or the *parameter values*) where a
//! balanced, coexisting ecosystem emerges — neither a flora monoculture (the fauna
//! collapsed) nor a total collapse (a predator overshoot). The seed of P5's
//! "run → score → …" (cf. ROADMAP §9), kept deliberately **sequential** and
//! in-process: each run builds its own manual-stepping world (the
//! `tests/common::stepping_app` pattern), advances a fixed tick budget, then is
//! scored and dropped.
//!
//! Two modes:
//! - **seed sweep** — `sweep <scenario.ron> [n_seeds=12] [ticks=6400]`: vary only
//!   the RNG seed, rank the seeds.
//! - **parameter sweep** — `sweep <scenario.ron> <knob> <min> <max> <steps> [seeds=2] [ticks=3200]`:
//!   vary a scenario knob over `[min, max]` (e.g. a species' count or a relation's
//!   grazing `rate`), running `seeds` seeds at each value — the search for the
//!   **coexistence band**. A `<knob>` is `kind:index`, `kind ∈ {count, mutation,
//!   photo, rate}` (the index is an archetype, or a relation for `rate`). Example:
//!   `sweep scenarios/coldstart.ron count:1 0 80 9`.
//!
//! **Score = Shannon diversity** `H = -Σ pᵢ·ln(pᵢ)` over the species with a living
//! population, rewarding both *richness* (how many species are alive) and *evenness*
//! (how balanced). `exp(H)` is the **effective number of species** (the Hill number).
//! Per Law 10 the parallel solver makes a seed reproduce the *order of magnitude*,
//! not the exact run — hence several seeds per parameter value, to smooth the noise.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Species};
use teemlab::{SimConfig, SimPlugin};

/// Shannon diversity `H = -Σ pᵢ ln pᵢ` over the species with a living population.
/// `0` for a dead or single-species world; `ln(k)` for `k` equally-abundant species.
fn shannon(counts: &[u32]) -> f64 {
    let total: u32 = counts.iter().sum();
    if total == 0 {
        return 0.0;
    }
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total as f64;
            -p * p.ln()
        })
        .sum()
}

/// Run `config` once under `seed` for `ticks` fixed steps, returning the final
/// per-species living counts (indexed like [`Species`]). Builds a manual-stepping
/// app (one `update()` = one fixed tick), the same world as both binaries.
fn run_once(config: &SimConfig, seed: u64, ticks: u64) -> Vec<u32> {
    let mut config = config.clone();
    config.seed = seed;
    let species_count = config.archetypes.len();

    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config));
    // Avian inserts some resources in these hooks; we pump the loop by hand.
    app.finish();
    app.cleanup();
    for _ in 0..ticks {
        app.update();
    }

    let world = app.world_mut();
    let mut query = world.query_filtered::<&Species, With<Agent>>();
    let mut counts = vec![0u32; species_count];
    for species in query.iter(world) {
        if let Some(slot) = counts.get_mut(species.0 as usize) {
            *slot += 1;
        }
    }
    counts
}

/// A scenario knob the parameter sweep can vary, addressed as `kind:index`. The
/// index is an **archetype** (a species) — except for `Rate`, where it is a
/// **relation**. Kept a small explicit set (Rust has no field reflection): adding a
/// knob is one arm here and one in [`Knob::parse`].
enum Knob {
    Count(usize),
    Mutation(usize),
    Photosynthesis(usize),
    Rate(usize),
}

impl Knob {
    /// Parses a `kind:index` spec (e.g. `count:1`, `photo:2`, `rate:0`).
    fn parse(spec: &str) -> Option<Self> {
        let (kind, idx) = spec.split_once(':')?;
        let idx: usize = idx.parse().ok()?;
        Some(match kind {
            "count" => Self::Count(idx),
            "mutation" => Self::Mutation(idx),
            "photo" => Self::Photosynthesis(idx),
            "rate" => Self::Rate(idx),
            _ => return None,
        })
    }

    /// Writes value `v` into the addressed field (out-of-range index → no-op).
    fn apply(&self, config: &mut SimConfig, v: f64) {
        match *self {
            Self::Count(i) => {
                if let Some(a) = config.archetypes.get_mut(i) {
                    a.count = v.max(0.0).round() as usize;
                }
            }
            Self::Mutation(i) => {
                if let Some(a) = config.archetypes.get_mut(i) {
                    a.genotype.mutation_rate = v as f32;
                }
            }
            Self::Photosynthesis(i) => {
                if let Some(a) = config.archetypes.get_mut(i) {
                    a.genotype.photosynthesis = v as f32;
                }
            }
            Self::Rate(i) => {
                if let Some(r) = config.relations.get_mut(i) {
                    r.rate = v as f32;
                }
            }
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Count(i) => format!("count[sp{i}]"),
            Self::Mutation(i) => format!("mutation[sp{i}]"),
            Self::Photosynthesis(i) => format!("photo[sp{i}]"),
            Self::Rate(i) => format!("rate[rel{i}]"),
        }
    }
}

/// Per-species labels for the printout.
fn species_names(config: &SimConfig) -> Vec<String> {
    config.archetypes.iter().map(|a| a.name.clone()).collect()
}

/// `"A=1  B=2"` for the living species of `counts`.
fn per_species(counts: &[u32], names: &[String]) -> String {
    counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(i, c)| format!("{}={c}", names.get(i).map_or("?", |s| s.as_str())))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Seed sweep: vary the RNG seed, rank the seeds by biodiversity.
fn seed_sweep(config: &SimConfig, n_seeds: u64, ticks: u64, names: &[String]) {
    eprintln!(
        "sweep[seed]: × {n_seeds} seeds × {ticks} ticks (~{:.0}s each) — Shannon biodiversity",
        ticks as f64 / config.tick_hz,
    );
    let mut rows: Vec<(u64, f64, u32, Vec<u32>)> = (0..n_seeds)
        .map(|seed| {
            let counts = run_once(config, seed, ticks);
            let total: u32 = counts.iter().sum();
            let h = shannon(&counts);
            eprintln!(
                "  seed {seed:>3}: H={h:.3}  eff.sp={:.2}  pop={total}",
                h.exp()
            );
            (seed, h, total, counts)
        })
        .collect();
    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.2.cmp(&a.2))
    });
    println!("\n=== ranked by biodiversity (Shannon H, effective species exp(H)) ===");
    for (rank, (seed, h, total, counts)) in rows.iter().enumerate() {
        println!(
            "{:>3}. seed {seed:>4}  H={h:.3}  eff.sp={:.2}  pop={total:>5}   [{}]",
            rank + 1,
            h.exp(),
            per_species(counts, names),
        );
    }
}

/// Parameter sweep: vary `knob` over `[min, max]` in `steps` values, running
/// `seeds` seeds at each, and print biodiversity **vs the parameter** (in value
/// order, so the coexistence band is visible as a peak). Each row keeps the
/// best-of-seeds run; the global peak is flagged at the end.
#[allow(clippy::too_many_arguments)] // a CLI driver: the knob + its range + run budget.
fn param_sweep(
    config: &SimConfig,
    knob: &Knob,
    min: f64,
    max: f64,
    steps: usize,
    seeds: u64,
    ticks: u64,
    names: &[String],
) {
    eprintln!(
        "sweep[{}]: [{min}, {max}] in {steps} steps × {seeds} seeds × {ticks} ticks (~{:.0}s/run) — Shannon biodiversity",
        knob.label(),
        ticks as f64 / config.tick_hz,
    );
    println!(
        "\n=== {} vs biodiversity (best-of-{seeds}-seeds per value) ===",
        knob.label()
    );
    let mut peak = (f64::NEG_INFINITY, 0.0f64);
    for s in 0..steps.max(1) {
        let v = if steps <= 1 {
            min
        } else {
            min + (max - min) * s as f64 / (steps - 1) as f64
        };
        let mut cfg = config.clone();
        knob.apply(&mut cfg, v);

        let mut best_h = f64::NEG_INFINITY;
        let mut best_counts = vec![0u32; config.archetypes.len()];
        let mut sum_h = 0.0;
        for seed in 0..seeds {
            let counts = run_once(&cfg, seed, ticks);
            let h = shannon(&counts);
            sum_h += h;
            if h > best_h {
                best_h = h;
                best_counts = counts;
            }
        }
        let mean_h = sum_h / seeds.max(1) as f64;
        if best_h > peak.0 {
            peak = (best_h, v);
        }
        println!(
            "{:>8.2}  bestH={best_h:.3}  meanH={mean_h:.3}  eff.sp={:.2}  [{}]",
            v,
            best_h.exp(),
            per_species(&best_counts, names),
        );
        eprintln!("  {} = {v:.2}: bestH={best_h:.3}", knob.label());
    }
    println!(
        "\npeak biodiversity at {} = {:.2}  (H={:.3}, eff.sp={:.2})",
        knob.label(),
        peak.1,
        peak.0,
        peak.0.exp(),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!(
            "usage:\n  sweep <scenario.ron> [n_seeds=12] [ticks=6400]            (seed sweep)\n  \
             sweep <scenario.ron> <kind:index> <min> <max> <steps> [seeds=2] [ticks=3200]  (parameter sweep)\n  \
             kind ∈ count|mutation|photo|rate"
        );
        std::process::exit(1);
    };
    let config = SimConfig::from_ron_file(path).unwrap_or_else(|err| {
        eprintln!("sweep: scenario \"{path}\" unreadable: {err}");
        std::process::exit(1);
    });
    let names = species_names(&config);

    // Parameter-sweep mode iff arg 2 parses as a `kind:index` knob.
    if let Some(knob) = args.get(1).and_then(|s| Knob::parse(s)) {
        let parse = |i: usize| args.get(i).and_then(|s| s.parse::<f64>().ok());
        let (Some(min), Some(max), Some(steps)) = (parse(2), parse(3), parse(4)) else {
            eprintln!("sweep: parameter sweep needs <min> <max> <steps>");
            std::process::exit(1);
        };
        let seeds = parse(5).map_or(2, |v| v as u64);
        let ticks = parse(6).map_or(3200, |v| v as u64);
        param_sweep(
            &config,
            &knob,
            min,
            max,
            steps as usize,
            seeds,
            ticks,
            &names,
        );
        return;
    }

    // Otherwise: seed sweep (back-compatible).
    let n_seeds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(12);
    let ticks = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6400);
    seed_sweep(&config, n_seeds, ticks, &names);
}
