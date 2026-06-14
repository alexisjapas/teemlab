//! Composants du *corps* d'un agent.
//!
//! [`Perception`] et [`Action`] matérialisent le contrat du cerveau :
//! *flottants normalisés en entrée → flottants en sortie*. C'est le corps qui
//! impose la forme de ces I/O.

use bevy::prelude::*;

/// Marqueur d'un agent simulé.
#[derive(Component)]
pub struct Agent;

/// Marqueur d'un mur statique de l'arène.
#[derive(Component)]
pub struct Wall;

/// Rayon du corps. Composant explicite pour que le code de rendu dimensionne
/// le mesh sans fouiller dans le collider Avian.
#[derive(Component, Clone, Copy, Debug)]
pub struct Radius(pub f32);

/// Magnitudes de locomotion — ce que les gènes feront varier (v1 : fixe).
#[derive(Component, Clone, Copy, Debug)]
pub struct Locomotion {
    /// Vitesse maximale.
    pub max_speed: f32,
    /// Vivacité du braquage vers la vitesse désirée, dans `[0, 1]`.
    pub agility: f32,
}

/// Instantané sensoriel normalisé. Écrit par `perceive`, lu par `decide`.
/// Conceptuellement le vecteur d'entrée du cerveau.
#[derive(Component, Default)]
pub struct Perception {
    /// Cap courant en vecteur unitaire (nul à l'arrêt).
    pub heading: Vec2,
}

/// Commande motrice. Écrite par `decide`, lue par `act`.
/// Conceptuellement le vecteur de sortie du cerveau.
#[derive(Component, Default)]
pub struct Action {
    /// Direction de déplacement désirée (quasi-unitaire).
    pub dir: Vec2,
    /// Fraction désirée de la vitesse max, dans `[0, 1]`.
    pub throttle: f32,
}
