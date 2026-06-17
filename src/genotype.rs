//! Le **génotype** : la description héritée et mutable d'un agent.
//!
//! §2 — *génotype ≠ phénotype*. On mute le génotype (cette structure de gènes),
//! puis on le **compile vers le phénotype vivant** (composants [`Locomotion`],
//! [`Vision`], …) au spawn. L'évolution ne touche jamais l'état physique en
//! cours : elle réécrit la recette, pas le plat.
//!
//! v1 — *forme verrouillée par espèce* : les gènes font varier les **magnitudes**
//! (portée de vision, vitesse, …), jamais le **nombre** de canaux sensoriels
//! (qui reste `vision_rays` du scénario). La topologie variable (NEAT) est le
//! mode hard, repoussé à l'item 16.
//!
//! Chaque gène forme avec ses bornes ([`crate::config::Bounds`]) et son couplage
//! de coût (l'économie d'énergie) le triplet du §2.

use crate::components::{Locomotion, Vision};
use crate::config::{Bounds, Heritability, SimConfig};
use crate::rng::Rng;
use bevy::prelude::*;

/// Les gènes d'un agent. Composant (porté par l'agent vivant, hérité par ses
/// enfants) **et** « génome » sérialisable d'une instance — la distinction
/// archétype (config) / génome (instance) de l'item 5.
#[derive(Component, Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Genotype {
    /// Vitesse maximale.
    pub max_speed: f32,
    /// Vivacité du braquage, dans `[0, 1]`.
    pub agility: f32,
    /// Portée de vision.
    pub vision_range: f32,
    /// Champ de vision *total*, en **degrés** — l'unité du designer (config,
    /// éditeur, bornes). Converti en radians au seul point de compilation
    /// phénotype (cf. [`Genotype::vision`]).
    pub vision_fov_deg: f32,
    /// Énergie à atteindre pour se reproduire. `0` → cet agent ne se reproduit
    /// pas. Caractéristique d'entité (§1, *le corps*) → la stratégie de
    /// reproduction est elle-même sélectionnable.
    pub reproduction_threshold: f32,
    /// Énergie passée à l'enfant, déduite du parent (conservation : rien créé).
    pub offspring_energy: f32,
    /// Taux de mutation transmis à la descendance (écart-type, en fraction de
    /// l'étendue d'un gène). Le gène qui pilote sa propre lignée. **Non héritable
    /// par défaut** ([`crate::config::Heritability`]) : laissé héritable, il dérive
    /// (méta-évolution) et peut se figer à 0 → évolution morte.
    pub mutation_rate: f32,
}

impl Genotype {
    /// Génotype fondateur d'un scénario : les valeurs initiales déclarées dans la
    /// config (l'« archétype »). Chaque gène dans son unité de stockage — le fov
    /// reste en degrés, à l'identique de la config.
    pub fn base(config: &SimConfig) -> Self {
        Self {
            max_speed: config.max_speed,
            agility: config.agility,
            vision_range: config.vision_range,
            vision_fov_deg: config.vision_fov_deg,
            reproduction_threshold: config.reproduction_threshold,
            offspring_energy: config.offspring_energy,
            mutation_rate: config.mutation_rate,
        }
    }

    /// Compile le gène de locomotion vers son phénotype.
    pub fn locomotion(&self) -> Locomotion {
        Locomotion {
            max_speed: self.max_speed,
            agility: self.agility,
        }
    }

    /// Compile les gènes de vision vers son phénotype. La *forme* (nombre de
    /// rayons) vient du scénario, pas du génotype. **Seul point** où le fov passe
    /// des degrés (gène) aux radians (phénotype, attendus par le raycast).
    pub fn vision(&self, ray_count: usize) -> Vision {
        Vision {
            ray_count,
            fov: self.vision_fov_deg.to_radians(),
            range: self.vision_range,
        }
    }

    /// Copie mutée pour un enfant : chaque gène **héritable** de la table
    /// [`TRAITS`] reçoit une perturbation gaussienne d'écart-type
    /// `mutation_rate · étendue`, puis est ramené dans ses bornes ; un gène non
    /// héritable reste à la valeur d'archétype. Boucle générique → ajouter un
    /// trait n'y touche pas. Tous les gènes sont dans l'unité de leurs bornes
    /// (fov en degrés), donc un seul chemin, sans conversion.
    ///
    /// Le taux vient **du génotype** (`self.mutation_rate`), pas d'un réglage
    /// global : chaque lignée porte sa propre vitesse d'évolution.
    pub fn mutate(&self, rng: &mut Rng, config: &SimConfig) -> Self {
        let rate = self.mutation_rate;
        let mut child = *self;
        for t in &TRAITS {
            // Trait non héritable : l'enfant garde la valeur d'archétype (déjà
            // copiée dans `child`) et ne consomme aucun tirage.
            if !(t.heritable)(&config.heritable) {
                continue;
            }
            let bounds = (t.bounds)(config);
            let drift = rng.next_gaussian() * rate * bounds.span();
            (t.set)(&mut child, bounds.clamp((t.get)(self) + drift));
        }
        child
    }
}

/// Le descripteur d'**une** caractéristique héritable : le triplet du §2 —
/// (valeur, bornes, …) — rendu *itérable*. La table [`TRAITS`] en est la source
/// de vérité unique ; les pilotes (mutation, éditeur, HUD, inspecteur) bouclent
/// dessus au lieu d'énumérer les gènes à la main. Ajouter un trait = une entrée
/// ici (+ un champ de [`Genotype`] et ses bornes en config) ; aucun pilote à
/// rééditer — c'est ce que l'item 15 falsifie contre la pluralité existante.
pub struct TraitSpec {
    /// Libellé pour l'éditeur et le HUD.
    pub name: &'static str,
    /// Valeur du gène dans le génotype (lecture).
    pub get: fn(&Genotype) -> f32,
    /// Valeur du gène dans le génotype (écriture).
    pub set: fn(&mut Genotype, f32),
    /// Bornes du gène, lues dans le scénario.
    pub bounds: fn(&SimConfig) -> Bounds,
    /// Le facet « héritable ? » de ce trait dans le scénario (lecture).
    pub heritable: fn(&Heritability) -> bool,
    /// Le facet « héritable ? » de ce trait (écriture, pour l'éditeur).
    pub set_heritable: fn(&mut Heritability, bool),
    /// Décimales d'affichage (inspecteur).
    pub decimals: u8,
}

/// Les caractéristiques héritables, **dans l'ordre des champs de [`Genotype`]**
/// (cet ordre fixe le flux de tirages de [`Genotype::mutate`], donc la
/// reproductibilité d'une config seedée). v1 — forme verrouillée par espèce —
/// une table constante partagée par tous les agents.
pub const TRAITS: [TraitSpec; 7] = [
    TraitSpec {
        name: "Vitesse max",
        get: |g| g.max_speed,
        set: |g, v| g.max_speed = v,
        bounds: |c| c.speed_bounds,
        heritable: |h| h.max_speed,
        set_heritable: |h, b| h.max_speed = b,
        decimals: 1,
    },
    TraitSpec {
        name: "Agilité",
        get: |g| g.agility,
        set: |g, v| g.agility = v,
        bounds: |c| c.agility_bounds,
        heritable: |h| h.agility,
        set_heritable: |h, b| h.agility = b,
        decimals: 3,
    },
    TraitSpec {
        name: "Portée vision",
        get: |g| g.vision_range,
        set: |g, v| g.vision_range = v,
        bounds: |c| c.vision_range_bounds,
        heritable: |h| h.vision_range,
        set_heritable: |h, b| h.vision_range = b,
        decimals: 1,
    },
    TraitSpec {
        name: "Champ vision (°)",
        get: |g| g.vision_fov_deg,
        set: |g, v| g.vision_fov_deg = v,
        bounds: |c| c.vision_fov_bounds,
        heritable: |h| h.vision_fov,
        set_heritable: |h, b| h.vision_fov = b,
        decimals: 0,
    },
    TraitSpec {
        name: "Seuil de repro",
        get: |g| g.reproduction_threshold,
        set: |g, v| g.reproduction_threshold = v,
        bounds: |c| c.reproduction_threshold_bounds,
        heritable: |h| h.reproduction_threshold,
        set_heritable: |h, b| h.reproduction_threshold = b,
        decimals: 0,
    },
    TraitSpec {
        name: "Énergie/enfant",
        get: |g| g.offspring_energy,
        set: |g, v| g.offspring_energy = v,
        bounds: |c| c.offspring_energy_bounds,
        heritable: |h| h.offspring_energy,
        set_heritable: |h, b| h.offspring_energy = b,
        decimals: 0,
    },
    TraitSpec {
        name: "Taux mutation",
        get: |g| g.mutation_rate,
        set: |g, v| g.mutation_rate = v,
        bounds: |c| c.mutation_rate_bounds,
        heritable: |h| h.mutation_rate,
        set_heritable: |h, b| h.mutation_rate = b,
        decimals: 3,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SimConfig {
        SimConfig::default()
    }

    /// Le génotype fondateur reflète les valeurs de la config, fov en radians.
    #[test]
    fn base_reads_config() {
        let c = config();
        let g = Genotype::base(&c);
        assert_eq!(g.max_speed, c.max_speed);
        assert_eq!(g.vision_fov_deg, c.vision_fov_deg);
        assert_eq!(g.reproduction_threshold, c.reproduction_threshold);
        assert_eq!(g.mutation_rate, c.mutation_rate);
    }

    /// Toute mutation laisse **chaque** gène de [`TRAITS`] dans ses bornes — même
    /// répétée, même partant d'une valeur au bord. Générique : un nouveau trait
    /// est couvert sans toucher ce test.
    #[test]
    fn mutation_stays_within_bounds() {
        let mut c = config();
        c.mutation_rate = 0.4; // forte, pour stresser le clamp
        let mut rng = Rng::new(42);
        let mut g = Genotype::base(&c);
        for _ in 0..1000 {
            g = g.mutate(&mut rng, &c);
            for t in &TRAITS {
                let b = (t.bounds)(&c);
                let v = (t.get)(&g);
                assert!(
                    v >= b.min - 1e-4 && v <= b.max + 1e-4,
                    "{} hors bornes : {v}",
                    t.name
                );
            }
        }
    }

    /// Mutation nulle = clone fidèle (régime évolution éteinte).
    #[test]
    fn zero_mutation_is_identity() {
        let c = config(); // mutation_rate = 0
        let mut rng = Rng::new(1);
        let g = Genotype::base(&c);
        assert_eq!(g.mutate(&mut rng, &c), g);
    }

    /// Le facet « héritable ? » : un trait marqué non héritable reste figé sur la
    /// valeur d'archétype au fil des générations, alors que les héritables dérivent.
    #[test]
    fn non_heritable_trait_stays_fixed() {
        let mut c = config();
        c.mutation_rate = 0.4; // forte mutation, pour que la dérive soit nette
        c.heritable.max_speed = false; // figé
        let mut rng = Rng::new(7);
        let base = Genotype::base(&c);
        let mut g = base;
        let mut drifted = false;
        for _ in 0..200 {
            g = g.mutate(&mut rng, &c);
            assert_eq!(g.max_speed, base.max_speed, "trait non héritable figé");
            if (g.vision_range - base.vision_range).abs() > 1e-3 {
                drifted = true;
            }
        }
        assert!(drifted, "un trait héritable doit, lui, dériver");
    }

    /// Le taux de mutation est désormais un gène **de l'entité** : la mutation lit
    /// `self.mutation_rate`, pas un réglage global. Un génotype à taux nul ne
    /// dérive donc pas, quoi que dise la config.
    #[test]
    fn mutation_rate_is_per_genotype() {
        let mut c = config();
        c.mutation_rate = 0.5; // « global » élevé...
        let mut rng = Rng::new(3);
        let mut g = Genotype::base(&c);
        g.mutation_rate = 0.0; // ...mais CE génotype ne mute pas.
        let before = g;
        for _ in 0..50 {
            g = g.mutate(&mut rng, &c);
        }
        assert_eq!(g, before, "un génotype à taux nul reste identique");
    }
}
