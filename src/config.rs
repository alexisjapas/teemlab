//! The *scenario*: a run's parameters, loaded from a RON file.
//!
//! This is where the project's central axis materializes — **one engine, many
//! scenarios**. [`SimConfig`] is no longer a hard-coded literal but *data*: a RON
//! file that the two entry points (windowed and headless) load identically.
//! Varying an experiment = editing a `.ron`, not recompiling.
//!
//! The **central** data is the list of [`Archetype`]s: each entry is a
//! first-order *species* (body + decider), and its **index** in the list is its
//! identity ([`crate::components::Species`]) — what the [`Relation`] table
//! targets. Since Phase 3b, **everything is an agent**: there is no longer a
//! special `Food` type. A "food source" is simply an agent with a
//! [`BrainKind::Sessile`] brain that lives on photosynthesis (a gene) and does not
//! reproduce — the degenerate case of a flora. No special number, hence no
//! collision.

use crate::brain::{Brain, BrainKind};
use crate::genotype::Genotype;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A run's parameters, deserialized from a RON scenario.
///
/// `#[serde(default)]`: a scenario only needs to mention the fields it wants to
/// change; everything else falls back to [`SimConfig::default`]. An empty `()`
/// file is therefore a valid scenario (= the defaults).
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimConfig {
    /// Rate of the fixed timestep, in Hz (solver stability, not rendering).
    pub tick_hz: f64,
    /// Half-side of the square arena, in world units.
    pub arena_half_extent: f32,
    /// The **archetypes**: the scenario's central data. Each entry is a *species*
    /// (name, color, count, body + decider), and its **index** is its identity
    /// ([`crate::components::Species`]) — what the [`relations`](Self::relations)
    /// table targets. A *food source* is an archetype like any other, with a
    /// [`BrainKind::Sessile`] brain (Phase 3b), without number collision. Empty →
    /// inert world (nothing at spawn).
    pub archetypes: Vec<Archetype>,
    /// Interaction table: who can act on whom (cf. §3, §4). `actor`/`target` are
    /// **archetype indices**. Empty by default → no interaction (inert world, as
    /// before item 7).
    pub relations: Vec<Relation>,
    /// Cells-per-side of **every** component [`Field`](crate::nutrients::Field) (one
    /// shared grid resolution over the arena). Default 48; only meaningful when
    /// [`components`](Self::components) is non-empty.
    pub field_resolution: usize,
    /// The scenario's **components** — diffusible substrates (a nutrient, a toxin, a
    /// pheromone, biomass), each a concentration field. A *distinct category* from
    /// archetypes; how a species relates to each (absorb/emit/sense/…) is declared by
    /// the (species, component) relations, never by a type (Law 11). Empty by default
    /// → no field (inert), existing scenarios unchanged.
    pub components: Vec<ComponentConfig>,
    /// **Substrate sources** (e.g. volcanic vents) that emit a component into its
    /// field; diffusion then makes gradients. A *distinct category*, **not**
    /// archetypes: spawned as **non-`Agent`** entities ignored by the life
    /// machinery. Empty by default → no source (inert), existing scenarios
    /// unchanged.
    pub sources: Vec<Source>,
    /// How each species relates to each **component** (absorb / emit / sense / affect /
    /// reproduce) — the environmental analogue of [`relations`](Self::relations)
    /// (`docs/component-emission-plan.md`), bundled one row per (species, component).
    /// Empty by default; a species with no row ignores the substrate.
    pub field_relations: Vec<FieldRelation>,
    /// Bounds of the maximum-speed gene.
    pub speed_bounds: Bounds,
    /// Bounds of the agility gene.
    pub agility_bounds: Bounds,
    /// Bounds of the vision-range gene.
    pub vision_range_bounds: Bounds,
    /// Bounds of the vision-field gene, **in degrees**.
    pub vision_fov_bounds: Bounds,
    /// Bounds of the reproduction-threshold gene.
    pub reproduction_threshold_bounds: Bounds,
    /// Bounds of the energy-passed-to-child gene.
    pub offspring_energy_bounds: Bounds,
    /// Bounds of the mutation-rate gene.
    pub mutation_rate_bounds: Bounds,
    /// Bounds of the vision-ray-count gene (visual precision). Integer bounds in
    /// practice (the gene is rounded at phenotype compilation).
    pub vision_rays_bounds: Bounds,
    /// Bounds of the photosynthesis gene (flora's passive energy gain, Phase 3).
    pub photosynthesis_bounds: Bounds,
    /// Bounds of the dispersal gene (flora's seeding distance, Phase 3).
    pub seed_dispersal_bounds: Bounds,
    /// Bounds of the brain-cost gene (energy/s per decision neuron). Drives the
    /// editor slider; the gene is non-mutable by default, so these bounds rarely
    /// clamp anything.
    pub brain_cost_bounds: Bounds,
    /// Bounds of the act-cost gene (energy/s while the eat/attack intent is held,
    /// deliberate eating). Drives the editor slider; non-mutable by default.
    pub act_cost_bounds: Bounds,
    /// **Allometric cost law** (`docs/emergent-trophics.md` §5): the world-level
    /// coefficients from which each agent's **size-derived** costs are computed
    /// (maintenance, locomotion, manoeuvring), replacing the former per-species
    /// `base_metabolism` / `move_cost` / `agility_cost` genes — the pricing of body
    /// size that makes emergent size-dominance (stage A4) obey Law 7.
    pub cost_law: CostLaw,
    /// **Emergent predation** knobs (size margin, bite range & rate) — the implicit
    /// trophic filter replacing the relation table (`docs/emergent-trophics.md` §3).
    pub predation: Predation,
    /// Background color of the **play area** (inside of the arena), sRGB `[r, g, b]`
    /// in `[0, 1]`. A **presentation** setting (windowed rendering only, cf.
    /// `main::draw_play_area`); lives in the scenario to be saved/loaded with it.
    pub play_area_color: [f32; 3],
    /// Color of the **off-game area** (behind the walls, beyond the arena), sRGB
    /// `[r, g, b]`. A presentation setting (windowed rendering only, `ClearColor`),
    /// saved with the scenario.
    pub off_game_color: [f32; 3],
    /// RNG seed: replay an *experiment config*, not bit-for-bit.
    pub seed: u64,
    /// **Generational regime** parameters (§4 axis A — batched reproduction). `None`
    /// (the default, every existing scenario) → the **continuous** regime, untouched:
    /// the field is absent from the RON and no orchestrator runs. Present → the
    /// `breed` bin / the windowed dashboard can run the `run → score → breed` loop, an
    /// **outside-sim** orchestrator (DEV Rule 1) whose inner match stays the
    /// byte-identical [`SimPlugin`](crate::SimPlugin). See `docs/p5-breeding-plan.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<BatchConfig>,
    /// **In-memory founder pools** (batch regime only), keyed by bred species index: a
    /// pool of *distinct* founder brains the orchestrator injects at generation ≥ 1 so
    /// a match starts from a **diverse** cohort — each founder a mutated variant of an
    /// elite — instead of `count` identical clones of a single
    /// [`captured_brain`](Archetype::captured_brain). This restores the founder diversity
    /// that a from-random generation 0 has (identical bodies, *diverse* brains) and that
    /// naïve re-seeding destroys (the collapse where breeding lost to a random start; see
    /// `docs/p5-breeding-plan.md` §7). **Never serialized** (`serde(skip)`): a transient,
    /// orchestrator-built construct — every scenario on disk is byte-identical and the
    /// continuous regime never sets it. A species absent from the map keeps the ordinary
    /// founder path ([`captured_brain_of`](Self::captured_brain_of) or a fresh brain).
    #[serde(skip)]
    pub founder_pools: std::collections::HashMap<u16, Vec<Brain>>,
}

/// **Allometric cost law** — the world-level coefficients from which an agent's costs
/// derive from its **body size** (radius) rather than per-species free genes
/// (`docs/emergent-trophics.md` §5). One functional form, `k · size^exponent · …`, so
/// specifying a species reduces to its size; and size, priced by maintenance +
/// locomotion, is a Law-7 trait — the prerequisite to the size-dominance of emergent
/// targeting (stage A4). `size = radius^exponent` (`exponent` default `2` — 2D area =
/// mass — but a scenario parameter, so metabolic scaling is itself an experimental
/// knob). Brain and vision keep their **own** couplings (neuron count, range × rays).
/// The future evolvable *muscular efficiency* (a per-species divisor) is fixed at 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CostLaw {
    /// The **size exponent** `e`: `size = radius^e`. Default `2` (area / mass in 2D).
    pub size_exponent: f32,
    /// **Maintenance** `k_m`: resting drain `= k_m · size` per second — the cost of
    /// survival (the former `base_metabolism`), now scaling with body size.
    pub maintenance: f32,
    /// **Locomotion** `k_v`: cruising drain `= k_v · size · (speed / founder max speed)`
    /// per second (the former `move_cost`). Linear in the speed fraction for now; a
    /// super-linear form is a documented follow-up (`docs/emergent-trophics.md` §5.4).
    pub locomotion: f32,
    /// **Manoeuvre** `k_a`: drain `= k_a · size · |Δv|` (the former `agility_cost`) —
    /// the transient cost of turning / accelerating a body of that size.
    pub maneuver: f32,
}

impl Default for CostLaw {
    /// Sane defaults: at a reference body (radius 10, `size = 100`) they reproduce the
    /// former default costs — maintenance `4`, locomotion `2` at full speed, manoeuvre
    /// `0.02 · |Δv|`. Scenario viability under the new law is re-tuned in the scenario
    /// rework; these keep the mechanism demonstrably alive.
    fn default() -> Self {
        Self {
            size_exponent: 2.0,
            maintenance: 0.04,
            locomotion: 0.02,
            maneuver: 0.0002,
        }
    }
}

impl CostLaw {
    /// All coefficients zero — a **cost-free** world: no maintenance, locomotion or
    /// manoeuvre drain. For deterministic tests and a deliberately inert scenario.
    pub fn inert() -> Self {
        Self {
            size_exponent: 2.0,
            maintenance: 0.0,
            locomotion: 0.0,
            maneuver: 0.0,
        }
    }
}

/// **Emergent predation** parameters (`docs/emergent-trophics.md` §3): the world-level
/// knobs of the *implicit* trophic filter that replaces the authored relation table
/// (amending SIM Law 8). An actor can eat a target when it **dominates** it in size (by
/// [`size_margin`](Self::size_margin)) **and** the target is **digestible** (holds a
/// component the actor needs — [`SimConfig::digestibility`]); the bite then transfers
/// reserve at `rate` within `range`. The former per-relation `transfer`/`rate`/`range`
/// become these scalars — predation is always a transfer; non-nutritional combat is
/// removed, awaiting a dedicated aggression mechanism (§9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Predation {
    /// Size advantage the actor needs: it preys on a target only if `actor_radius ≥
    /// target_radius · (1 + size_margin)` (the actor is `(1+margin)×` larger). `0` → the
    /// actor need only be `≥` the prey.
    pub size_margin: f32,
    /// Reach as a **surface-to-surface clearance** (world units): the actor bites while
    /// the gap between the two bodies is `≤ range` (`0` = contact).
    pub range: f32,
    /// Reserve transferred from prey to actor **per second** on a bite.
    pub rate: f32,
}

impl Default for Predation {
    fn default() -> Self {
        Self {
            size_margin: 0.1,
            range: 20.0,
            rate: 40.0,
        }
    }
}

/// An **archetype**: a first-order species. Its index in
/// [`SimConfig::archetypes`] is its identity ([`crate::components::Species`]).
///
/// Since Phase 3b, **every archetype is an agent**: it always carries a founding
/// genotype (an evolvable body), a brain (the decision's author, §1) and a
/// **per-species mutability**. What was once a *food source* (`Food`) is now just
/// an archetype with a [`BrainKind::Sessile`] brain living on photosynthesis and
/// without reproduction — the degenerate case of a flora. No type `enum`, hence no
/// special branch nor two-shape schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archetype {
    /// Label for the palette / the inspector.
    pub name: String,
    /// Visual identity (linear sRGB, `[r, g, b]` in `[0, 1]`).
    pub color: [f32; 3],
    /// Count at spawn (the lever of a trophic pyramid). For a sessile food source:
    /// the fixed number of bushes (the regrowth lives in the `photosynthesis` gene,
    /// no longer in a separate `regen`).
    pub count: usize,
    /// Body (and collider) radius.
    pub radius: f32,
    /// Reserve capacity (energy/HP). For a source: its full energy.
    pub reserve_max: f32,
    /// Founding genotype (the evolvable body): the genome each individual receives
    /// as a copy at spawn, which then mutates on its own (§2). `#[serde(default)]` →
    /// a scenario can mention only the useful genes.
    #[serde(default)]
    pub genotype: Genotype,
    /// Founding brain (the decision's author, §1). `Sessile` for a plant/source.
    #[serde(default)]
    pub brain: BrainKind,
    /// **Per-species mutability**: which genes are allowed to mutate (§2).
    #[serde(default)]
    pub mutable: Mutability,
    /// Provenance: the `species/*.ron` file from which this archetype was
    /// **imported** (species library). The import makes it a *copy* (the scenario
    /// stays self-contained, §9), but retains this link to allow
    /// **resynchronization** — reloading the up-to-date definition from the file
    /// while keeping the local count. `None` for an archetype defined directly in
    /// the scenario. Omitted from the RON when absent (`skip_serializing_if`):
    /// scenarios without an import are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// **Concrete captured brain** of a living agent ("capture as archetype" item):
    /// when present, this archetype's **founders** are born with THIS brain (cloned
    /// learned weights) instead of a fresh brain recompiled from
    /// [`brain`](Self::brain) (cf. [`crate::spawn`]). This is what allows
    /// **reusing trained weights**: a population is relaunched from an
    /// already-competent individual, which then diverges by mutation. Omitted from
    /// the RON when absent — scenarios without a capture are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_brain: Option<Brain>,
    /// **Origin link** of a captured archetype, *informative* (label
    /// `"<species> · G<generation>"`). Deliberately a display label and not an
    /// index: archetype indices are remapped on reorder/deletion (cf.
    /// `swap_archetypes`/`remove_archetype`), and a living population in the middle
    /// of evolving is not "resynchronizable" like a library file
    /// ([`source`](Self::source), which stays distinct). Omitted from the RON when
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_from: Option<String>,
    /// **Anchoring** to the substrate ([`AnchorConfig`]): a spring holds this species to
    /// its spawn point, with a tear-off tension and a die-on-detach choice. `None`
    /// (default, every existing scenario) ⇒ a free body, no spring → byte-identical.
    /// Omitted from the RON when absent (`skip_serializing_if`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<AnchorConfig>,
}

impl Archetype {
    /// Default color palette (shared with rendering via the *values*), to give a
    /// distinct tint to a new archetype without depending on `visuals`.
    pub const PALETTE: [[f32; 3]; 4] = [
        [0.30, 0.70, 1.00], // blue
        [1.00, 0.45, 0.35], // coral
        [0.55, 0.90, 0.45], // green
        [0.95, 0.80, 0.30], // amber
    ];

    /// Default color of the archetype at index `i` (cyclic over the palette).
    pub fn default_color(i: usize) -> [f32; 3] {
        Self::PALETTE[i % Self::PALETTE.len()]
    }

    /// A new **agent** archetype, at index `i`: default genotype/brain/mutability,
    /// standard count, palette color.
    pub fn new_agent(i: usize) -> Self {
        Self {
            name: format!("Species {i}"),
            color: Self::default_color(i),
            count: 48,
            radius: 8.0,
            reserve_max: 100.0,
            genotype: Genotype::default(),
            brain: BrainKind::default(),
            mutable: Mutability::default(),
            source: None,
            captured_brain: None,
            captured_from: None,
            anchor: None,
        }
    }

    /// A new **food source** archetype, at index `i` (Phase 3b): a *photosynthetic
    /// patch* — a sessile plant ([`BrainKind::Sessile`]) that regains its energy in
    /// place (`photosynthesis`) after being grazed, **immobile** (`max_speed: 0`)
    /// and **without reproduction** (`reproduction_threshold: 0`, repro off → fixed
    /// count). Minimal vision (negligible cost). All genes frozen (`mutable:
    /// false`): it is scenery, not a subject of evolution. The preset of the
    /// editor's "＋ Food" button; a scenario adjusts `photosynthesis` and `count` to
    /// set the ecosystem's energy throughput.
    pub fn new_food(i: usize) -> Self {
        Self {
            name: "Food".to_string(),
            color: Self::default_color(i),
            count: 0,
            radius: 6.0,
            reserve_max: 50.0,
            genotype: Genotype {
                max_speed: 0.0,
                vision_range: 30.0,
                vision_rays: 1.0,
                reproduction_threshold: 0.0,
                photosynthesis: 6.0,
                seed_dispersal: 0.0,
                ..Genotype::default()
            },
            brain: BrainKind::Sessile,
            mutable: Mutability::all_fixed(),
            source: None,
            captured_brain: None,
            captured_from: None,
            anchor: None,
        }
    }

    /// `true` if it is a **sessile** entity ([`BrainKind::Sessile`] brain): a plant
    /// / food source, as opposed to a mobile decider. Replaces the old `is_food`
    /// (the special `Food` type having been dissolved, Phase 3b): what makes an
    /// archetype a "source" is its brain, not a schema variant.
    pub fn is_sessile(&self) -> bool {
        matches!(self.brain, BrainKind::Sessile)
    }

    /// An archetype **derived from a living agent**: a clone of *this* archetype
    /// (color, radius, reserve, count, brain type, mutability) where the genome is
    /// replaced by the agent's **evolved genotype** and where its **concrete
    /// weights are frozen** (`captured_brain`). It is the seam that allows reusing
    /// trained weights: the new archetype's founders will be born with this exact
    /// brain (cf. [`crate::spawn`]). The origin link
    /// ([`captured_from`](Self::captured_from)) keeps track of the source species
    /// and generation; `source` (the library link) is cleared — it is not a file
    /// import. The name receives a ` (captured)` suffix (the list's `✦` marker, for
    /// its part, comes from `captured_brain.is_some()`).
    pub fn capture(&self, genotype: Genotype, brain: Brain, generation: u32) -> Archetype {
        Archetype {
            name: format!("{} (captured)", self.name),
            genotype,
            captured_brain: Some(brain),
            captured_from: Some(format!("{} · G{generation}", self.name)),
            source: None,
            ..self.clone()
        }
    }

    /// Serializes the archetype to readable RON — the **export** of a reusable
    /// species to the library (`species/*.ron`, item 4).
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Deserializes an archetype (a *species*) from a RON string.
    pub fn from_ron_str(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Loads a species from a RON file of the library.
    pub fn from_ron_file(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path)?;
        Ok(Self::from_ron_str(&text)?)
    }

    /// Writes the archetype to a RON file (a reusable *species*).
    pub fn save_ron_file(&self, path: impl AsRef<Path>) -> Result<(), ScenarioError> {
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// A **library file** entry: a reusable [`Archetype`] plus catalog metadata — the
/// on-disk unit of the species library (`species/<library>/*.ron`). Wrapping the
/// archetype (rather than piling catalog-only fields onto the sim's [`Archetype`])
/// keeps the engine type clean and gives the catalog a home that can grow: variants
/// today; inhabited-scenario tags and per-scenario behaviour notes later (§9).
///
/// A **base** has `variant_of: None`. A **variant** is an evolved snapshot captured
/// from a run — its `archetype` carries the evolved genotype and a frozen
/// `captured_brain` — linked to its base by name, with a `variant_id` `"<scenario>-<n>"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeciesEntry {
    /// The reusable archetype (body + brain; for a variant, the evolved genotype and
    /// the frozen `captured_brain`).
    pub archetype: Archetype,
    /// Base species name this varies; `None` for a base form. Omitted from the RON
    /// when absent (a base file stays minimal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_of: Option<String>,
    /// Variant id `"<scenario>-<n>"`; `None` for a base form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_id: Option<String>,
}

impl SpeciesEntry {
    /// A base entry wrapping `archetype` (no variant metadata).
    pub fn base(archetype: Archetype) -> Self {
        Self {
            archetype,
            variant_of: None,
            variant_id: None,
        }
    }

    /// A variant entry: an evolved `archetype` linked to base `variant_of` with `variant_id`.
    pub fn variant(archetype: Archetype, variant_of: String, variant_id: String) -> Self {
        Self {
            archetype,
            variant_of: Some(variant_of),
            variant_id: Some(variant_id),
        }
    }

    /// `true` if this entry is an evolved variant (vs a base form).
    pub fn is_variant(&self) -> bool {
        self.variant_of.is_some()
    }

    /// Serializes to readable RON — a library file.
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Loads a library entry from a RON file.
    pub fn from_ron_file(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path)?;
        Ok(ron::from_str(&text)?)
    }

    /// Writes the library entry to a RON file, creating the parent directory (e.g.
    /// `species/saved/` on a fresh checkout).
    pub fn save_ron_file(&self, path: impl AsRef<Path>) -> Result<(), ScenarioError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// `[min, max]` bounds of a gene. Materializes, together with the value (in
/// [`crate::genotype::Genotype`]) and the cost coupling (in the economy), the §2
/// triplet: *a characteristic is not a number*.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub min: f32,
    pub max: f32,
}

impl Bounds {
    /// Span (`max - min`), the natural scale of a mutation.
    pub fn span(&self) -> f32 {
        self.max - self.min
    }

    /// Brings a value back into `[min, max]`.
    pub fn clamp(&self, v: f32) -> f32 {
        v.clamp(self.min, self.max)
    }
}

/// The **mutable?** facet of §2, per trait, **per species**: is a gene allowed to
/// mutate (cf. [`crate::genotype::Genotype::mutate`]) — hence to drift and pass on
/// selectable variation — or does it stay nailed to the founder's value?
///
/// Note (and the word is deliberately *mutable*, not *heritable*): a non-mutable
/// gene is **still transmitted** to the child (copy of the parent); what this flag
/// governs is only the **mutation**. Lives in each [`Archetype`], so one species
/// can freeze a gene that another lets drift. `Default` = everything mutable
/// except the costs and the mutation rate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mutability {
    pub max_speed: bool,
    pub agility: bool,
    pub vision_range: bool,
    pub vision_fov: bool,
    pub reproduction_threshold: bool,
    pub offspring_energy: bool,
    pub mutation_rate: bool,
    pub vision_rays: bool,
    pub photosynthesis: bool,
    pub seed_dispersal: bool,
    pub brain_cost: bool,
    pub act_cost: bool,
}

impl Mutability {
    /// All genes **frozen** (no mutation): the mutability of a food source /
    /// scenery, which does not evolve (cf. [`Archetype::new_food`]).
    pub fn all_fixed() -> Self {
        Self {
            max_speed: false,
            agility: false,
            vision_range: false,
            vision_fov: false,
            reproduction_threshold: false,
            offspring_energy: false,
            mutation_rate: false,
            vision_rays: false,
            photosynthesis: false,
            seed_dispersal: false,
            brain_cost: false,
            act_cost: false,
        }
    }
}

impl Default for Mutability {
    fn default() -> Self {
        Self {
            max_speed: true,
            agility: true,
            vision_range: true,
            vision_fov: true,
            reproduction_threshold: true,
            offspring_energy: true,
            // Visual precision (ray count): mutable — it is the point of the gene,
            // and its metabolic cost (cf. `Vision::metabolic_cost`) bounds its
            // drift.
            vision_rays: true,
            // Not mutable by default: the mutation rate (unstable meta-evolution),
            // which *is* the selection pressure — if evolvable it would be whittled to
            // 0. (The metabolism/locomotion costs are now the world's allometric
            // `CostLaw`, not genes.)
            mutation_rate: false,
            // Flora genes (Phase 3), not mutable by default: lacking a cost
            // coupling, photosynthesis would drift toward the maximum (§2); and this
            // default **preserves the RNG stream** of existing scenarios (a
            // non-mutable gene does not draw in [`Genotype::mutate`]). A flora
            // scenario enables them.
            photosynthesis: false,
            seed_dispersal: false,
            // Decision-system cost: non-mutable by default (evolvable, it would be
            // driven to 0) and absent from the draw stream.
            brain_cost: false,
            // Deliberate-eating cost: like the other costs, non-mutable by default
            // (evolvable, it would be whittled to 0 and the restraint pressure would
            // vanish) and absent from the draw stream.
            act_cost: false,
        }
    }
}

/// An entry of the interaction table. Materializes the §3 insight — *eating and
/// attacking are the same verb*: a directed interaction where the actor reduces
/// the target's reserve, within range. The only semantic axis in v1 is `transfer`:
///
/// - `transfer: true`  → **predation**: what is removed from the target is gained
///   by the actor.
/// - `transfer: false` → **combat**: the reserve is destroyed, without transfer.
///
/// `actor`/`target` are **[`Archetype`] indices**. (The energy/HP distinction will
/// wait until an agent carries *several* reserves; v1 has only one.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Archetype index of the actor.
    pub actor: u16,
    /// Archetype index of the target.
    pub target: u16,
    /// Transfer (predation) or plain destruction (combat).
    pub transfer: bool,
    /// Amount of reserve transferred/destroyed **per second** of simulated time.
    pub rate: f32,
    /// Action range as a **surface-to-surface clearance**, in world units: the
    /// actor acts on a target while the gap between their bodies is `≤ range`, so
    /// `0` (the default for a new relation) means **contact**. Effective reach =
    /// `range + actor_radius (+ target_radius)`, cf. [`crate::interaction`].
    pub range: f32,
}

/// A **component**: a diffusible substrate (nutrient / toxin / pheromone / biomass),
/// rendered as a [`Field`](crate::nutrients::Field). Differentiated only by the
/// (species, component) relations that reference it (Law 11), never by a type. The
/// grid resolution is shared ([`SimConfig::field_resolution`]); per-component are its
/// `diffusion` (spreading) and `decay` (dissipation — a pheromone fades, detritus
/// decomposes; `0` = a conserved nutrient). See `docs/component-emission-plan.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ComponentConfig {
    /// Display name (editor, heatmap legend).
    pub name: String,
    /// Rebalance fraction per tick, in `[0, 1]` — the *local vs global* knob (`0` →
    /// never spreads).
    pub diffusion: f32,
    /// Per-tick fractional decay, in `[0, 1]` (`0` → conserved, a nutrient).
    pub decay: f32,
}

impl Default for ComponentConfig {
    fn default() -> Self {
        Self {
            name: "Component".to_string(),
            diffusion: 0.0,
            decay: 0.0,
        }
    }
}

/// A substrate **source**: a fixed point that emits a component into its field. A
/// *distinct category* from [`Archetype`] (it is not a life form): spawned as a
/// **non-`Agent`** entity ([`crate::spawn::spawn_sources`]) carrying
/// [`Emits`](crate::nutrients::Emits), with no collider (intangible) but a visual.
/// Sources are hand-edited in the RON (GUI editing is roadmapped).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// World position (the cell it emits into).
    pub pos: [f32; 2],
    /// Component index (into [`SimConfig::components`]).
    pub component: usize,
    /// Emission per second of simulated time.
    pub rate: f32,
    /// Visual color (linear sRGB, `[r, g, b]` in `[0, 1]`).
    pub color: [f32; 3],
    /// Visual radius; **also the collider radius** when [`solid`](Self::solid).
    pub radius: f32,
    /// **Solid** (a rock / obstacle): spawn a static circle collider of `radius` so the
    /// feature is *tangible* — it blocks bodies and carves spatial refugia / winding,
    /// inaccessible zones. Independent of `rate` (a pure rock emits nothing, `rate: 0`,
    /// and still blocks; a leaching rock does both). `false` (default) = the historical
    /// intangible emitter (a vent). `#[serde(default)]` → scenarios written before this
    /// field parse as `false` → byte-identical.
    #[serde(default)]
    pub solid: bool,
}

/// **Anchoring** of a species to the substrate (a rooted plant, a sessile filter-feeder,
/// a barnacle): the body is held to a fixed point — its spawn position, stored as
/// [`Anchor`](crate::components::Anchor) — by a **spring**, not pinned rigidly, so it can
/// be jostled and springs back. Absent by default (`Archetype::anchor == None`) → no
/// anchor component, the spring system a no-op → **byte-identical**.
///
/// The spring is realized in [`crate::movement::anchor_spring`] (an anchored body is
/// therefore **exempt from `act`**: the spring + the physics solver govern it, not the
/// locomotion velocity override). The tear-off criterion is a **tension threshold**
/// (`stiffness · displacement`, a portable quantity — no mass, unlike a contact impulse
/// ∝ radius², the non-portability that shelved the `crush`, §9): pulled past it, the
/// anchor **snaps**.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorConfig {
    /// Spring **stiffness**: the restoring pull toward the anchor per unit of
    /// displacement (a restoring *acceleration* per unit length — mass is deliberately
    /// folded out so the feel is body-size-independent). Higher ⇒ resists displacement
    /// more.
    pub stiffness: f32,
    /// Velocity **damping** of the anchored body (set as Avian `LinearDamping`) so the
    /// spring settles after a jolt instead of oscillating forever.
    pub damping: f32,
    /// **Tear-off tension**: the anchor snaps once `stiffness · |pos − anchor|` exceeds
    /// this. Coupled with `stiffness` (a stiffer spring reaches the same tension at a
    /// shorter displacement), so it reads as a breaking *tension* of the tether.
    pub tear_force: f32,
    /// On tear-off, does the body **die**? `true` — an uprooted organism cannot survive:
    /// routed through the uniform death path ([`crate::ecology::reap`], via a zeroed
    /// reserve), so it leaves a corpse (`emit_at_death`) and recycles — a portable,
    /// physical **turnover** lever. `false` — it survives, detached: the
    /// [`Anchor`](crate::components::Anchor) is dropped and it becomes a free body (a
    /// dislodged fragment that drifts / settles where it was knocked).
    pub die_on_detach: bool,
}

/// How a **species relates to a component** — the environmental analogue of the
/// interaction [`Relation`] table (`docs/component-emission-plan.md`). One **bundled
/// row per (species, component)**: any subset of its verbs may be non-zero at once (a
/// species can `sense` a component **and** be `affect`-ed by it, or be affected
/// **without** sensing it — humans ↔ CO). The scenario declares the topology; the
/// engine runs the verbs uniformly, differentiating components only here (Law 11).
/// Sparse: only the pairs that interact carry a row; `#[serde(default)]` → a row lists
/// only its non-zero verbs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FieldRelation {
    /// Archetype index of the species.
    pub species: u16,
    /// Component index (into [`SimConfig::components`]).
    pub component: usize,
    /// Field → store, per second (`0` = does not absorb).
    pub absorb: f32,
    /// This species' store capacity for the component (`0` = holds none).
    pub capacity: f32,
    /// Store/body → field, per second — **alive** emission (Phase 3; `0` = none).
    pub emit: f32,
    /// Fraction `[0, 1]` of this component's store returned to the field **at death**
    /// (recycling; generalizes the nutrient loop).
    pub emit_at_death: f32,
    /// Local concentration → a **brain perception channel** (Phase 3 — pheromones).
    pub sense: bool,
    /// Concentration → [`Reserve`](crate::components::Reserve), per second (a **toxin**
    /// `< 0`, a boon `> 0`; later, config-only on this substrate).
    pub affect: f32,
    /// Store spent per child — the **reproduction gate** (`0` = no gate).
    pub repro_cost: f32,
    /// **Nutritional requirement**: how much of this component the species needs (`0` =
    /// does not need it) — its *diet* in component terms. Consumed by **emergent
    /// targeting** (`docs/emergent-trophics.md` §3.2): an actor finds a target
    /// *digestible* when the target's stores hold the components the actor needs, so
    /// prey/predator roles emerge from `need` ∩ content instead of an authored table.
    /// Data only until stage A4 wires digestibility; default `0` → byte-identical.
    pub need: f32,
}

impl Default for FieldRelation {
    fn default() -> Self {
        Self {
            species: 0,
            component: 0,
            absorb: 0.0,
            capacity: 0.0,
            emit: 0.0,
            emit_at_death: 0.0,
            sense: false,
            affect: 0.0,
            repro_cost: 0.0,
            need: 0.0,
        }
    }
}

/// **Generational ("batched repro") regime** parameters — §4 axis A (reproduction at a
/// generation boundary) × axis B (explicit fitness). Carried on [`SimConfig::batch`] as
/// an `Option`, **absent by default** so every continuous scenario is byte-identical.
///
/// The regime is **not** a reified `enum Regime` (§4 architectural guard): it is a
/// *recomposition* — an `breeding::Orchestrator` (outside the sim) runs a cohort of
/// matches, scores each by [`Fitness`], selects the top `survivors` and re-seeds them as
/// the next generation's founders (via [`Archetype::capture`]). The **inner match** stays
/// the byte-identical [`SimPlugin`](crate::SimPlugin), its continuous in-match evolution
/// (`ecology::reproduce`) untouched. See `docs/p5-breeding-plan.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfig {
    /// Number of generation boundaries the orchestrator runs.
    pub generations: usize,
    /// Independent seeded matches per generation (the cohort) — the **founder-diversity**
    /// lever motivating P5 (the item-18b variance finding).
    pub matches_per_gen: usize,
    /// Terminal condition of one match: a fixed tick budget (v1). Richer conditions
    /// (extinction, score threshold) are a later enum — kept a single `ticks` to avoid a
    /// premature abstraction.
    pub match_ticks: u64,
    /// The archetypes under selection — the factions the orchestrator **breeds**. A single
    /// faction (`[0]`) is the foraging / single-faction-battle case; **several** factions
    /// (`[0, 1]`) are bred at once — co-evolution (the Red-Queen battle, item 19), each
    /// scored against the others by [`Fitness`] and re-seeded from its own elites.
    pub scored_species: Vec<u16>,
    /// The explicit fitness function (§4 axis B).
    pub fitness: Fitness,
    /// Top-K genomes carried into the next generation's founders (selection pressure).
    pub survivors: usize,
    /// Base seed for the per-match seeds, so a whole breeding run replays from one number
    /// (an *experiment*, not bit-for-bit — Law 10).
    pub seed_base: u64,
}

/// Explicit fitness — how a match **scores** a genome (§4 axis B). A small, growable menu of
/// engine primitives (an exhaustive `match` in [`breeding::MatchMetrics::of`]), the
/// heterogeneous counterpart of the cost / relation tables. Every reading is **time-robust**
/// — aggregated over the whole match trajectory, not the terminal tick (living food booms
/// then busts, so the last tick is a poor read; cf. `breeding::MatchMetrics`).
///
/// [`breeding::MatchMetrics::of`]: crate::breeding::MatchMetrics::of
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fitness {
    /// **Sustained biomass** — the MEAN standing population over the match. The robust forager
    /// default: rewards a lineage that keeps a population fed (not a one-tick bloom nor a dying
    /// remnant). The MLP-breeding default.
    Population,
    /// **Peak** standing population over the match — the strongest bloom the lineage reached.
    Peak,
    /// **Survival** (longevity) — the fraction of the match the species stayed alive (`0..=1`).
    /// Rewards simply not dying out.
    Survival,
    /// **Deepest lineage** reached ever over the match (highest `Generation`, caught at its
    /// peak) — how far the in-match neuroevolution got. NB *perverse* on a free reproducer
    /// (rewards reproduce-to-collapse) — prefer `Population`; kept for skill-gated foraging.
    BestEvolved,
    /// Combat **dominance** (terminal): the scored species' living count **minus** its living
    /// rivals (every other non-sessile agent — food excluded) at the match's end. Rewards both
    /// surviving *and* eliminating the enemy — the battle / factions primitive (item 19).
    Dominance,
}

impl Default for BatchConfig {
    /// Sensible breeding defaults — the `13_mlp_breed.ron` showcase's regime (a forager
    /// scored by standing biomass). The **editor** uses this when the user enables `batch`.
    fn default() -> Self {
        Self {
            generations: 8,
            matches_per_gen: 4,
            match_ticks: 5000,
            scored_species: vec![0],
            fitness: Fitness::Population,
            survivors: 2,
            seed_base: 1,
        }
    }
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_hz: 64.0,
            arena_half_extent: 400.0,
            archetypes: vec![Archetype::new_agent(0)],
            relations: Vec::new(),
            field_resolution: 48,
            components: Vec::new(),
            sources: Vec::new(),
            field_relations: Vec::new(),
            speed_bounds: Bounds {
                min: 40.0,
                max: 260.0,
            },
            agility_bounds: Bounds {
                min: 0.02,
                max: 0.5,
            },
            vision_range_bounds: Bounds {
                min: 40.0,
                max: 300.0,
            },
            vision_fov_bounds: Bounds {
                min: 40.0,
                max: 280.0,
            },
            reproduction_threshold_bounds: Bounds {
                min: 0.0,
                max: 200.0,
            },
            offspring_energy_bounds: Bounds {
                min: 10.0,
                max: 120.0,
            },
            mutation_rate_bounds: Bounds { min: 0.0, max: 0.5 },
            // Min 0: a blind agent (0 rays) is legitimate (cf. `Genotype::ray_count`).
            vision_rays_bounds: Bounds {
                min: 0.0,
                max: 21.0,
            },
            photosynthesis_bounds: Bounds {
                min: 0.0,
                max: 30.0,
            },
            seed_dispersal_bounds: Bounds {
                min: 0.0,
                max: 200.0,
            },
            // Per-neuron scale: a typical MLP has ~10 decision neurons, so a max of
            // 2.0 ≈ 20 energy/s.
            brain_cost_bounds: Bounds { min: 0.0, max: 2.0 },
            // Deliberate-eating cost, non-mutable by default: min 0 (default gene 0 →
            // inert). An editor-slider range enough for the act cost to bite against
            // the food it buys.
            act_cost_bounds: Bounds {
                min: 0.0,
                max: 10.0,
            },
            cost_law: CostLaw::default(),
            predation: Predation::default(),
            // Default backgrounds: dark play area, off-game one notch lighter —
            // enough to delimit the arena without any zone looking empty. (Reuses
            // the tints previously hard-coded in `main`.)
            play_area_color: [0.07, 0.07, 0.09],
            off_game_color: [0.17, 0.17, 0.19],
            seed: 0x00C0_FFEE,
            batch: None,
            founder_pools: std::collections::HashMap::new(),
        }
    }
}

impl SimConfig {
    /// *Empty* scenario: the arena and one default agent archetype, but **no entity
    /// at spawn** (count 0). The editor's canvas — everything is placed by hand
    /// (drag-and-drop), then launched. It is the no-argument fallback of the
    /// windowed build.
    pub fn empty() -> Self {
        let mut agent = Archetype::new_agent(0);
        agent.count = 0;
        Self {
            archetypes: vec![agent],
            ..Self::default()
        }
    }

    /// Number of archetypes (= number of species, agents **and** food combined), at
    /// least 1. The HUD, editor and palette refer to it.
    pub fn species_cardinality(&self) -> u16 {
        (self.archetypes.len() as u16).max(1)
    }

    /// The **founding genotype** of archetype `species` (the "archetype" in the
    /// genetic sense). Falls back to the default genotype for an out-of-list index.
    pub fn genotype_of(&self, species: u16) -> Genotype {
        self.archetypes
            .get(species as usize)
            .map(|a| a.genotype)
            .unwrap_or_default()
    }

    /// The **founding max speed** of archetype `species` — the *reference* speed of
    /// the locomotion cost ([`crate::ecology::metabolize`]). Reads the one useful
    /// field rather than copying the whole [`Genotype`]
    /// ([`genotype_of`](Self::genotype_of)) for every agent and every tick. Same
    /// fallback as `genotype_of` (the default founding value) → identical result.
    pub fn founder_max_speed_of(&self, species: u16) -> f32 {
        self.archetypes
            .get(species as usize)
            .map(|a| a.genotype.max_speed)
            .unwrap_or_else(|| Genotype::default().max_speed)
    }

    /// The **nutrient relationship** of `species` to the nutrient (component `0`) —
    /// `(absorb, capacity, repro_cost)` from its [`FieldRelation`] row, or `(0, 0, 0)`
    /// if it has none (the species is outside the nutrient axis). The declarative table
    /// is the single source (`docs/component-emission-plan.md`).
    pub fn nutrient_of(&self, species: u16) -> (f32, f32, f32) {
        self.field_relations
            .iter()
            .find(|f| f.species == species && f.component == 0)
            .map(|fr| (fr.absorb, fr.capacity, fr.repro_cost))
            .unwrap_or((0.0, 0.0, 0.0))
    }

    /// A **per-component vector** for `species`, reading `verb` off each of its
    /// [`FieldRelation`] rows (`0` where it has no row for that component). The vector
    /// spans **at least** every component the species' relations reference, so a store
    /// or a diet can exist without a declared field (a component held/needed but never
    /// absorbed — e.g. the trophic-transfer test's non-absorbing forager). For a
    /// well-formed scenario (every referenced component declared) its length equals
    /// `components.len()`. Backs [`capacities_of`](Self::capacities_of) and
    /// [`needs_of`](Self::needs_of).
    fn per_component(&self, species: u16, verb: impl Fn(&FieldRelation) -> f32) -> Vec<f32> {
        let n = self.components.len().max(
            self.field_relations
                .iter()
                .filter(|f| f.species == species)
                .map(|f| f.component + 1)
                .max()
                .unwrap_or(0),
        );
        (0..n)
            .map(|c| {
                self.field_relations
                    .iter()
                    .find(|f| f.species == species && f.component == c)
                    .map(&verb)
                    .unwrap_or(0.0)
            })
            .collect()
    }

    /// The per-component **store capacities** of `species` (the `capacity` verb). Sizes
    /// the agent's [`Nutrients`](crate::nutrients::Nutrients) store at spawn; a
    /// single-axis scenario (capacity only on component `0`) yields `[cap, 0, …]` →
    /// byte-identical with the former single `max` from [`nutrient_of`](Self::nutrient_of).
    pub fn capacities_of(&self, species: u16) -> Vec<f32> {
        self.per_component(species, |f| f.capacity)
    }

    /// The per-component **nutritional requirements** of `species` (the `need` verb) —
    /// its *diet*. Emergent targeting (stage A4) finds a target digestible when it holds
    /// the components the actor needs (`docs/emergent-trophics.md` §3.2). `0` everywhere
    /// until a scenario declares a `need` → byte-identical.
    pub fn needs_of(&self, species: u16) -> Vec<f32> {
        self.per_component(species, |f| f.need)
    }

    /// **Digestibility** of `prey` for `actor` ∈ `[0, 1]` — the fraction of the
    /// components `actor` **needs** ([`needs_of`](Self::needs_of)) that `prey`'s species
    /// can **hold** ([`capacities_of`](Self::capacities_of)). `0` when the actor needs
    /// nothing, or the prey holds none of it. The nutritional half of the emergent
    /// target filter (`docs/emergent-trophics.md` §3.2); it also grades the brain's
    /// *target* channel (how appetising a prey is). Species-level (config only) — the
    /// *amount* actually transferred on a bite is the prey entity's live store (cf.
    /// [`crate::interaction`]).
    pub fn digestibility(&self, actor: u16, prey: u16) -> f32 {
        let need = self.needs_of(actor);
        let cap = self.capacities_of(prey);
        let needed = need.iter().filter(|&&n| n > 0.0).count();
        if needed == 0 {
            return 0.0;
        }
        let met = need
            .iter()
            .enumerate()
            .filter(|&(c, &n)| n > 0.0 && cap.get(c).copied().unwrap_or(0.0) > 0.0)
            .count();
        met as f32 / needed as f32
    }

    /// `true` if archetype `actor` can **prey on** `target` — the **emergent** target
    /// filter (§3, amending SIM Law 8): the actor must **dominate** it in size (by
    /// [`Predation::size_margin`]) **and** the target must be **digestible**
    /// ([`digestibility`](Self::digestibility) `> 0`). Replaces the authored relation
    /// table; no cannibalism (`actor == target`). Perception derives from it: a *target*
    /// is what the actor can eat, a *threat* is what can eat the actor (the inverse).
    pub fn can_eat(&self, actor: u16, target: u16) -> bool {
        actor != target
            && self.agent_radius_of(actor)
                >= self.agent_radius_of(target) * (1.0 + self.predation.size_margin)
            && self.digestibility(actor, target) > 0.0
    }

    /// The components `species` **senses** (a [`FieldRelation`] with `sense: true`),
    /// sorted by component index — the order in which their local concentrations fill
    /// [`Perception::field_state`](crate::components::Perception::field_state) and the
    /// tail of the MLP input. Empty → the species senses no field (byte-identical: no
    /// extra input channels).
    pub fn sensed_components(&self, species: u16) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .field_relations
            .iter()
            .filter(|f| f.species == species && f.sense)
            .map(|f| f.component)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The founding **brain type** of archetype `species` (the decision's author,
    /// §1). Falls back to wandering for an out-of-list index. Beyond the founder,
    /// the brain is transmitted by inheritance at reproduction
    /// ([`crate::brain::Brain::reproduce`]), without re-reading this field.
    pub fn brain_of(&self, species: u16) -> BrainKind {
        self.archetypes
            .get(species as usize)
            .map(|a| a.brain.clone())
            .unwrap_or_default()
    }

    /// The **concrete captured brain** of archetype `species`, if it has one
    /// ("capture as archetype" item). Present ⇒ its founders are born with this
    /// exact brain (learned weights) rather than a fresh brain compiled from
    /// [`brain_of`](Self::brain_of). `None` (the default case, all existing
    /// scenarios) → unchanged spawn path.
    pub fn captured_brain_of(&self, species: u16) -> Option<&Brain> {
        self.archetypes
            .get(species as usize)
            .and_then(|a| a.captured_brain.as_ref())
    }

    /// The **anchor config** of archetype `species`, if it is rooted to the substrate
    /// ([`AnchorConfig`]). `None` (default, all existing scenarios) → a free body, the
    /// spring system skips it → byte-identical.
    pub fn anchor_of(&self, species: u16) -> Option<&AnchorConfig> {
        self.archetypes
            .get(species as usize)
            .and_then(|a| a.anchor.as_ref())
    }

    /// The **mutability** ("mutable?" facet per gene) of archetype `species`. Falls
    /// back to the default for an out-of-list index.
    pub fn mutable_of(&self, species: u16) -> Mutability {
        self.archetypes
            .get(species as usize)
            .map(|a| a.mutable)
            .unwrap_or_default()
    }

    /// The **max reserve** (body capacity) of archetype `species`. The **fill %**
    /// ([`crate::components::Reserve::fraction`]) stays normalized `[0, 1]` whatever
    /// the capacity, hence comparable across species.
    pub fn reserve_max_of(&self, species: u16) -> f32 {
        self.archetypes
            .get(species as usize)
            .map(|a| a.reserve_max)
            .unwrap_or(100.0)
    }

    /// The **body radius** of archetype `species` (body + collider).
    pub fn agent_radius_of(&self, species: u16) -> f32 {
        self.archetypes
            .get(species as usize)
            .map(|a| a.radius)
            .unwrap_or(8.0)
    }

    /// The **color** of archetype `species` (falls back to the palette by index).
    pub fn color_of(&self, species: u16) -> [f32; 3] {
        self.archetypes
            .get(species as usize)
            .map(|a| a.color)
            .unwrap_or_else(|| Archetype::default_color(species as usize))
    }

    /// `true` if archetype `actor` can act on archetype `target` — a [`Relation`]
    /// allows it. It is the **target filter** of the interaction primitive (§3:
    /// *eating and attacking are the same verb*): what makes an entity a *target* in
    /// `Brain::Hunter`'s perception channel (item 16).
    pub fn acts_on(&self, actor: u16, target: u16) -> bool {
        self.relations
            .iter()
            .any(|r| r.actor == actor && r.target == target)
    }

    /// Builds the scenario from the 1st positional argument (RON path), with
    /// `fallback` when no argument is given.
    ///
    /// - No argument → `fallback`.
    /// - Unreadable / invalid file → we fail **loudly** (exit 1). Silently running
    ///   the wrong world is worse than stopping.
    ///
    /// With an argument, both binaries load **exactly the same scenario, the same
    /// way**; they differ only in their no-argument fallback (cf.
    /// [`SimConfig::from_cli`], populated, and [`SimConfig::empty`], empty).
    pub fn from_cli_or(fallback: Self) -> Self {
        match std::env::args().nth(1) {
            None => fallback,
            Some(path) => Self::from_ron_file(&path).unwrap_or_else(|err| {
                eprintln!("teemlab: scenario \"{path}\" unreadable: {err}");
                std::process::exit(1);
            }),
        }
    }

    /// [`from_cli_or`](SimConfig::from_cli_or) with the default (populated) scenario
    /// as fallback — the headless build, whose smoke test needs agents.
    pub fn from_cli() -> Self {
        Self::from_cli_or(Self::default())
    }

    /// Loads and deserializes a scenario from a RON file.
    pub fn from_ron_file(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path)?;
        Ok(Self::from_ron_str(&text)?)
    }

    /// Deserializes a scenario from a RON string.
    pub fn from_ron_str(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Serializes the scenario to readable RON (export from the editor, item 5).
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Writes the scenario to a RON file.
    pub fn save_ron_file(&self, path: impl AsRef<Path>) -> Result<(), ScenarioError> {
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// Failure to load a scenario: I/O or RON parsing.
#[derive(Debug)]
pub enum ScenarioError {
    /// The file could not be read (absent, permissions, …).
    Io(std::io::Error),
    /// The content is not valid RON for [`SimConfig`].
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScenarioError::Io(e) => write!(f, "cannot read: {e}"),
            ScenarioError::Parse(e) => write!(f, "invalid RON: {e}"),
        }
    }
}

impl std::error::Error for ScenarioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScenarioError::Io(e) => Some(e),
            ScenarioError::Parse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ScenarioError {
    fn from(e: std::io::Error) -> Self {
        ScenarioError::Io(e)
    }
}

impl From<ron::error::SpannedError> for ScenarioError {
    fn from(e: ron::error::SpannedError) -> Self {
        ScenarioError::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::{Brain, BrainKind, MlpBrain};

    /// First **mobile** (non-sessile) archetype — test helper.
    fn first_mobile(cfg: &SimConfig) -> &Archetype {
        cfg.archetypes
            .iter()
            .find(|a| !a.is_sessile())
            .expect("a mobile agent")
    }

    /// A partial scenario parses, and the omitted fields fall back to the default.
    #[test]
    fn partial_scenario_falls_back_to_default() {
        let cfg = SimConfig::from_ron_str("(tick_hz: 30.0, arena_half_extent: 200.0, seed: 7)")
            .expect("valid RON");
        assert_eq!(cfg.tick_hz, 30.0);
        assert_eq!(cfg.arena_half_extent, 200.0);
        assert_eq!(cfg.seed, 7);
        assert_eq!(cfg.archetypes, SimConfig::default().archetypes);

        let empty = SimConfig::from_ron_str("()").expect("valid empty RON");
        assert_eq!(empty, SimConfig::default());
    }

    /// A RON hexadecimal literal indeed yields the expected seed.
    #[test]
    fn hex_seed_literal() {
        let cfg = SimConfig::from_ron_str("(seed: 0x00C0FFEE)").expect("valid RON");
        assert_eq!(cfg.seed, 0x00C0_FFEE);
    }

    /// An unknown field is rejected rather than silently ignored
    /// (`deny_unknown_fields`): a typo in a scenario must be visible.
    #[test]
    fn unknown_field_is_rejected() {
        assert!(SimConfig::from_ron_str("(seedz: 9)").is_err());
    }

    /// `capture` derives an archetype that freezes the agent's **evolved genome**
    /// AND its **concrete weights**, keeps an origin link, and leaves the source
    /// species intact — the seam of "reusing trained weights".
    #[test]
    fn capture_freezes_genome_and_weights_with_origin_link() {
        let source = Archetype::new_agent(0); // "Species 0", default BrainKind
        let evolved = Genotype {
            vision_range: 123.0, // a genome distinct from the default
            ..Genotype::default()
        };
        let brain = Brain::Mlp(MlpBrain::random(7, MlpBrain::input_size(3, 0), &[6]));

        let captured = source.capture(evolved, brain.clone(), 42);

        assert_eq!(
            captured.captured_brain,
            Some(brain),
            "concrete weights frozen"
        );
        assert_eq!(
            captured.genotype.vision_range, 123.0,
            "evolved genome carried over"
        );
        assert_eq!(
            captured.captured_from.as_deref(),
            Some("Species 0 · G42"),
            "origin link = species + generation"
        );
        assert_eq!(captured.source, None, "not a library import");
        assert!(captured.name.contains("(captured)"), "name marked");
        // The rest is cloned from the source.
        assert_eq!(captured.color, source.color);
        assert_eq!(captured.radius, source.radius);
        assert_eq!(captured.count, source.count);
        // The source stays intact.
        assert!(
            source.captured_brain.is_none(),
            "the original species is intact"
        );
    }

    /// The captured weights survive a RON round-trip (this is what makes a trained
    /// species reusable via the library / a saved scenario). An archetype without a
    /// capture does not emit the field (`skip_serializing_if`), so existing
    /// scenarios stay unchanged.
    #[test]
    fn captured_brain_survives_ron_round_trip() {
        let brain = Brain::Mlp(MlpBrain::random(3, MlpBrain::input_size(4, 0), &[5]));
        let captured = Archetype::new_agent(0).capture(Genotype::default(), brain, 1);
        let ron = captured.to_ron_string().expect("serializable");
        let back = Archetype::from_ron_str(&ron).expect("deserializable");
        assert_eq!(back, captured, "faithful RON round-trip (weights included)");

        let plain = Archetype::new_agent(0);
        let plain_ron = plain.to_ron_string().expect("serializable");
        assert!(
            !plain_ron.contains("captured_brain"),
            "field omitted when absent: {plain_ron}"
        );
    }

    /// Serialization round-trip: what the editor saves reads back identically.
    #[test]
    fn ron_roundtrip_is_lossless() {
        let mut cfg = SimConfig::default();
        cfg.archetypes.push(Archetype::new_food(1));
        cfg.relations.push(Relation {
            actor: 0,
            target: 1,
            transfer: true,
            rate: 12.0,
            range: 9.0,
        });
        let text = cfg.to_ron_string().expect("RON serialization");
        let back = SimConfig::from_ron_str(&text).expect("RON re-read");
        assert_eq!(cfg, back);
    }

    /// The **generational regime** is optional: a continuous scenario omits `batch` from
    /// its RON (`skip_serializing_if`) and reads back as `None` — so every existing
    /// scenario stays byte-identical (no migration, DEV Rule 3). A scenario **with** a
    /// batch round-trips losslessly.
    #[test]
    fn batch_regime_is_optional_and_roundtrips() {
        // Absent by default → field omitted from the RON, reads back None.
        let plain = SimConfig::default();
        assert_eq!(plain.batch, None);
        let text = plain.to_ron_string().expect("RON serialization");
        assert!(
            !text.contains("batch"),
            "the batch field must be omitted when None:\n{text}"
        );

        // Present → lossless round-trip.
        let cfg = SimConfig {
            batch: Some(BatchConfig {
                generations: 12,
                matches_per_gen: 6,
                match_ticks: 6000,
                scored_species: vec![0],
                fitness: Fitness::BestEvolved,
                survivors: 3,
                seed_base: 1,
            }),
            ..SimConfig::default()
        };
        let back = SimConfig::from_ron_str(&cfg.to_ron_string().expect("RON serialization"))
            .expect("re-read");
        assert_eq!(cfg, back);
    }

    /// A species (archetype) makes a lossless RON round-trip, `source` included —
    /// the library's export/import (item 4).
    #[test]
    fn archetype_ron_roundtrip_is_lossless() {
        let mut a = Archetype::new_agent(0);
        a.source = Some("species/wolf.ron".into());
        let back =
            Archetype::from_ron_str(&a.to_ron_string().expect("serialization")).expect("re-read");
        assert_eq!(a, back);
    }

    /// The bundled species library file parses into a `SpeciesEntry` base (no variant
    /// metadata) wrapping a hunter agent archetype without `source` (the file *is* the
    /// source). Guardrail on the library file schema.
    #[test]
    fn bundled_species_parses_as_a_hunter_base() {
        let text = include_str!("../species/examples/hunter.ron");
        let entry: SpeciesEntry = ron::from_str(text).expect("valid hunter species entry");
        assert!(!entry.is_variant(), "the bundled hunter is a base form");
        assert_eq!(entry.variant_of, None);
        assert_eq!(entry.variant_id, None);
        let a = entry.archetype;
        assert!(!a.is_sessile());
        assert_eq!(a.brain, BrainKind::Hunter);
        assert_eq!(a.source, None, "a base library file has no source");
    }

    /// A `SpeciesEntry` round-trips through RON, base and variant alike — and a base
    /// omits the variant metadata (skip_serializing_if), so its file stays minimal.
    #[test]
    fn species_entry_roundtrips() {
        let base = SpeciesEntry::base(Archetype::new_agent(0));
        let text = base.to_ron_string().expect("serialization");
        assert!(
            !text.contains("variant_of"),
            "a base omits variant metadata:\n{text}"
        );
        assert_eq!(ron::from_str::<SpeciesEntry>(&text).expect("re-read"), base);

        let variant = SpeciesEntry::variant(
            Archetype::new_agent(1),
            "Hunter".to_string(),
            "predator_prey-1".to_string(),
        );
        let text = variant.to_ron_string().expect("serialization");
        assert!(variant.is_variant());
        assert_eq!(
            ron::from_str::<SpeciesEntry>(&text).expect("re-read"),
            variant
        );
    }

    /// A species without `source` does not emit the field (skip_serializing_if) and
    /// reads back as `None`: non-imported scenario archetypes stay unchanged (no
    /// migration).
    #[test]
    fn archetype_without_source_omits_the_field() {
        let a = Archetype::new_food(1);
        assert_eq!(a.source, None);
        let text = a.to_ron_string().expect("serialization");
        assert!(
            !text.contains("source"),
            "the source field must be omitted when None:\n{text}"
        );
        assert_eq!(Archetype::from_ron_str(&text).expect("re-read"), a);
    }

    /// The bundled default scenario stays in sync with [`SimConfig::default`].
    #[test]
    fn bundled_default_matches_default() {
        let text = include_str!("../scenarios/examples/01_default.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid default scenario");
        assert_eq!(cfg, SimConfig::default());
    }

    /// The bundled empty scenario stays in sync with [`SimConfig::empty`] and spawns
    /// no entity.
    #[test]
    fn bundled_empty_matches_empty() {
        let text = include_str!("../scenarios/examples/00_empty.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid empty scenario");
        assert_eq!(cfg, SimConfig::empty());
        assert!(cfg.archetypes.iter().all(|a| a.count == 0));
    }

    /// The relation table parses, and an unknown field is rejected in it.
    #[test]
    fn relations_parse_from_ron() {
        let cfg = SimConfig::from_ron_str(
            "(relations: [(actor: 0, target: 1, transfer: true, rate: 40.0, range: 28.0)])",
        )
        .expect("valid RON");
        assert_eq!(cfg.relations.len(), 1);
        assert_eq!(cfg.relations[0].actor, 0);
        assert_eq!(cfg.relations[0].target, 1);
        assert!(cfg.relations[0].transfer);

        assert!(
            SimConfig::from_ron_str(
                "(relations: [(actor: 0, target: 1, transfer: true, rate: 1.0, range: 1.0, oops: 2)])"
            )
            .is_err()
        );
    }

    /// `acts_on` reflects the relation table (the target filter, directed).
    #[test]
    fn acts_on_follows_relations() {
        let cfg = SimConfig::from_ron_str(
            "(relations: [(actor: 0, target: 1, transfer: true, rate: 1.0, range: 1.0)])",
        )
        .unwrap();
        assert!(cfg.acts_on(0, 1));
        assert!(!cfg.acts_on(1, 0), "the relation is directed");
        assert!(!cfg.acts_on(0, 2), "species not targeted");
    }

    /// The per-archetype resolvers read the index entry, with an out-of-list fallback.
    #[test]
    fn resolvers_read_archetype_by_index() {
        let mut cfg = SimConfig::default();
        // Species 0: mobile agent. Species 1: sessile source (photosynthetic patch).
        cfg.archetypes.push(Archetype::new_food(1));
        cfg.archetypes[0].brain = BrainKind::Hunter;
        cfg.archetypes[0].reserve_max = 120.0;
        cfg.archetypes[0].radius = 10.0;
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        assert_eq!(cfg.reserve_max_of(0), 120.0);
        assert_eq!(cfg.agent_radius_of(0), 10.0);
        assert!(cfg.archetypes[1].is_sessile());
        // `founder_max_speed_of` reads the same field as `genotype_of`, without
        // copying the whole genotype — strict equivalence (present case and fallback).
        assert_eq!(cfg.founder_max_speed_of(0), cfg.genotype_of(0).max_speed);
        // Out-of-list index → fallbacks.
        assert_eq!(cfg.brain_of(9), BrainKind::default());
        assert_eq!(cfg.reserve_max_of(9), 100.0);
        assert_eq!(cfg.founder_max_speed_of(9), cfg.genotype_of(9).max_speed);
    }

    /// The hunt scenario: a hunter agent **and** a relation designating the food
    /// (another archetype) as a target — otherwise the "target" channel stays zero.
    #[test]
    fn bundled_hunt_scenario_uses_hunter_on_a_target() {
        let text = include_str!("../scenarios/examples/05_hunt.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid hunt scenario");
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        let food = cfg
            .archetypes
            .iter()
            .position(|a| a.is_sessile())
            .expect("a sessile source") as u16;
        assert!(
            cfg.relations.iter().any(|r| r.target == food),
            "the hunter needs a designated target (the food)"
        );
    }

    /// The predator-prey scenario: a three-level trophic chain in pure data
    /// (pyramid by counts, hunter brain, two chained relations).
    #[test]
    fn bundled_predator_prey_is_a_trophic_chain() {
        let text = include_str!("../scenarios/examples/10_predator_prey.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid predator-prey scenario");
        // Pyramid: strictly fewer predators (species 0) than prey (1).
        assert!(
            cfg.archetypes[0].count < cfg.archetypes[1].count,
            "a pyramid wants prey ≫ predators"
        );
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        // The predator eats a species that itself eats a food.
        let prey = cfg
            .relations
            .iter()
            .find(|r| r.actor == 0 && r.transfer)
            .expect("the predator eats someone")
            .target;
        let foods: Vec<u16> = cfg
            .archetypes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_sessile())
            .map(|(i, _)| i as u16)
            .collect();
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.actor == prey && foods.contains(&r.target) && r.transfer),
            "the predator's prey must itself graze a food (3 levels)"
        );
    }

    /// The evolution scenario activates the loop (reproduction + mutation) and bounds
    /// the food (finite regrowth → carrying capacity).
    #[test]
    fn bundled_evolution_scenario_closes_the_loop() {
        let text = include_str!("../scenarios/examples/04_evolution.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid evolution scenario");
        let agent = first_mobile(&cfg);
        let genotype = &agent.genotype;
        assert!(
            genotype.reproduction_threshold > 0.0,
            "reproduction must be active"
        );
        assert!(genotype.mutation_rate > 0.0, "mutation must be active");
        assert!(
            cfg.archetypes
                .iter()
                .any(|a| a.is_sessile() && a.genotype.photosynthesis > 0.0),
            "a photosynthetic source feeds the economy (carrying capacity)"
        );
        assert!(
            genotype.reproduction_threshold <= agent.reserve_max,
            "a threshold above the max would be unreachable"
        );
    }

    /// The cohabitation scenario pits TWO brains (hunter vs wander) at equal counts
    /// on the same food (driver `tests/cohabitation`).
    #[test]
    fn bundled_cohabitation_pits_two_brains_on_shared_food() {
        let text = include_str!("../scenarios/examples/06_cohabitation.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid cohabitation scenario");
        assert_eq!(cfg.archetypes[0].count, cfg.archetypes[1].count);
        assert_eq!(
            cfg.brain_of(0),
            BrainKind::Hunter,
            "species 0 = competent control"
        );
        assert!(
            matches!(cfg.brain_of(1), BrainKind::Wander { .. }),
            "species 1 = naive control"
        );
        let food = cfg
            .archetypes
            .iter()
            .position(|a| a.is_sessile())
            .expect("a sessile source") as u16;
        for s in [0u16, 1] {
            assert!(
                cfg.relations
                    .iter()
                    .any(|r| r.actor == s && r.target == food && r.transfer),
                "species {s} must be able to eat the food"
            );
        }
    }

    /// The MLP scenario pits a LEARNED brain (species 0) against the wander control
    /// (species 1) on the same food (driver `tests/mlp`).
    #[test]
    fn bundled_mlp_brain_pits_a_learned_brain_against_wander() {
        let text = include_str!("../scenarios/examples/07_mlp_brain.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid MLP scenario");
        assert!(
            matches!(cfg.brain_of(0), BrainKind::Mlp { ref hidden } if !hidden.is_empty()),
            "species 0 = learned brain (MLP)"
        );
        assert!(
            matches!(cfg.brain_of(1), BrainKind::Wander { .. }),
            "species 1 = wander control"
        );
    }

    /// The MLP-breeding scenario (P5): a generational `batch` whose scored species is the
    /// learned MLP foraging the oasis flora — the carrier the `breed` bin runs (a
    /// generator, not a CI sim). Guardrail on the batch schema + the scenario wiring.
    #[test]
    fn bundled_mlp_breed_carries_a_batch_regime() {
        let text = include_str!("../scenarios/examples/13_mlp_breed.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid MLP-breeding scenario");
        let batch = cfg.batch.as_ref().expect("a batch regime");
        assert!(batch.generations > 1, "a generational run");
        assert!(
            matches!(cfg.brain_of(batch.scored_species[0]), BrainKind::Mlp { .. }),
            "the scored species is the learned MLP"
        );
    }

    /// The battle-breeding scenario (P5 item 19): a generational `batch` scored by **combat
    /// dominance**, with two factions waging mutual `transfer:false` war. The scored faction
    /// is bred to dominate. Guardrail on the combat-fitness schema + the scenario wiring.
    #[test]
    fn bundled_battle_breed_is_a_combat_dominance_regime() {
        let text = include_str!("../scenarios/examples/14_battle_breed.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid battle-breeding scenario");
        let batch = cfg.batch.as_ref().expect("a batch regime");
        assert_eq!(
            batch.fitness,
            Fitness::Dominance,
            "scored by combat dominance"
        );
        let scored = batch.scored_species[0];
        // The scored faction is a mobile fighter, not the sessile food.
        assert!(!cfg.archetypes[scored as usize].is_sessile());
        // It is at war: a combat (transfer:false) relation where it is the actor…
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.actor == scored && !r.transfer),
            "the scored faction must wage combat (transfer:false)"
        );
        // …and the enemy strikes back (the war is mutual).
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.target == scored && !r.transfer),
            "the war is mutual (the enemy attacks the scored faction too)"
        );
    }

    /// The Red-Queen scenario (P5 item 19, **co-evolution**): a generational `batch`
    /// breeding **two** factions at once (`scored_species` has ≥ 2), each a mobile fighter
    /// at mutual war, scored by `Dominance`. Guardrail on the multiple-scored-species schema.
    #[test]
    fn bundled_red_queen_breeds_two_warring_factions() {
        let text = include_str!("../scenarios/examples/15_red_queen.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid Red-Queen scenario");
        let batch = cfg.batch.as_ref().expect("a batch regime");
        assert!(
            batch.scored_species.len() >= 2,
            "co-evolution breeds several factions"
        );
        assert_eq!(batch.fitness, Fitness::Dominance);
        for &s in &batch.scored_species {
            assert!(
                !cfg.archetypes[s as usize].is_sessile(),
                "a bred faction is a mobile fighter, not food"
            );
        }
        // The two bred factions wage mutual combat (each attacks the other, transfer:false).
        let (a, b) = (batch.scored_species[0], batch.scored_species[1]);
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.actor == a && r.target == b && !r.transfer)
                && cfg
                    .relations
                    .iter()
                    .any(|r| r.actor == b && r.target == a && !r.transfer),
            "the two bred factions are at war"
        );
    }

    /// The flora scenario (Phase 3): a **sessile** plant that lives on
    /// photosynthesis and **self-limits** through intraspecific competition (a
    /// relation on itself, without transfer — the §3 interaction primitive).
    #[test]
    fn bundled_flora_is_a_self_competing_sessile_plant() {
        let text = include_str!("../scenarios/examples/03_flora.ron");
        let cfg = SimConfig::from_ron_str(text).expect("valid flora scenario");
        assert_eq!(cfg.brain_of(0), BrainKind::Sessile);
        assert!(
            cfg.archetypes[0].genotype.photosynthesis > 0.0,
            "the flora lives on photosynthesis"
        );
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.actor == 0 && r.target == 0 && !r.transfer),
            "self-competition expected (Plant→Plant, without transfer)"
        );
    }
}
