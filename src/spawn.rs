//! Peuplement initial du monde : l'arène (murs statiques) et les agents
//! (corps dynamiques + cerveau). Tourne une fois, au `Startup`.

use crate::brain::{Brain, MlpBrain};
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
        (Vec2::new(0.0, -h), Vec2::Y),    // bas    : solide en dessous
        (Vec2::new(0.0, h), Vec2::NEG_Y), // haut   : solide au-dessus
        (Vec2::new(-h, 0.0), Vec2::X),    // gauche : solide à gauche
        (Vec2::new(h, 0.0), Vec2::NEG_X), // droite : solide à droite
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
///
/// Deux modes pour l'**effectif par espèce** : explicite si le scénario donne
/// `agents_per_species` (`[s]` agents de l'espèce `s` — le levier d'une pyramide
/// trophique), sinon le partage *uniforme* historique (`agent_count` en
/// round-robin sur `species_count`). Dans le cas uniforme, la suite (espèce,
/// tirages RNG) est rigoureusement celle d'avant l'ajout → un scénario existant
/// spawn à l'identique.
fn spawn_agents(commands: &mut Commands, config: &SimConfig) {
    let mut rng = Rng::new(config.seed);
    let r = config.agent_radius;
    let span = config.arena_half_extent - r - 5.0;
    let genotype = Genotype::base(config);

    // La suite des espèces à peupler, dans l'ordre de spawn.
    let species_seq: Vec<u16> = if config.agents_per_species.is_empty() {
        // Uniforme : round-robin (au moins une espèce, même si un scénario met 0).
        let species_count = config.species_count.max(1);
        (0..config.agent_count)
            .map(|i| (i as u16) % species_count)
            .collect()
    } else {
        // Explicite : `n` agents de l'espèce `s`, espèces dans l'ordre.
        config
            .agents_per_species
            .iter()
            .enumerate()
            .flat_map(|(s, &n)| std::iter::repeat_n(s as u16, n))
            .collect()
    };

    for (i, species) in species_seq.into_iter().enumerate() {
        let pos = Vec2::new(rng.next_signed() * span, rng.next_signed() * span);
        let heading = rng.next_f32() * std::f32::consts::TAU;
        let brain_seed = config.seed ^ (i as u64).wrapping_mul(0x9E37_79B1);
        spawn_agent(
            commands,
            config,
            genotype,
            Species(species),
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
    // Le scénario choisit le *type* de cerveau **par espèce** (item 18a) ; on le
    // compile ici en un cerveau frais (§1, l'auteur de la décision). La graine sert
    // les cerveaux à état (errance, poids initiaux du MLP) ; `n_inputs` dimensionne la
    // couche d'entrée du MLP (= les canaux de perception, item 18b).
    let n_inputs = MlpBrain::input_size(config.vision_rays);
    let brain = config
        .brain_of(species.0)
        .build(brain_seed, heading, n_inputs);
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
