//! Initial population of the world: the arena (static walls) and the agents
//! (dynamic bodies + brain). Runs once, at `Startup`.

use crate::brain::{Brain, MlpBrain};
use crate::components::{
    Action, Age, Agent, Generation, Maneuver, Perception, Radius, Reserve, Species, Wall,
};
use crate::config::SimConfig;
use crate::genotype::Genotype;
use crate::nutrients::{Emits, Nutrients};
use crate::rng::Rng;
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

    for (i, species) in species_seq.into_iter().enumerate() {
        let span = config.arena_half_extent - config.agent_radius_of(species) - 5.0;
        let pos = Vec2::new(rng.next_signed() * span, rng.next_signed() * span);
        // `heading` is drawn **in all cases** (even if a capture ignores it) to
        // keep the RNG stream bit-for-bit identical to scenarios without capture;
        // `brain_seed` is not a draw (derived from the seed).
        let heading = rng.next_f32() * std::f32::consts::TAU;
        let brain_seed = config.seed ^ (i as u64).wrapping_mul(0x9E37_79B1);
        let genotype = config.genotype_of(species);
        // If the archetype carries a **captured brain** (reused trained weights),
        // the founder is born with this exact brain; otherwise, the usual path
        // compiles a fresh brain from the seed. Building a fresh brain only uses a
        // local `Rng` → the global RNG stream is the same in both branches.
        match config.captured_brain_of(species) {
            Some(brain) => spawn_agent_with_brain(
                commands,
                config,
                genotype,
                Species(species),
                pos,
                brain.clone(),
                config.reserve_max_of(species),
                0.0, // founder: born with no nutrient (T2).
                0,   // founder: generation 0.
                0.0, // ...born at age 0.
            ),
            None => spawn_agent(
                commands,
                config,
                genotype,
                Species(species),
                pos,
                heading,
                brain_seed,
                config.reserve_max_of(species),
                0, // founder: generation 0.
            ),
        }
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
    generation: u32,
) {
    // The scenario chooses the *type* of brain **per species** (item 18a); we
    // compile it here into a fresh brain (§1, the author of the decision). The
    // seed serves the stateful brains (wandering, the MLP's initial weights);
    // `n_inputs` sizes the MLP's input layer (= the perception channels), drawn
    // from this agent's visual-precision **gene** (item 3) rather than from a
    // scenario setting.
    let n_inputs = MlpBrain::input_size(genotype.ray_count());
    let brain = config
        .brain_of(species.0)
        .build(brain_seed, heading, n_inputs);
    // A freshly compiled agent is born at age 0, and (as a founder) with no
    // nutrient — only reproduction endows a child with `offspring_nutrient` (T2).
    spawn_agent_with_brain(
        commands, config, genotype, species, pos, brain, energy, 0.0, generation, 0.0,
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
) {
    let r = config.agent_radius_of(species.0);
    // The shape (number of rays) now comes from the visual-precision gene.
    let vision = genotype.vision();
    commands.spawn((
        Agent,
        species,
        genotype,
        Reserve {
            current: energy,
            max: config.reserve_max_of(species.0),
        },
        Radius(r),
        // Genealogy (depth fixed, age grows per tick) + the **nutrient store** (T2,
        // filled by `absorb_nutrients`, spent at reproduction). Grouped in a
        // sub-tuple to stay under Bevy's bundle arity bound. With the nutrient genes
        // at 0 the store is inert (`max == 0`) → byte-identical.
        (
            Generation(generation),
            Age(age),
            Nutrients {
                current: nutrients,
                max: genotype.nutrient_capacity,
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
}

/// Spawns the scenario's substrate **sources** (T2): for each [`crate::config::Source`],
/// a **non-`Agent`** entity carrying [`Emits`] at a fixed position. It has **no**
/// `Agent` / `Reserve` / `Genotype` / `Brain` / `Collider` — so every life system
/// (all `With<Agent>`) ignores it *by construction* (no metabolism, death,
/// reproduction or decision), and it is intangible. Only [`emit_nutrients`] reads
/// it. The visual (color, radius) lives in the config and is drawn by a dedicated
/// render path (rendering is a later step); the sources never move, so nothing needs
/// to be stored on the entity for that. Uses **no** RNG → adding sources leaves the
/// agent RNG stream of [`spawn_agents`] untouched (and an empty `sources` list is a
/// no-op → existing scenarios byte-identical).
///
/// [`emit_nutrients`]: crate::nutrients::emit_nutrients
fn spawn_sources(commands: &mut Commands, config: &SimConfig) {
    for source in &config.sources {
        commands.spawn((
            Emits {
                nutrient: source.nutrient,
                rate: source.rate,
            },
            Transform::from_translation(Vec2::from(source.pos).extend(0.0)),
        ));
    }
}
