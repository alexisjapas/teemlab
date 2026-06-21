//! Driver : un archétype portant un **cerveau capturé** fait naître ses fondateurs
//! avec CES poids exacts (réutilisation de poids entraînés), via le *vrai* SimPlugin
//! — le même monde que les binaires. Le pendant ECS, bout-en-bout, des tests
//! unitaires de `Archetype::capture` / aller-retour RON (cf. `config.rs`).

mod common;

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::{Brain, BrainKind, MlpBrain};
use teemlab::components::Agent;
use teemlab::config::{Archetype, Mutability};
use teemlab::genotype::Genotype;

/// Un monde d'un seul archétype MLP, `count` fondateurs, gènes stables (ni mutation,
/// ni métabolisme, ni reproduction → la population et les cerveaux restent figés au
/// premier tick). `captured` injecte (ou non) un cerveau concret capturé.
fn world(captured: Option<Brain>, count: usize) -> SimConfig {
    let genotype = Genotype {
        mutation_rate: 0.0,
        base_metabolism: 0.0,
        move_cost: 0.0,
        ..Genotype::default()
    };
    let arch = Archetype {
        count,
        genotype,
        brain: BrainKind::Mlp { hidden: vec![6] }, // topologie cohérente avec le cerveau capturé
        mutable: Mutability::default(),
        captured_brain: captured,
        ..Archetype::new_agent(0)
    };
    SimConfig {
        archetypes: vec![arch],
        relations: Vec::new(),
        seed: 0x5EED,
        ..SimConfig::default()
    }
}

/// Un cerveau MLP dont la couche d'entrée colle à la précision visuelle par défaut
/// (ce que reçoit un fondateur au génotype par défaut).
fn mlp_brain(seed: u64) -> Brain {
    let rays = Genotype::default().ray_count();
    Brain::Mlp(MlpBrain::random(seed, MlpBrain::input_size(rays), &[6]))
}

/// Tous les cerveaux d'agents vivants, après peuplement.
fn agent_brains(app: &mut App) -> Vec<Brain> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Brain, With<Agent>>();
    q.iter(world).cloned().collect()
}

/// Avec un cerveau capturé, **chaque fondateur** naît avec ces poids exacts (clone),
/// au lieu d'un cerveau frais aléatoire — c'est la couture qui réutilise des poids
/// entraînés.
#[test]
fn founders_are_born_with_the_captured_weights() {
    let captured = mlp_brain(99);
    let config = world(Some(captured.clone()), 3);
    let mut app = common::stepping_app(&config);
    app.update(); // le Startup peuple les fondateurs.

    let brains = agent_brains(&mut app);
    assert_eq!(brains.len(), 3, "trois fondateurs peuplés");
    for b in &brains {
        assert_eq!(*b, captured, "un fondateur naît avec les poids capturés");
    }
}

/// Contre-épreuve : **sans** capture (même topologie), chaque fondateur reçoit un
/// cerveau frais graîné distinctement → leurs poids diffèrent. C'est ce qui rend
/// l'égalité du test précédent significative (ce sont bien les poids capturés, pas un
/// artefact de construction).
#[test]
fn without_capture_founders_get_distinct_fresh_brains() {
    let config = world(None, 2);
    let mut app = common::stepping_app(&config);
    app.update();

    let brains = agent_brains(&mut app);
    assert_eq!(brains.len(), 2, "deux fondateurs peuplés");
    assert_ne!(
        brains[0], brains[1],
        "sans capture, deux fondateurs ont des poids frais distincts"
    );
}
