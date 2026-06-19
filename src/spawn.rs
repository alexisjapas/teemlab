//! Peuplement initial du monde : l'arène (murs statiques) et les agents
//! (corps dynamiques + cerveau). Tourne une fois, au `Startup`.

use crate::brain::{Brain, MlpBrain};
use crate::components::{
    Action, Age, Agent, Generation, Perception, Radius, Reserve, Species, Wall,
};
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

/// Population fondatrice : pour chaque archétype d'**agent**, son effectif (`count`)
/// d'agents dispersés au hasard, chacun compilé depuis le génotype et le cerveau de
/// son archétype, graîné de façon déterministe. La nourriture est peuplée par
/// [`crate::ecology::replenish_food`]. L'ordre — espèces **contiguës** dans l'ordre
/// des archétypes — fixe le flux de tirages RNG.
fn spawn_agents(commands: &mut Commands, config: &SimConfig) {
    let mut rng = Rng::new(config.seed);
    // La suite des espèces à peupler : `count` agents par archétype d'agent, dans
    // l'ordre des archétypes.
    let species_seq: Vec<u16> = config
        .archetypes
        .iter()
        .enumerate()
        .filter(|(_, a)| a.is_agent())
        .flat_map(|(i, a)| std::iter::repeat_n(i as u16, a.count))
        .collect();

    for (i, species) in species_seq.into_iter().enumerate() {
        let span = config.arena_half_extent - config.agent_radius_of(species) - 5.0;
        let pos = Vec2::new(rng.next_signed() * span, rng.next_signed() * span);
        let heading = rng.next_f32() * std::f32::consts::TAU;
        let brain_seed = config.seed ^ (i as u64).wrapping_mul(0x9E37_79B1);
        spawn_agent(
            commands,
            config,
            config.genotype_of(species),
            Species(species),
            pos,
            heading,
            brain_seed,
            config.reserve_max_of(species),
            0, // fondateur : génération 0.
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
    generation: u32,
) {
    // Le scénario choisit le *type* de cerveau **par espèce** (item 18a) ; on le
    // compile ici en un cerveau frais (§1, l'auteur de la décision). La graine sert
    // les cerveaux à état (errance, poids initiaux du MLP) ; `n_inputs` dimensionne la
    // couche d'entrée du MLP (= les canaux de perception), tirée du **gène** de
    // précision visuelle de cet agent (item 3) et non plus d'un réglage de scénario.
    let n_inputs = MlpBrain::input_size(genotype.ray_count());
    let brain = config
        .brain_of(species.0)
        .build(brain_seed, heading, n_inputs);
    // Un agent fraîchement compilé naît à l'âge 0.
    spawn_agent_with_brain(
        commands, config, genotype, species, pos, brain, energy, generation, 0.0,
    );
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
    generation: u32,
    age: f32,
) {
    let r = config.agent_radius_of(species.0);
    // La forme (nombre de rayons) vient désormais du gène de précision visuelle.
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
        // Généalogie : profondeur (fixe) et âge (croît au tick). Groupés en
        // sous-tuple pour rester sous la borne d'arité des bundles Bevy.
        (Generation(generation), Age(age)),
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
