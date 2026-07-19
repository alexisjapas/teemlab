//! **Observation metrics**: the sliding history of curves and the live
//! statistics — the display *data*, shared by the two backends that render it
//! (egui in the windowed binary, native Bevy in [`crate::dataviz`]).
//!
//! Lives in the lib (no longer in the windowed binary) so that the video
//! recorder ([`crate::dataviz`] on the `record` side) samples and plots
//! **exactly** the same curves/stats as the live preview — one data computation,
//! two plots.
//!
//! Strictly **observation**: everything runs in `Update` (never `FixedUpdate`),
//! read-only over the world — the sim stays byte-identical (cardinal invariant).
//!
//! Sampling is keyed to `Time<Virtual>`: it freezes with the pause and follows
//! the fast-forward, like the sim (§6).

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::brain::Brain;
use crate::components::{Agent, Reserve, Species};
use crate::config::{Bounds, SimConfig};
use crate::genotype::{Genotype, TRAITS};

/// Adds curve sampling (the [`History`] resource + the [`sample_history`]
/// system). To be combined with a plotting backend (egui or [`crate::dataviz`]).
pub struct MetricsPlugin;

impl Plugin for MetricsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<History>()
            .add_systems(Update, sample_history);
    }
}

/// A metrics snapshot, timestamped in simulated time.
struct Sample {
    /// Simulated time (`Time<Virtual>`) of the sample, in seconds.
    t: f32,
    /// Living population per species (indexed like `Species`).
    population: Vec<u32>,
    /// Food sources present (sum of the **sessile** species, Phase 3b).
    food: u32,
    /// Mean genes **pooled over all fauna**, one per [`TRAITS`] characteristic (same
    /// order), each **normalized within its bounds** (`[0, 1]`) so traits of different
    /// scales (speed vs angle) compare on a single graph. This is the drift graph's
    /// default (species-agnostic) curve.
    traits: Vec<f32>,
    /// The same per-trait means, **split per species** (`traits_by_species[species]`
    /// indexed like `Species`): the drift graph's per-species curves, so a scenario can
    /// tell one lineage's drift from another's (03's mutable vs frozen eyes) instead of
    /// only the pooled average. A species with no living fauna at this sample **carries
    /// forward** its last known means (its curve holds, rather than dropping to zero);
    /// a sessile species' row is unused (its frozen genes never plot).
    traits_by_species: Vec<Vec<f32>>,
}

/// Sliding history of metrics. Shared by the two plotting backends.
#[derive(Resource)]
pub struct History {
    /// Interval between two samples, in simulated seconds.
    interval: f32,
    /// Maximum number of samples kept (sliding window).
    max_samples: usize,
    /// Next sampling instant, in **run time** (see [`Self::epoch`]).
    next_at: f32,
    /// Virtual-clock reading at the last world (re)build — the origin of *run time*.
    /// Sample times and the HUD read-out are `Time<Virtual>::elapsed_secs() - epoch`,
    /// so both restart at `0` on a hot reset (item 11) while the global clock keeps
    /// running. Advanced only by [`Self::restart`]; a graph "Clear" leaves it be.
    epoch: f32,
    /// The samples, from oldest to newest.
    samples: VecDeque<Sample>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            interval: 0.5,
            max_samples: 1200, // 0.5 s × 1200 = 10 min of simulated time
            next_at: 0.0,
            epoch: 0.0,
            samples: VecDeque::new(),
        }
    }
}

impl History {
    /// Clears the samples and rearms the sampling clock, **keeping the run epoch** —
    /// the HUD's "Clear" button: the graph empties but the run (and its timer) goes
    /// on, so a resumed curve picks up at the current run time.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.next_at = 0.0;
    }

    /// Restarts the run at `now` (the current `Time<Virtual>::elapsed_secs()`): clears
    /// the samples and re-bases the run epoch, so sample times and the HUD read-out
    /// both count from `0` again. The hot reset (item 11) calls this — a rebuilt world
    /// is a fresh run — where "Clear" ([`Self::clear`]) deliberately does not.
    pub fn restart(&mut self, now: f32) {
        self.samples.clear();
        self.next_at = 0.0;
        self.epoch = now;
    }

    /// The virtual-clock reading at the last world (re)build; subtract it from
    /// `Time<Virtual>::elapsed_secs()` to get *run time* (the HUD read-out).
    pub fn epoch(&self) -> f32 {
        self.epoch
    }

    /// Number of samples kept (for the "N samples" display).
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// True as long as no sample has been taken (nothing to plot).
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Simulated time of the latest sample (seconds), or `0.0` if none. Read-only
    /// observation — the windowed HUD uses it for the run-time readout; it resets with
    /// the history (and hence with the world).
    pub fn latest_time(&self) -> f32 {
        self.samples.back().map(|s| s.t).unwrap_or(0.0)
    }

    /// The latest sample's **per-species living population** (indexed like `Species`),
    /// or an empty slice before the first sample. Read-only — the Observe live-stats
    /// panel reads it (the same source the population curve plots, so they agree).
    pub fn latest_population(&self) -> &[u32] {
        self.samples
            .back()
            .map(|s| s.population.as_slice())
            .unwrap_or(&[])
    }

    /// Total living population of the latest sample and its **change** since the
    /// previous one — the `78  −4` read-out. `(total, delta)`; `delta` is `0` before a
    /// second sample.
    pub fn population_delta(&self) -> (u32, i32) {
        let sum = |s: &Sample| s.population.iter().sum::<u32>();
        let latest = self.samples.back().map(sum).unwrap_or(0);
        let delta = if self.samples.len() >= 2 {
            latest as i32 - sum(&self.samples[self.samples.len() - 2]) as i32
        } else {
            0
        };
        (latest, delta)
    }
}

/// Normalizes a gene value within its bounds, to `[0, 1]`.
fn norm(v: f32, b: Bounds) -> f32 {
    if b.span() > 0.0 {
        ((v - b.min) / b.span()).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Samples the world's metrics at a fixed rate in simulated time. Read-only: it
/// is observation for display, not sim logic — hence its rightful place in
/// `Update`.
pub fn sample_history(
    time: Res<Time<Virtual>>,
    config: Res<SimConfig>,
    mut history: ResMut<History>,
    agents: Query<(&Species, &Genotype), With<Agent>>,
) {
    // Run time: seconds since the last (re)build, so a hot reset restarts the axis at
    // 0 (the global virtual clock is never rewound — cf. `History::epoch`).
    let now = time.elapsed_secs() - history.epoch;
    if now < history.next_at {
        return;
    }
    history.next_at = now + history.interval;

    let species_count = config.species_cardinality() as usize;
    let mut population = vec![0u32; species_count];
    let mut sums = vec![0.0_f32; TRAITS.len()]; // pooled over all fauna
    let mut sp_sums = vec![vec![0.0_f32; TRAITS.len()]; species_count]; // per species
    let mut sp_counts = vec![0u32; species_count]; // fauna per species (the per-row weight)
    let cfg = &*config;
    let mut n = 0u32;
    for (species, g) in &agents {
        let idx = (species.0 as usize).min(species_count - 1);
        population[idx] += 1;
        // Gene means over the **fauna** alone: sessile sources (frozen genes,
        // often numerous) would swamp the fauna's drift. They count toward
        // population/food, not here.
        if cfg.archetypes.get(idx).is_some_and(|a| a.is_sessile()) {
            continue;
        }
        // One `norm` per gene, folded into both the pooled sum and this species' sum.
        for ((sum, sp_sum), t) in sums.iter_mut().zip(sp_sums[idx].iter_mut()).zip(&TRAITS) {
            let v = norm((t.get)(g), (t.bounds)(cfg));
            *sum += v;
            *sp_sum += v;
        }
        sp_counts[idx] += 1;
        n += 1;
    }

    let last = history.samples.back();
    // Zero population → we keep the last known mean genes (a graph collapsing to
    // zero would suggest the genes melted away, not that the population went
    // extinct).
    let traits = if n > 0 {
        let inv = 1.0 / n as f32;
        sums.iter().map(|s| s * inv).collect()
    } else if let Some(last) = last {
        last.traits.clone()
    } else {
        vec![0.0; TRAITS.len()]
    };
    // Same carry-forward, but **per species**: a lineage with no living fauna holds its
    // last known means instead of dropping to zero.
    let traits_by_species: Vec<Vec<f32>> = (0..species_count)
        .map(|s| {
            if sp_counts[s] > 0 {
                let inv = 1.0 / sp_counts[s] as f32;
                sp_sums[s].iter().map(|x| x * inv).collect()
            } else if let Some(prev) = last.and_then(|l| l.traits_by_species.get(s)) {
                prev.clone()
            } else {
                vec![0.0; TRAITS.len()]
            }
        })
        .collect();

    // "Food" = sum of the sessile species (sources/flora), derived from the
    // per-species population (Phase 3b: no more `Food` marker to count).
    let food = population
        .iter()
        .enumerate()
        .filter(|(i, _)| config.archetypes.get(*i).is_some_and(|a| a.is_sessile()))
        .map(|(_, &p)| p)
        .sum();

    history.samples.push_back(Sample {
        t: now,
        population,
        food,
        traits,
        traits_by_species,
    });
    while history.samples.len() > history.max_samples {
        history.samples.pop_front();
    }
}

// ---------------------------------------------------------------------------
// Shared display data (anti-divergence egui ↔ native Bevy)
// ---------------------------------------------------------------------------

/// Live global statistics — the same numbers for the egui bar
/// ([`stats_section`](../editor/fn.stats_section.html)) and the native
/// visualizer. Computed over the **fauna** (sessile sources count toward `food`,
/// not toward the means — otherwise their frozen genes would swamp the drift).
pub struct LiveStats {
    /// Living mobile agents.
    pub population: usize,
    /// Sessile sources (flora / food).
    pub food: usize,
    /// Mean reserve of the fauna.
    pub mean_reserve: f32,
    /// Mean of each [`TRAITS`] gene (same order), over the fauna (raw value).
    pub mean_traits: Vec<f32>,
}

/// Computes [`LiveStats`] in a single pass over the agents. The "mobile vs
/// sessile" filter (the [`Brain::Sessile`] brain) is the single source of truth
/// shared by both backends.
pub fn live_stats(agents: &Query<(&Reserve, &Genotype, &Brain), With<Agent>>) -> LiveStats {
    let mut population = 0usize;
    let mut total = 0usize;
    let mut reserve_sum = 0.0f32;
    let mut trait_sums = vec![0.0f32; TRAITS.len()];
    for (reserve, g, brain) in agents {
        total += 1;
        if matches!(brain, Brain::Sessile(_)) {
            continue;
        }
        population += 1;
        reserve_sum += reserve.current;
        for (sum, t) in trait_sums.iter_mut().zip(&TRAITS) {
            *sum += (t.get)(g);
        }
    }
    let n = population.max(1) as f32;
    LiveStats {
        population,
        food: total - population,
        mean_reserve: reserve_sum / n,
        mean_traits: trait_sums.iter().map(|s| s / n).collect(),
    }
}

/// A curve to plot: a name, an **sRGB** color `[r, g, b] ∈ [0, 1]`
/// (backend-agnostic), and its `[time, value]` points. egui and Bevy only plot it.
pub struct Curve {
    pub name: String,
    pub color: [f32; 3],
    pub pts: Vec<[f32; 2]>,
}

/// Curves of **population per species** + the "food" aggregate (sum of the
/// sessiles). Also returns the observed `y_max` (≥ 1). We plot ONLY the species
/// that exist (or have existed) over the window: an archetype defined but never
/// populated does not add a zero curve. The sessiles are aggregated into "food",
/// not plotted on their own.
pub fn population_curves(history: &History, config: &SimConfig) -> (Vec<Curve>, f32) {
    let Some(last) = history.samples.back() else {
        return (Vec::new(), 1.0);
    };
    let n_species = last.population.len();
    let mut peak = vec![0u32; n_species];
    for s in &history.samples {
        for (i, &p) in s.population.iter().enumerate() {
            if let Some(pk) = peak.get_mut(i) {
                *pk = (*pk).max(p);
            }
        }
    }

    let mut curves = Vec::new();
    let mut y_max = 1.0_f32;
    for (sp, &pk) in peak.iter().enumerate() {
        let sessile = config.archetypes.get(sp).is_some_and(|a| a.is_sessile());
        if sessile || pk == 0 {
            continue;
        }
        let pts: Vec<[f32; 2]> = history
            .samples
            .iter()
            .map(|s| [s.t, *s.population.get(sp).unwrap_or(&0) as f32])
            .collect();
        for q in &pts {
            y_max = y_max.max(q[1]);
        }
        let name = config
            .archetypes
            .get(sp)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("species {sp}"));
        curves.push(Curve {
            name,
            color: config.color_of(sp as u16),
            pts,
        });
    }

    // "Food" = sum of the sessiles, plotted only if a source has existed.
    if history.samples.iter().any(|s| s.food > 0) {
        let pts: Vec<[f32; 2]> = history
            .samples
            .iter()
            .map(|s| [s.t, s.food as f32])
            .collect();
        for q in &pts {
            y_max = y_max.max(q[1]);
        }
        curves.push(Curve {
            name: "food".to_string(),
            color: [0.59, 0.59, 0.59],
            pts,
        });
    }

    (curves, y_max)
}

/// Whether the population curve named `name` is shown under the shared species selector
/// ([`SimConfig::species_display`]), given the `candidates` present on the graph (species
/// names + `"food"`). Empty selector → every curve; a non-empty one → only the curves it
/// names (so the food line and unnamed species drop away when a species focus is set),
/// **unless** it matches none of the candidates (all names stale), in which case it falls
/// back to showing all — the single rule shared by the selector chips (which curve is
/// "on") and [`filter_population_curves`] (which curve is plotted), so the two agree.
pub fn population_shown(config: &SimConfig, name: &str, candidates: &[String]) -> bool {
    if config.species_display.is_empty() {
        return true;
    }
    let any_match = candidates
        .iter()
        .any(|c| config.species_display.iter().any(|n| n == c));
    if !any_match {
        return true; // stale/empty selector → show all rather than an empty graph
    }
    config.species_display.iter().any(|n| n == name)
}

/// Narrow `curves` (the full set from [`population_curves`]) to those the
/// population-per-species graph should **plot**, honoring the shared species selector
/// [`SimConfig::species_display`] via [`population_shown`], and return them with the
/// filtered set's `y_max` (≥ 1, so the video's axis fits the visible curves). The single
/// computation shared by the live HUD and the recorded video.
pub fn filter_population_curves(curves: Vec<Curve>, config: &SimConfig) -> (Vec<Curve>, f32) {
    let candidates: Vec<String> = curves.iter().map(|c| c.name.clone()).collect();
    let filtered: Vec<Curve> = curves
        .into_iter()
        .filter(|c| population_shown(config, &c.name, &candidates))
        .collect();
    let mut y_max = 1.0_f32;
    for c in &filtered {
        for p in &c.pts {
            y_max = y_max.max(p[1]);
        }
    }
    (filtered, y_max)
}

/// Indices into [`TRAITS`] of the genes that **actually evolve** — mutable in at
/// least one archetype. A gene frozen in every archetype is a flat line that only
/// clutters the drift graph, so it is omitted; these evolving genes are the
/// candidates a display filter chooses among.
pub fn mutable_trait_indices(config: &SimConfig) -> Vec<usize> {
    TRAITS
        .iter()
        .enumerate()
        .filter(|(_, t)| config.archetypes.iter().any(|a| (t.mutable)(&a.mutable)))
        .map(|(i, _)| i)
        .collect()
}

/// Indices into [`TRAITS`] of the genes the drift graph should **plot**, honoring
/// [`SimConfig::gene_display`]. An empty filter shows every evolving gene
/// ([`mutable_trait_indices`]); a non-empty one narrows to the named subset (still
/// among the evolving genes, so a name that is frozen everywhere adds nothing). A
/// filter that matches none of them (every name stale) falls back to the full set
/// rather than leaving an empty graph.
pub fn displayed_trait_indices(config: &SimConfig) -> Vec<usize> {
    let candidates = mutable_trait_indices(config);
    if config.gene_display.is_empty() {
        return candidates;
    }
    let named: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&i| config.gene_display.iter().any(|n| n == TRAITS[i].name))
        .collect();
    if named.is_empty() { candidates } else { named }
}

/// Indices of the **fauna** archetypes (non-sessile species) — the candidates the
/// drift graph's species selector chooses among (a sessile species has frozen genes,
/// nothing to plot). Order = archetype/species order, for a stable legend.
pub fn fauna_species_indices(config: &SimConfig) -> Vec<usize> {
    config
        .archetypes
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.is_sessile())
        .map(|(i, _)| i)
        .collect()
}

/// Fauna-species indices the drift graph should **split by**, honoring the shared species
/// selector ([`SimConfig::species_display`]). **Empty** → the pooled view (return empty,
/// the signal for "one curve per gene, all fauna averaged"). A non-empty selector → the
/// named fauna species; a selector naming no fauna species also returns empty (fall back
/// to pooled rather than an empty graph).
pub fn drift_species_indices(config: &SimConfig) -> Vec<usize> {
    if config.species_display.is_empty() {
        return Vec::new();
    }
    fauna_species_indices(config)
        .into_iter()
        .filter(|&s| {
            config
                .species_display
                .iter()
                .any(|n| Some(n.as_str()) == config.archetypes.get(s).map(|a| a.name.as_str()))
        })
        .collect()
}

/// The **gene-drift curves** to plot (normalized `[0, 1]`), filtered by
/// [`SimConfig::gene_display`] (which genes) and split by the shared species selector
/// [`SimConfig::species_display`] (whose genes) — the single computation shared by the
/// live HUD and the recorded video, so the two graphs always match. With no species
/// split it is one **pooled** curve per gene; split, it is one curve per (gene × species).
/// Either way a curve keeps its **gene's** color ([`trait_color`]) — so the same gene's
/// two lineages read as one hue (one melting, one flat), the species named in the legend.
pub fn drift_curves(history: &History, config: &SimConfig) -> Vec<Curve> {
    let genes = displayed_trait_indices(config);
    let species = drift_species_indices(config);
    if species.is_empty() {
        // Pooled: one curve per shown gene, from the fauna-wide mean.
        return trait_curves(history)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| genes.contains(i))
            .map(|(_, c)| c)
            .collect();
    }
    // Split: one curve per (species, gene). The color stays the **gene's** (so a gene's
    // lineages share a hue and read as one story); the species is named in the legend.
    let mut curves = Vec::with_capacity(species.len() * genes.len());
    for &sp in &species {
        let name = config
            .archetypes
            .get(sp)
            .map(|a| a.name.as_str())
            .unwrap_or("species");
        for &g in &genes {
            let pts = history
                .samples
                .iter()
                .map(|s| {
                    let v = s
                        .traits_by_species
                        .get(sp)
                        .and_then(|t| t.get(g))
                        .copied()
                        .unwrap_or(0.0);
                    [s.t, v]
                })
                .collect();
            curves.push(Curve {
                name: format!("{name} · {}", TRAITS[g].name),
                color: trait_color(g),
                pts,
            });
        }
    }
    curves
}

/// Curves of **gene drift** (normalized `[0, 1]`): one per [`TRAITS`]
/// characteristic, color drawn from [`trait_color`]. Fixed Y bounds `[0, 1]` on
/// the plotting side. The display filter is applied by [`drift_curves`]; this is
/// the unfiltered primitive it (and any all-genes view) builds on.
pub fn trait_curves(history: &History) -> Vec<Curve> {
    TRAITS
        .iter()
        .enumerate()
        .map(|(i, t)| Curve {
            name: t.name.to_string(),
            color: trait_color(i),
            pts: history
                .samples
                .iter()
                .map(|s| [s.t, *s.traits.get(i).unwrap_or(&0.0)])
                .collect(),
        })
        .collect()
}

/// sRGB color of the curve for the trait at index `i` (the HUD palette; color is
/// a display matter, so it lives here and not in [`TRAITS`]).
pub fn trait_color(i: usize) -> [f32; 3] {
    const PALETTE: [[f32; 3]; 9] = [
        [0.47, 0.78, 1.00], // blue
        [1.00, 0.67, 0.35], // orange
        [0.59, 0.90, 0.47], // green
        [0.86, 0.55, 0.90], // mauve
        [0.94, 0.86, 0.47], // yellow
        [0.47, 0.90, 0.86], // cyan
        [0.92, 0.51, 0.51], // red
        [0.71, 0.71, 0.71], // light gray
        [0.78, 0.63, 0.43], // brown
    ];
    PALETTE[i % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mutability;

    /// The index of the [`TRAITS`] entry with the given display name (test helper).
    fn trait_idx(name: &str) -> usize {
        TRAITS.iter().position(|t| t.name == name).unwrap()
    }

    /// The evolving-gene candidates are exactly the genes mutable in ≥1 archetype:
    /// the default (one archetype, `Mutability::default`) yields the 7 mutable-by-default
    /// genes, and none of the frozen ones (mutation rate, the costs, the flora genes).
    #[test]
    fn candidates_are_the_mutable_genes() {
        let cfg = SimConfig::default();
        let cand = mutable_trait_indices(&cfg);
        assert!(cand.contains(&trait_idx("Vision range")));
        assert!(cand.contains(&trait_idx("Rays (precision)")));
        // Frozen-by-default genes are not candidates.
        assert!(!cand.contains(&trait_idx("Mutation rate")));
        assert!(!cand.contains(&trait_idx("Brain cost/neuron")));
    }

    /// An empty filter shows every candidate; a named subset narrows to it; and the
    /// display order stays the [`TRAITS`] order (so the plot legend is stable).
    #[test]
    fn empty_filter_shows_all_named_filter_narrows() {
        let mut cfg = SimConfig::default();
        assert_eq!(
            displayed_trait_indices(&cfg),
            mutable_trait_indices(&cfg),
            "an empty gene_display shows every evolving gene"
        );

        cfg.gene_display = vec!["Rays (precision)".into(), "Vision range".into()];
        assert_eq!(
            displayed_trait_indices(&cfg),
            vec![trait_idx("Vision range"), trait_idx("Rays (precision)")],
            "a named filter narrows to it, in TRAITS order"
        );
    }

    /// A filter that names only genes which are frozen everywhere (not candidates), or
    /// only stale names, matches nothing — and falls back to the full set rather than
    /// leaving an empty graph.
    #[test]
    fn filter_matching_no_candidate_falls_back_to_all() {
        // "Mutation rate" is frozen by default → not a candidate; plus a stale name.
        let cfg = SimConfig {
            gene_display: vec!["Mutation rate".into(), "no such gene".into()],
            ..SimConfig::default()
        };
        assert_eq!(
            displayed_trait_indices(&cfg),
            mutable_trait_indices(&cfg),
            "a filter with no matching candidate shows the full set, not nothing"
        );
    }

    /// A population curve (test helper): a name and a flat two-point series at `y`.
    fn pop_curve(name: &str, y: f32) -> Curve {
        Curve {
            name: name.into(),
            color: [1.0, 1.0, 1.0],
            pts: vec![[0.0, y], [1.0, y]],
        }
    }

    /// The shared species selector drives the population graph: empty shows every curve
    /// (species + food); a named species narrows to it — hiding the food line and the
    /// unnamed species (the focus a teaching scenario wants).
    #[test]
    fn population_selector_empty_shows_all_named_narrows() {
        let cands: Vec<String> = ["Mutable eyes", "Frozen eyes", "food"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let cfg = SimConfig::default(); // empty species_display
        for c in &cands {
            assert!(
                population_shown(&cfg, c, &cands),
                "empty selector shows {c}"
            );
        }

        let cfg = SimConfig {
            species_display: vec!["Mutable eyes".into()],
            ..SimConfig::default()
        };
        assert!(population_shown(&cfg, "Mutable eyes", &cands));
        assert!(
            !population_shown(&cfg, "Frozen eyes", &cands),
            "an unnamed species is hidden"
        );
        assert!(
            !population_shown(&cfg, "food", &cands),
            "the food line drops away when a species focus is set"
        );
    }

    /// A selector that names none of the present curves (all stale) falls back to showing
    /// all — never an empty population graph.
    #[test]
    fn population_selector_no_match_falls_back_to_all() {
        let cands: Vec<String> = ["Mutable eyes", "food"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg = SimConfig {
            species_display: vec!["Ghost species".into()],
            ..SimConfig::default()
        };
        for c in &cands {
            assert!(
                population_shown(&cfg, c, &cands),
                "stale selector shows all ({c})"
            );
        }
    }

    /// `filter_population_curves` keeps exactly the shown curves and rescales `y_max` to
    /// them (so the video's axis fits the visible series, not the hidden ones).
    #[test]
    fn filter_population_curves_narrows_and_rescales_y() {
        let all = vec![
            pop_curve("Mutable eyes", 40.0),
            pop_curve("Frozen eyes", 90.0), // the tallest — hidden below
            pop_curve("food", 10.0),
        ];
        let cfg = SimConfig {
            species_display: vec!["Mutable eyes".into()],
            ..SimConfig::default()
        };
        let (kept, y_max) = filter_population_curves(all, &cfg);
        let names: Vec<&str> = kept.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Mutable eyes"],
            "only the named species survives (food and the other species hidden)"
        );
        assert_eq!(
            y_max, 40.0,
            "y_max rescales to the shown curves, dropping the hidden 90"
        );
    }

    /// A three-archetype config — two fauna named `A`/`B` and one sessile — for the
    /// species-split tests.
    fn two_fauna_and_a_plant() -> SimConfig {
        use crate::config::Archetype;
        let fauna = |i, name: &str| {
            let mut a = Archetype::new_agent(i);
            a.name = name.into();
            a
        };
        SimConfig {
            archetypes: vec![fauna(0, "A"), fauna(1, "B"), Archetype::new_food(2)],
            ..SimConfig::default()
        }
    }

    /// The species-split candidates are the **fauna** archetypes only — the sessile plant,
    /// with its frozen genes, is never a candidate to plot.
    #[test]
    fn fauna_candidates_exclude_sessile() {
        let cfg = two_fauna_and_a_plant();
        assert_eq!(fauna_species_indices(&cfg), vec![0, 1]);
    }

    /// The split selector: empty → pooled (empty indices, the "average all fauna" signal);
    /// named fauna → those indices; a name that is stale or names the plant → pooled again.
    #[test]
    fn drift_species_split_selects_named_fauna() {
        let mut cfg = two_fauna_and_a_plant();
        assert!(
            drift_species_indices(&cfg).is_empty(),
            "empty split = pooled (no per-species curves)"
        );

        cfg.species_display = vec!["B".into()];
        assert_eq!(drift_species_indices(&cfg), vec![1]);

        cfg.species_display = vec!["A".into(), "B".into()];
        assert_eq!(drift_species_indices(&cfg), vec![0, 1]);

        // A stale name, or the sessile species' name, matches no fauna → back to pooled.
        cfg.species_display = vec!["Ghost".into()];
        assert!(drift_species_indices(&cfg).is_empty());
        cfg.species_display = vec![cfg.archetypes[2].name.clone()];
        assert!(
            drift_species_indices(&cfg).is_empty(),
            "the sessile plant is not a fauna candidate → pooled"
        );
    }

    /// The filter honors **per-archetype** mutability: a gene frozen in one archetype
    /// but mutable in another is still a candidate (the drift graph pools the fauna).
    #[test]
    fn candidate_if_mutable_in_any_archetype() {
        let mut cfg = SimConfig::default();
        // Two archetypes: one freezes vision range, the other lets it drift.
        cfg.archetypes.push(cfg.archetypes[0].clone());
        cfg.archetypes[0].mutable = Mutability {
            vision_range: false,
            ..Mutability::default()
        };
        // Still mutable in archetype 1 → still a candidate.
        assert!(mutable_trait_indices(&cfg).contains(&trait_idx("Vision range")));

        // Frozen in *both* → dropped.
        cfg.archetypes[1].mutable.vision_range = false;
        assert!(!mutable_trait_indices(&cfg).contains(&trait_idx("Vision range")));
    }

    // The run epoch is the seam between the two "start over" gestures: the hot reset
    // (a fresh run → timer back to 0) re-bases it, the graph "Clear" does not.
    #[test]
    fn restart_rebases_epoch_but_clear_keeps_it() {
        let mut h = History::default();
        assert_eq!(h.epoch(), 0.0, "a fresh history runs from t=0");

        h.restart(120.0);
        assert_eq!(h.epoch(), 120.0, "a reset re-bases the run epoch");

        h.clear();
        assert_eq!(
            h.epoch(),
            120.0,
            "clearing the graph must not restart the run timer",
        );
    }
}
