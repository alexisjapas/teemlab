//! Initial population of the world: the arena (static walls) and the agents
//! (dynamic bodies + brain). Runs once, at `Startup`.

use crate::brain::{Brain, MlpBrain};
use crate::components::{
    Action, Age, Agent, Anchor, Generation, Lineage, Maneuver, Perception, Radius, Reserve,
    Species, Wall,
};
use crate::config::{SimConfig, SpawnZone, ZoneShape};
use crate::genotype::Genotype;
use crate::rng::Rng;
use crate::substrate::{ComponentStore, Emits};
use avian2d::prelude::*;
use bevy::prelude::*;

pub fn setup_world(mut commands: Commands, config: Res<SimConfig>) {
    populate(&mut commands, &config);
}

/// Populates the world: arena (static walls) + founding population. Shared by
/// `Startup` ([`setup_world`]) and the **hot reset** (item 11), so that reset and
/// first population produce rigorously the same world.
pub fn populate(commands: &mut Commands, config: &SimConfig) {
    spawn_arena(commands, config);
    spawn_agents(commands, config);
    spawn_sources(commands, config);
}

/// Four **half-spaces** (infinite planes) forming a closed box around the arena.
/// A half-space has an *infinite* solid side: an agent therefore can neither
/// tunnel through it in one tick, nor escape if it is born (reproduction) or
/// dropped (editor) beyond the edge — the solver always pushes it back inward. A
/// wall of finite thickness, by contrast, leaves a free exit "outside".
///
/// The normal passed to [`Collider::half_space`] points toward the **free** side
/// (away from the solid), like a floor's "upward" normal. We therefore aim it
/// toward the inside of the arena, and place each plane exactly on the
/// `±arena_half_extent` edge (aligned with the box drawn by `draw_arena`).
///
/// Public so that snapshot restoration (item 13) rebuilds the arena before
/// putting the saved agents back into it (the snapshot does not store the walls,
/// which are derived from the `SimConfig`).
pub fn spawn_arena(commands: &mut Commands, config: &SimConfig) {
    let h = config.arena_half_extent;
    let walls = [
        (Vec2::new(0.0, -h), Vec2::Y),    // bottom : solid below
        (Vec2::new(0.0, h), Vec2::NEG_Y), // top    : solid above
        (Vec2::new(-h, 0.0), Vec2::X),    // left   : solid on the left
        (Vec2::new(h, 0.0), Vec2::NEG_X), // right  : solid on the right
    ];
    for (origin, inward_normal) in walls {
        commands.spawn((
            Wall,
            RigidBody::Static,
            Collider::half_space(inward_normal),
            Transform::from_translation(origin.extend(0.0)),
        ));
    }
}

/// Founding population: for **each** archetype, its head count (`count`) of
/// agents scattered at random, each compiled from its archetype's genotype and
/// brain, seeded deterministically. Since Phase 3b, food sources are sessile
/// agents like any other: they are therefore populated here too (fixed count, no
/// `replenish_food` faucet). The order — species **contiguous** in archetype
/// order — fixes the stream of RNG draws; the mobile archetypes generally coming
/// before the sources, their draws stay unchanged by adding the sources at the
/// end.
fn spawn_agents(commands: &mut Commands, config: &SimConfig) {
    let mut rng = Rng::new(config.seed);
    // The sequence of species to populate: `count` agents per archetype, in
    // archetype order (sessile food sources included).
    let species_seq: Vec<u16> = config
        .archetypes
        .iter()
        .enumerate()
        .flat_map(|(i, a)| std::iter::repeat_n(i as u16, a.count))
        .collect();

    // Per-species founder counter (the k-th founder of a species), so a **founder pool**
    // (batch regime) can hand each founder a *distinct* brain. `species_seq` is grouped by
    // species, but a plain counter is robust to any order.
    let mut founder_k = vec![0usize; config.archetypes.len()];
    for (i, species) in species_seq.into_iter().enumerate() {
        let span = config.arena_half_extent - config.agent_radius_of(species) - 5.0;
        // Founding position: the whole-arena scatter unless the archetype declares a
        // spawn zone. The `None` branch is byte-for-byte the historical draw (two
        // `next_signed`), so a scenario without zones keeps its exact RNG stream
        // (chaos-sensitive: [[mlp-test-chaos-sensitive]]); only a zoned species reroutes.
        let pos = match config.spawn_zone_of(species) {
            None => Vec2::new(rng.next_signed() * span, rng.next_signed() * span),
            Some(zone) => sample_in_zone(&mut rng, zone, span),
        };
        // `heading` is drawn **in all cases** (even if a capture ignores it) to
        // keep the RNG stream bit-for-bit identical to scenarios without capture;
        // `brain_seed` is not a draw (derived from the seed).
        let heading = rng.next_f32() * std::f32::consts::TAU;
        let brain_seed = config.seed ^ (i as u64).wrapping_mul(0x9E37_79B1);
        let genotype = config.genotype_of(species);
        let k = founder_k[species as usize];
        founder_k[species as usize] += 1;
        // Founder brain, in priority order:
        // 1. a **founder pool** (batch regime, generation ≥ 1): the k-th distinct brain
        //    of a diverse cohort the orchestrator built (each a mutated variant of an
        //    elite). Only ever set in-memory for a bred species → no RNG draw, non-batch
        //    scenarios never reach this branch and stay byte-identical.
        // 2. a **captured brain** (reused trained weights): the founder is born with this
        //    exact brain — the showcase / snapshot-restore semantics, unchanged.
        // 3. otherwise the usual path compiles a **fresh** brain from the seed (a local
        //    `Rng` → the global stream is the same in all branches).
        let pooled = config
            .founder_pools
            .get(&species)
            .filter(|pool| !pool.is_empty())
            .map(|pool| pool[k % pool.len()].clone());
        // A founder **is** its own lineage: the k-th founder of a species founds
        // lineage `k`. Its descendants inherit the tag at reproduction, so the
        // scorer can read each founder-variant's share of the population.
        let lineage = k as u16;
        // Founder **starting stock** (opt-in). Both draws come *after* the
        // position/heading draws, and only when their flag is set, so a scenario with
        // neither flag never draws → byte-for-byte the historical stream
        // ([[mlp-test-chaos-sensitive]]). Canonical order when both on: nutrient, then
        // energy. A random store lets producers photosynthesise from tick 0 and staggers
        // the first starvation wave; random energy (founders are otherwise born full)
        // only spreads the reserve, so it is a separate opt-in.
        let nutrients = if config.random_initial_nutrients {
            rng.next_f32() * config.nutrient_of(species).1
        } else {
            0.0 // founder: born with no nutrient (T2).
        };
        let energy = if config.random_initial_energy {
            rng.next_f32() * config.reserve_max_of(species)
        } else {
            config.reserve_max_of(species) // founder: born full.
        };
        match pooled.or_else(|| config.captured_brain_of(species).cloned()) {
            Some(brain) => spawn_agent_with_brain(
                commands,
                config,
                genotype,
                Species(species),
                pos,
                brain,
                energy,
                nutrients,
                0,   // founder: generation 0.
                0.0, // ...born at age 0.
                lineage,
            ),
            None => spawn_agent(
                commands,
                config,
                genotype,
                Species(species),
                pos,
                heading,
                brain_seed,
                energy,
                nutrients,
                0, // founder: generation 0.
                lineage,
            ),
        }
    }
}

/// Draws a founding position honouring a [`SpawnZone`], within the spawnable arena
/// (`±span`). **Inside** a shape (`exclude: false`) samples the shape directly — a disc
/// by polar sampling (uniform over the area), a rectangle by two lerps — so it always
/// succeeds; the point is clamped back into the arena. **Outside** (`exclude: true`)
/// rejection-samples the whole-arena scatter until the point misses the shape, capped at
/// 64 tries (a keep-out covering almost the whole arena then falls back to the last draw
/// rather than looping). Only reached for a zoned species, so the byte-identity of
/// unzoned scenarios is unaffected (cf. [`spawn_agents`]).
fn sample_in_zone(rng: &mut Rng, zone: &SpawnZone, span: f32) -> Vec2 {
    let lo = Vec2::splat(-span);
    let hi = Vec2::splat(span);
    if zone.exclude {
        let mut p = Vec2::ZERO;
        for _ in 0..64 {
            p = Vec2::new(rng.next_signed() * span, rng.next_signed() * span);
            if !zone.shape.contains(p.x, p.y) {
                return p;
            }
        }
        p // keep-out too large to escape: accept the last draw (author misconfigured).
    } else {
        let p = match zone.shape {
            ZoneShape::Circle { center, radius } => {
                // Polar with r ∝ √u for a uniform disc (else points bunch at the centre).
                let r = radius * rng.next_f32().sqrt();
                let a = rng.next_f32() * std::f32::consts::TAU;
                Vec2::new(center[0] + r * a.cos(), center[1] + r * a.sin())
            }
            ZoneShape::Rect { min, max } => Vec2::new(
                min[0] + rng.next_f32() * (max[0] - min[0]),
                min[1] + rng.next_f32() * (max[1] - min[1]),
            ),
        };
        p.clamp(lo, hi)
    }
}

/// Spawns an **agent** from a genotype: the only place where the genotype is
/// *compiled* into its living phenotype (§2). Shared by the initial population
/// and reproduction (item 9), so that a newborn is in every respect an agent like
/// any other.
#[allow(clippy::too_many_arguments)]
pub fn spawn_agent(
    commands: &mut Commands,
    config: &SimConfig,
    genotype: Genotype,
    species: Species,
    pos: Vec2,
    heading: f32,
    brain_seed: u64,
    energy: f32,
    nutrients: f32,
    generation: u32,
    lineage: u16,
) {
    // The scenario chooses the *type* of brain **per species** (item 18a); we
    // compile it here into a fresh brain (§1, the author of the decision). The
    // seed serves the stateful brains (wandering, the MLP's initial weights);
    // `n_inputs` sizes the MLP's input layer (= the perception channels), drawn
    // from this agent's visual-precision **gene** (item 3) rather than from a
    // scenario setting.
    let n_inputs = MlpBrain::input_size(
        genotype.ray_count(),
        config.sensed_components(species.0).len(),
    );
    let brain = config
        .brain_of(species.0)
        .build(brain_seed, heading, n_inputs);
    // A freshly compiled agent is born at age 0; its nutrient store is whatever the
    // caller seeds (`0.0` for a hand-placed agent / a founder that did not opt into a
    // starting stock — only reproduction otherwise endows a child, `offspring_nutrient`).
    spawn_agent_with_brain(
        commands, config, genotype, species, pos, brain, energy, nutrients, generation, 0.0,
        lineage,
    );
}

/// Variant taking an **already-built** [`Brain`] rather than a seed: this is the
/// snapshot-restoration path (item 13), which reinjects the exact brain
/// (including the wander RNG state) read from the file. [`spawn_agent`] is only
/// its "fresh brain from a seed" case. The single source of the agent *bundle*,
/// so that a restored agent is in every respect an agent like any other.
#[allow(clippy::too_many_arguments)]
pub fn spawn_agent_with_brain(
    commands: &mut Commands,
    config: &SimConfig,
    genotype: Genotype,
    species: Species,
    pos: Vec2,
    brain: Brain,
    energy: f32,
    nutrients: f32,
    generation: u32,
    age: f32,
    lineage: u16,
) {
    let r = config.agent_radius_of(species.0);
    // The shape (number of rays) now comes from the visual-precision gene.
    let vision = genotype.vision();
    let mut entity = commands.spawn((
        Agent,
        species,
        genotype,
        Reserve {
            current: energy,
            max: config.reserve_max_of(species.0),
        },
        Radius(r),
        // Genealogy (depth fixed, age grows per tick) + the **nutrient store** (T2,
        // filled by `absorb_components`, spent at reproduction). Grouped in a
        // sub-tuple to stay under Bevy's bundle arity bound. With the nutrient genes
        // at 0 the store is inert (`max == 0`) → byte-identical.
        (
            Generation(generation),
            Age(age),
            // Lineage tag (founder index, inherited at reproduction) — a pure
            // observation label read only by the generational scorer, never by the
            // sim → byte-identical (cf. [`Lineage`]).
            Lineage(lineage),
            // Per-component store sized to the scenario's components; the `nutrients`
            // param seeds the nutrient (component 0). Founders and children are born
            // empty (`nutrients == 0`) → byte-identical.
            {
                let mut store = ComponentStore::new(config.capacities_of(species.0));
                store.set(0, nutrients);
                store
            },
        ),
        genotype.locomotion(),
        vision,
        Perception {
            vision: vec![0.0; vision.ray_count].into_boxed_slice(),
            target: vec![0.0; vision.ray_count].into_boxed_slice(),
            threat: vec![0.0; vision.ray_count].into_boxed_slice(),
            ray_dirs: vec![Vec2::ZERO; vision.ray_count].into_boxed_slice(),
            ..default()
        },
        // Motor command + the steering effort `act` realizes from it (consumed by
        // the agility cost in `metabolize`). Grouped to stay under Bevy's bundle
        // arity bound.
        (Action::default(), Maneuver::default()),
        brain,
        // A **solid** body (not a *sensor*) including for a sessile entity (flora,
        // food source — Phase 3b): physical exclusion between bodies is the
        // mechanism that bounds a flora's density (spatial carrying capacity), a
        // photosynthetic source being otherwise *immortal* under the
        // interact→metabolize→reap order. A forager eats it **within range** (the
        // interaction range exceeds the sum of the radii), without having to
        // overlap it. A sessile's genotype fixes `max_speed: 0` (+ no-op brain) →
        // it does not move.
        RigidBody::Dynamic,
        Collider::circle(r),
        LinearVelocity::default(),
        Transform::from_translation(pos.extend(0.0)),
    ));
    // **Anchoring** (Feature 2): a rooted species is held to its spawn point by a spring.
    // Tag it with its [`Anchor`] (this very position) and set the body's `LinearDamping`
    // so the spring settles after a jolt; [`crate::movement::anchor_spring`] then applies
    // the restoring pull and the tear-off, and `act` skips it (`Without<Anchor>`). A
    // seeded child anchors at *its* own spawn point (this `pos`) → each plant roots where
    // it grows. `None` (every existing scenario) → nothing added → byte-identical.
    if let Some(anchor) = config.anchor_of(species.0) {
        entity.insert((Anchor(pos), LinearDamping(anchor.damping)));
    }
}

/// Spawns the scenario's substrate **sources** (T2): for each [`crate::config::Source`],
/// a **non-`Agent`** entity carrying [`Emits`] at a fixed position. It has **no**
/// `Agent` / `Reserve` / `Genotype` / `Brain` / `Collider` — so every life system
/// (all `With<Agent>`) ignores it *by construction* (no metabolism, death,
/// reproduction or decision), and it is intangible. Only [`emit_sources`] reads
/// it. The visual (color, radius) lives in the config and is drawn by a dedicated
/// render path (rendering is a later step); the sources never move, so nothing needs
/// to be stored on the entity for that. Uses **no** RNG → adding sources leaves the
/// agent RNG stream of [`spawn_agents`] untouched (and an empty `sources` list is a
/// no-op → existing scenarios byte-identical).
///
/// [`emit_sources`]: crate::substrate::emit_sources
fn spawn_sources(commands: &mut Commands, config: &SimConfig) {
    for source in &config.sources {
        let mut entity = commands.spawn((
            Emits {
                component: source.component,
                rate: source.rate,
            },
            Transform::from_translation(Vec2::from(source.pos).extend(0.0)),
        ));
        // A **solid** source (a rock / obstacle): a static circle collider of its
        // visual `radius` makes the feature *tangible* — dynamic bodies collide with
        // it, so it blocks passage and carves spatial refugia / winding, inaccessible
        // zones. It stays a **non-`Agent`** entity, so every life system (`With<Agent>`)
        // still ignores it by construction (Law 11 untouched). `false` (default) → the
        // historical intangible emitter, no collider → byte-identical, no RNG.
        if source.solid {
            entity.insert((RigidBody::Static, Collider::circle(source.radius)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SpawnZone, ZoneShape};

    /// `sample_in_zone` respects the zone: `Inside` always lands within the shape (and the
    /// arena), `Outside` never does — for both a disc and a rectangle.
    #[test]
    fn sample_in_zone_respects_the_shape() {
        let mut rng = Rng::new(0xC0FFEE);
        let span = 400.0;
        let disc = ZoneShape::Circle {
            center: [60.0, -40.0],
            radius: 50.0,
        };
        let inside = SpawnZone {
            shape: disc,
            exclude: false,
        };
        let outside = SpawnZone {
            shape: disc,
            exclude: true,
        };
        for _ in 0..500 {
            let p = sample_in_zone(&mut rng, &inside, span);
            assert!(disc.contains(p.x, p.y), "inside sample escaped: {p:?}");
            assert!(
                p.x.abs() <= span && p.y.abs() <= span,
                "outside the arena: {p:?}"
            );
            let q = sample_in_zone(&mut rng, &outside, span);
            assert!(
                !disc.contains(q.x, q.y),
                "outside sample fell in the disc: {q:?}"
            );
        }
        let rect = ZoneShape::Rect {
            min: [-120.0, 20.0],
            max: [-40.0, 160.0],
        };
        let in_rect = SpawnZone {
            shape: rect,
            exclude: false,
        };
        for _ in 0..500 {
            let p = sample_in_zone(&mut rng, &in_rect, span);
            assert!(rect.contains(p.x, p.y), "rect sample escaped: {p:?}");
        }
    }
}
