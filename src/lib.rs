//! teemlab — moteur de simulation évolutive.
//!
//! Un *seul* moteur interprète de la donnée ; chaque simulation n'est qu'un
//! scénario. La boucle est toujours **percevoir → décider → agir**.
//!
//! Ce crate expose le cœur *render-agnostic* ([`SimPlugin`]) partagé par les
//! deux points d'entrée (fenêtré et headless), pour qu'ils fassent avancer
//! exactement le même monde.

// Les requêtes Bevy (tuples de composants + filtres) déclenchent `type_complexity`
// par nature ; c'est la forme idiomatique d'un système ECS, pas une dette. On
// l'autorise au niveau du crate plutôt que de saupoudrer des `#[allow]` ou
// d'inventer des alias qui masqueraient ce qu'un système lit vraiment.
#![allow(clippy::type_complexity)]

pub mod brain;
pub mod components;
pub mod config;
pub mod ecology;
pub mod genotype;
pub mod interaction;
pub mod movement;
pub mod rng;
pub mod snapshot;
pub mod spawn;
pub mod visuals;

use avian2d::prelude::*;
use bevy::prelude::*;

pub use config::SimConfig;

/// Le cœur de la simulation : tout ce qui fait avancer le monde.
///
/// **Règle absolue : aucune logique de sim dans `Update`.** L'agentivité vit
/// dans [`FixedUpdate`] et la physique Avian dans [`FixedPostUpdate`]. `Update`
/// est réservé au rendu / UI du binaire fenêtré. Ainsi le build headless et le
/// build fenêtré font tourner le *même* monde, à l'identique.
#[derive(Default)]
pub struct SimPlugin {
    pub config: SimConfig,
}

impl SimPlugin {
    pub fn new(config: SimConfig) -> Self {
        Self { config }
    }
}

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            // Physique placée explicitement dans FixedPostUpdate.
            .add_plugins(PhysicsPlugins::new(FixedPostUpdate))
            // Vue top-down : pas de gravité (inséré après le plugin pour
            // l'emporter sur son défaut).
            .insert_resource(Gravity(Vec2::ZERO))
            // Cadence de sim constante (64 Hz par défaut), indépendante du rendu.
            .insert_resource(Time::<Fixed>::from_hz(self.config.tick_hz))
            // Flux aléatoire de la sim (réapparition de nourriture, …), seedé à
            // part du peuplement pour ne pas corréler les deux.
            .insert_resource(ecology::SimRng::from_config(&self.config))
            .init_resource::<ecology::FoodRegen>()
            .add_systems(Startup, spawn::setup_world)
            // percevoir → décider → agir, strictement dans FixedUpdate.
            // `interact` prolonge l'« agir » (manger/attaquer) ; puis l'économie
            // d'énergie : métaboliser, mourir, réensemencer la nourriture.
            .add_systems(
                FixedUpdate,
                (
                    movement::perceive,
                    movement::decide,
                    movement::act,
                    interaction::interact,
                    ecology::metabolize,
                    ecology::reap,
                    ecology::age_agents,
                    ecology::reproduce,
                    ecology::replenish_food,
                )
                    .chain(),
            );
    }
}
