//! La boucle **percevoir → décider → agir**, chaînée dans `FixedUpdate`.
//!
//! Trois systèmes distincts pour garder la couture cerveau/corps nette : le
//! cerveau ne lit que [`Perception`] et n'écrit que [`Action`] ; lui seul
//! ignore tout d'Avian.

use crate::brain::Brain;
use crate::components::{Action, Agent, Locomotion, Perception};
use avian2d::prelude::*;
use bevy::prelude::*;

/// PERCEVOIR : remplir l'entrée sensorielle depuis le monde.
/// (P0 : juste la direction de déplacement courante de l'agent.)
pub fn perceive(mut agents: Query<(&LinearVelocity, &mut Perception), With<Agent>>) {
    for (velocity, mut perception) in &mut agents {
        perception.heading = velocity.0.normalize_or_zero();
    }
}

/// DÉCIDER : faire tourner chaque cerveau sur sa perception → commande motrice.
pub fn decide(mut agents: Query<(&mut Brain, &Perception, &mut Action)>) {
    for (mut brain, perception, mut action) in &mut agents {
        *action = brain.think(perception);
    }
}

/// AGIR : traduire la commande en mouvement, borné par les magnitudes du corps.
///
/// On braque la vitesse vers la vitesse désirée (lerp), au lieu de l'imposer :
/// les impulsions de collision d'Avian perturbent alors visiblement la
/// trajectoire avant que le cerveau ne re-corrige.
pub fn act(mut agents: Query<(&Action, &Locomotion, &mut LinearVelocity)>) {
    for (action, loco, mut velocity) in &mut agents {
        let desired = action.dir.normalize_or_zero() * loco.max_speed * action.throttle;
        velocity.0 = velocity.0.lerp(desired, loco.agility);
    }
}
