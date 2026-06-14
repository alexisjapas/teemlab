//! La boucle **percevoir → décider → agir**, chaînée dans `FixedUpdate`.
//!
//! Trois systèmes distincts pour garder la couture cerveau/corps nette : le
//! cerveau ne lit que [`Perception`] et n'écrit que [`Action`] ; lui seul
//! ignore tout d'Avian.

use crate::brain::Brain;
use crate::components::{Action, Agent, Locomotion, Perception, Vision};
use avian2d::prelude::*;
use bevy::prelude::*;

/// PERCEVOIR : remplir l'entrée sensorielle depuis le monde.
///
/// Vision par raycast via les *spatial queries* d'Avian (la broad-phase fait
/// office de structure de voisinage — pas de hash maison, cf. §5). Chaque agent
/// éventaille `ray_count` rayons sur son champ de vision, centrés sur son cap.
/// Le rayon ne retient que le hit le plus proche : **l'occlusion est
/// intrinsèque** (un mur masque un agent derrière lui). Le résultat est une
/// *proximité* normalisée par rayon, prête à devenir une entrée du cerveau.
///
/// La requête lit l'arbre de colliders tel qu'il était au tick précédent (la
/// physique tourne en `FixedPostUpdate`, après nous) : un décalage d'un tick,
/// sans conséquence pour de la perception.
pub fn perceive(
    spatial: SpatialQuery,
    mut agents: Query<(Entity, &Transform, &LinearVelocity, &Vision, &mut Perception), With<Agent>>,
) {
    for (entity, transform, velocity, vision, mut perception) in &mut agents {
        // Cap = direction de déplacement, repli sur +X à l'arrêt (1er tick).
        let facing = velocity.0.normalize_or_zero();
        let facing = if facing == Vec2::ZERO { Vec2::X } else { facing };
        perception.heading = facing;

        // Buffer de la bonne taille (l'espèce peut avoir changé de forme entre
        // deux runs ; au régime établi c'est un no-op).
        if perception.vision.len() != vision.ray_count {
            perception.vision = vec![0.0; vision.ray_count].into_boxed_slice();
        }

        let origin = transform.translation.truncate();
        // On ne se voit pas soi-même ; tout le reste (murs ET agents) occlut.
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);

        for i in 0..vision.ray_count {
            let dir = vision.ray_dir(i, facing);
            let Ok(direction) = Dir2::new(dir) else {
                perception.vision[i] = 0.0;
                continue;
            };
            perception.vision[i] = spatial
                .cast_ray(origin, direction, vision.range, true, &filter)
                .map(|hit| 1.0 - (hit.distance / vision.range).clamp(0.0, 1.0))
                .unwrap_or(0.0);
        }
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
