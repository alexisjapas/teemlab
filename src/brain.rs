//! Le cerveau.
//!
//! Contrat : `&Perception` (flottants normalisés) → `Action` (flottants).
//! L'intérieur est interchangeable. On stocke en **`enum`**, pas en
//! `Box<dyn>` : dispatch statique, `serde` propre, et le compilateur liste les
//! `match` à compléter quand on ajoute un type de cerveau. Le crossover est de
//! toute façon intra-type (on ne croise pas un NN avec une FSM).

use crate::components::{Action, Perception};
use crate::rng::Rng;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Le cerveau d'un agent. Un variant par implémentation.
///
/// `serde` propre (§2) : un cerveau est sérialisable, donc capturable dans un
/// snapshot de run (item 13) — et le sera pour un futur MLP sans changer le
/// contrat.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Brain {
    /// Échafaudage déterministe trivial : marche aléatoire (errance). Dérisque
    /// toute la chaîne percevoir→décider→agir avant qu'aucun cerveau appris
    /// n'existe, et servira plus tard de groupe témoin.
    Wander(WanderBrain),
}

impl Brain {
    /// Le contrat. `match` exhaustif → ajout d'un variant = erreur de compile
    /// ici, exactement ce qu'on veut.
    pub fn think(&mut self, perception: &Perception) -> Action {
        match self {
            Brain::Wander(b) => b.think(perception),
        }
    }
}

/// Errance par braquage : le cap dérive d'un petit incrément aléatoire à chaque
/// tick, produisant des trajectoires courbes plausibles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WanderBrain {
    rng: Rng,
    /// Cap courant, en radians.
    heading: f32,
    /// Amplitude maximale de la dérive de cap par tick, en radians.
    turn_rate: f32,
}

impl WanderBrain {
    pub fn new(seed: u64, initial_heading: f32) -> Self {
        Self {
            rng: Rng::new(seed),
            heading: initial_heading,
            turn_rate: 0.25,
        }
    }

    fn think(&mut self, _perception: &Perception) -> Action {
        self.heading += self.rng.next_signed() * self.turn_rate;
        Action {
            dir: Vec2::new(self.heading.cos(), self.heading.sin()),
            throttle: 1.0,
        }
    }
}
