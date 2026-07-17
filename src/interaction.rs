//! The **single interaction primitive** (§3: *predation*).
//!
//! The engine has one mechanism: an actor reduces the [`Reserve`] of a target within
//! reach and gains its share. The **target filter is emergent** (SIM Law 8): an actor
//! preys on what it [`can_eat`](crate::config::SimConfig::can_eat) — a target it
//! dominates in size and finds digestible — with no authored relation table.
//! Neighborhood queries go through Avian's broad-phase (no homemade structure, cf. §5).

use crate::components::{Action, Agent, Reserve, Species};
use crate::config::SimConfig;
use crate::substrate::ComponentStore;
use avian2d::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// ACT (continued): resolve the tick's directed interactions.
///
/// For each actor, we look for entities **within reach** (Avian's broad-phase) that it
/// [`can_eat`](crate::config::SimConfig::can_eat) — size dominance ∧ digestibility, the
/// emergent filter (§3, SIM Law 8) — each takes a demand of `rate · dt`, and the actor
/// gains its share of what is *actually* drawn (predation always transfers).
///
/// **Deliberate eating (SIM Law 8).** The primitive is *brain-driven*: an actor only
/// acts this tick if its [`Action::act`] intent is positive; otherwise it abstains from
/// all its interactions. The hand-written brains hold `1.0` (reflex), the MLP learns it
/// (a 3rd output), and *holding* the intent is priced by the `act_cost` gene in
/// [`crate::ecology::metabolize`] (SIM Law 7).
///
/// **Reach = a surface-to-surface clearance.** [`Predation::range`](crate::config::Predation::range)
/// is the gap between the two bodies' edges: the actor bites while that gap is `≤ range`,
/// so `range = 0` means *touching* (contact). The query uses a disk of radius `range +
/// actor_radius` (per species), and `shape_intersections` accounts for the target's
/// radius — so contact happens at center distance `range + actor_radius + target_radius`.
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
/// **Trophic nutrient transfer (T3).** Predation carries not only energy but the
/// **components** embodied in the prey's biomass: an actor that eats a fraction `f =
/// actual/avail` of the target's reserve also receives that same fraction of each of the
/// target's [`ComponentStore`] stores — the nutrient flowing **up** the food chain. Inert when
/// the prey holds nothing; at the actor's capacity the surplus is **clamped away** (lost),
/// exactly as energy beyond `reserve.max` is — the leak **recycling** closes.
///
/// Death at zero lives in `ecology` (item 8); here we only transfer reserve (and the
/// components it carries).
///
/// Only agents *initiate* (a body that moves), but a target can be any entity carrying
/// [`Species`] + [`Reserve`] — another agent **or** a sessile food source (both go
/// through the one primitive). Colliders without `Species` (walls) are ignored.
///
/// `too_many_arguments`: an ECS system — 7 real parameters, plus the **`Local`
/// buffers** reused from tick to tick (raycast filter, `hits`, `demand`, `deltas`,
/// `nut_deltas`). The Bevy idiom, as on spawn functions.
#[allow(clippy::too_many_arguments)]
pub fn interact(
    spatial: SpatialQuery,
    time: Res<Time>,
    config: Res<SimConfig>,
    // The actor's motor command carries its **eat/attack intent** (`Action::act`):
    // deliberate eating (SIM Law 8) gates the primitive on it. Hand-written brains set
    // `1.0` (reflex → byte-identical), the MLP learns it.
    actors: Query<(Entity, &Transform, &Species, &Action), With<Agent>>,
    species_of: Query<&Species>,
    mut reserves: Query<&mut Reserve>,
    // The prey's nutrient store, carried up the chain by predation (T3). Read in
    // pass 2 (the start-of-tick amount, like `avail` for energy), written at the
    // end. Disjoint from `reserves` (a different component).
    mut nutrients: Query<&mut ComponentStore>,
    // Reach filter reused from one actor to the next (cf. loop): we avoid
    // reallocating an `EntityHashSet` for every actor and every tick.
    mut filter: Local<SpatialQueryFilter>,
    // Buffers for the two passes, reused from tick to tick (cf. below): cleared
    // at the top, they keep their capacity instead of reallocating a `Vec` + the
    // `HashMap`s every tick. `hits`/`demand` carry pass 1, `deltas`/`nut_deltas`
    // carry pass 2 (energy and the nutrient it carries).
    mut hits: Local<Vec<(Entity, Entity, f32)>>,
    mut demand: Local<HashMap<Entity, f32>>,
    mut deltas: Local<HashMap<Entity, f32>>,
    mut nut_deltas: Local<HashMap<(Entity, usize), f32>>,
    // Reach colliders, one **per species** — cached across ticks (rebuilt only when the
    // scenario changes): the shape is scenario data (radius + bite range), not per-tick.
    mut reaches: Local<Vec<Collider>>,
) {
    if config.archetypes.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    // We start from empty buffers (capacity kept from the previous tick).
    hits.clear();
    demand.clear();
    deltas.clear();
    nut_deltas.clear();

    // One reach collider **per species** (`bite range + species radius`, a
    // surface-to-surface clearance), cached across ticks and rebuilt only when the
    // scenario changes — the shape is scenario data (radius + `predation.range`), not
    // per-tick state. `config.is_changed()` fires on the first run and on any edit/reset.
    if config.is_changed() || reaches.len() != config.archetypes.len() {
        reaches.clear();
        reaches.extend(
            (0..config.archetypes.len() as u16)
                .map(|s| Collider::circle(config.predation.range + config.agent_radius_of(s))),
        );
    }

    // Pass 1: tally the draws (actor, target, amount) and the total **demand** per
    // target. **Emergent targeting** (§3, SIM Law 8): an actor bites everything in reach
    // it [`can_eat`](SimConfig::can_eat) (size dominance ∧ digestibility), no authored
    // relation table. We do not touch the reserves yet.
    let amount = config.predation.rate * dt;
    for (actor, transform, species, action) in &actors {
        // Deliberate eating (SIM Law 8): the brain gates the primitive. Without the
        // intent this tick, the actor abstains from all its interactions. The
        // hand-written brains hold `1.0` → always act.
        if action.act <= 0.0 {
            continue;
        }
        let Some(reach) = reaches.get(species.0 as usize) else {
            continue;
        };
        let origin = transform.translation.truncate();
        // We never act on ourselves; the filter excludes the actor. `Local` reused: we
        // just re-insert the excluded entity, instead of rebuilding one per actor/tick.
        filter.excluded_entities.clear();
        filter.excluded_entities.insert(actor);
        spatial.shape_intersections_callback(reach, origin, 0.0, &filter, |target| {
            if let Ok(ts) = species_of.get(target)
                && config.can_eat(species.0, ts.0)
            {
                *demand.entry(target).or_insert(0.0) += amount;
                hits.push((actor, target, amount));
            }
            true // keep iterating over the targets
        });
    }

    // Pass 2: scale by the target's availability (conservation), then accumulate the
    // deltas. Every emergent interaction is a **predation** (transfer); `avail` is the
    // reserve at the start of the tick — it bounds what can, in total, be drawn.
    for &(actor, target, amount) in hits.iter() {
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
        *deltas.entry(actor).or_insert(0.0) += actual;
        // Trophic transfer (T3): the components embodied in the eaten biomass follow the
        // energy — eating a fraction `actual/avail` of the target's reserve carries that
        // same fraction of **each** of its component stores (conservative; a no-op when
        // the prey holds nothing).
        if avail > 0.0
            && let Ok(store) = nutrients.get(target)
        {
            for c in 0..store.len() {
                let held = store.current(c).max(0.0);
                if held > 0.0 {
                    let moved = (actual / avail) * held;
                    *nut_deltas.entry((target, c)).or_insert(0.0) -= moved;
                    *nut_deltas.entry((actor, c)).or_insert(0.0) += moved;
                }
            }
        }
    }

    for (&entity, &delta) in deltas.iter() {
        if let Ok(mut reserve) = reserves.get_mut(entity) {
            reserve.current = (reserve.current + delta).clamp(0.0, reserve.max);
        }
    }

    // Apply the per-component transfers. Empty when no prey held anything → no store is
    // touched (byte-identical). `apply_delta` clamps at the actor's capacity: the
    // surplus is lost, mirroring energy beyond `reserve.max` (the "clamp & lose" choice
    // — an interim leak closed by recycling).
    for (&(entity, c), &delta) in nut_deltas.iter() {
        if let Ok(mut store) = nutrients.get_mut(entity) {
            store.apply_delta(c, delta);
        }
    }
}
