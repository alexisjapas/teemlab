//! Components of an agent's *body*.
//!
//! [`Perception`] and [`Action`] materialize the brain's contract:
//! *normalized floats in → floats out*. It is the body that imposes the shape
//! of these I/O.

use bevy::prelude::*;

/// Marker for a simulated agent.
#[derive(Component)]
pub struct Agent;

/// Species / faction identity. Serves as a target filter for the interaction
/// primitive: it is the *scenario* (via its relation table) that gives meaning
/// to this integer — trophic relation predator→prey, or enemy→enemy faction.
/// The engine itself only knows that "species A may act on species B".
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Species(pub u16);

/// Reserve: the *resource* an interaction reduces, and that predation
/// transfers. Deliberately generic — the scenario decides whether it represents
/// energy (natural selection) or hit points (battle). The economy that feeds it
/// and death at zero arrive with scenario #1 (item 8).
#[derive(Component, Clone, Copy, Debug)]
pub struct Reserve {
    pub current: f32,
    pub max: f32,
}

impl Reserve {
    /// Full reserve.
    pub fn full(max: f32) -> Self {
        Self { current: max, max }
    }

    /// Fill fraction in `[0, 1]` (0 if `max` is zero).
    pub fn fraction(&self) -> f32 {
        if self.max > 0.0 {
            (self.current / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Marker for a static arena wall.
#[derive(Component)]
pub struct Wall;

/// Body radius. Explicit component so the rendering code can size the mesh
/// without digging into the Avian collider.
#[derive(Component, Clone, Copy, Debug)]
pub struct Radius(pub f32);

/// Agent generation: `0` for a founder (population, editor placement),
/// `parent + 1` for a newborn. Set at birth and never modified — it is the
/// genealogical depth, not a living state. Readable to track a lineage's
/// progress (inspector, snapshot).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Generation(pub u32);

/// Agent age, in **simulated seconds** elapsed since its birth.
/// Incremented every tick by [`crate::ecology::age_agents`]. Zero at birth;
/// restored as-is from a snapshot.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Age(pub f32);

/// Locomotion magnitudes — what the genes will vary (v1: fixed).
#[derive(Component, Clone, Copy, Debug)]
pub struct Locomotion {
    /// Maximum speed.
    pub max_speed: f32,
    /// Steering responsiveness toward the desired velocity, in `[0, 1]`.
    pub agility: f32,
}

impl Locomotion {
    /// `true` if the entity **cannot move** (zero max speed) — sessile flora /
    /// source. Such a body has no heading to show (its "heading" is a fixed `+X`
    /// fallback, not a gaze direction) nor any vision to exploit: rendering and
    /// phenotype compilation use this to give it neither a heading indicator
    /// ([`crate::visuals`]) nor a ray ([`crate::genotype::Genotype::vision`]).
    /// It is the engine's convention: a sessile entity always has `max_speed: 0`
    /// (cf. the food preset and `spawn`).
    pub fn is_immobile(&self) -> bool {
        self.max_speed <= 0.0
    }
}

/// Visual sensor by raycast. The *shape* — the number of rays — is locked per
/// species (v1); the genes will vary the *magnitudes* (`fov`, `range`), never
/// the number of channels. It is this fixed shape that imposes the size of the
/// brain's input vector.
#[derive(Component, Clone, Copy, Debug)]
pub struct Vision {
    /// Number of rays (= number of proximity channels produced).
    pub ray_count: usize,
    /// *Total* field of view, in radians, centered on the heading.
    pub fov: f32,
    /// Range of a ray, in world units.
    pub range: f32,
}

impl Vision {
    /// Angular offset (radians) of ray `i` relative to the heading, spread
    /// symmetrically over `[-fov/2, +fov/2]`. A single ray → straight ahead.
    pub fn ray_offset(&self, i: usize) -> f32 {
        if self.ray_count <= 1 {
            0.0
        } else {
            let t = i as f32 / (self.ray_count - 1) as f32; // 0..=1
            (t - 0.5) * self.fov
        }
    }

    /// World direction of ray `i` for an agent looking toward `facing`.
    pub fn ray_dir(&self, i: usize, facing: Vec2) -> Vec2 {
        self.ray_dir_from_angle(i, facing.to_angle())
    }

    /// Like [`ray_dir`](Self::ray_dir), but from the **heading already converted
    /// to an angle** (`facing.to_angle()`). `perceive` fans out `ray_count` rays
    /// from a single heading: we then compute the `atan2` **only once** per agent
    /// (then one `from_angle` per ray) instead of redoing it for every ray.
    /// **Bit-for-bit identical** result — same `from_angle(base + offset)`
    /// expression, just minus the redundant atan2.
    pub fn ray_dir_from_angle(&self, i: usize, base_angle: f32) -> Vec2 {
        Vec2::from_angle(base_angle + self.ray_offset(i))
    }

    /// Metabolic cost of the sensor, per tick (cf. §2 "value, bounds, cost
    /// coupling" and §7 "treat vision as a cost"). Bounds the drift: more range
    /// and more rays = more expensive. The *consumer* (the energy economy)
    /// arrives with the natural-selection scenario; here we already quantify the
    /// coupling so it only has to subtract.
    pub fn metabolic_cost(&self) -> f32 {
        const COST_PER_UNIT_RAY: f32 = 0.0005;
        self.range * self.ray_count as f32 * COST_PER_UNIT_RAY
    }
}

/// Sensory snapshot. Written by `perceive`, read by `decide` — conceptually the
/// brain's input vector. It gathers the **normalized channels** (`vision`,
/// `target`, `threat`, in `[0, 1]`) and the **geometry** that situates them
/// (`heading`, `ray_dirs`), so a brain can decide without knowing anything about
/// the body ([`Vision`]).
#[derive(Component, Default)]
pub struct Perception {
    /// Current heading as a unit vector (zero when stopped).
    pub heading: Vec2,
    /// **Obstacle** proximity per ray, one channel per [`Vision`] ray, in
    /// `[0, 1]`: `0` = nothing in range, `1` = in contact. Intrinsic occlusion
    /// (each ray keeps only the nearest hit); the hit is taken whatever it is —
    /// wall, agent or food.
    pub vision: Box<[f32]>,
    /// **Target** proximity per ray, in `[0, 1]`: `0` if this ray's nearest hit
    /// is not a species ours can target (relation table, cf.
    /// [`crate::config::SimConfig::acts_on`]), otherwise its proximity.
    /// Occlusion is included — a prey behind a wall is not read here; the wall
    /// (nearest hit) occupies the ray instead. The channel that *attracts*
    /// `Brain::Hunter`.
    pub target: Box<[f32]>,
    /// **Threat** proximity per ray, in `[0, 1]`: the **inverse symmetric** of
    /// the `target` channel. It is `0` unless this ray's nearest hit carries a
    /// species that can act **on us** (the *inverse* relation,
    /// `acts_on(other, us)`, cf. [`crate::config::SimConfig::acts_on`]), in which
    /// case it equals its proximity. A prey reads its predator here; an apex
    /// predator reads nothing (zero channel → unchanged behavior). Occlusion
    /// included, like `target`. The channel that makes `Brain::Hunter` **flee**
    /// (repulsion) — the exact counterpart of the `target` channel that attracts
    /// it.
    pub threat: Box<[f32]>,
    /// **World** direction (unit) of each ray, situating the channels above.
    /// `perceive` already derives it to cast the raycast; exposing it spares the
    /// brain from knowing [`Vision`]'s geometry (fov, ray count): a reflex
    /// decodes "ray i → direction" without depending on the body, and the
    /// `Perception → Action` contract stays pure (an MLP will ignore this field).
    pub ray_dirs: Box<[Vec2]>,
}

/// Motor command. Written by `decide`, read by `act`.
/// Conceptually the brain's output vector.
#[derive(Component, Default)]
pub struct Action {
    /// Desired movement direction (near-unit).
    pub dir: Vec2,
    /// Desired fraction of max speed, in `[0, 1]`.
    pub throttle: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vision(rays: usize) -> Vision {
        Vision {
            ray_count: rays,
            fov: std::f32::consts::FRAC_PI_2, // 90°
            range: 100.0,
        }
    }

    /// The fan is symmetric: first and last ray at the FOV edges, and the
    /// central ray exactly on the heading.
    #[test]
    fn ray_offsets_span_fov_symmetrically() {
        let v = vision(5);
        assert!((v.ray_offset(0) + v.fov / 2.0).abs() < 1e-6);
        assert!((v.ray_offset(4) - v.fov / 2.0).abs() < 1e-6);
        assert!(v.ray_offset(2).abs() < 1e-6);
    }

    /// A single ray looks straight ahead, without division by zero.
    #[test]
    fn single_ray_points_forward() {
        assert_eq!(vision(1).ray_offset(0), 0.0);
    }

    /// `ray_dir` is unit and, with heading = +X, the central ray indeed points to +X.
    #[test]
    fn ray_dir_is_unit_and_centered() {
        let v = vision(3);
        let d = v.ray_dir(1, Vec2::X);
        assert!((d.length() - 1.0).abs() < 1e-5);
        assert!((d - Vec2::X).length() < 1e-5);
    }

    /// The cost grows strictly with range and ray count: this is what will
    /// bound evolutionary drift (cf. §7).
    #[test]
    fn metabolic_cost_grows_with_range_and_rays() {
        let small = vision(3);
        let more_rays = vision(7);
        let mut longer = vision(3);
        longer.range = 200.0;
        assert!(more_rays.metabolic_cost() > small.metabolic_cost());
        assert!(longer.metabolic_cost() > small.metabolic_cost());
    }

    /// The reserve fraction is in `[0, 1]`, robust to a zero `max`.
    #[test]
    fn reserve_fraction_is_clamped() {
        assert_eq!(Reserve::full(100.0).fraction(), 1.0);
        assert_eq!(
            Reserve {
                current: 50.0,
                max: 100.0
            }
            .fraction(),
            0.5
        );
        assert_eq!(
            Reserve {
                current: 0.0,
                max: 0.0
            }
            .fraction(),
            0.0
        );
        assert_eq!(
            Reserve {
                current: 999.0,
                max: 100.0
            }
            .fraction(),
            1.0
        );
    }
}
