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

/// A single **time sample** of one scored species during a match — the cheap per-tick
/// snapshot the time-robust metrics aggregate over. The *terminal* tick is a poor read on
/// living food (Lotka–Volterra: a boom then a bust, so the last tick is often a dying
/// remnant); sampling the whole trajectory fixes that. Built by the match sampler.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MatchSample {
    /// Living count of the scored species at this sample.
    pub population: usize,
    /// Deepest lineage (max `Generation`) among the scored species at this sample.
    pub best_gen: u32,
    /// Living **non-sessile rivals** at this sample (every other non-sessile agent — food
    /// excluded), for the combat `Dominance` reading.
    pub rivals: usize,
    /// Sum of the scored species' energy reserves (÷ population = the sample's mean reserve).
    pub reserve_sum: f64,
}

/// **Time-robust** selection metrics for one match's scored faction — aggregated over the
/// **whole match trajectory**, not the terminal tick. The dashboard shows them all per match
/// (diagnostics); only `batch.fitness` drives selection ([`of`](Self::of)). Adding a
/// [`Fitness`] primitive is one field here + one arm in [`of`](Self::of).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MatchMetrics {
    /// MEAN standing population over the match — **sustained biomass** ([`Fitness::Population`]),
    /// the robust forager fitness: a lineage that keeps a population fed, not a one-tick bloom
    /// nor a dying remnant.
    pub mean_population: f64,
    /// PEAK standing population over the match ([`Fitness::Peak`]) — the strongest bloom.
    pub peak_population: f64,
    /// SURVIVAL — the fraction of the match the species stayed alive (`0..=1`), i.e. longevity
    /// ([`Fitness::Survival`]): rewards not dying out.
    pub survival: f64,
    /// Deepest lineage reached **ever** over the match ([`Fitness::BestEvolved`]) — how far the
    /// in-match neuroevolution got, caught at its peak (before any collapse). NB: *perverse* on
    /// a free reproducer (rewards reproduce-to-collapse) — prefer `Population`.
    pub best_evolved: f64,
    /// TERMINAL combat dominance ([`Fitness::Dominance`]) — own − living non-sessile rivals at
    /// the last sample (the battle outcome is by nature terminal: who is left standing).
    pub dominance: f64,
    /// MEAN energy reserve of survivors over the match — a foraging-health / efficiency proxy
    /// (a diagnostic, not a selectable [`Fitness`]). `0` when the species never lived.
    pub mean_reserve: f64,
}

impl MatchMetrics {
    /// Aggregates a match's [`MatchSample`] trajectory into the time-robust metrics. Empty or
    /// all-extinct → every field `0`.
    pub fn from_samples(samples: &[MatchSample]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let n = samples.len() as f64;
        let pop_sum: f64 = samples.iter().map(|s| s.population as f64).sum();
        let peak = samples.iter().map(|s| s.population).max().unwrap_or(0) as f64;
        let alive = samples.iter().filter(|s| s.population > 0).count() as f64;
        let best_gen = samples.iter().map(|s| s.best_gen).max().unwrap_or(0) as f64;
        let last = samples.last().copied().unwrap_or_default();
        // Mean reserve = mean over the ALIVE samples of each sample's mean reserve.
        let (rsum, ralive) = samples
            .iter()
            .filter(|s| s.population > 0)
            .fold((0.0, 0.0), |(rs, ra), s| {
                (rs + s.reserve_sum / s.population as f64, ra + 1.0)
            });
        Self {
            mean_population: pop_sum / n,
            peak_population: peak,
            survival: alive / n,
            best_evolved: best_gen,
            dominance: last.population as f64 - last.rivals as f64,
            mean_reserve: if ralive > 0.0 { rsum / ralive } else { 0.0 },
        }
    }

    /// The scalar that **drives selection** for `fitness` — the generation curve's Y and the
    /// match-ranking key. Exhaustive over the selectable [`Fitness`] primitives (`mean_reserve`
    /// is a diagnostic only, not selectable).
    pub fn of(&self, fitness: Fitness) -> f64 {
        match fitness {
            Fitness::Population => self.mean_population,
            Fitness::Peak => self.peak_population,
            Fitness::Survival => self.survival,
            Fitness::BestEvolved => self.best_evolved,
            Fitness::Dominance => self.dominance,
        }
    }
}

/// One finished match's outcome, per scored faction (parallel to the `scored` slice passed to
/// [`run_match`]): the **time-robust** [`MatchMetrics`] and the match's **best-ever genome**
/// (deepest lineage, tie-broken by reserve — the `train` bin's rule, caught at its peak rather
/// than read off a dying terminal population). `best` is `None` when that faction never lived.
struct MatchOutcome {
    metrics: Vec<MatchMetrics>,
    best: Vec<Option<Individual>>,
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
        let scored = self.batch.scored_species.as_slice();
        let cohort: Vec<MatchOutcome> = std::thread::scope(|scope| {
            let handles: Vec<_> = cfgs
                .iter()
                .map(|cfg| scope.spawn(move || run_match(cfg, ticks, scored)))
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
            for outcome in &cohort {
                // The match already aggregated **every** metric over its trajectory (the
                // selected `Fitness` picks the selection scalar, the rest ride along as
                // diagnostics) and tracked this faction's best-ever genome.
                let mm = outcome.metrics[faction];
                let s = mm.of(self.batch.fitness);
                scores.push(s);
                match_metrics.push(mm);
                if let Some(best) = &outcome.best[faction] {
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

/// Number of trajectory samples per match, whatever its length — enough to read the
/// population curve's shape (peak, sustain, collapse) without weighing on the run.
const SAMPLES_PER_MATCH: u64 = 100;

/// Runs `config` headless for `ticks` fixed steps, **sampling the trajectory** (not just the
/// terminal tick), and returns the time-robust [`MatchOutcome`] per scored faction — the
/// `sweep`/`train` pattern (`MinimalPlugins + SimPlugin`, manual `update()` loop; §6 — manual
/// stepping needs `finish`/`cleanup` first). Sampling is cheap (a count/gen/reserve query, no
/// brain clones); the best-ever genome is cloned only when it improves (the `train` pattern).
fn run_match(config: &SimConfig, ticks: u64, scored: &[u16]) -> MatchOutcome {
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config.clone()));
    // Avian inserts some resources in these hooks; we pump the loop by hand.
    app.finish();
    app.cleanup();
    let mut samples: Vec<Vec<MatchSample>> = vec![Vec::new(); scored.len()];
    let mut best: Vec<Option<(u32, f32, Individual)>> = vec![None; scored.len()];
    let every = (ticks / SAMPLES_PER_MATCH).max(1);
    for tick in 0..ticks {
        app.update();
        if tick % every == 0 {
            sample_match(app.world_mut(), scored, &mut samples, &mut best);
        }
    }
    // A terminal sample, so `Dominance` (which reads the last sample) sees the final state.
    sample_match(app.world_mut(), scored, &mut samples, &mut best);
    MatchOutcome {
        metrics: samples
            .iter()
            .map(|s| MatchMetrics::from_samples(s))
            .collect(),
        best: best.into_iter().map(|b| b.map(|(_, _, i)| i)).collect(),
    }
}

/// One trajectory sample: a single query pass that appends a [`MatchSample`] per scored
/// species (counts / deepest lineage / rivals / reserve sum) **and** updates each faction's
/// best-ever genome (deepest lineage, tie-broken by reserve), cloning only on an improvement.
fn sample_match(
    world: &mut World,
    scored: &[u16],
    samples: &mut [Vec<MatchSample>],
    best: &mut [Option<(u32, f32, Individual)>],
) {
    let mut non_sessile = 0usize;
    // (population, best_gen, reserve_sum) accumulator per scored species.
    let mut acc: Vec<(usize, u32, f64)> = vec![(0, 0, 0.0); scored.len()];
    let mut query =
        world.query_filtered::<(&Species, &Generation, &Reserve, &Genotype, &Brain), With<Agent>>();
    for (species, generation, reserve, genotype, brain) in query.iter(world) {
        if !matches!(brain, Brain::Sessile(_)) {
            non_sessile += 1;
        }
        if let Some(i) = scored.iter().position(|&s| s == species.0) {
            let (count, best_gen, reserve_sum) = &mut acc[i];
            *count += 1;
            *best_gen = (*best_gen).max(generation.0);
            *reserve_sum += reserve.current as f64;
            let key = (generation.0, reserve.current);
            if best[i].as_ref().is_none_or(|(g, r, _)| (*g, *r) < key) {
                best[i] = Some((
                    generation.0,
                    reserve.current,
                    Individual {
                        species: species.0,
                        generation: generation.0,
                        reserve: reserve.current,
                        genotype: *genotype,
                        brain: brain.clone(),
                    },
                ));
            }
        }
    }
    for (i, (count, best_gen, reserve_sum)) in acc.into_iter().enumerate() {
        // Rivals of a (non-sessile) scored species = every OTHER non-sessile agent.
        samples[i].push(MatchSample {
            population: count,
            best_gen,
            rivals: non_sessile.saturating_sub(count),
            reserve_sum,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::Brain;

    /// A trajectory sample (population, deepest lineage, living rivals, reserve sum).
    fn sample(population: usize, best_gen: u32, rivals: usize, reserve_sum: f64) -> MatchSample {
        MatchSample {
            population,
            best_gen,
            rivals,
            reserve_sum,
        }
    }

    /// [`MatchMetrics::from_samples`] aggregates the trajectory **time-robustly** — mean +
    /// peak population, survival fraction, deepest-**ever** lineage, TERMINAL dominance, and
    /// the mean reserve over alive samples — never the single terminal tick.
    #[test]
    fn from_samples_aggregates_the_trajectory() {
        // A bloom (peak 10) that busts to a dying remnant; deepest lineage mid-run.
        let samples = [
            sample(2, 1, 0, 40.0),   // mean reserve 20
            sample(10, 5, 3, 300.0), // peak; deepest lineage 5; mean reserve 30
            sample(4, 4, 6, 40.0),   // mean reserve 10
            sample(1, 3, 9, 5.0),    // terminal remnant: pop 1, rivals 9; mean reserve 5
        ];
        let m = MatchMetrics::from_samples(&samples);
        assert_eq!(m.peak_population, 10.0);
        assert_eq!(m.mean_population, (2.0 + 10.0 + 4.0 + 1.0) / 4.0);
        assert_eq!(
            m.best_evolved, 5.0,
            "deepest lineage EVER, not the terminal 3"
        );
        assert_eq!(m.survival, 1.0, "alive at every sample");
        assert_eq!(m.dominance, 1.0 - 9.0, "TERMINAL own − rivals");
        assert_eq!(m.mean_reserve, (20.0 + 30.0 + 10.0 + 5.0) / 4.0);
    }

    /// Survival = the fraction of samples the species was alive; an extinction mid-run shows,
    /// and the mean reserve averages only the alive samples.
    #[test]
    fn from_samples_survival_and_extinction() {
        let samples = [
            sample(5, 2, 0, 50.0),
            sample(0, 0, 0, 0.0),
            sample(0, 0, 0, 0.0),
        ];
        let m = MatchMetrics::from_samples(&samples);
        assert_eq!(
            m.survival,
            1.0 / 3.0,
            "alive only the first of three samples"
        );
        assert_eq!(m.peak_population, 5.0);
        assert_eq!(
            m.mean_reserve, 10.0,
            "mean reserve over the ALIVE samples only"
        );
        // An empty trajectory is all-zero; a never-alive one likewise.
        assert_eq!(MatchMetrics::from_samples(&[]), MatchMetrics::default());
        let dead = MatchMetrics::from_samples(&[sample(0, 0, 2, 0.0)]);
        assert_eq!(
            (dead.survival, dead.mean_population, dead.mean_reserve),
            (0.0, 0.0, 0.0)
        );
    }

    /// `of` selects exactly the field each [`Fitness`] names (exhaustive).
    #[test]
    fn of_selects_the_named_metric() {
        let m = MatchMetrics {
            mean_population: 1.0,
            peak_population: 2.0,
            survival: 0.5,
            best_evolved: 3.0,
            dominance: -4.0,
            mean_reserve: 9.0,
        };
        assert_eq!(m.of(Fitness::Population), 1.0);
        assert_eq!(m.of(Fitness::Peak), 2.0);
        assert_eq!(m.of(Fitness::Survival), 0.5);
        assert_eq!(m.of(Fitness::BestEvolved), 3.0);
        assert_eq!(m.of(Fitness::Dominance), -4.0);
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
}
