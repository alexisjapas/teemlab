//! The **genotype**: an agent's inherited, mutable description.
//!
//! §2 — *genotype ≠ phenotype*. We mutate the genotype (this gene struct), then
//! **compile it into the living phenotype** ([`Locomotion`], [`Vision`], …
//! components) at spawn. Evolution never touches the ongoing physical state: it
//! rewrites the recipe, not the dish.
//!
//! The genes vary the **magnitudes** (vision range, speed, …) *and*, since the
//! `vision_rays` gene, the **number of sensory channels** (visual precision).
//! This number therefore varies per individual: the MLP's input layer adapts to
//! it at reproduction (cf. [`crate::brain::MlpBrain::reproduced`]) — a first step
//! toward variable topology, without going all the way to full NEAT.
//!
//! Each gene forms, together with its bounds ([`crate::config::Bounds`]) and its
//! cost coupling (the energy economy), the triplet of §2.

use crate::components::{Locomotion, Vision};
use crate::config::{Bounds, Mutability, SimConfig};
use crate::rng::Rng;
use bevy::prelude::*;

/// An agent's genes. A component (carried by the living agent, inherited by its
/// children) **and** the serializable "genome" of an instance — the archetype
/// (config) / genome (instance) distinction of item 5.
#[derive(Component, Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Genotype {
    /// Maximum speed.
    pub max_speed: f32,
    /// Steering responsiveness, in `[0, 1]`.
    pub agility: f32,
    /// Vision range.
    pub vision_range: f32,
    /// *Total* field of view, in **degrees** — the designer's unit (config,
    /// editor, bounds). Converted to radians at the single phenotype-compilation
    /// point (cf. [`Genotype::vision`]).
    pub vision_fov_deg: f32,
    /// Energy to reach in order to reproduce. `0` → this agent does not reproduce.
    /// An entity characteristic (§1, *the body*) → the reproduction strategy is
    /// itself selectable.
    pub reproduction_threshold: f32,
    /// Energy passed to the child, deducted from the parent (conservation: nothing created).
    pub offspring_energy: f32,
    /// Mutation rate transmitted to the offspring (std-dev, as a fraction of a
    /// gene's span). The gene that drives its own lineage. **Not mutable by
    /// default** ([`crate::config::Mutability`]): left mutable, it drifts
    /// (meta-evolution) and may freeze at 0 → dead evolution.
    pub mutation_rate: f32,
    /// Base metabolism: energy drained **per second** at rest — the cost of
    /// survival (§2), the base selection pressure. Per-species, **not mutable by
    /// default**: evolvable, it would be whittled down to 0 (the pressure would vanish).
    pub base_metabolism: f32,
    /// Locomotion surcharge: energy/s at full speed. Couples speed to a cost (§2).
    /// Per-species, **not mutable by default**: otherwise the speed gene could
    /// "cancel out" its own cost.
    pub move_cost: f32,
    /// Visual precision: the **number of vision rays**. A full-fledged gene
    /// (mutable, inherited), stored as `f32` to fit the common Gaussian-mutation
    /// machinery, and rounded to an integer at phenotype compilation (cf.
    /// [`Genotype::ray_count`]). More rays = finer vision *but* more expensive
    /// ([`Vision::metabolic_cost`]) — the cost coupling that bounds its drift.
    pub vision_rays: f32,
    /// **Flora gene** (Phase 3): energy **gained** per second, passively — the
    /// *photosynthesis*. It is a sessile entity's energy source, the counterpart
    /// of "eating" for fauna. `0` for fauna (inert). Added at the **end** (like
    /// `vision_rays`) to preserve [`mutate`](Genotype::mutate)'s draw stream.
    pub photosynthesis: f32,
    /// **Flora gene** (Phase 3): distance at which an offspring is seeded from the
    /// parent (the *dispersal*). `0` → falls back to the default close offset
    /// (radius × 2.5, fauna behavior, unchanged). A flora increases it to scatter
    /// its seeds instead of clustering.
    pub seed_dispersal: f32,
    /// **Brain cost**: energy drained **per second and per decision neuron** of the
    /// MLP (hidden + output, cf. [`crate::brain::Brain::neuron_count`]) — the cost
    /// coupling of the *decision system* (§2), the counterpart for the brain of
    /// what [`Vision::metabolic_cost`] is for the sensor. `0` for a hand-written
    /// brain (no network) and `0` by default (inert). Per-species, **not mutable by
    /// default** (like `base_metabolism`/`move_cost`): evolvable, it would be
    /// whittled down to 0 and the pressure would vanish. Added at the **end** to
    /// preserve [`mutate`](Genotype::mutate)'s draw stream.
    pub brain_cost: f32,
    /// **Agility cost**: energy drained per unit of *maneuvering effort* — the
    /// magnitude `|Δv|` of the velocity change `act` applies each tick (cf.
    /// [`crate::components::Maneuver`]). The transient counterpart of `move_cost`
    /// (which prices steady-state cruising): turning and accelerating do work
    /// against inertia, cruising in a straight line is nearly free. Per-species,
    /// **not mutable by default** like the other costs (evolvable, it would fall to
    /// 0). `0` by default (inert). Appended at the **end** (draw stream).
    pub agility_cost: f32,
    /// **Nutrient gene** (T2): rate at which the entity **absorbs** nutrient from
    /// the local field ([`crate::nutrients::NutrientField`]) into its store
    /// ([`crate::nutrients::Nutrients`]), per second. `0` → no absorption (fauna,
    /// and every pre-T2 scenario). The nutrient axis gates *reproduction* only, not
    /// survival (the two-axis design, ROADMAP §9). Appended at the **end** and
    /// **not mutable by default** → [`mutate`](Genotype::mutate)'s draw stream and
    /// the sim stay byte-identical.
    pub nutrient_absorption: f32,
    /// **Nutrient gene** (T2): the **capacity** of the per-entity nutrient store.
    /// `0` (default) → an inert store. Appended at the **end**, not mutable by
    /// default (draw stream preserved).
    pub nutrient_capacity: f32,
    /// **Nutrient gene** (T2): nutrient **spent per child** at reproduction.
    /// Reproduction is gated on `nutrients.current >= offspring_nutrient`; on success
    /// it is deducted from the parent and **consumed** — the child is born with an
    /// **empty** store and must absorb its own (unlike
    /// [`offspring_energy`](Self::offspring_energy), which is carried over). This is
    /// what makes the nutrient a genuine *limiting* resource rather than a self-
    /// perpetuating endowment (cf. [`crate::ecology::reproduce`]). `0` (default) →
    /// the gate always passes spending nothing → pre-T2 reproduction unchanged.
    /// Appended at the **end**, not mutable by default (draw stream preserved).
    pub offspring_nutrient: f32,
}

impl Default for Genotype {
    /// Default founding values (the base "archetype"): reused by
    /// [`Archetype::new_agent`](crate::config::Archetype::new_agent) and by any
    /// gene omitted from a partial RON genotype (`#[serde(default)]`). Each gene in
    /// its storage unit — the fov in degrees.
    fn default() -> Self {
        Self {
            max_speed: 140.0,
            agility: 0.12,
            vision_range: 160.0,
            vision_fov_deg: 120.0,
            reproduction_threshold: 0.0,
            offspring_energy: 30.0,
            mutation_rate: 0.0,
            base_metabolism: 0.0,
            move_cost: 0.0,
            vision_rays: 7.0,
            // Flora genes inactive by default (fauna): no passive gain, close
            // seeding by default.
            photosynthesis: 0.0,
            seed_dispersal: 0.0,
            // Decision-system cost inactive by default: the brain is free until a
            // scenario opts in (like base_metabolism/move_cost).
            brain_cost: 0.0,
            // Maneuvering is free until a scenario opts in (like the other costs).
            agility_cost: 0.0,
            // Nutrient genes (T2) inert by default: no absorption, no store, no
            // nutrient cost per child → the reproduction gate always passes.
            nutrient_absorption: 0.0,
            nutrient_capacity: 0.0,
            offspring_nutrient: 0.0,
        }
    }
}

impl Genotype {
    /// Compiles the locomotion gene into its phenotype.
    pub fn locomotion(&self) -> Locomotion {
        Locomotion {
            max_speed: self.max_speed,
            agility: self.agility,
        }
    }

    /// *Effective* ray count: the `vision_rays` gene rounded to the nearest
    /// integer (**≥ 0** — `0` is a legitimate *blind* agent; the MLP then receives
    /// an empty input vector, cf. [`crate::brain::MlpBrain`]). The float→int cast
    /// saturates at 0, so a negative gene value cannot underflow. The only point
    /// where visual precision (an f32 gene) becomes a discrete shape (channels).
    pub fn ray_count(&self) -> usize {
        self.vision_rays.round() as usize
    }

    /// Compiles the vision genes into their phenotype. The *shape* (number of
    /// rays) now comes from the `vision_rays` gene ([`Genotype::ray_count`]), so it
    /// varies per individual. **The only point** where the fov goes from degrees
    /// (gene) to radians (phenotype, expected by the raycast).
    ///
    /// Note — an **immobile** entity (flora, [`Locomotion::is_immobile`]) *casts*
    /// no ray (`perceive` skips it) and displays none (inspector, gizmos): without
    /// a heading or locomotion, its vision is unusable. We nonetheless keep its
    /// dimensions here (and thus its metabolic cost unchanged) so as not to alter
    /// the energy economy of existing scenarios — removing the rays is
    /// **observable** (nothing to perceive or draw), not a re-calibration of the
    /// sim.
    pub fn vision(&self) -> Vision {
        Vision {
            ray_count: self.ray_count(),
            fov: self.vision_fov_deg.to_radians(),
            range: self.vision_range,
        }
    }

    /// A mutated copy for a child: each **mutable** gene of the [`TRAITS`] table
    /// receives a Gaussian perturbation of std-dev `mutation_rate · span`, then is
    /// brought back within its bounds; a non-mutable gene is **still copied from
    /// the parent** but without perturbation (it therefore stays frozen on the
    /// founder's value along a lineage). A generic loop → adding a trait does not
    /// touch it. All genes are in their bounds' unit (fov in degrees), so a single
    /// path, without conversion.
    ///
    /// The rate comes **from the genotype** (`self.mutation_rate`), not from a
    /// global setting: each lineage carries its own evolution speed.
    ///
    /// **Zero drift is a faithful clone — even out of bounds.** We clamp to the
    /// gene's `[min, max]` **only when the gene actually drifted**: a value
    /// deliberately set *outside* its bounds (e.g. a sessile plant's `max_speed = 0`,
    /// below `speed_bounds.min`) must survive reproduction unchanged. Clamping it
    /// unconditionally would, with zero mutation, silently lift it to the bound —
    /// turning an immobile plant **mobile** at its first child. The `next_gaussian`
    /// draw is still consumed for every mutable gene (RNG stream unchanged).
    pub fn mutate(&self, rng: &mut Rng, mutable: &Mutability, config: &SimConfig) -> Self {
        let rate = self.mutation_rate;
        let mut child = *self;
        for t in &TRAITS {
            // Non-mutable trait: the child keeps the parent's value (already copied
            // into `child`) and consumes no draw.
            if !(t.mutable)(mutable) {
                continue;
            }
            let bounds = (t.bounds)(config);
            let drift = rng.next_gaussian() * rate * bounds.span();
            let value = (t.get)(self) + drift;
            // Only enforce the bounds when the gene moved (drift ≠ 0): no drift ⇒
            // faithful clone, preserving a deliberately out-of-bounds founder value.
            (t.set)(
                &mut child,
                if drift == 0.0 {
                    value
                } else {
                    bounds.clamp(value)
                },
            );
        }
        child
    }
}

/// A **semantic grouping** of genes for the editor. The flat [`TRAITS`] table
/// grew to 17 entries — a "wall" of sliders; a category lets the gene editor draw
/// collapsible sections instead, **without** breaking the single-source-of-truth
/// property (item 15): a new gene only needs a category, no new UI branch. The
/// display order is the declaration order, surfaced through [`GeneCategory::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneCategory {
    /// Moving: speed, steering and their costs.
    Locomotion,
    /// Seeing: range, field of view, ray count and the brain it feeds.
    Vision,
    /// The base cost of staying alive.
    Metabolism,
    /// Making offspring: threshold, endowment, mutation.
    Reproduction,
    /// The sessile life: passive gain and seeding.
    Flora,
    /// The substrate axis (T2): absorbing and spending nutrient.
    Nutrients,
}

impl GeneCategory {
    /// The categories in **display order**. The gene editor iterates this and, for
    /// each, the [`TRAITS`] filtered by `category` — so a new category must be
    /// listed here to appear (the counterpart, for the grouping, of adding a gene
    /// to `TRAITS`).
    pub const ALL: [GeneCategory; 6] = [
        GeneCategory::Locomotion,
        GeneCategory::Vision,
        GeneCategory::Metabolism,
        GeneCategory::Reproduction,
        GeneCategory::Flora,
        GeneCategory::Nutrients,
    ];

    /// The section's display label.
    pub fn label(self) -> &'static str {
        match self {
            GeneCategory::Locomotion => "Locomotion",
            GeneCategory::Vision => "Vision",
            GeneCategory::Metabolism => "Metabolism",
            GeneCategory::Reproduction => "Reproduction",
            GeneCategory::Flora => "Flora",
            GeneCategory::Nutrients => "Nutrients",
        }
    }

    /// Whether the section starts **expanded**. The advanced, usually-zero axes
    /// (flora, nutrients) start collapsed to keep the common fauna case uncluttered;
    /// the core ones start open.
    pub fn default_open(self) -> bool {
        !matches!(self, GeneCategory::Flora | GeneCategory::Nutrients)
    }
}

/// The descriptor of **one** heritable characteristic: the §2 triplet —
/// (value, bounds, …) — made *iterable*. The [`TRAITS`] table is its single
/// source of truth; the drivers (mutation, editor, HUD, inspector) loop over it
/// instead of enumerating the genes by hand. Adding a trait = one entry here
/// (+ a [`Genotype`] field and its bounds in config); no driver to re-edit —
/// that is what item 15 falsifies against the existing plurality.
pub struct TraitSpec {
    /// Label for the editor and the HUD.
    pub name: &'static str,
    /// The editor section this gene belongs to (presentation only — never read by
    /// [`Genotype::mutate`], so it leaves the RNG stream untouched).
    pub category: GeneCategory,
    /// `true` if this gene is a **cost** (a price paid — §2, SIM Law 7 "every
    /// characteristic is priced") rather than a capability or parameter: the
    /// metabolic, locomotion, brain and maneuvering costs. The gene editor sorts the
    /// costs to the **bottom** of their category, for a uniform reading order across
    /// categories. Presentation only (never read by [`Genotype::mutate`]).
    pub is_cost: bool,
    /// The gene's value in the genotype (read).
    pub get: fn(&Genotype) -> f32,
    /// The gene's value in the genotype (write).
    pub set: fn(&mut Genotype, f32),
    /// The gene's bounds, read from the scenario.
    pub bounds: fn(&SimConfig) -> Bounds,
    /// The gene's bounds in **write**: the same `*_bounds` field as
    /// [`bounds`](Self::bounds), on the mutable side. The world editor loops over
    /// it to expose min/max without a hard-coded field (item 3) — the "write"
    /// counterpart of the read/write pair already offered for the value
    /// ([`get`](Self::get)/[`set`](Self::set)) and the mutability
    /// ([`mutable`](Self::mutable)/[`set_mutable`](Self::set_mutable)).
    pub bounds_mut: fn(&mut SimConfig) -> &mut Bounds,
    /// This trait's "mutable?" facet in the scenario (read).
    pub mutable: fn(&Mutability) -> bool,
    /// This trait's "mutable?" facet (write, for the editor).
    pub set_mutable: fn(&mut Mutability, bool),
    /// Display decimals (inspector).
    pub decimals: u8,
    /// `true` if this gene is **inert on an immobile entity** (flora): the
    /// locomotion genes (agility, locomotion cost) and vision genes (range, fov,
    /// rays) have no effect without movement or a ray to exploit (cf.
    /// [`Genotype::vision`]). `max_speed`, by contrast, stays relevant — it is the
    /// mobility switch. The UI drivers (editor, inspector) hide these genes when
    /// the entity cannot move, so as not to expose characteristics without effect.
    pub inert_when_immobile: bool,
}

/// The mutable characteristics, **in [`Genotype`]'s field order** (this order
/// fixes [`Genotype::mutate`]'s draw stream, and hence the reproducibility of a
/// seeded config — whence the addition at the **end** of the table, which leaves
/// the pre-existing traits' stream intact). A constant table shared by all
/// agents.
pub const TRAITS: [TraitSpec; 17] = [
    TraitSpec {
        name: "Max speed",
        category: GeneCategory::Locomotion,
        is_cost: false,
        get: |g| g.max_speed,
        set: |g, v| g.max_speed = v,
        bounds: |c| c.speed_bounds,
        bounds_mut: |c| &mut c.speed_bounds,
        mutable: |m| m.max_speed,
        set_mutable: |m, b| m.max_speed = b,
        decimals: 1,
        // The mobility switch: always relevant (setting it to 0 makes a flora).
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Agility",
        category: GeneCategory::Locomotion,
        is_cost: false,
        get: |g| g.agility,
        set: |g, v| g.agility = v,
        bounds: |c| c.agility_bounds,
        bounds_mut: |c| &mut c.agility_bounds,
        mutable: |m| m.agility,
        set_mutable: |m, b| m.agility = b,
        decimals: 3,
        // Locomotion: steering a speed you do not have has no effect.
        inert_when_immobile: true,
    },
    TraitSpec {
        name: "Vision range",
        category: GeneCategory::Vision,
        is_cost: false,
        get: |g| g.vision_range,
        set: |g, v| g.vision_range = v,
        bounds: |c| c.vision_range_bounds,
        bounds_mut: |c| &mut c.vision_range_bounds,
        mutable: |m| m.vision_range,
        set_mutable: |m, b| m.vision_range = b,
        decimals: 1,
        // Vision: an immobile entity has no ray (cf. `Genotype::vision`).
        inert_when_immobile: true,
    },
    TraitSpec {
        name: "Vision FOV (°)",
        category: GeneCategory::Vision,
        is_cost: false,
        get: |g| g.vision_fov_deg,
        set: |g, v| g.vision_fov_deg = v,
        bounds: |c| c.vision_fov_bounds,
        bounds_mut: |c| &mut c.vision_fov_bounds,
        mutable: |m| m.vision_fov,
        set_mutable: |m, b| m.vision_fov = b,
        decimals: 0,
        // Vision: without a ray, the cone's angle has nothing to cover.
        inert_when_immobile: true,
    },
    TraitSpec {
        name: "Repro threshold",
        category: GeneCategory::Reproduction,
        is_cost: false,
        get: |g| g.reproduction_threshold,
        set: |g, v| g.reproduction_threshold = v,
        bounds: |c| c.reproduction_threshold_bounds,
        bounds_mut: |c| &mut c.reproduction_threshold_bounds,
        mutable: |m| m.reproduction_threshold,
        set_mutable: |m, b| m.reproduction_threshold = b,
        decimals: 0,
        // Reproduction applies to flora too (local seeding): relevant.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Energy/child",
        category: GeneCategory::Reproduction,
        is_cost: false,
        get: |g| g.offspring_energy,
        set: |g, v| g.offspring_energy = v,
        bounds: |c| c.offspring_energy_bounds,
        bounds_mut: |c| &mut c.offspring_energy_bounds,
        mutable: |m| m.offspring_energy,
        set_mutable: |m, b| m.offspring_energy = b,
        decimals: 0,
        // A newborn's endowment: relevant for flora seeding too.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Mutation rate",
        category: GeneCategory::Reproduction,
        is_cost: false,
        get: |g| g.mutation_rate,
        set: |g, v| g.mutation_rate = v,
        bounds: |c| c.mutation_rate_bounds,
        bounds_mut: |c| &mut c.mutation_rate_bounds,
        mutable: |m| m.mutation_rate,
        set_mutable: |m, b| m.mutation_rate = b,
        decimals: 3,
        // Drives the lineage's evolution speed: relevant whatever the mobility.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Metabolism/s",
        category: GeneCategory::Metabolism,
        is_cost: true,
        get: |g| g.base_metabolism,
        set: |g, v| g.base_metabolism = v,
        bounds: |c| c.base_metabolism_bounds,
        bounds_mut: |c| &mut c.base_metabolism_bounds,
        mutable: |m| m.base_metabolism,
        set_mutable: |m, b| m.base_metabolism = b,
        decimals: 1,
        // Base cost of survival: drains flora as well as fauna.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Locomotion cost",
        category: GeneCategory::Locomotion,
        is_cost: true,
        get: |g| g.move_cost,
        set: |g, v| g.move_cost = v,
        bounds: |c| c.move_cost_bounds,
        bounds_mut: |c| &mut c.move_cost_bounds,
        mutable: |m| m.move_cost,
        set_mutable: |m, b| m.move_cost = b,
        decimals: 1,
        // Energy surcharge for moving: no effect on an entity that does not move.
        inert_when_immobile: true,
    },
    TraitSpec {
        name: "Rays (precision)",
        category: GeneCategory::Vision,
        is_cost: false,
        get: |g| g.vision_rays,
        set: |g, v| g.vision_rays = v,
        bounds: |c| c.vision_rays_bounds,
        bounds_mut: |c| &mut c.vision_rays_bounds,
        mutable: |m| m.vision_rays,
        set_mutable: |m, b| m.vision_rays = b,
        decimals: 0,
        // Visual precision: an immobile entity is compiled with no ray at all.
        inert_when_immobile: true,
    },
    TraitSpec {
        name: "Photosynthesis/s",
        category: GeneCategory::Flora,
        is_cost: false,
        get: |g| g.photosynthesis,
        set: |g, v| g.photosynthesis = v,
        bounds: |c| c.photosynthesis_bounds,
        bounds_mut: |c| &mut c.photosynthesis_bounds,
        mutable: |m| m.photosynthesis,
        set_mutable: |m, b| m.photosynthesis = b,
        decimals: 1,
        // Passive gain: this is precisely the immobile flora's energy source.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Dispersal",
        category: GeneCategory::Flora,
        is_cost: false,
        get: |g| g.seed_dispersal,
        set: |g, v| g.seed_dispersal = v,
        bounds: |c| c.seed_dispersal_bounds,
        bounds_mut: |c| &mut c.seed_dispersal_bounds,
        mutable: |m| m.seed_dispersal,
        set_mutable: |m, b| m.seed_dispersal = b,
        decimals: 0,
        // Seeding distance: this is a (sessile) flora's dispersal — relevant.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Brain cost/neuron",
        category: GeneCategory::Vision,
        is_cost: true,
        get: |g| g.brain_cost,
        set: |g, v| g.brain_cost = v,
        bounds: |c| c.brain_cost_bounds,
        bounds_mut: |c| &mut c.brain_cost_bounds,
        mutable: |m| m.brain_cost,
        set_mutable: |m, b| m.brain_cost = b,
        decimals: 2,
        // A metabolic cost of the brain tissue (like base metabolism): paid
        // whatever the mobility — a non-MLP brain simply counts zero neurons.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Agility cost",
        category: GeneCategory::Locomotion,
        is_cost: true,
        get: |g| g.agility_cost,
        set: |g, v| g.agility_cost = v,
        bounds: |c| c.agility_cost_bounds,
        bounds_mut: |c| &mut c.agility_cost_bounds,
        mutable: |m| m.agility_cost,
        set_mutable: |m, b| m.agility_cost = b,
        decimals: 3,
        // Cost of maneuvering: like locomotion cost and agility, it has no effect on
        // an entity that does not move (a sessile body never maneuvers).
        inert_when_immobile: true,
    },
    TraitSpec {
        name: "Nutrient absorb/s",
        category: GeneCategory::Nutrients,
        is_cost: false,
        get: |g| g.nutrient_absorption,
        set: |g, v| g.nutrient_absorption = v,
        bounds: |c| c.nutrient_absorption_bounds,
        bounds_mut: |c| &mut c.nutrient_absorption_bounds,
        mutable: |m| m.nutrient_absorption,
        set_mutable: |m, b| m.nutrient_absorption = b,
        decimals: 2,
        // Absorbing nutrient from the substrate: precisely a (sessile) plant's
        // behavior — relevant on an immobile entity.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Nutrient capacity",
        category: GeneCategory::Nutrients,
        is_cost: false,
        get: |g| g.nutrient_capacity,
        set: |g, v| g.nutrient_capacity = v,
        bounds: |c| c.nutrient_capacity_bounds,
        bounds_mut: |c| &mut c.nutrient_capacity_bounds,
        mutable: |m| m.nutrient_capacity,
        set_mutable: |m, b| m.nutrient_capacity = b,
        decimals: 0,
        // The store's size: relevant for a plant.
        inert_when_immobile: false,
    },
    TraitSpec {
        name: "Nutrient/child",
        category: GeneCategory::Nutrients,
        is_cost: false,
        get: |g| g.offspring_nutrient,
        set: |g, v| g.offspring_nutrient = v,
        bounds: |c| c.offspring_nutrient_bounds,
        bounds_mut: |c| &mut c.offspring_nutrient_bounds,
        mutable: |m| m.offspring_nutrient,
        set_mutable: |m, b| m.offspring_nutrient = b,
        decimals: 0,
        // Nutrient endowment of a seed: relevant for flora reproduction.
        inert_when_immobile: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mutability;

    fn config() -> SimConfig {
        SimConfig::default()
    }

    /// The gene editor groups [`TRAITS`] by [`GeneCategory`]; the grouping must
    /// **partition** the table — every gene shown in exactly one section, none
    /// lost in the wall rework. Counting per category and summing recovers the
    /// whole table (each trait's category is, by type, one of `ALL`).
    #[test]
    fn categories_partition_traits() {
        let grouped: usize = GeneCategory::ALL
            .iter()
            .map(|c| TRAITS.iter().filter(|t| t.category == *c).count())
            .sum();
        assert_eq!(
            grouped,
            TRAITS.len(),
            "every gene must fall in exactly one editor category"
        );
        // And no category is empty (an empty section would draw a dead header).
        for c in GeneCategory::ALL {
            assert!(
                TRAITS.iter().any(|t| t.category == c),
                "category {} has no gene",
                c.label()
            );
        }
    }

    /// The cost genes (priced characteristics, §2/Law 7) are flagged `is_cost` and the
    /// capabilities/parameters are not — the editor sorts costs to the bottom of each
    /// category. Guards against an accidental flag flip.
    #[test]
    fn cost_genes_are_flagged() {
        let is_cost = |name: &str| TRAITS.iter().find(|t| t.name == name).unwrap().is_cost;
        for c in [
            "Metabolism/s",
            "Locomotion cost",
            "Brain cost/neuron",
            "Agility cost",
        ] {
            assert!(is_cost(c), "{c} should be a cost");
        }
        for n in [
            "Max speed",
            "Vision range",
            "Repro threshold",
            "Photosynthesis/s",
        ] {
            assert!(!is_cost(n), "{n} should not be a cost");
        }
    }

    /// Within each category, a **stable** sort by `is_cost` puts every cost after every
    /// non-cost — the "costs at the bottom" reading order the gene editor relies on.
    #[test]
    fn stable_sort_puts_costs_last_per_category() {
        for c in GeneCategory::ALL {
            let mut idx: Vec<usize> = (0..TRAITS.len())
                .filter(|&i| TRAITS[i].category == c)
                .collect();
            idx.sort_by_key(|&i| TRAITS[i].is_cost);
            let first_cost = idx.iter().position(|&i| TRAITS[i].is_cost);
            if let Some(p) = first_cost {
                assert!(
                    idx[p..].iter().all(|&i| TRAITS[i].is_cost),
                    "category {} interleaves a non-cost after a cost",
                    c.label()
                );
            }
        }
    }

    /// The default genotype carries consistent founding values (fov in degrees,
    /// integer rays stored as `f32`).
    #[test]
    fn default_has_founder_values() {
        let g = Genotype::default();
        assert_eq!(g.max_speed, 140.0);
        assert_eq!(g.vision_fov_deg, 120.0);
        assert_eq!(g.vision_rays, 7.0);
        assert_eq!(g.ray_count(), 7);
        // The decision-system and maneuvering costs are inert by default.
        assert_eq!(g.brain_cost, 0.0);
        assert_eq!(g.agility_cost, 0.0);
    }

    /// Immobility is read from the locomotion phenotype (zero max speed): it is
    /// this signal that deprives flora of a displayed heading and of rays (cf.
    /// `visuals`, `movement`, `inspector`). The genotype keeps its vision genes
    /// (and thus its metabolic cost unchanged) — removing the rays is observable,
    /// not a re-calibration of the sim.
    #[test]
    fn immobility_is_read_from_zero_max_speed() {
        let plant = Genotype {
            max_speed: 0.0,
            ..Genotype::default()
        };
        assert!(plant.locomotion().is_immobile(), "zero max speed = flora");

        let fauna = Genotype {
            max_speed: 140.0,
            ..Genotype::default()
        };
        assert!(!fauna.locomotion().is_immobile(), "a mobile agent can move");

        // Consistency guardrail: only the locomotion and vision genes are marked
        // inert on an immobile entity (and `max_speed`, the switch, is not). These
        // are the ones the editor and inspector then hide.
        let inert: Vec<&str> = TRAITS
            .iter()
            .filter(|t| t.inert_when_immobile)
            .map(|t| t.name)
            .collect();
        assert_eq!(
            inert,
            vec![
                "Agility",
                "Vision range",
                "Vision FOV (°)",
                "Locomotion cost",
                "Rays (precision)",
                "Agility cost",
            ]
        );
    }

    /// A trait's two bounds accessors target the **same** field: the read
    /// (`bounds`) and the write (`bounds_mut`) cannot diverge — a guard against a
    /// copy-paste mistake in the [`TRAITS`] table (the world editor relies on
    /// `bounds_mut`, item 3).
    #[test]
    fn bounds_and_bounds_mut_target_the_same_field() {
        let mut c = config();
        for t in &TRAITS {
            let read = (t.bounds)(&c);
            let write = *(t.bounds_mut)(&mut c);
            assert_eq!(read, write, "inconsistent bounds for \"{}\"", t.name);
        }
    }

    /// Any mutation leaves **every** [`TRAITS`] gene within its bounds — even
    /// repeated, even starting from a value at the edge. Generic: a new trait is
    /// covered without touching this test.
    #[test]
    fn mutation_stays_within_bounds() {
        let c = config();
        let mutable = Mutability::default();
        let mut rng = Rng::new(42);
        let mut g = Genotype {
            mutation_rate: 0.4, // strong, to stress the clamp
            ..Genotype::default()
        };
        for _ in 0..1000 {
            g = g.mutate(&mut rng, &mutable, &c);
            for t in &TRAITS {
                let b = (t.bounds)(&c);
                let v = (t.get)(&g);
                assert!(
                    v >= b.min - 1e-4 && v <= b.max + 1e-4,
                    "{} out of bounds: {v}",
                    t.name
                );
            }
        }
    }

    /// Zero mutation = faithful clone (evolution-off regime).
    #[test]
    fn zero_mutation_is_identity() {
        let c = config();
        let mutable = Mutability::default();
        let mut rng = Rng::new(1);
        let g = Genotype::default(); // mutation_rate = 0
        assert_eq!(g.mutate(&mut rng, &mutable, &c), g);
    }

    /// Zero mutation stays a faithful clone **even for a founder value deliberately
    /// outside the gene's bounds** — a sessile plant's `max_speed = 0` (below
    /// `speed_bounds.min`) must NOT be clamped up at reproduction (which made the
    /// children mobile, growing a ray). Regression for that bug; the gene is
    /// mutable, so only the *absence of drift* protects it.
    #[test]
    fn zero_mutation_preserves_out_of_bounds_value() {
        let c = config(); // speed_bounds.min = 40
        let mutable = Mutability::default(); // max_speed IS mutable
        assert!(
            mutable.max_speed,
            "the regression assumes max_speed is mutable"
        );
        let mut rng = Rng::new(5);
        let plant = Genotype {
            max_speed: 0.0,     // sessile: deliberately below [40, 260]
            mutation_rate: 0.0, // no mutation
            ..Genotype::default()
        };
        let child = plant.mutate(&mut rng, &mutable, &c);
        assert_eq!(child.max_speed, 0.0, "a sessile plant stays immobile");
        assert_eq!(
            child, plant,
            "zero mutation = faithful clone, out of bounds included"
        );
    }

    /// The "mutable?" facet (per species): a trait marked non-mutable stays frozen
    /// on the founder's value across generations, while the mutable ones drift.
    #[test]
    fn non_mutable_trait_stays_fixed() {
        let c = config();
        let mutable = Mutability {
            max_speed: false, // frozen
            ..Mutability::default()
        };
        let mut rng = Rng::new(7);
        let base = Genotype {
            mutation_rate: 0.4, // strong mutation, so the drift is clear
            ..Genotype::default()
        };
        let mut g = base;
        let mut drifted = false;
        for _ in 0..200 {
            g = g.mutate(&mut rng, &mutable, &c);
            assert_eq!(g.max_speed, base.max_speed, "non-mutable trait frozen");
            if (g.vision_range - base.vision_range).abs() > 1e-3 {
                drifted = true;
            }
        }
        assert!(drifted, "a mutable trait, for its part, must drift");
    }

    /// The mutation rate is an **entity** gene: mutation reads
    /// `self.mutation_rate`. A genotype with a zero rate therefore does not drift.
    #[test]
    fn mutation_rate_is_per_genotype() {
        let c = config();
        let mutable = Mutability::default();
        let mut rng = Rng::new(3);
        let mut g = Genotype {
            mutation_rate: 0.0, // this genotype does not mutate
            ..Genotype::default()
        };
        let before = g;
        for _ in 0..50 {
            g = g.mutate(&mut rng, &mutable, &c);
        }
        assert_eq!(g, before, "a genotype with a zero rate stays identical");
    }
}
