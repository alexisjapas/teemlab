//! **Métriques d'observation** : l'historique glissant des courbes et les statistiques
//! en direct — la *donnée* d'affichage, partagée par les deux backends qui la rendent
//! (egui dans le binaire fenêtré, natif Bevy dans [`crate::dataviz`]).
//!
//! Vit dans la lib (et non plus dans le binaire fenêtré) pour que l'enregistreur vidéo
//! ([`crate::dataviz`] côté `record`) échantillonne et trace **exactement** les mêmes
//! courbes/stats que l'aperçu live — un seul calcul de donnée, deux tracés.
//!
//! Strictement de l'**observation** : tout tourne dans `Update` (jamais `FixedUpdate`),
//! lecture seule du monde — la sim reste byte-identique (invariant cardinal).
//!
//! L'échantillonnage se cale sur `Time<Virtual>` : il se fige avec la pause et suit
//! l'accéléré, comme la sim (§6).

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::brain::Brain;
use crate::components::{Agent, Reserve, Species};
use crate::config::{Bounds, SimConfig};
use crate::genotype::{Genotype, TRAITS};

/// Ajoute l'échantillonnage des courbes (ressource [`History`] + système
/// [`sample_history`]). À combiner avec un backend de tracé (egui ou [`crate::dataviz`]).
pub struct MetricsPlugin;

impl Plugin for MetricsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<History>()
            .add_systems(Update, sample_history);
    }
}

/// Un instantané de métriques, daté en temps simulé.
struct Sample {
    /// Temps simulé (`Time<Virtual>`) de l'échantillon, en secondes.
    t: f32,
    /// Population vivante par espèce (indexée comme `Species`).
    population: Vec<u32>,
    /// Sources de nourriture présentes (somme des espèces **sessiles**, Phase 3b).
    food: u32,
    /// Gènes moyens, un par caractéristique de [`TRAITS`] (même ordre), chacun
    /// **normalisé dans ses bornes** (`[0, 1]`) pour que des traits d'échelles
    /// différentes (vitesse vs angle) se comparent sur un seul graphe.
    traits: Vec<f32>,
}

/// Historique glissant des métriques. Partagé par les deux backends de tracé.
#[derive(Resource)]
pub struct History {
    /// Intervalle entre deux échantillons, en secondes simulées.
    interval: f32,
    /// Nombre maximal d'échantillons conservés (fenêtre glissante).
    max_samples: usize,
    /// Prochain instant d'échantillonnage (temps simulé).
    next_at: f32,
    /// Les échantillons, du plus ancien au plus récent.
    samples: VecDeque<Sample>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            interval: 0.5,
            max_samples: 1200, // 0,5 s × 1200 = 10 min de temps simulé
            next_at: 0.0,
            samples: VecDeque::new(),
        }
    }
}

impl History {
    /// Repart de zéro : vide les échantillons et réarme l'horloge. Appelé par le
    /// bouton « Effacer » du HUD et par la réinitialisation à chaud (item 11).
    pub fn clear(&mut self) {
        self.samples.clear();
        self.next_at = 0.0;
    }

    /// Nombre d'échantillons conservés (pour l'affichage « N échantillons »).
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Vrai tant qu'aucun échantillon n'a été pris (rien à tracer).
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Normalise une valeur de gène dans ses bornes, vers `[0, 1]`.
fn norm(v: f32, b: Bounds) -> f32 {
    if b.span() > 0.0 {
        ((v - b.min) / b.span()).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Échantillonne les métriques du monde à cadence fixe en temps simulé. Lecture
/// seule : c'est de l'observation pour affichage, pas de la logique de sim — d'où
/// sa place légitime dans `Update`.
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
        // Moyennes de gènes sur la **faune** seule : les sources sessiles (gènes figés,
        // souvent en grand nombre) écraseraient la dérive de la faune. Elles comptent
        // dans la population/nourriture, pas ici.
        if cfg.archetypes.get(idx).is_some_and(|a| a.is_sessile()) {
            continue;
        }
        for (sum, t) in sums.iter_mut().zip(&TRAITS) {
            *sum += norm((t.get)(g), (t.bounds)(cfg));
        }
        n += 1;
    }

    // Population zéro → on garde les derniers gènes moyens connus (un graphe qui
    // s'effondre à zéro laisserait croire que les gènes ont fondu, pas que la
    // population s'est éteinte).
    let traits = if n > 0 {
        let inv = 1.0 / n as f32;
        sums.iter().map(|s| s * inv).collect()
    } else if let Some(last) = history.samples.back() {
        last.traits.clone()
    } else {
        vec![0.0; TRAITS.len()]
    };

    // « Nourriture » = somme des espèces sessiles (sources/flore), dérivée de la
    // population par espèce (Phase 3b : plus de marqueur `Food` à compter).
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
// Donnée d'affichage partagée (anti-divergence egui ↔ Bevy natif)
// ---------------------------------------------------------------------------

/// Statistiques globales en direct — les mêmes nombres pour la barre egui
/// ([`stats_section`](../editor/fn.stats_section.html)) et le visualiseur natif.
/// Calculées sur la **faune** (les sources sessiles comptent dans `food`, pas dans
/// les moyennes — sinon leurs gènes figés écraseraient la dérive).
pub struct LiveStats {
    /// Agents mobiles vivants.
    pub population: usize,
    /// Sources sessiles (flore / nourriture).
    pub food: usize,
    /// Réserve moyenne de la faune.
    pub mean_reserve: f32,
    /// Moyenne de chaque gène de [`TRAITS`] (même ordre), sur la faune (valeur brute).
    pub mean_traits: Vec<f32>,
}

/// Calcule [`LiveStats`] en une passe sur les agents. Le filtre « mobile vs sessile »
/// (cerveau [`Brain::Sessile`]) est l'unique source de vérité partagée par les deux
/// backends.
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

/// Une courbe à tracer : un nom, une couleur **sRGB** `[r, g, b] ∈ [0, 1]` (agnostique
/// au backend), et ses points `[temps, valeur]`. egui et Bevy ne font que la tracer.
pub struct Curve {
    pub name: String,
    pub color: [f32; 3],
    pub pts: Vec<[f32; 2]>,
}

/// Courbes de **population par espèce** + l'agrégat « nourriture » (somme des sessiles).
/// Renvoie aussi le `y_max` observé (≥ 1). On ne trace QUE les espèces qui existent (ou
/// ont existé) sur la fenêtre : un archétype défini mais jamais peuplé n'ajoute pas une
/// courbe à zéro. Les sessiles sont agrégées dans « nourriture », pas tracées seules.
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
            .unwrap_or_else(|| format!("espèce {sp}"));
        curves.push(Curve {
            name,
            color: config.color_of(sp as u16),
            pts,
        });
    }

    // « Nourriture » = somme des sessiles, tracée seulement si une source a existé.
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
            name: "nourriture".to_string(),
            color: [0.59, 0.59, 0.59],
            pts,
        });
    }

    (curves, y_max)
}

/// Courbes de **dérive des gènes** (normalisées `[0, 1]`) : une par caractéristique de
/// [`TRAITS`], couleur tirée de [`trait_color`]. Bornes Y fixes `[0, 1]` côté tracé.
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

/// Couleur sRGB de la courbe du trait d'indice `i` (palette du HUD ; la couleur est
/// une affaire d'affichage, donc elle vit ici et non dans [`TRAITS`]).
pub fn trait_color(i: usize) -> [f32; 3] {
    const PALETTE: [[f32; 3]; 9] = [
        [0.47, 0.78, 1.00], // bleu
        [1.00, 0.67, 0.35], // orange
        [0.59, 0.90, 0.47], // vert
        [0.86, 0.55, 0.90], // mauve
        [0.94, 0.86, 0.47], // jaune
        [0.47, 0.90, 0.86], // cyan
        [0.92, 0.51, 0.51], // rouge
        [0.71, 0.71, 0.71], // gris clair
        [0.78, 0.63, 0.43], // brun
    ];
    PALETTE[i % PALETTE.len()]
}
