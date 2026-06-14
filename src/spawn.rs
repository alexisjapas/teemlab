//! Peuplement initial du monde : l'arène (murs statiques) et les agents
//! (corps dynamiques + cerveau). Tourne une fois, au `Startup`.

use crate::brain::{Brain, WanderBrain};
use crate::components::{Action, Agent, Perception, Radius, Reserve, Species, Wall};
use crate::config::SimConfig;
use crate::genotype::Genotype;
use crate::rng::Rng;
use avian2d::prelude::*;
use bevy::prelude::*;

pub fn setup_world(mut commands: Commands, config: Res<SimConfig>) {
    spawn_arena(&mut commands, &config);
    spawn_agents(&mut commands, &config);
}

/// Quatre murs statiques formant une boîte fermée autour de l'arène.
fn spawn_arena(commands: &mut Commands, config: &SimConfig) {
    let h = config.arena_half_extent;
    let t = 20.0; // épaisseur des murs
    let span = 2.0 * h + 2.0 * t;
    let walls = [
        (Vec2::new(0.0, h + t * 0.5), Vec2::new(span, t)), // haut
        (Vec2::new(0.0, -h - t * 0.5), Vec2::new(span, t)), // bas
        (Vec2::new(-h - t * 0.5, 0.0), Vec2::new(t, 2.0 * h)), // gauche
        (Vec2::new(h + t * 0.5, 0.0), Vec2::new(t, 2.0 * h)), // droite
    ];
    for (center, size) in walls {
        commands.spawn((
            Wall,
            RigidBody::Static,
            Collider::rectangle(size.x, size.y),
            Transform::from_translation(center.extend(0.0)),
        ));
    }
}

/// Population fondatrice : agents dispersés au hasard, tous issus du génotype
/// fondateur du scénario (l'« archétype »), graînés de façon déterministe.
fn spawn_agents(commands: &mut Commands, config: &SimConfig) {
    let mut rng = Rng::new(config.seed);
    let r = config.agent_radius;
    let span = config.arena_half_extent - r - 5.0;
    let genotype = Genotype::base(config);
    // Au moins une espèce, même si un scénario met 0 par mégarde.
    let species_count = config.species_count.max(1);

    for i in 0..config.agent_count {
        let pos = Vec2::new(rng.next_signed() * span, rng.next_signed() * span);
        let heading = rng.next_f32() * std::f32::consts::TAU;
        let brain_seed = config.seed ^ (i as u64).wrapping_mul(0x9E37_79B1);
        let species = Species((i as u16) % species_count);
        spawn_agent(
            commands,
            config,
            genotype,
            species,
            pos,
            heading,
            brain_seed,
            config.reserve_max,
        );
    }
}

/// Spawn d'un **agent** à partir d'un génotype : le seul endroit où le génotype
/// est *compilé* vers son phénotype vivant (§2). Partagé par le peuplement
/// initial et la reproduction (item 9), pour qu'un nouveau-né soit en tout point
/// un agent comme un autre.
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
) {
    let r = config.agent_radius;
    let vision = genotype.vision(config.vision_rays);
    commands.spawn((
        Agent,
        species,
        genotype,
        Reserve {
            current: energy,
            max: config.reserve_max,
        },
        Radius(r),
        genotype.locomotion(),
        vision,
        Perception {
            vision: vec![0.0; vision.ray_count].into_boxed_slice(),
            ..default()
        },
        Action::default(),
        Brain::Wander(WanderBrain::new(brain_seed, heading)),
        RigidBody::Dynamic,
        Collider::circle(r),
        LinearVelocity::default(),
        Transform::from_translation(pos.extend(0.0)),
    ));
}
