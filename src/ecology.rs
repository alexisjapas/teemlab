//! L'**économie d'énergie** du scénario de sélection naturelle (item 8) :
//! *manger, dépenser, mourir*.
//!
//! C'est ici que se joue, selon §7, tout l'équilibre de la sélection naturelle —
//! du **réglage**, pas de l'algo. Trois systèmes :
//!
//! - [`metabolize`] draine l'énergie (base + locomotion + **coût de la vision**,
//!   le couplage quantifié à l'item 6 trouvant enfin son consommateur) ;
//! - [`reap`] retire les agents à court d'énergie ;
//! - [`replenish_food`] entretient les sources de nourriture pour garder
//!   l'économie soutenable.
//!
//! Manger, lui, n'est pas ici : c'est la primitive d'interaction (item 7) qui
//! transfère l'énergie de la nourriture vers l'agent. Le moteur n'a qu'un verbe.

use crate::components::{Agent, Food, Locomotion, Radius, Reserve, Species, Vision};
use crate::config::SimConfig;
use crate::rng::Rng;
use avian2d::prelude::*;
use bevy::prelude::*;

/// Flux aléatoire de la simulation pour les événements stochastiques (ici, les
/// positions de réapparition de la nourriture). Vit dans le monde de sim, seedé
/// depuis la config — on rejoue une *expérience*, pas le bit-à-bit (§5).
#[derive(Resource)]
pub struct SimRng(pub Rng);

/// MÉTABOLISME : drainer l'énergie de chaque agent — base + surcoût de vitesse +
/// coût du capteur de vision. Plancher à zéro ; la mort est laissée à [`reap`].
pub fn metabolize(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut agents: Query<(&mut Reserve, &Vision, &LinearVelocity, &Locomotion), With<Agent>>,
) {
    if config.base_metabolism == 0.0 && config.move_cost == 0.0 {
        // Monde inerte : on évite même de payer le coût de vision si aucun
        // métabolisme n'est configuré (scénarios pré-item-8).
        return;
    }
    let dt = time.delta_secs();
    for (mut reserve, vision, velocity, loco) in &mut agents {
        let speed_frac = if loco.max_speed > 0.0 {
            (velocity.0.length() / loco.max_speed).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let drain =
            config.base_metabolism + config.move_cost * speed_frac + vision.metabolic_cost();
        reserve.current = (reserve.current - drain * dt).max(0.0);
    }
}

/// MOURIR : retirer du monde les agents dont l'énergie est épuisée.
pub fn reap(mut commands: Commands, agents: Query<(Entity, &Reserve), With<Agent>>) {
    for (entity, reserve) in &agents {
        if reserve.current <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Entretenir la nourriture : retirer les sources épuisées et réensemencer pour
/// maintenir `food_count` constant. C'est le robinet d'énergie qui entre dans
/// l'écosystème ; son débit (vs le métabolisme cumulé) fixe le point d'équilibre.
pub fn replenish_food(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut rng: ResMut<SimRng>,
    food: Query<(Entity, &Reserve), With<Food>>,
) {
    if config.food_count == 0 {
        return;
    }
    let mut alive = 0usize;
    for (entity, reserve) in &food {
        if reserve.current <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            alive += 1;
        }
    }
    let span = config.arena_half_extent - config.food_radius - 5.0;
    for _ in alive..config.food_count {
        let x = rng.0.next_signed() * span;
        let y = rng.0.next_signed() * span;
        spawn_food(&mut commands, &config, Vec2::new(x, y));
    }
}

/// Poser une source de nourriture : une réserve d'énergie statique et *sensor*
/// (les agents la traversent sans la heurter), mangée via la primitive
/// d'interaction comme n'importe quelle cible.
fn spawn_food(commands: &mut Commands, config: &SimConfig, pos: Vec2) {
    commands.spawn((
        Food,
        Species(config.food_species),
        Reserve::full(config.food_energy),
        Radius(config.food_radius),
        RigidBody::Static,
        Collider::circle(config.food_radius),
        Sensor,
        Transform::from_translation(pos.extend(0.0)),
    ));
}
