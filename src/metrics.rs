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
    /// Mean genes, one per [`TRAITS`] characteristic (same order), each
    /// **normalized within its bounds** (`[0, 1]`) so traits of different scales
    /// (speed vs angle) compare on a single graph.
    traits: Vec<f32>,
}

/// Sliding history of metrics. Shared by the two plotting backends.
#[derive(Resource)]
pub struct History {
    /// Interval between two samples, in simulated seconds.
    interval: f32,
    /// Maximum number of samples kept (sliding window).
    max_samples: usize,
    /// Next sampling instant (simulated time).
    next_at: f32,
    /// The samples, from oldest to newest.
    samples: VecDeque<Sample>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            interval: 0.5,
            max_samples: 1200, // 0.5 s × 1200 = 10 min of simulated time
            next_at: 0.0,
            samples: VecDeque::new(),
        }
    }
}

impl History {
    /// Starts over: clears the samples and rearms the clock. Called by the HUD's
    /// "Clear" button and by the hot reset (item 11).
    pub fn clear(&mut self) {
        self.samples.clear();
        self.next_at = 0.0;
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
    let now = time.elapsed_secs();
    if now < history.next_at {
        return;
    }
    history.next_at = now + history.interval;

    let species_count = config.species_cardinality() as usize;
    let mut population = vec![0u32; species_count];
    let mut sums = vec![0.0_f32; TRAITS.len()];
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
        for (sum, t) in sums.iter_mut().zip(&TRAITS) {
            *sum += norm((t.get)(g), (t.bounds)(cfg));
        }
        n += 1;
    }

    // Zero population → we keep the last known mean genes (a graph collapsing to
    // zero would suggest the genes melted away, not that the population went
    // extinct).
    let traits = if n > 0 {
        let inv = 1.0 / n as f32;
        sums.iter().map(|s| s * inv).collect()
    } else if let Some(last) = history.samples.back() {
        last.traits.clone()
    } else {
        vec![0.0; TRAITS.len()]
    };

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

/// Curves of **gene drift** (normalized `[0, 1]`): one per [`TRAITS`]
/// characteristic, color drawn from [`trait_color`]. Fixed Y bounds `[0, 1]` on
/// the plotting side.
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
