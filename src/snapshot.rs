//! Snapshot d'une **run** : l'état vivant du monde, sérialisable (item 13).
//!
//! À distinguer du *scénario* ([`crate::config::SimConfig`]) : le scénario décrit
//! les *règles* d'une expérience, le snapshot fige une *partie en cours* — chaque
//! agent (position, génotype, énergie, cerveau) et chaque source de nourriture, de
//! quoi reprendre exactement là où on s'était arrêté.
//!
//! v1 : sérialisé en **RON** (comme tout le reste — lisible, zéro dépendance
//! nouvelle). Le §5 prévoit du binaire pour les snapshots à terme (volume, perf) ;
//! tant qu'on sauve à la main quelques centaines d'agents, RON suffit largement.
//!
//! Ce module ne contient que de la *donnée* : la capture depuis l'`World` et la
//! restauration vers l'`World` vivent côté binaire (pilotées par l'UI), en
//! réutilisant les `spawn_*` du moteur.

use serde::{Deserialize, Serialize};

use crate::brain::Brain;
use crate::config::{ScenarioError, SimConfig};
use crate::genotype::Genotype;
use crate::rng::Rng;

/// Un agent figé : tout ce qu'il faut pour le faire renaître à l'identique.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentSnap {
    pub pos: [f32; 2],
    pub genotype: Genotype,
    pub reserve: f32,
    pub species: u16,
    /// Profondeur généalogique de l'agent au moment de la capture.
    pub generation: u32,
    /// Âge (secondes simulées) au moment de la capture.
    pub age: f32,
    /// Le cerveau entier, état du RNG d'errance compris → la run reprend son cours
    /// fidèlement, pas seulement la population.
    pub brain: Brain,
}

/// Une source de nourriture figée. `species` est son **index d'archétype** (une
/// même run peut désormais porter plusieurs archétypes-nourriture distincts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FoodSnap {
    pub pos: [f32; 2],
    pub reserve: f32,
    pub species: u16,
}

/// L'état complet d'une run à un instant donné.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Les règles sous lesquelles la run tournait : restaurées avec elle pour que
    /// bornes, relations et coûts collent aux agents sauvegardés.
    pub config: SimConfig,
    /// État du flux aléatoire de sim (repousse de nourriture, mutations).
    pub sim_rng: Rng,
    /// Reliquat fractionnaire de repousse de nourriture, **par archétype**.
    pub food_regen: Vec<f32>,
    pub agents: Vec<AgentSnap>,
    pub food: Vec<FoodSnap>,
}

impl Snapshot {
    /// Sérialise en RON lisible.
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Désérialise depuis une chaîne RON.
    pub fn from_ron_str(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Écrit le snapshot dans un fichier RON.
    pub fn save_ron_file(&self, path: impl AsRef<std::path::Path>) -> Result<(), ScenarioError> {
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Charge un snapshot depuis un fichier RON.
    pub fn from_ron_file(path: impl AsRef<std::path::Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path)?;
        Ok(Self::from_ron_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::WanderBrain;

    /// Aller-retour RON : un snapshot se relit à l'identique, cerveau compris.
    #[test]
    fn snapshot_ron_roundtrip_is_lossless() {
        let config = SimConfig::default();
        let snap = Snapshot {
            config: config.clone(),
            sim_rng: Rng::new(0xABCD),
            food_regen: vec![0.42],
            agents: vec![AgentSnap {
                pos: [12.5, -7.0],
                genotype: Genotype::default(),
                reserve: 73.0,
                species: 0,
                generation: 3,
                age: 4.5,
                brain: Brain::Wander(WanderBrain::new(99, 1.2, 0.25)),
            }],
            food: vec![FoodSnap {
                pos: [-3.0, 4.0],
                reserve: 21.0,
                species: 1,
            }],
        };
        let text = snap.to_ron_string().expect("sérialisation RON");
        let back = Snapshot::from_ron_str(&text).expect("relecture RON");
        assert_eq!(snap, back);
    }
}
