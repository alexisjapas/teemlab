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
/// d'Avian) : chacune subit une demande de `rate · dt`, et si la relation
/// transfère, l'acteur gagne sa part de ce qui est *réellement* prélevé.
///
/// **Conservation sous contention.** Quand plusieurs acteurs visent la **même**
/// cible dans le même tick (p. ex. des fourrageurs agglutinés sur un même patch),
/// on ne peut leur transférer, en tout, plus que la réserve disponible de la cible.
/// On procède donc en **deux passes** : d'abord on accumule la *demande* totale par
/// cible, puis on **met à l'échelle** chaque prélèvement par `min(1, réserve/demande)`.
/// La cible perd ainsi exactement `min(demande, réserve)` et chaque acteur reçoit sa
/// part *proportionnelle* — jamais de l'énergie créée ex nihilo. (Sans cette mise à
/// l'échelle, le clamp final bornait bien la **perte** de la cible mais pas le **gain**
/// cumulé des acteurs : un patch épuisé pouvait nourrir N fourrageurs de sa pleine
/// valeur chacun → emballement. La nourriture sessile à position fixe, sur laquelle les
/// fourrageurs s'agglutinent, a révélé ce défaut à la Phase 3b.) Les deux passes sont
/// **indépendantes de l'ordre** de visite.
///
/// La mort à zéro et la régénération vivent dans `ecology` (item 8) ; ici on ne
/// fait que transférer/détruire de la réserve.
///
/// Seuls les agents *initient* (ils ont un corps qui se meut), mais une cible
/// peut être n'importe quelle entité portant [`Species`] + [`Reserve`] — un
/// autre agent (prédation) **ou** une source de nourriture (manger passe ainsi
/// par la même primitive). Les colliders sans `Species` (murs) sont ignorés.
pub fn interact(
    spatial: SpatialQuery,
    time: Res<Time>,
    config: Res<SimConfig>,
    actors: Query<(Entity, &Transform, &Species), With<Agent>>,
    species_of: Query<&Species>,
    mut reserves: Query<&mut Reserve>,
    // Filtre de portée réutilisé d'un acteur à l'autre (cf. boucle) : on évite de
    // réallouer un `EntityHashSet` à chaque acteur et chaque tick.
    mut filter: Local<SpatialQueryFilter>,
) {
    if config.relations.is_empty() {
        return;
    }
    let dt = time.delta_secs();

    // Un collider de portée par relation, construit **une seule fois** : la forme
    // ne dépend que de la relation, pas de l'acteur. Construire un collider parry
    // n'est pas gratuit ; le faire par (acteur × relation × tick) était du gâchis.
    let reaches: Vec<Collider> = config
        .relations
        .iter()
        .map(|r| Collider::circle(r.range))
        .collect();

    // Passe 1 : recenser les prélèvements (acteur, cible, montant, transfert) et la
    // **demande** totale par cible. On ne touche pas encore aux réserves.
    let mut hits: Vec<(Entity, Entity, f32, bool)> = Vec::new();
    let mut demand: HashMap<Entity, f32> = HashMap::default();
    for (actor, transform, species) in &actors {
        let origin = transform.translation.truncate();
        // On ne s'inflige rien à soi-même ; le filtre exclut l'acteur (il ne dépend pas
        // de la relation). `Local` réutilisé : on remet juste l'entité exclue, au lieu
        // d'en reconstruire un par acteur et par tick.
        filter.excluded_entities.clear();
        filter.excluded_entities.insert(actor);
        for (relation, reach) in config.relations.iter().zip(&reaches) {
            if relation.actor != species.0 {
                continue;
            }
            let amount = relation.rate * dt;
            spatial.shape_intersections_callback(reach, origin, 0.0, &filter, |target| {
                if species_of.get(target).is_ok_and(|s| s.0 == relation.target) {
                    *demand.entry(target).or_insert(0.0) += amount;
                    hits.push((actor, target, amount, relation.transfer));
                }
                true // continuer à parcourir les cibles
            });
        }
    }

    // Passe 2 : mettre à l'échelle par disponibilité de la cible (conservation), puis
    // accumuler les deltas. `avail` est la réserve au début du tick — borne ce qu'on
    // peut, en tout, prélever sur la cible.
    let mut deltas: HashMap<Entity, f32> = HashMap::default();
    for (actor, target, amount, transfer) in hits {
        let total = demand.get(&target).copied().unwrap_or(0.0);
        if total <= 0.0 {
            continue;
        }
        let avail = reserves
            .get(target)
            .map(|r| r.current.max(0.0))
            .unwrap_or(0.0);
        let scale = if total > avail { avail / total } else { 1.0 };
        let actual = amount * scale;
        *deltas.entry(target).or_insert(0.0) -= actual;
        if transfer {
            *deltas.entry(actor).or_insert(0.0) += actual;
        }
    }

    for (entity, delta) in deltas {
        if let Ok(mut reserve) = reserves.get_mut(entity) {
            reserve.current = (reserve.current + delta).clamp(0.0, reserve.max);
        }
    }
}
