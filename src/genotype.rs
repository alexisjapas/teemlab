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
use crate::config::SimConfig;
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
    /// Champ de vision *total*, en **radians**.
    pub vision_fov: f32,
}

impl Genotype {
    /// Génotype fondateur d'un scénario : les valeurs initiales déclarées dans la
    /// config (l'« archétype »). Le `fov` est converti une fois en radians.
    pub fn base(config: &SimConfig) -> Self {
        Self {
            max_speed: config.max_speed,
            agility: config.agility,
            vision_range: config.vision_range,
            vision_fov: config.vision_fov_deg.to_radians(),
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
    /// rayons) vient du scénario, pas du génotype.
    pub fn vision(&self, ray_count: usize) -> Vision {
        Vision {
            ray_count,
            fov: self.vision_fov,
            range: self.vision_range,
        }
    }

    /// Copie mutée pour un enfant : chaque gène reçoit une perturbation gaussienne
    /// d'écart-type `mutation_rate · étendue`, puis est ramené dans ses bornes.
    pub fn mutate(&self, rng: &mut Rng, config: &SimConfig) -> Self {
        let rate = config.mutation_rate;
        let mut jitter = |value: f32, bounds: &crate::config::Bounds| {
            bounds.clamp(value + rng.next_gaussian() * rate * bounds.span())
        };
        // Bornes du fov stockées en degrés → on mute en radians.
        let fov_bounds = crate::config::Bounds {
            min: config.vision_fov_bounds.min.to_radians(),
            max: config.vision_fov_bounds.max.to_radians(),
        };
        Self {
            max_speed: jitter(self.max_speed, &config.speed_bounds),
            agility: jitter(self.agility, &config.agility_bounds),
            vision_range: jitter(self.vision_range, &config.vision_range_bounds),
            vision_fov: fov_bounds.clamp(
                self.vision_fov + rng.next_gaussian() * rate * fov_bounds.span(),
            ),
        }
    }
}

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
        assert!((g.vision_fov - c.vision_fov_deg.to_radians()).abs() < 1e-6);
    }

    /// Une mutation reste toujours dans les bornes, même répétée et même partant
    /// d'une valeur déjà au bord.
    #[test]
    fn mutation_stays_within_bounds() {
        let mut c = config();
        c.mutation_rate = 0.5; // forte, pour stresser le clamp
        let mut rng = Rng::new(42);
        let mut g = Genotype::base(&c);
        for _ in 0..1000 {
            g = g.mutate(&mut rng, &c);
            assert!(g.max_speed >= c.speed_bounds.min && g.max_speed <= c.speed_bounds.max);
            assert!(g.agility >= c.agility_bounds.min && g.agility <= c.agility_bounds.max);
            assert!(
                g.vision_range >= c.vision_range_bounds.min
                    && g.vision_range <= c.vision_range_bounds.max
            );
            assert!(g.vision_fov >= c.vision_fov_bounds.min.to_radians() - 1e-4);
            assert!(g.vision_fov <= c.vision_fov_bounds.max.to_radians() + 1e-4);
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
}
