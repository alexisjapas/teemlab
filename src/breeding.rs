//! **Generational regime** — the outside-sim orchestrator's *pure core* (P5, §4 axis A).
//!
//! This module holds the **App-free** core of the `run → score → breed` loop: the
//! per-individual extract pulled from a finished match, the explicit **fitness** scoring
//! (§4 axis B), and the **selection** of the genome to carry into the next generation.
//! Decoupling these from the ECS keeps them unit-testable without building an `App` — the
//! match-*running* half lands in the orchestrator, which drives isolated headless
//! `World`s (§6, DEV Rule 1: no sim logic in `Update`). See `docs/p5-breeding-plan.md`.
//!
//! The regime is **not** a reified `enum Regime` (§4 architectural guard): the inner
//! match stays the byte-identical [`SimPlugin`](crate::SimPlugin) with its continuous
//! in-match evolution (`ecology::reproduce`); this core only acts at the **generation
//! boundary** — score the cohort, pick the survivors, re-seed them as founders (via
//! [`crate::config::Archetype::capture`]).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::brain::Brain;
use crate::components::{Agent, Generation, Reserve, Species};
use crate::config::{BatchConfig, Fitness};
use crate::genotype::Genotype;
use crate::{SimConfig, SimPlugin};

/// One individual extracted from a finished match's world — the data fitness and
/// selection need, lifted out of the ECS so this core is testable without an `App`.
///
/// `genotype` + `brain` are what selection **captures** to re-seed the next generation
/// ([`crate::config::Archetype::capture`]); `species` / `generation` / `reserve` drive
/// the scoring and the selection key.
#[derive(Clone, Debug, PartialEq)]
pub struct Individual {
    /// Archetype index ([`crate::components::Species`]).
    pub species: u16,
    /// Genealogy depth (`0` at a founder, parent+1 at reproduction) — the in-match
    /// evolution's progress, the `BestEvolved` fitness's primary key.
    pub generation: u32,
    /// Energy reserve at the terminal condition — the selection tie-break.
    pub reserve: f32,
    /// The evolved genome (carried into the next generation's founders on selection).
    pub genotype: Genotype,
    /// The evolved brain (frozen weights re-seeded via `captured_brain`).
    pub brain: Brain,
}

/// The match's **fitness scalar** for `fitness` over `scored_species`, from a finished
/// match's `individuals`. Match-level (an aggregate, or the best individual's score) — it
/// feeds the generation curve and ranks matches. `0.0` for a match where the scored
/// species died out.
///
/// An exhaustive `match` over [`Fitness`] — adding a primitive is one arm here, the
/// homogeneous counterpart of the cost / relation tables.
pub fn score(individuals: &[Individual], fitness: Fitness, scored_species: u16) -> f64 {
    let scored = || individuals.iter().filter(|i| i.species == scored_species);
    match fitness {
        // Deepest lineage reached: how far the in-match neuroevolution got (a sustained
        // lineage = competent foraging). The best individual's `generation`. The reserve
        // tie-break is a *selection* concern (which genome), not the curve scalar — see
        // [`best_individual`].
        Fitness::BestEvolved => scored().map(|i| i.generation).max().unwrap_or(0) as f64,
        // Standing biomass of the scored species at the terminal condition (an
        // ecological score — coexistence / dominance).
        Fitness::Population => scored().count() as f64,
        // Combat dominance: own survivors minus living rivals (every other non-sessile
        // agent — food excluded). The battle / factions primitive (item 19): a faction
        // wins by both surviving and eliminating the enemy.
        Fitness::Dominance => {
            let own = scored().count() as f64;
            let rivals = individuals
                .iter()
                .filter(|i| i.species != scored_species && !matches!(i.brain, Brain::Sessile(_)))
                .count() as f64;
            own - rivals
        }
    }
}

/// The **best individual** of `scored_species` to carry forward (capture into the next
/// generation's founders): the one maximizing the selection key `(generation, reserve)` —
/// exactly the `train` bin's rule (deepest lineage, tie-broken by reserve). `None` if the
/// scored species has no living member.
///
/// Decoupled from [`score`] on purpose: the *curve* wants a scalar, *selection* wants the
/// genome itself.
pub fn best_individual(individuals: &[Individual], scored_species: u16) -> Option<&Individual> {
    individuals
        .iter()
        .filter(|i| i.species == scored_species)
        .max_by(|a, b| {
            (a.generation, a.reserve)
                .partial_cmp(&(b.generation, b.reserve))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// One generation's outcome — the data the `breed` bin prints and (step 5) the dashboard
/// plots. `best` is the generation's single top genome (for display + the final catalog
/// capture), independent of how many `survivors` are *carried* (so a no-selection run
/// still surfaces a best, while breeding nothing forward).
#[derive(Clone, Debug)]
pub struct GenerationReport {
    /// 0-based generation index.
    pub generation: usize,
    /// Best match fitness this generation (the curve's upper line).
    pub best_fitness: f64,
    /// Mean match fitness over the cohort.
    pub mean_fitness: f64,
    /// Per-match fitness scalars (the cohort, for the bin printout / the dashboard).
    pub match_scores: Vec<f64>,
    /// The generation's top genome by the selection key, or `None` if the scored species
    /// died out in every match.
    pub best: Option<Individual>,
    /// The generation's per-match best genomes, ranked by the selection key (descending) —
    /// the dashboard **leaderboard**'s data (a superset of the carried `survivors`).
    pub elites: Vec<Individual>,
}

/// The **generational orchestrator** (P5, §4 axis A): runs `generations` cohorts of
/// headless matches, scoring each by [`Fitness`] and re-seeding the next cohort from the
/// top `survivors`. Outside-sim (DEV Rule 1) — each match is an isolated headless `World`
/// (the `sweep`/`train` pattern, §6), the inner sim untouched.
pub struct Orchestrator {
    /// The carrier scenario, with `batch` cleared (a match never recurses into a batch).
    base: SimConfig,
    /// The generational parameters.
    batch: BatchConfig,
    /// The current elites, re-seeded as the next cohort's founders (empty at generation 0
    /// → the cohort starts from the scenario's own founders).
    survivors: Vec<Individual>,
    /// Next generation to run.
    next_gen: usize,
}

impl Orchestrator {
    /// Builds an orchestrator from a carrier scenario, or `None` if it carries no `batch`
    /// regime (a continuous scenario — nothing to breed).
    pub fn new(config: SimConfig) -> Option<Self> {
        let batch = config.batch.clone()?;
        let mut base = config;
        base.batch = None;
        Some(Self {
            base,
            batch,
            survivors: Vec::new(),
            next_gen: 0,
        })
    }

    /// Total generations the run will execute.
    pub fn generations(&self) -> usize {
        self.batch.generations
    }

    /// The archetype index under selection.
    pub fn scored_species(&self) -> u16 {
        self.batch.scored_species
    }

    /// The current **elites** carried into the next generation's founders (empty before
    /// the first [`step`](Self::step), and after any step with `survivors: 0` — the
    /// no-selection case). Exposed for the dashboard's leaderboard and as the falsifiable
    /// handle on "selection re-seeds, no-selection does not".
    pub fn survivors(&self) -> &[Individual] {
        &self.survivors
    }

    /// `true` once every generation has run.
    pub fn is_done(&self) -> bool {
        self.next_gen >= self.batch.generations
    }

    /// Runs **one generation**: build + run the cohort, score each match, select the top
    /// `survivors` across the cohort (carried into the next generation's founders), and
    /// return the [`GenerationReport`].
    pub fn step(&mut self) -> GenerationReport {
        // Build every match's config up front, then run the cohort **in parallel** (item
        // 20): each match is an isolated headless `World` (§6), so the cohort is
        // embarrassingly parallel. Determinism is already abandoned (Law 10), so running
        // them concurrently changes nothing the project relies on — the matches share
        // Bevy's global task pool (initialised once, then reused). Scoped OS threads keep
        // it dependency-free, and a borrow of `cfgs` suffices (the threads join before the
        // scope ends). NB: one thread per match — fine for a realistic `matches_per_gen`;
        // a bounded pool would only matter for a very large cohort.
        let cfgs: Vec<SimConfig> = (0..self.batch.matches_per_gen)
            .map(|m| self.build_match_config(m))
            .collect();
        let ticks = self.batch.match_ticks;
        let cohort: Vec<Vec<Individual>> = std::thread::scope(|scope| {
            let handles: Vec<_> = cfgs
                .iter()
                .map(|cfg| scope.spawn(move || run_match(cfg, ticks)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("a breeding match thread panicked"))
                .collect()
        });

        // Score every match (for the curve) and pair each with its representative genome
        // — the match's `best_individual` (deepest lineage, then reserve). **Selection is
        // driven by the fitness**: the elites carried forward are the representatives of
        // the highest-*scoring* matches, so a combat `Dominance` (item 19) actually breeds
        // better fighters. (For foraging — `Population` / `BestEvolved` — the fitness and
        // the representative key align; for battle they must be coupled, or selection would
        // ignore the fitness entirely.)
        let mut match_scores = Vec::with_capacity(cohort.len());
        let mut ranked: Vec<(f64, Individual)> = Vec::new();
        for individuals in &cohort {
            let s = score(individuals, self.batch.fitness, self.batch.scored_species);
            match_scores.push(s);
            if let Some(best) = best_individual(individuals, self.batch.scored_species) {
                ranked.push((s, best.clone()));
            }
        }
        // Carry the representatives of the best matches (descending fitness): `gen_best`
        // is the top one (surfaces even with `survivors: 0`), `survivors` the top-K prefix
        // (possibly **none** — the falsifiable no-selection contrast), and `elites` (the
        // full ranked list) feeds the dashboard leaderboard.
        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let elites: Vec<Individual> = ranked.into_iter().map(|(_, i)| i).collect();
        let gen_best = elites.first().cloned();
        self.survivors = elites.iter().take(self.batch.survivors).cloned().collect();

        // Allow a **negative** best (a losing Dominance) through, rather than flooring at 0.
        let best_fitness = match_scores.iter().copied().reduce(f64::max).unwrap_or(0.0);
        let mean_fitness = if match_scores.is_empty() {
            0.0
        } else {
            match_scores.iter().sum::<f64>() / match_scores.len() as f64
        };
        let report = GenerationReport {
            generation: self.next_gen,
            best_fitness,
            mean_fitness,
            match_scores,
            best: gen_best,
            elites,
        };
        self.next_gen += 1;
        report
    }

    /// The match config for match `m` of the current generation: the carrier scenario
    /// with a per-match seed and, **from generation 1**, the scored species re-seeded from
    /// one elite (round-robin over the survivors) — its founders born with the elite's
    /// genome + frozen weights (`captured_brain`), then diverging by in-match mutation.
    /// Cross-match seeding + the per-match seed give the cohort its diversity (the
    /// founder-diversity lever, item 18b).
    fn build_match_config(&self, m: usize) -> SimConfig {
        let mut cfg = self.base.clone();
        cfg.seed = self
            .batch
            .seed_base
            .wrapping_add(self.next_gen as u64 * self.batch.matches_per_gen as u64)
            .wrapping_add(m as u64);
        if !self.survivors.is_empty() {
            let elite = &self.survivors[m % self.survivors.len()];
            if let Some(arch) = cfg.archetypes.get_mut(self.batch.scored_species as usize) {
                arch.genotype = elite.genotype;
                arch.captured_brain = Some(elite.brain.clone());
            }
        }
        cfg
    }
}

/// Runs `config` headless for `ticks` fixed steps and extracts the final population — the
/// `sweep`/`train` pattern (`MinimalPlugins + SimPlugin`, manual `update()` loop; §6 —
/// manual stepping needs `finish`/`cleanup` first, cf. ROADMAP §9).
fn run_match(config: &SimConfig, ticks: u64) -> Vec<Individual> {
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config.clone()));
    // Avian inserts some resources in these hooks; we pump the loop by hand.
    app.finish();
    app.cleanup();
    for _ in 0..ticks {
        app.update();
    }
    extract_individuals(app.world_mut())
}

/// Lifts the living agents of a finished match's world into [`Individual`]s.
fn extract_individuals(world: &mut World) -> Vec<Individual> {
    let mut query =
        world.query_filtered::<(&Species, &Generation, &Reserve, &Genotype, &Brain), With<Agent>>();
    query
        .iter(world)
        .map(
            |(species, generation, reserve, genotype, brain)| Individual {
                species: species.0,
                generation: generation.0,
                reserve: reserve.current,
                genotype: *genotype,
                brain: brain.clone(),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::{Brain, HunterBrain, SessileBrain};

    /// A cheap individual (a unit `HunterBrain`, default genome) — scoring and selection
    /// read only species / generation / reserve, so the genotype / brain are inert here.
    fn ind(species: u16, generation: u32, reserve: f32) -> Individual {
        Individual {
            species,
            generation,
            reserve,
            genotype: Genotype::default(),
            brain: Brain::Hunter(HunterBrain),
        }
    }

    /// `BestEvolved` = the deepest generation reached **within the scored species**;
    /// other species are ignored (a deeper-evolved off-target species must not score).
    #[test]
    fn best_evolved_takes_deepest_generation_of_scored_species() {
        let pop = [
            ind(0, 3, 50.0),
            ind(0, 7, 10.0), // scored species, deepest
            ind(0, 5, 99.0),
            ind(1, 20, 99.0), // another species, deeper — must NOT count
        ];
        assert_eq!(score(&pop, Fitness::BestEvolved, 0), 7.0);
    }

    /// `Population` = the living count of the scored species (an ecological score).
    #[test]
    fn population_counts_the_scored_species() {
        let pop = [ind(0, 1, 1.0), ind(0, 2, 1.0), ind(1, 1, 1.0)];
        assert_eq!(score(&pop, Fitness::Population, 0), 2.0);
        assert_eq!(score(&pop, Fitness::Population, 1), 1.0);
    }

    /// A match where the scored species died out scores `0.0` (neither fitness conjures a
    /// score from an empty cohort).
    #[test]
    fn extinct_scored_species_scores_zero() {
        let pop = [ind(1, 9, 99.0)];
        assert_eq!(score(&pop, Fitness::BestEvolved, 0), 0.0);
        assert_eq!(score(&pop, Fitness::Population, 0), 0.0);
        assert_eq!(score(&[], Fitness::BestEvolved, 0), 0.0);
    }

    /// `Dominance` = own survivors − living rivals (other **non-sessile** species); food
    /// (sessile) is excluded. The battle / factions fitness — and it is symmetric (the
    /// loser's dominance is the winner's, negated).
    #[test]
    fn dominance_is_own_minus_living_non_sessile_rivals() {
        let sessile = Individual {
            species: 2,
            generation: 0,
            reserve: 50.0,
            genotype: Genotype::default(),
            brain: Brain::Sessile(SessileBrain),
        };
        let pop = [
            ind(0, 1, 1.0),
            ind(0, 1, 1.0),
            ind(0, 1, 1.0),  // 3 own (scored species 0)
            ind(1, 1, 1.0),  // 1 rival (enemy faction)
            sessile.clone(), // food — must NOT count as a rival
        ];
        assert_eq!(score(&pop, Fitness::Dominance, 0), 2.0); // 3 own − 1 rival
        assert_eq!(score(&pop, Fitness::Dominance, 1), -2.0); // 1 own − 3 rivals
        // A wiped-out faction with only food left scores its full deficit.
        assert_eq!(score(&[sessile], Fitness::Dominance, 0), 0.0); // 0 own − 0 rivals
    }

    /// Selection key: `generation` dominates (a deeper lineage wins over a shallower,
    /// richer one), and **reserve** breaks ties at equal generation.
    #[test]
    fn best_individual_is_generation_then_reserve() {
        let pop = [
            ind(0, 5, 99.0), // richer but shallower
            ind(0, 7, 10.0), // deeper — wins on generation
            ind(0, 7, 40.0), // same depth, richer — wins the tie
            ind(1, 9, 99.0), // off-target — ignored
        ];
        let best = best_individual(&pop, 0).expect("a living scored individual");
        assert_eq!((best.generation, best.reserve), (7, 40.0));
    }

    /// No living member of the scored species → no genome to carry forward.
    #[test]
    fn best_individual_is_none_when_scored_species_extinct() {
        let pop = [ind(1, 3, 50.0)];
        assert!(best_individual(&pop, 0).is_none());
        assert!(best_individual(&[], 0).is_none());
    }
}
