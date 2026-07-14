//! The **perceive → decide → act** loop, chained in `FixedUpdate`.
//!
//! Three distinct systems to keep the brain/body seam clean: the brain only
//! reads [`Perception`] and only writes [`Action`]; it alone knows nothing of
//! Avian.

use crate::brain::Brain;
use crate::components::{
    Action, Agent, Anchor, Locomotion, Maneuver, Perception, Reserve, Species, Vision,
};
use crate::config::SimConfig;
use crate::nutrients::Nutrients;
use avian2d::prelude::*;
use bevy::prelude::*;

/// PERCEIVE: fill the sensory input from the world.
///
/// Raycast vision via Avian's *spatial queries* (the broad-phase serves as the
/// neighborhood structure — no homemade hash, cf. §5). Each agent fans out
/// `ray_count` rays over its field of view, centered on its heading. The ray
/// keeps only the nearest hit: **occlusion is intrinsic** (a wall hides an agent
/// behind it). The result is a normalized *proximity* per ray, ready to become a
/// brain input.
///
/// The query reads the collider tree as it was at the previous tick (physics
/// runs in `FixedPostUpdate`, after us): a one-tick lag, inconsequential for
/// perception.
pub fn perceive(
    spatial: SpatialQuery,
    config: Res<SimConfig>,
    fields: Res<crate::nutrients::Fields>,
    mut agents: Query<
        (
            Entity,
            &Transform,
            &LinearVelocity,
            &Species,
            &Vision,
            &Locomotion,
            &Reserve,
            &Nutrients,
            &mut Perception,
        ),
        With<Agent>,
    >,
    species_of: Query<&Species>,
    // Raycast filter reused from one agent to the next (cf. loop): we avoid
    // reallocating an `EntityHashSet` for every agent and every tick.
    mut filter: Local<SpatialQueryFilter>,
    // Per-species sensed-component lists, cached across ticks and rebuilt on a
    // config change (the `interact` reach-collider pattern): `sensed_components`
    // allocates + sorts, and calling it per agent per tick was the field-sense
    // scenarios' one remaining per-tick allocation. Same values → byte-identical.
    mut sensed_cache: Local<Vec<Vec<usize>>>,
) {
    // Any field-sense relation in the scenario? Computed once; a scenario with none
    // does no per-agent field-sense work (byte-identical, no perf cost).
    let any_sense = config.field_relations.iter().any(|f| f.sense);
    if any_sense && (config.is_changed() || sensed_cache.len() != config.archetypes.len()) {
        sensed_cache.clear();
        sensed_cache
            .extend((0..config.archetypes.len()).map(|s| config.sensed_components(s as u16)));
    }
    for (entity, transform, velocity, species, vision, loco, reserve, nutrients, mut perception) in
        &mut agents
    {
        // An **immobile** entity (flora / sessile source) casts no ray: without a
        // heading or locomotion, its vision is unusable (its brain ignores it).
        // We therefore skip it — we do not write its perception (nothing reads it
        // numerically: its action stays zero, its energy depends only on
        // photosynthesis and predation), which spares `ray_count` raycasts per
        // tick and per plant. The sim therefore stays rigorously unchanged; only
        // useless rays disappear.
        if loco.is_immobile() {
            continue;
        }
        // Heading = movement direction, falling back to +X when stopped (1st tick).
        let facing = velocity.0.normalize_or_zero();
        let facing = if facing == Vec2::ZERO {
            Vec2::X
        } else {
            facing
        };
        perception.heading = facing;

        // PROPRIOCEPTION: the agent's own internal state (scalar channels), so a
        // brain can modulate on itself (eat when hungry, not on contact —
        // `docs/persistent-ecosystems.md` §2). Order fixed by [`Perception`]:
        // energy fraction, nutrient fraction, speed fraction. `max_speed > 0` here
        // (the immobile check above already returned), so the division is safe.
        // A read-only fill: the hand-written brains ignore it and no RNG is drawn →
        // non-MLP scenarios stay byte-identical (only the MLP reads these channels).
        perception.self_state = [
            reserve.fraction(),
            nutrients.fraction(),
            (velocity.0.length() / loco.max_speed).clamp(0.0, 1.0),
        ];

        // FIELD SENSE (pheromones / chemoreception): the local concentration of each
        // component the species senses, saturating-normalized to `[0, 1)` (`c/(c+1)` —
        // scenario-independent, monotonic), appended after `self_state` in the MLP
        // input. Read-only, no RNG → a non-sensing species keeps an empty `field_state`
        // and the input is byte-identical.
        if any_sense {
            let sensed: &[usize] = sensed_cache
                .get(species.0 as usize)
                .map_or(&[], |v| v.as_slice());
            if perception.field_state.len() != sensed.len() {
                perception.field_state = vec![0.0; sensed.len()].into_boxed_slice();
            }
            let pos = transform.translation.truncate();
            for (k, &c) in sensed.iter().enumerate() {
                let conc = fields.get(c).map(|f| f.sample(pos)).unwrap_or(0.0).max(0.0);
                perception.field_state[k] = conc / (conc + 1.0);
            }
        }

        // Buffers of the right size (the species may have changed shape between
        // two runs; at steady state this is a no-op). The three channels share
        // the `ray_count` cardinality.
        if perception.vision.len() != vision.ray_count {
            perception.vision = vec![0.0; vision.ray_count].into_boxed_slice();
            perception.target = vec![0.0; vision.ray_count].into_boxed_slice();
            perception.threat = vec![0.0; vision.ray_count].into_boxed_slice();
            perception.ray_dirs = vec![Vec2::ZERO; vision.ray_count].into_boxed_slice();
        }

        let origin = transform.translation.truncate();
        // We do not see ourselves; everything else (walls AND agents) occludes.
        // The filter is a reused `Local`: we just re-insert the excluded entity
        // (the current agent), instead of rebuilding one per agent and per tick.
        filter.excluded_entities.clear();
        filter.excluded_entities.insert(entity);

        // Heading converted to an angle **once** per agent; `ray_dir_from_angle`
        // adds each ray's offset without redoing the atan2 (cf. `Vision::ray_dir`).
        let base_angle = facing.to_angle();
        for i in 0..vision.ray_count {
            let dir = vision.ray_dir_from_angle(i, base_angle);
            perception.ray_dirs[i] = dir;
            let Ok(direction) = Dir2::new(dir) else {
                perception.vision[i] = 0.0;
                perception.target[i] = 0.0;
                perception.threat[i] = 0.0;
                continue;
            };
            match spatial.cast_ray(origin, direction, vision.range, true, &filter) {
                Some(hit) => {
                    let proximity = 1.0 - (hit.distance / vision.range).clamp(0.0, 1.0);
                    perception.vision[i] = proximity;
                    // "target" and "threat" channels, from the **emergent** filter
                    // (§3, SIM Law 8): we read the nearest hit's species once, and
                    // `can_eat` decides both directions — we can eat it (target, graded
                    // by how digestible/appetising it is) or it can eat us (threat, we
                    // flee). A wall (no [`Species`]) or a size/diet mismatch either way →
                    // both at 0.
                    let (target, threat) = species_of.get(hit.entity).map_or((0.0, 0.0), |hs| {
                        let target = if config.can_eat(species.0, hs.0) {
                            proximity * config.digestibility(species.0, hs.0)
                        } else {
                            0.0
                        };
                        let threat = if config.can_eat(hs.0, species.0) {
                            proximity
                        } else {
                            0.0
                        };
                        (target, threat)
                    });
                    perception.target[i] = target;
                    perception.threat[i] = threat;
                }
                None => {
                    perception.vision[i] = 0.0;
                    perception.target[i] = 0.0;
                    perception.threat[i] = 0.0;
                }
            }
        }
    }
}

/// DECIDE: run each brain on its perception → motor command.
pub fn decide(mut agents: Query<(&mut Brain, &Perception, &mut Action)>) {
    for (mut brain, perception, mut action) in &mut agents {
        *action = brain.think(perception);
    }
}

/// ACT: translate the command into movement, bounded by the body's magnitudes.
///
/// We steer the velocity toward the desired velocity (lerp), instead of forcing
/// it: Avian's collision impulses then visibly perturb the trajectory before the
/// brain re-corrects.
// Anchored bodies (Feature 2) are **excluded** (`Without<Anchor>`): their velocity is
// governed by [`anchor_spring`] + the physics solver, not this locomotion override — so
// the spring can actually displace them (else this would reset their velocity each tick).
pub fn act(
    mut agents: Query<(&Action, &Locomotion, &mut LinearVelocity, &mut Maneuver), Without<Anchor>>,
) {
    for (action, loco, mut velocity, mut maneuver) in &mut agents {
        let desired = action.dir.normalize_or_zero() * loco.max_speed * action.throttle;
        let before = velocity.0;
        velocity.0 = before.lerp(desired, loco.agility);
        // Voluntary steering effort = magnitude of the velocity change we just
        // applied (the agility cost reads it in `metabolize`). Computed here, where
        // both ends of the lerp are known; collision impulses from the solver land
        // afterwards and are deliberately **not** attributed to maneuvering.
        maneuver.0 = (velocity.0 - before).length();
    }
}

/// ANCHORING (Feature 2): hold a **rooted** body to its [`Anchor`] point by a spring, and
/// **tear it off** when pulled too hard. A sessile organism (plant, coral, barnacle) is
/// not pinned rigidly — it can be jostled by a grazer or a crowd and springs back — but
/// past a **breaking tension** the anchor snaps.
///
/// Portable by construction (the whole point, §9): the tear criterion is a **tension**,
/// `stiffness · |pos − anchor|`, a mass-free quantity — unlike a contact impulse
/// (∝ mass ∝ radius²), the non-portability that shelved the `crush`. On tear-off, per the
/// scenario's [`AnchorConfig::die_on_detach`]:
/// - `true` → **death**: zero the reserve so the *uniform* death path
///   ([`crate::ecology::reap`], next in the chain) despawns it and fires `emit_at_death` /
///   recycling — an uprooted body becomes detritus, the physical **turnover** lever
///   (Law 11: one death rule, no special-casing).
/// - `false` → **release**: drop the [`Anchor`] and let it become a free body.
///
/// Below the threshold, a restoring velocity impulse (`−stiffness · displacement · dt`,
/// mass-free) pulls it home; `LinearDamping` (set at spawn) settles the oscillation, while
/// collisions from the solver still push it around. No anchored body (every existing
/// scenario) → the query is empty → a no-op, **byte-identical**.
///
/// [`AnchorConfig::die_on_detach`]: crate::config::AnchorConfig::die_on_detach
pub fn anchor_spring(
    mut commands: Commands,
    config: Res<SimConfig>,
    time: Res<Time>,
    mut anchored: Query<
        (
            Entity,
            &Transform,
            &mut LinearVelocity,
            &mut Reserve,
            &Anchor,
            &Species,
        ),
        With<Agent>,
    >,
) {
    let dt = time.delta_secs();
    for (entity, transform, mut velocity, mut reserve, anchor, species) in &mut anchored {
        let Some(cfg) = config.anchor_of(species.0) else {
            continue;
        };
        let displacement = transform.translation.truncate() - anchor.0;
        let tension = cfg.stiffness * displacement.length();
        if tension > cfg.tear_force {
            if cfg.die_on_detach {
                // Uprooted → dead: `reap` (next) turns the zeroed reserve into a despawn
                // + a corpse (`emit_at_death`) + recycling — turnover for free (Law 11).
                reserve.current = 0.0;
            } else {
                // Uprooted → survives, now a free body (a dislodged fragment that drifts
                // and settles). The removal applies at the next command flush.
                commands.entity(entity).remove::<Anchor>();
            }
            continue;
        }
        // Restoring pull toward the anchor (mass-free impulse); damping does the settling.
        velocity.0 -= cfg.stiffness * displacement * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `act` records the **voluntary steering effort**: the magnitude of the
    /// velocity change it applies (the agility cost in `metabolize` reads it). From
    /// rest toward a desired `(100, 0)` with agility `0.5`, the lerp jumps the
    /// velocity to `(50, 0)` → effort `50`; the next tick covers half the remaining
    /// gap → effort `25`. Once already at the desired velocity, steering is a no-op
    /// → zero effort: cruising in a straight line is free.
    #[test]
    fn act_records_voluntary_steering_effort() {
        let mut world = World::new();
        let e = world
            .spawn((
                Action {
                    dir: Vec2::X,
                    throttle: 1.0,
                    act: 1.0,
                },
                Locomotion {
                    max_speed: 100.0,
                    agility: 0.5,
                },
                LinearVelocity(Vec2::ZERO),
                Maneuver::default(),
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(act);

        schedule.run(&mut world);
        assert_eq!(
            world.get::<LinearVelocity>(e).unwrap().0,
            Vec2::new(50.0, 0.0)
        );
        assert!((world.get::<Maneuver>(e).unwrap().0 - 50.0).abs() < 1e-3);

        schedule.run(&mut world);
        assert!((world.get::<Maneuver>(e).unwrap().0 - 25.0).abs() < 1e-3);

        // Already cruising at the desired velocity: nothing to steer → free.
        world.get_mut::<LinearVelocity>(e).unwrap().0 = Vec2::new(100.0, 0.0);
        schedule.run(&mut world);
        assert!(
            world.get::<Maneuver>(e).unwrap().0.abs() < 1e-3,
            "a straight cruise costs no maneuvering effort"
        );
    }

    use crate::config::AnchorConfig;
    use std::time::Duration;

    /// A bare `World` + `Schedule` running [`anchor_spring`] over a single anchored agent,
    /// with a `SimConfig` whose archetype 0 carries `anchor_cfg`. `dt` is fixed so the
    /// impulse arithmetic is exact. The body starts at `pos`, anchored at the origin,
    /// still (velocity 0) and half-full (reserve 50/100).
    fn anchor_world(anchor_cfg: AnchorConfig, pos: Vec2, dt: f32) -> (World, Entity, Schedule) {
        let mut world = World::new();
        let mut config = SimConfig::default();
        config.archetypes[0].anchor = Some(anchor_cfg);
        world.insert_resource(config);
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(dt));
        world.insert_resource(time);
        let e = world
            .spawn((
                Agent,
                Species(0),
                Transform::from_translation(pos.extend(0.0)),
                LinearVelocity(Vec2::ZERO),
                Reserve {
                    current: 50.0,
                    max: 100.0,
                },
                Anchor(Vec2::ZERO),
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(anchor_spring);
        (world, e, schedule)
    }

    /// Below the tear tension, the spring applies a restoring impulse **toward the
    /// anchor**: `−stiffness · displacement · dt = −10 · (100,0) · 0.1 = (−100, 0)`.
    #[test]
    fn anchor_spring_pulls_toward_the_anchor() {
        let (mut world, e, mut schedule) = anchor_world(
            AnchorConfig {
                stiffness: 10.0,
                damping: 0.0,
                tear_force: 1.0e9,
                die_on_detach: false,
            },
            Vec2::new(100.0, 0.0),
            0.1,
        );
        schedule.run(&mut world);
        let v = world.get::<LinearVelocity>(e).unwrap().0;
        assert!(
            (v - Vec2::new(-100.0, 0.0)).length() < 1e-3,
            "the restoring impulse must point at the anchor, got {v:?}"
        );
        assert!(world.get::<Anchor>(e).is_some(), "not torn: still anchored");
        assert_eq!(
            world.get::<Reserve>(e).unwrap().current,
            50.0,
            "not torn: alive"
        );
    }

    /// Past the tear tension (`stiffness · |disp| = 10 · 100 = 1000 > 500`), with
    /// `die_on_detach`, the body is **killed** — its reserve is zeroed so the uniform
    /// death path (`reap`, next in the real chain) despawns it as a corpse. No restoring
    /// impulse is applied (we tear off first).
    #[test]
    fn anchor_tears_off_and_kills_when_die_on_detach() {
        let (mut world, e, mut schedule) = anchor_world(
            AnchorConfig {
                stiffness: 10.0,
                damping: 0.0,
                tear_force: 500.0,
                die_on_detach: true,
            },
            Vec2::new(100.0, 0.0),
            0.1,
        );
        schedule.run(&mut world);
        assert_eq!(
            world.get::<Reserve>(e).unwrap().current,
            0.0,
            "an uprooted body is marked dead (reserve zeroed → reap)"
        );
        assert_eq!(
            world.get::<LinearVelocity>(e).unwrap().0,
            Vec2::ZERO,
            "no restoring impulse once torn"
        );
    }

    /// Past the tear tension but **without** `die_on_detach`, the body is **released**:
    /// its [`Anchor`] is dropped (a freed, drifting fragment) and it is not killed.
    #[test]
    fn anchor_tears_off_and_releases_when_not_die_on_detach() {
        let (mut world, e, mut schedule) = anchor_world(
            AnchorConfig {
                stiffness: 10.0,
                damping: 0.0,
                tear_force: 500.0,
                die_on_detach: false,
            },
            Vec2::new(100.0, 0.0),
            0.1,
        );
        schedule.run(&mut world);
        assert!(
            world.get::<Anchor>(e).is_none(),
            "released: the anchor is dropped → a free body"
        );
        assert_eq!(
            world.get::<Reserve>(e).unwrap().current,
            50.0,
            "a released body is not killed"
        );
    }
}
