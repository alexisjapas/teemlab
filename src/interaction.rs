//! The **single interaction primitive** (§3: *eating and attacking are the same
//! verb*).
//!
//! The engine has only one mechanism: an actor reduces the [`Reserve`] of a
//! target within range. The *scenario* sets its semantics via its table of
//! [`Relation`]s — who acts on whom, transfer (predation) or destruction
//! (combat). Neighborhood queries go through Avian's broad-phase (no homemade
//! structure, cf. §5).

use crate::components::{Agent, Reserve, Species};
use crate::config::SimConfig;
use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// ACT (continued): resolve the tick's directed interactions.
///
/// For each actor and each relation in which it is the actor, we look for
/// targets of the right species **within reach** (Avian's broad-phase): each one
/// takes a demand of `rate · dt`, and if the relation transfers, the actor gains
/// its share of what is *actually* drawn.
///
/// **Reach = a surface-to-surface clearance.** The relation's `range` is the gap
/// between the two bodies' edges: the actor reaches a target while that gap is
/// `≤ range`, so `range = 0` means *touching* (contact). Concretely the query uses
/// a disk of radius `range + actor_radius` (the actor's species radius, fixed per
/// relation), and `shape_intersections` already accounts for the target's radius —
/// so contact happens at center distance `range + actor_radius + target_radius`.
///
/// **Conservation under contention.** When several actors target the **same**
/// target in the same tick (e.g. foragers clustered on a single patch), we
/// cannot transfer to them, in total, more than the target's available reserve.
/// We therefore proceed in **two passes**: first we accumulate the total
/// *demand* per target, then we **scale** each draw by `min(1, reserve/demand)`.
/// The target thus loses exactly `min(demand, reserve)` and each actor receives
/// its *proportional* share — never energy created out of nothing. (Without this
/// scaling, the final clamp did bound the target's **loss** but not the actors'
/// cumulative **gain**: a depleted patch could feed N foragers at its full value
/// each → runaway. Fixed-position sessile food, on which foragers cluster,
/// revealed this flaw in Phase 3b.) Both passes are **order-independent** of the
/// visiting order.
///
/// Death at zero and regeneration live in `ecology` (item 8); here we only
/// transfer/destroy reserve.
///
/// Only agents *initiate* (they have a body that moves), but a target can be any
/// entity carrying [`Species`] + [`Reserve`] — another agent (predation) **or** a
/// food source (eating thus goes through the same primitive). Colliders without
/// `Species` (walls) are ignored.
///
/// `too_many_arguments`: an ECS system — 6 real parameters, plus the **`Local`
/// buffers** reused from tick to tick (raycast filter, `hits`, `demand`,
/// `deltas`). The Bevy idiom, as on spawn functions.
#[allow(clippy::too_many_arguments)]
pub fn interact(
    spatial: SpatialQuery,
    time: Res<Time>,
    config: Res<SimConfig>,
    actors: Query<(Entity, &Transform, &Species), With<Agent>>,
    species_of: Query<&Species>,
    mut reserves: Query<&mut Reserve>,
    // Reach filter reused from one actor to the next (cf. loop): we avoid
    // reallocating an `EntityHashSet` for every actor and every tick.
    mut filter: Local<SpatialQueryFilter>,
    // Buffers for the two passes, reused from tick to tick (cf. below): cleared
    // at the top, they keep their capacity instead of reallocating a `Vec` + two
    // `HashMap`s every tick. `hits`/`demand` carry pass 1, `deltas` carries pass 2.
    mut hits: Local<Vec<(Entity, Entity, f32, bool)>>,
    mut demand: Local<HashMap<Entity, f32>>,
    mut deltas: Local<HashMap<Entity, f32>>,
) {
    if config.relations.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    // We start from empty buffers (capacity kept from the previous tick).
    hits.clear();
    demand.clear();
    deltas.clear();

    // One reach collider per relation, built **only once**: the shape depends
    // only on the relation (the actor's species radius is fixed per relation), not
    // on the individual actor. Building a parry collider is not free; doing it per
    // (actor × relation × tick) was wasteful. The radius is `range + actor_radius`
    // so the configured `range` is a surface-to-surface clearance (0 = contact).
    let reaches: Vec<Collider> = config
        .relations
        .iter()
        .map(|r| Collider::circle(r.range + config.agent_radius_of(r.actor)))
        .collect();

    // Pass 1: tally the draws (actor, target, amount, transfer) and the total
    // **demand** per target. We do not touch the reserves yet.
    for (actor, transform, species) in &actors {
        let origin = transform.translation.truncate();
        // We never act on ourselves; the filter excludes the actor (it does not
        // depend on the relation). `Local` reused: we just re-insert the excluded
        // entity, instead of rebuilding one per actor and per tick.
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
                true // keep iterating over the targets
            });
        }
    }

    // Pass 2: scale by the target's availability (conservation), then accumulate
    // the deltas. `avail` is the reserve at the start of the tick — it bounds
    // what can, in total, be drawn from the target.
    for &(actor, target, amount, transfer) in hits.iter() {
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

    for (&entity, &delta) in deltas.iter() {
        if let Ok(mut reserve) = reserves.get_mut(entity) {
            reserve.current = (reserve.current + delta).clamp(0.0, reserve.max);
        }
    }
}
