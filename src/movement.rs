//! The **perceive → decide → act** loop, chained in `FixedUpdate`.
//!
//! Three distinct systems to keep the brain/body seam clean: the brain only
//! reads [`Perception`] and only writes [`Action`]; it alone knows nothing of
//! Avian.

use crate::brain::Brain;
use crate::components::{Action, Agent, Locomotion, Maneuver, Perception, Species, Vision};
use crate::config::SimConfig;
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
    mut agents: Query<
        (
            Entity,
            &Transform,
            &LinearVelocity,
            &Species,
            &Vision,
            &Locomotion,
            &mut Perception,
        ),
        With<Agent>,
    >,
    species_of: Query<&Species>,
    // Raycast filter reused from one agent to the next (cf. loop): we avoid
    // reallocating an `EntityHashSet` for every agent and every tick.
    mut filter: Local<SpatialQueryFilter>,
) {
    for (entity, transform, velocity, species, vision, loco, mut perception) in &mut agents {
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
                    // "target" and "threat" channels, inverse symmetric: we read
                    // the nearest hit's species **only once**, and the (directed)
                    // relation table decides both directions — we act on it
                    // (target, it attracts us) or it acts on us (threat, it makes
                    // us flee). A wall (without [`Species`]) or a species with no
                    // relation either way → both at 0.
                    let (is_target, is_threat) =
                        species_of
                            .get(hit.entity)
                            .map_or((false, false), |hit_species| {
                                (
                                    config.acts_on(species.0, hit_species.0),
                                    config.acts_on(hit_species.0, species.0),
                                )
                            });
                    perception.target[i] = if is_target { proximity } else { 0.0 };
                    perception.threat[i] = if is_threat { proximity } else { 0.0 };
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
pub fn act(mut agents: Query<(&Action, &Locomotion, &mut LinearVelocity, &mut Maneuver)>) {
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
}
