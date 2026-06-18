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

use crate::brain::Brain;
use crate::components::{Agent, Food, Radius, Reserve, Species, Vision};
use crate::config::SimConfig;
use crate::genotype::Genotype;
use crate::rng::Rng;
use crate::spawn::spawn_agent_with_brain;
use avian2d::prelude::*;
use bevy::prelude::*;

/// Flux aléatoire de la simulation pour les événements stochastiques (ici, les
/// positions de réapparition de la nourriture). Vit dans le monde de sim, seedé
/// depuis la config — on rejoue une *expérience*, pas le bit-à-bit (§5).
#[derive(Resource)]
pub struct SimRng(pub Rng);

impl SimRng {
    /// Flux de sim seedé depuis la config, décalé du peuplement (`^ 0xF00D`) pour
    /// ne pas corréler les deux flux. Source unique : utilisée à l'insertion de la
    /// ressource (au build) **et** à la réinitialisation à chaud (item 11).
    pub fn from_config(config: &SimConfig) -> Self {
        Self(Rng::new(config.seed ^ 0xF00D))
    }
}

/// Reliquat fractionnaire de repousse de nourriture, accumulé entre les ticks
/// pour qu'un débit `food_regen` non entier par tick produise quand même le bon
/// nombre de sources au fil du temps.
#[derive(Resource, Default)]
pub struct FoodRegen(pub f32);

/// MÉTABOLISME : drainer l'énergie de chaque agent — base + surcoût de vitesse +
/// coût du capteur de vision. Plancher à zéro ; la mort est laissée à [`reap`].
pub fn metabolize(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut agents: Query<(&mut Reserve, &Genotype, &Vision, &LinearVelocity), With<Agent>>,
) {
    if config.base_metabolism == 0.0 && config.move_cost == 0.0 {
        // Monde inerte : pas d'économie métabolique dans ce *scénario* (fondateurs
        // à zéro) → on évite même le coût de vision (scénarios pré-item-8).
        return;
    }
    let dt = time.delta_secs();
    // Vitesse de *référence* (génotype fondateur) : on rapporte le coût de
    // locomotion à la vitesse absolue, pas à la fraction de la vitesse propre de
    // l'agent — sinon un mutant deux fois plus rapide paierait pareil et le gène
    // de vitesse n'aurait aucun coût. Ainsi « vitesse → énergie » (§2) tient.
    let reference_speed = config.max_speed.max(1e-3);
    for (mut reserve, genotype, vision, velocity) in &mut agents {
        // Métabolisme et coût de locomotion sont des gènes (per-espèce) ; le coût
        // de vision vient du phénotype, déjà per-entité.
        let speed_ratio = velocity.0.length() / reference_speed;
        let drain =
            genotype.base_metabolism + genotype.move_cost * speed_ratio + vision.metabolic_cost();
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

/// REPRODUCTION (régime continu-implicite, §4) : la fitness est endogène — *tu
/// t'es reproduit*. Un agent dont l'énergie atteint son seuil paie son
/// `offspring_energy` (conservation : rien n'est créé) pour engendrer un enfant
/// au génotype muté, posé près de lui. C'est ce qui ferme la **boucle évolutive
/// continue** : la sélection agit sur les gènes via le simple fait de survivre
/// assez pour se reproduire.
///
/// Seuil, coût et taux de mutation sont des **gènes de l'entité** (§1, *le
/// corps*) : la stratégie de reproduction évolue elle-même et peut différer d'une
/// espèce à l'autre.
///
/// Le cerveau de l'enfant **hérite de celui du parent**
/// ([`Brain::reproduce`], item 18a) plutôt que d'être reconstruit depuis le
/// `config` : c'est ce qui fait cohabiter durablement un témoin déterministe et un
/// cerveau appris (§4), et la couture que 18b étendra pour muter les poids.
pub fn reproduce(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut rng: ResMut<SimRng>,
    mut parents: Query<(&Transform, &mut Reserve, &Genotype, &Species, &Brain), With<Agent>>,
) {
    for (transform, mut reserve, genotype, species, brain) in &mut parents {
        // Seuil et coût sont des **gènes** (per-entité, évolvables) : un seuil nul
        // = cet agent ne se reproduit pas.
        //
        // On exige aussi `current >= offspring_energy` : seuil et coût étant deux
        // gènes qui dérivent indépendamment, rien ne garantit `seuil >= coût`. Sans
        // ce garde, un parent dont le coût dépasse la réserve passerait en négatif
        // (puis mourrait), MAIS l'enfant emporterait quand même la pleine
        // `offspring_energy` → de l'énergie créée ex nihilo, et une lignée
        // « seuil bas / enfant cher » serait *avantagée* (runaway). Le garde rend la
        // conservation **inconditionnelle** : on ne paie jamais plus qu'on n'a.
        if genotype.reproduction_threshold <= 0.0
            || reserve.current < genotype.reproduction_threshold
            || reserve.current < genotype.offspring_energy
        {
            continue;
        }
        reserve.current -= genotype.offspring_energy;
        let child = genotype.mutate(&mut rng.0, &config);
        // L'enfant naît légèrement décalé pour ne pas chevaucher exactement.
        let offset = Vec2::new(rng.0.next_signed(), rng.0.next_signed()).normalize_or_zero()
            * config.agent_radius
            * 2.5;
        let pos = transform.translation.truncate() + offset;
        // Mêmes tirages (heading puis seed) qu'avant l'héritage : le cerveau enfant
        // les consomme via `reproduce` au lieu de `config.brain.build` → flux RNG
        // inchangé pour les scénarios non-MLP. Le MLP, lui, tire en plus dans `rng.0`
        // pour muter ses poids (neuroévolution), pilotée par `mutation_rate` (item 18b).
        let heading = rng.0.next_f32() * std::f32::consts::TAU;
        let brain_seed = rng.0.next_u64();
        let child_brain = brain.reproduce(brain_seed, heading, &mut rng.0, genotype.mutation_rate);
        spawn_agent_with_brain(
            &mut commands,
            &config,
            child,
            *species,
            pos,
            child_brain,
            genotype.offspring_energy,
        );
    }
}

/// Entretenir la nourriture : retirer les sources épuisées et réensemencer pour
/// maintenir `food_count` constant. C'est le robinet d'énergie qui entre dans
/// l'écosystème ; son débit (vs le métabolisme cumulé) fixe le point d'équilibre.
pub fn replenish_food(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<SimConfig>,
    mut rng: ResMut<SimRng>,
    mut regen: ResMut<FoodRegen>,
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
    let deficit = config.food_count.saturating_sub(alive);
    if deficit == 0 {
        regen.0 = 0.0; // à pleine capacité : pas de repousse en réserve.
        return;
    }
    let to_spawn = if config.food_regen <= 0.0 {
        deficit // maintien instantané (item 8).
    } else {
        // Repousse à débit limité : on accumule le reliquat fractionnaire.
        regen.0 += config.food_regen * time.delta_secs();
        let n = (regen.0 as usize).min(deficit);
        regen.0 -= n as f32;
        n
    };
    let span = config.arena_half_extent - config.food_radius - 5.0;
    for _ in 0..to_spawn {
        let x = rng.0.next_signed() * span;
        let y = rng.0.next_signed() * span;
        spawn_food(&mut commands, &config, Vec2::new(x, y));
    }
}

/// Poser une source de nourriture : une réserve d'énergie statique et *sensor*
/// (les agents la traversent sans la heurter), mangée via la primitive
/// d'interaction comme n'importe quelle cible. Public pour que le placement
/// manuel de l'éditeur (item 4) puisse en déposer.
pub fn spawn_food(commands: &mut Commands, config: &SimConfig, pos: Vec2) {
    spawn_food_with_energy(commands, config, pos, config.food_energy);
}

/// Variante posant une source avec une réserve **partielle** donnée (au lieu de
/// pleine) : chemin de la restauration d'un snapshot (item 13), qui réinjecte une
/// nourriture à demi mangée à l'identique. [`spawn_food`] en est le cas « pleine ».
pub fn spawn_food_with_energy(
    commands: &mut Commands,
    config: &SimConfig,
    pos: Vec2,
    current: f32,
) {
    commands.spawn((
        Food,
        Species(config.food_species),
        Reserve {
            current,
            max: config.food_energy,
        },
        Radius(config.food_radius),
        RigidBody::Static,
        Collider::circle(config.food_radius),
        Sensor,
        Transform::from_translation(pos.extend(0.0)),
    ));
}
