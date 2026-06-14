//! Peuplement initial du monde : l'arène (murs statiques) et les agents
//! (corps dynamiques + cerveau). Tourne une fois, au `Startup`.

use crate::brain::{Brain, WanderBrain};
use crate::components::{Action, Agent, Locomotion, Perception, Radius, Vision, Wall};
use crate::config::SimConfig;
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

/// Agents dynamiques, dispersés au hasard, chacun avec son cerveau d'errance
/// graîné de façon déterministe.
fn spawn_agents(commands: &mut Commands, config: &SimConfig) {
    let mut rng = Rng::new(config.seed);
    let r = config.agent_radius;
    let span = config.arena_half_extent - r - 5.0;

    // Forme du capteur, verrouillée par espèce (v1) : partagée par tous les
    // agents de cette run.
    let vision = Vision {
        ray_count: config.vision_rays,
        fov: config.vision_fov_deg.to_radians(),
        range: config.vision_range,
    };

    for i in 0..config.agent_count {
        let x = rng.next_signed() * span;
        let y = rng.next_signed() * span;
        let heading = rng.next_f32() * std::f32::consts::TAU;
        let brain_seed = config.seed ^ (i as u64).wrapping_mul(0x9E37_79B1);

        commands.spawn((
            Agent,
            Radius(r),
            Locomotion {
                max_speed: config.max_speed,
                agility: 0.12,
            },
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
            Transform::from_xyz(x, y, 0.0),
        ));
    }
}
