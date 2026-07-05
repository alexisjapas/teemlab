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

use crate::brain::{Brain, MlpBrain};
use crate::components::{Agent, Generation, Reserve, Species};
use crate::config::{BatchConfig, Fitness};
use crate::genotype::Genotype;
use crate::rng::Rng;
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

/// **All** selection metrics for one match's scored faction — the **diagnostics** the
/// dashboard shows per match (P5 "several metrics between each simulation"). Only
/// `batch.fitness` *drives* selection ([`of`](Self::of)); the rest are computed in the same
/// pass so a match can be read under every angle without re-selecting. Adding a [`Fitness`]
/// primitive is one field here + one arm in [`of`](Self::of) — the homogeneous counterpart
/// of the cost / relation tables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchMetrics {
    /// Deepest lineage reached ([`Fitness::BestEvolved`]): how far the in-match
    /// neuroevolution got (a sustained lineage = competent foraging).
    pub best_evolved: f64,
    /// Standing biomass of the scored species ([`Fitness::Population`]) — the ecological
    /// score (coexistence / dominance).
    pub population: f64,
    /// Combat dominance ([`Fitness::Dominance`]): own survivors minus living rivals (every
    /// other non-sessile agent — food excluded). A faction wins by both surviving and
    /// eliminating the enemy (item 19).
    pub dominance: f64,
    /// Mean energy reserve of the scored species' survivors — a **health** diagnostic (how
    /// well-fed the cohort is), not a selectable [`Fitness`]. `0.0` when the species is
    /// extinct.
    pub mean_reserve: f64,
}

impl MatchMetrics {
    /// Computes every metric for `scored_species` from a finished match's `individuals`.
    /// One pass' worth of cheap aggregates — all `0.0` when the scored species died out.
    pub fn compute(individuals: &[Individual], scored_species: u16) -> Self {
        let scored = || individuals.iter().filter(|i| i.species == scored_species);
        let population = scored().count() as f64;
        let best_evolved = scored().map(|i| i.generation).max().unwrap_or(0) as f64;
        // Rivals = every other non-sessile agent (food excluded).
        let rivals = individuals
            .iter()
            .filter(|i| i.species != scored_species && !matches!(i.brain, Brain::Sessile(_)))
            .count() as f64;
        let mean_reserve = if population > 0.0 {
            scored().map(|i| i.reserve as f64).sum::<f64>() / population
        } else {
            0.0
        };
        Self {
            best_evolved,
            population,
            dominance: population - rivals,
            mean_reserve,
        }
    }

    /// The scalar that **drives selection** for `fitness` — the generation curve's Y and the
    /// match-ranking key. An exhaustive `match` over the selectable [`Fitness`] primitives
    /// (`mean_reserve` is a diagnostic only, not selectable).
    pub fn of(&self, fitness: Fitness) -> f64 {
        match fitness {
            Fitness::BestEvolved => self.best_evolved,
            Fitness::Population => self.population,
            Fitness::Dominance => self.dominance,
        }
    }
}

/// The match's **fitness scalar** for `fitness` over `scored_species`, from a finished
/// match's `individuals` — the selection score (the generation curve's Y, the match rank).
/// `0.0` for a match where the scored species died out. A thin selector over
/// [`MatchMetrics`] (which computes every metric at once for the dashboard's diagnostics).
pub fn score(individuals: &[Individual], fitness: Fitness, scored_species: u16) -> f64 {
    MatchMetrics::compute(individuals, scored_species).of(fitness)
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

/// **Seeds a live-runnable scenario from a generation's cohort** — the mechanism the
/// windowed dashboard uses to **replay** any generation in the live world. It sets
/// `species`' founders to a **diverse pool** drawn from `elites` (a bred faction's ranked
/// genomes): founder 0 is the top elite intact, the rest are mutated variants cycled over
/// the whole cohort — the same founder-diversity the orchestrator seeds a match with, so the
/// live world re-renders a cohort *like* that generation's (a fresh re-render: Law 10 forbids
/// exact replay). A no-op if `elites` is empty. Leaves `batch` untouched (the live sim
/// ignores it, so the breeding panel stays open); the founders live in
/// [`SimConfig::founder_pools`], consumed at the next reset by `spawn`.
pub fn seed_founders(config: &mut SimConfig, species: u16, elites: &[Individual], seed: u64) {
    let Some(top) = elites.first() else {
        return;
    };
    // One body for the whole pool (a single input size); the diversity is in the weights.
    let genotype = top.genotype;
    let n_sensed = config.sensed_components(species).len();
    let n_inputs = MlpBrain::input_size(genotype.ray_count(), n_sensed);
    let count = config
        .archetypes
        .get(species as usize)
        .map_or(0, |a| a.count);
    let mut rng = Rng::new(seed ^ (species as u64).wrapping_mul(0x9E37_79B1));
    let founders: Vec<Brain> = (0..count)
        .map(|k| {
            if k == 0 {
                top.brain.clone() // the champion, intact (its size matches `genotype`).
            } else {
                // Cycle over the WHOLE cohort, each variant re-homed to the shared input
                // size (`reproduce` adapts a differing ray count) and jittered at its own
                // rate — a diverse founding population representing the generation.
                let base = &elites[k % elites.len()];
                let s = rng.next_u64();
                let heading = rng.next_f32() * std::f32::consts::TAU;
                base.brain.reproduce(
                    s,
                    heading,
                    &mut rng,
                    base.genotype.mutation_rate,
                    n_inputs,
                    n_sensed,
                )
            }
        })
        .collect();
    if let Some(arch) = config.archetypes.get_mut(species as usize) {
        arch.genotype = genotype;
        arch.captured_brain = Some(top.brain.clone());
    }
    config.founder_pools.insert(species, founders);
}

/// One **bred faction's** outcome in a generation — the data the dashboard's per-faction
/// curve + leaderboard read. (`best` is `elites.first()`; absent when the faction died out
/// in every match.)
#[derive(Clone, Debug)]
pub struct FactionReport {
    /// The bred archetype index.
    pub species: u16,
    /// Best match fitness this generation (the curve's line for this faction).
    pub best_fitness: f64,
    /// Mean match fitness over the cohort.
    pub mean_fitness: f64,
    /// Per-match fitness scalars (the cohort), under the **selected** [`Fitness`].
    pub match_scores: Vec<f64>,
    /// Per-match **diagnostics** — every metric for each match of the cohort (parallel to
    /// `match_scores`), so the dashboard can show a match scored several ways even though
    /// only the selected `Fitness` drove selection ("several metrics between simulations").
    pub match_metrics: Vec<MatchMetrics>,
    /// The generation's per-match best genomes, ranked by fitness (descending) — the
    /// faction's **leaderboard** (a superset of its carried `survivors`).
    pub elites: Vec<Individual>,
}

impl FactionReport {
    /// The faction's top genome this generation (for display + the final catalog capture),
    /// or `None` if it died out in every match.
    pub fn best(&self) -> Option<&Individual> {
        self.elites.first()
    }
}

/// One generation's outcome — a [`FactionReport`] per bred faction (one for a foraging /
/// single-faction run, several under co-evolution). The bin + the dashboard read the
/// factions; a single-faction view is `factions[0]`.
#[derive(Clone, Debug)]
pub struct GenerationReport {
    /// 0-based generation index.
    pub generation: usize,
    /// One report per bred faction (parallel to `batch.scored_species`).
    pub factions: Vec<FactionReport>,
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
    /// The current elites **per scored faction** (parallel to `batch.scored_species`),
    /// re-seeded as the next cohort's founders. Each inner `Vec` is empty at generation 0
    /// (the cohort starts from the scenario's own founders) and after a `survivors: 0` step.
    /// Several factions ⇒ **co-evolution** (each bred from its own elites — the Red Queen).
    survivors: Vec<Vec<Individual>>,
    /// The **all-time best** genome per faction (score + individual), for
    /// **inter-generation elitism**: it always leads the next cohort's survivor pool, so a
    /// bad generation can never erase progress. The baseline regressed *below its own
    /// generation 0* for want of this (`docs/p5-breeding-plan.md` §7). `None` per faction
    /// until its first scored genome; never populated when `survivors: 0` (breeding OFF).
    best_ever: Vec<Option<(f64, Individual)>>,
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
        // A live replay may have left founder pools on the config; the orchestrator seeds
        // its own per match, so start from a clean slate (a bred species is re-seeded, a
        // non-bred one must not inherit a stale replay pool).
        base.founder_pools.clear();
        let factions = batch.scored_species.len();
        Some(Self {
            base,
            batch,
            survivors: vec![Vec::new(); factions],
            best_ever: vec![None; factions],
            next_gen: 0,
        })
    }

    /// Total generations the run will execute.
    pub fn generations(&self) -> usize {
        self.batch.generations
    }

    /// The archetype indices under selection (the bred factions).
    pub fn scored_species(&self) -> &[u16] {
        &self.batch.scored_species
    }

    /// The current **elites of the first scored faction** carried into the next cohort's
    /// founders (empty before the first [`step`](Self::step), and after any step with
    /// `survivors: 0` — the no-selection case). The falsifiable handle on "selection
    /// re-seeds, no-selection does not"; per-faction pools drive the actual co-evolution.
    pub fn survivors(&self) -> &[Individual] {
        self.survivors.first().map_or(&[], Vec::as_slice)
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

        // **Co-evolution**: select for EACH scored faction independently — score the cohort
        // by that faction's fitness and carry the representatives of its highest-*scoring*
        // matches into its own survivor pool. **Selection is fitness-driven** (the elites
        // come from the best-scoring matches, so a combat `Dominance` actually breeds better
        // fighters; for foraging the fitness and the representative key align). One faction
        // ⇒ the single-faction case; several ⇒ each is bred against the others' current best
        // → the **Red Queen** (item 19). One [`FactionReport`] per faction (the dashboard's
        // per-faction curve + leaderboard); a **negative** fitness (a losing Dominance)
        // passes through.
        let n_factions = self.batch.scored_species.len();
        let mut new_survivors: Vec<Vec<Individual>> = Vec::with_capacity(n_factions);
        let mut factions: Vec<FactionReport> = Vec::with_capacity(n_factions);
        for faction in 0..n_factions {
            let species = self.batch.scored_species[faction];
            let mut scores = Vec::with_capacity(cohort.len());
            let mut match_metrics = Vec::with_capacity(cohort.len());
            let mut ranked: Vec<(f64, Individual)> = Vec::new();
            for individuals in &cohort {
                // One pass computes **every** metric; the selected `Fitness` picks the
                // selection scalar, the rest ride along as per-match diagnostics.
                let mm = MatchMetrics::compute(individuals, species);
                let s = mm.of(self.batch.fitness);
                scores.push(s);
                match_metrics.push(mm);
                if let Some(best) = best_individual(individuals, species) {
                    ranked.push((s, best.clone()));
                }
            }
            ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let elites: Vec<Individual> = ranked.into_iter().map(|(_, i)| i).collect();
            let best_fitness = scores.iter().copied().reduce(f64::max).unwrap_or(0.0);
            let mean_fitness = if scores.is_empty() {
                0.0
            } else {
                scores.iter().sum::<f64>() / scores.len() as f64
            };
            // The carried survivors: this cohort's top-K (possibly **none** — the
            // no-selection contrast, `survivors: 0`).
            let mut pool: Vec<Individual> =
                elites.iter().take(self.batch.survivors).cloned().collect();
            // **Inter-generation elitism** (`survivors > 0` only, so the no-selection
            // contrast is untouched): update the all-time best and guarantee it leads the
            // pool, so a bad generation never regresses below past progress (the baseline
            // fell below its own random generation 0 — §7). The all-time best is scored by
            // this faction's `Fitness` (`best_fitness`), consistent with selection.
            if let Some(champ) = elites.first()
                && self.best_ever[faction]
                    .as_ref()
                    .is_none_or(|(bs, _)| best_fitness > *bs)
            {
                self.best_ever[faction] = Some((best_fitness, champ.clone()));
            }
            if self.batch.survivors > 0
                && let Some((_, best)) = &self.best_ever[faction]
                && !pool.iter().any(|i| i.brain == best.brain)
            {
                pool.insert(0, best.clone());
                pool.truncate(self.batch.survivors);
            }
            new_survivors.push(pool);
            factions.push(FactionReport {
                species,
                best_fitness,
                mean_fitness,
                match_scores: scores,
                match_metrics,
                elites,
            });
        }
        self.survivors = new_survivors;

        let report = GenerationReport {
            generation: self.next_gen,
            factions,
        };
        self.next_gen += 1;
        report
    }

    /// The match config for match `m` of the current generation: the carrier scenario with
    /// a per-match seed and, **from generation 1**, **each** scored faction re-seeded from
    /// one of its elites (round-robin over its survivor pool) — its founders born with the
    /// elite's genome + frozen weights (`captured_brain`), then diverging by in-match
    /// mutation. Cross-match seeding + the per-match seed give the cohort its diversity (the
    /// founder-diversity lever, item 18b); re-seeding *every* faction is what makes the
    /// co-evolution (Red Queen).
    fn build_match_config(&self, m: usize) -> SimConfig {
        let mut cfg = self.base.clone();
        cfg.seed = self
            .batch
            .seed_base
            .wrapping_add(self.next_gen as u64 * self.batch.matches_per_gen as u64)
            .wrapping_add(m as u64);
        for (faction, &species) in self.batch.scored_species.iter().enumerate() {
            let elites = &self.survivors[faction];
            if elites.is_empty() {
                continue; // generation 0 (or no-selection): keep the scenario's founders.
            }
            let elite = &elites[m % elites.len()];
            let Some(arch) = cfg.archetypes.get_mut(species as usize) else {
                continue;
            };
            // The elite's evolved **body** — a single genotype, so every founder's brain
            // shares one input size (the diversity is in the WEIGHTS, mirroring the way
            // generation 0 has identical bodies but diverse random brains).
            arch.genotype = elite.genotype;
            // Fallback for the ordinary founder path (e.g. a stale/mismatched pool): the
            // exact elite brain, as before.
            arch.captured_brain = Some(elite.brain.clone());
            // **Founder diversity**: `count` distinct brains, each a mutated variant of the
            // elite, so the match explores a NEIGHBOURHOOD of the elite instead of `count`
            // identical clones — the fix for the founder-diversity collapse that made naïve
            // re-seeding lose to a random start (`docs/p5-breeding-plan.md` §7). Founder 0
            // is the elite itself (unmutated), so a proven genome is always present
            // (within-match elitism); the rest are jittered at the elite's own
            // `mutation_rate` — the same Gaussian weight step as in-match reproduction, via
            // the identical [`Brain::reproduce`] seam.
            let count = arch.count;
            let n_sensed = self.base.sensed_components(species).len();
            let n_inputs = MlpBrain::input_size(elite.genotype.ray_count(), n_sensed);
            let rate = elite.genotype.mutation_rate;
            // A per-(match, species) RNG so the cohort's pools differ (determinism is
            // order-of-magnitude, not bit-for-bit — Law 10).
            let mut rng = Rng::new(cfg.seed ^ (species as u64).wrapping_mul(0x9E37_79B1));
            let founders: Vec<Brain> = (0..count)
                .map(|k| {
                    if k == 0 {
                        elite.brain.clone()
                    } else {
                        let seed = rng.next_u64();
                        let heading = rng.next_f32() * std::f32::consts::TAU;
                        elite
                            .brain
                            .reproduce(seed, heading, &mut rng, rate, n_inputs, n_sensed)
                    }
                })
                .collect();
            cfg.founder_pools.insert(species, founders);
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

    /// [`MatchMetrics::compute`] fills **every** diagnostic in one pass, and [`of`] selects
    /// exactly the scalar `score` returns — so the diagnostics and the selection score stay
    /// consistent (the dashboard shows the others, selection uses `of`).
    #[test]
    fn match_metrics_computes_every_diagnostic() {
        let sessile = Individual {
            species: 2,
            generation: 0,
            reserve: 99.0,
            genotype: Genotype::default(),
            brain: Brain::Sessile(SessileBrain),
        };
        let pop = [
            ind(0, 4, 10.0),
            ind(0, 7, 30.0), // scored species: deepest lineage, richer
            ind(1, 9, 5.0),  // a living non-sessile rival
            sessile,         // food — not a rival
        ];
        let m = MatchMetrics::compute(&pop, 0);
        assert_eq!(m.best_evolved, 7.0, "deepest generation of species 0");
        assert_eq!(m.population, 2.0, "two survivors of species 0");
        assert_eq!(
            m.dominance, 1.0,
            "2 own − 1 non-sessile rival (food excluded)"
        );
        assert_eq!(m.mean_reserve, 20.0, "(10 + 30) / 2");
        // `of` selects exactly what `score` returns.
        for fitness in [
            Fitness::BestEvolved,
            Fitness::Population,
            Fitness::Dominance,
        ] {
            assert_eq!(m.of(fitness), score(&pop, fitness, 0));
        }
        // Extinct scored species → every metric collapses to 0.
        let m0 = MatchMetrics::compute(&[ind(1, 3, 5.0)], 0);
        assert_eq!(
            (m0.best_evolved, m0.population, m0.mean_reserve),
            (0.0, 0.0, 0.0)
        );
    }

    /// [`seed_founders`] (the replay mechanism) fills a `count`-sized founder pool led by the
    /// champion **intact**, the rest diversified, and re-homes the bred species' genotype.
    #[test]
    fn seed_founders_builds_a_pool_led_by_the_champion() {
        use crate::brain::MlpBrain;
        let mut config = SimConfig::default();
        config.archetypes[0].count = 5;
        let genotype = config.archetypes[0].genotype;
        let n_inputs = MlpBrain::input_size(genotype.ray_count(), 0);
        let champ = Brain::Mlp(MlpBrain::random(1, n_inputs, &[4]));
        let elites = vec![
            Individual {
                species: 0,
                generation: 3,
                reserve: 50.0,
                genotype,
                brain: champ.clone(),
            },
            Individual {
                species: 0,
                generation: 2,
                reserve: 40.0,
                genotype,
                brain: Brain::Mlp(MlpBrain::random(2, n_inputs, &[4])),
            },
        ];
        seed_founders(&mut config, 0, &elites, 7);
        let pool = config.founder_pools.get(&0).expect("a founder pool");
        assert_eq!(pool.len(), 5, "one founder per archetype count");
        assert_eq!(pool[0], champ, "founder 0 is the champion, intact");
        assert!(
            pool[1..].iter().any(|b| *b != champ),
            "the rest are diversified variants of the cohort"
        );
        // An empty cohort is a no-op (no pool inserted).
        let mut c2 = SimConfig::default();
        seed_founders(&mut c2, 0, &[], 7);
        assert!(c2.founder_pools.is_empty());
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
