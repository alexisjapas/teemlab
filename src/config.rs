use bevy::prelude::*;

/// Paramètres d'une run. Embryon du futur *fichier de scénario* : pour l'instant
/// quelques magnitudes globales, plus tard chargé depuis du RON.
#[derive(Resource, Clone, Debug)]
pub struct SimConfig {
    /// Cadence du timestep fixe, en Hz (stabilité du solveur, pas le rendu).
    pub tick_hz: f64,
    /// Demi-côté de l'arène carrée, en unités monde.
    pub arena_half_extent: f32,
    /// Nombre d'agents au spawn.
    pub agent_count: usize,
    /// Rayon d'un agent.
    pub agent_radius: f32,
    /// Vitesse maximale d'un agent.
    pub max_speed: f32,
    /// Graine RNG : rejouer une *config d'expérience*, pas le bit-à-bit.
    pub seed: u64,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_hz: 64.0,
            arena_half_extent: 400.0,
            agent_count: 48,
            agent_radius: 8.0,
            max_speed: 140.0,
            seed: 0x00C0_FFEE,
        }
    }
}
