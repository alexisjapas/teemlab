//! La **primitive d'interaction unique** (§3 : *manger et attaquer sont le même
//! verbe*).
//!
//! Le moteur n'a qu'un seul mécanisme : un acteur réduit la [`Reserve`] d'une
//! cible à portée. Le *scénario* en fixe la sémantique via sa table de
//! [`Relation`]s — qui agit sur qui, transfert (prédation) ou destruction
//! (combat). Les requêtes de voisinage passent par la broad-phase d'Avian (pas
//! de structure maison, cf. §5).

use crate::components::{Agent, Reserve, Species};
use crate::config::SimConfig;
use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// AGIR (suite) : résoudre les interactions dirigées du tick.
///
/// Pour chaque acteur et chaque relation dont il est l'acteur, on cherche les
/// cibles de la bonne espèce dans un disque de rayon `range` (broad-phase
/// d'Avian), et on accumule un delta de réserve : la cible perd `rate · dt`, et
/// si la relation transfère, l'acteur gagne autant.
///
/// On **accumule d'abord, on applique ensuite** : sans cela, l'ordre de visite
/// des prédateurs sur une même proie influencerait le résultat et le borrow
/// checker refuserait la double-mutation. Les deltas étant additifs, leur somme
/// est indépendante de l'ordre (au clamp final près).
///
/// v1 : pas de mort à zéro ni de régénération — c'est l'économie d'énergie de
/// l'item 8. Ici, on ne fait qu'établir et exercer le mécanisme.
pub fn interact(
    spatial: SpatialQuery,
    time: Res<Time>,
    config: Res<SimConfig>,
    actors: Query<(Entity, &Transform, &Species), With<Agent>>,
    species_of: Query<&Species, With<Agent>>,
    mut reserves: Query<&mut Reserve, With<Agent>>,
) {
    if config.relations.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let mut deltas: HashMap<Entity, f32> = HashMap::default();

    for (actor, transform, species) in &actors {
        let origin = transform.translation.truncate();
        for relation in &config.relations {
            if relation.actor != species.0 {
                continue;
            }
            let amount = relation.rate * dt;
            let reach = Collider::circle(relation.range);
            // On ne s'inflige rien à soi-même ; le filtre exclut l'acteur.
            let filter = SpatialQueryFilter::from_excluded_entities([actor]);
            spatial.shape_intersections_callback(&reach, origin, 0.0, &filter, |target| {
                if species_of.get(target).is_ok_and(|s| s.0 == relation.target) {
                    *deltas.entry(target).or_insert(0.0) -= amount;
                    if relation.transfer {
                        *deltas.entry(actor).or_insert(0.0) += amount;
                    }
                }
                true // continuer à parcourir les cibles
            });
        }
    }

    for (entity, delta) in deltas {
        if let Ok(mut reserve) = reserves.get_mut(entity) {
            reserve.current = (reserve.current + delta).clamp(0.0, reserve.max);
        }
    }
}
