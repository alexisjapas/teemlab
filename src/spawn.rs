//! Peuplement initial du monde : l'arène (murs statiques) et les agents
//! (corps dynamiques + cerveau). Tourne une fois, au `Startup`.

use crate::brain::Brain;
use crate::components::{Action, Agent, Perception, Radius, Reserve, Species, Wall};
use crate::config::SimConfig;
use crate::genotype::Genotype;
use crate::rng::Rng;
use avian2d::prelude::*;
use bevy::prelude::*;

pub fn setup_world(mut commands: Commands, config: Res<SimConfig>) {
    populate(&mut commands, &config);
}

/// Peuple le monde : arène (murs statiques) + population fondatrice. Partagé par
/// le `Startup` ([`setup_world`]) et la **réinitialisation à chaud** (item 11),
/// pour que reset et premier peuplement produisent rigoureusement le même monde.
pub fn populate(commands: &mut Commands, config: &SimConfig) {
    spawn_arena(commands, config);
    spawn_agents(commands, config);
}

/// Quatre **demi-espaces** (plans infinis) formant une boîte fermée autour de
/// l'arène. Un demi-espace a un côté solide *infini* : un agent ne peut donc ni
/// le tunneler en un tick, ni s'échapper s'il naît (reproduction) ou est déposé
/// (éditeur) au-delà du bord — le solveur le repousse toujours vers l'intérieur.
/// Un mur d'épaisseur finie, lui, laisse « dehors » une issue libre.
///
/// La normale passée à [`Collider::half_space`] pointe vers le côté **libre**
/// (à l'opposé du solide), comme la normale « vers le haut » d'un sol. On la
/// dirige donc vers l'intérieur de l'arène, et on pose chaque plan pile sur le
/// bord `±arena_half_extent` (aligné avec la boîte dessinée par `draw_arena`).
///
/// Public pour que la restauration d'un snapshot (item 13) reconstruise l'arène
/// avant d'y reposer les agents sauvegardés (le snapshot ne stocke pas les murs,
/// qui se déduisent du `SimConfig`).
pub fn spawn_arena(commands: &mut Commands, config: &SimConfig) {
    let h = config.arena_half_extent;
    let walls = [
        (Vec2::new(0.0, -h), Vec2::Y),     // bas    : solide en dessous
        (Vec2::new(0.0, h), Vec2::NEG_Y),  // haut   : solide au-dessus
        (Vec2::new(-h, 0.0), Vec2::X),     // gauche : solide à gauche
        (Vec2::new(h, 0.0), Vec2::NEG_X),  // droite : solide à droite
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
    // Le scénario choisit le *type* de cerveau ; on le compile ici en un cerveau
    // frais (§1, l'auteur de la décision). La graine ne sert qu'aux cerveaux à
    // état (errance) ; le chasseur, déterministe, l'ignore.
    let brain = config.brain.build(brain_seed, heading);
    spawn_agent_with_brain(commands, config, genotype, species, pos, brain, energy);
}

/// Variante prenant un [`Brain`] **déjà construit** plutôt qu'une graine : c'est
/// le chemin de la restauration d'un snapshot (item 13), qui réinjecte le cerveau
/// exact (état du RNG d'errance compris) lu dans le fichier. [`spawn_agent`] n'en
/// est que le cas « cerveau neuf depuis une graine ». Source unique du *bundle*
/// d'agent, pour qu'un agent restauré soit en tout point un agent comme un autre.
#[allow(clippy::too_many_arguments)]
pub fn spawn_agent_with_brain(
    commands: &mut Commands,
    config: &SimConfig,
    genotype: Genotype,
    species: Species,
    pos: Vec2,
    brain: Brain,
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
            target: vec![0.0; vision.ray_count].into_boxed_slice(),
            ray_dirs: vec![Vec2::ZERO; vision.ray_count].into_boxed_slice(),
            ..default()
        },
        Action::default(),
        brain,
        RigidBody::Dynamic,
        Collider::circle(r),
        LinearVelocity::default(),
        Transform::from_translation(pos.extend(0.0)),
    ));
}
