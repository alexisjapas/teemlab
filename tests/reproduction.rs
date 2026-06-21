//! Driver de la **conservation d'énergie à la reproduction** (review #1).
//!
//! `ecology::reproduce` déduit `offspring_energy` du parent et la donne à l'enfant :
//! l'énergie doit être *conservée*, jamais créée. Or seuil et coût de reproduction
//! sont deux gènes qui dérivent indépendamment — rien ne garantit `seuil >= coût`.
//! Le garde `reserve >= offspring_energy` rend la conservation **inconditionnelle** ;
//! ces tests font tourner le *vrai* `SimPlugin` (le même que les binaires) et
//! vérifient qu'aucune énergie n'apparaît, dans le régime normal comme dans le cas
//! pathologique (coût > réserve) que le garde doit neutraliser.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Reserve};
use teemlab::{SimConfig, SimPlugin};

/// Un monde de reproduction *pur* : un seul agent, pas de métabolisme ni de
/// nourriture ni d'interaction → la seule chose qui peut bouger l'énergie est la
/// reproduction. On isole ainsi l'invariant testé. La capacité, le seuil et le coût
/// de reproduction sont paramétrés (gènes de l'unique archétype).
fn repro_world(reserve_max: f32, threshold: f32, offspring: f32) -> SimConfig {
    use teemlab::brain::BrainKind;
    use teemlab::config::{Archetype, Mutability};
    use teemlab::genotype::Genotype;
    let genotype = Genotype {
        reproduction_threshold: threshold,
        offspring_energy: offspring,
        mutation_rate: 0.0, // gènes stables : on raisonne sur des valeurs exactes.
        base_metabolism: 0.0,
        move_cost: 0.0,
        ..Genotype::default()
    };
    SimConfig {
        archetypes: vec![Archetype {
            name: "Agent".into(),
            color: Archetype::default_color(0),
            count: 1,
            radius: 8.0,
            reserve_max,
            genotype,
            brain: BrainKind::default(),
            mutable: Mutability::default(),
            source: None,
            captured_brain: None,
            captured_from: None,
        }],
        relations: Vec::new(),
        seed: 0x5EED,
        ..SimConfig::default()
    }
}

/// App en pas-à-pas manuel (cf. les autres drivers) : un `update()` = un tick fixe.
fn stepping_app(config: SimConfig) -> App {
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config));
    // Avian insère certaines ressources dans finish()/cleanup() : à déclencher
    // soi-même quand on pompe la boucle à la main.
    app.finish();
    app.cleanup();
    app
}

/// Population vivante + énergie totale en réserve, à un instant donné.
fn population_and_energy(app: &mut App) -> (usize, f32) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Reserve, With<Agent>>();
    let mut n = 0;
    let mut total = 0.0;
    for r in q.iter(world) {
        n += 1;
        total += r.current;
    }
    (n, total)
}

/// Régime normal (`seuil >= coût`, comme les scénarios livrés) : le fondateur, né à
/// pleine réserve, se reproduit **une fois** ; l'énergie passe du parent à l'enfant
/// sans rien créer, et la population se stabilise à deux.
#[test]
fn reproduction_conserves_energy_in_the_normal_regime() {
    // seuil atteignable (= réserve de départ), coût < seuil : régime sain.
    let config = repro_world(120.0, 95.0, 45.0);
    let initial = 120.0; // l'unique fondateur naît plein.
    let mut app = stepping_app(config);

    // Une seconde de sim : largement assez pour la (seule) reproduction.
    for _ in 0..64 {
        app.update();
        let (_, energy) = population_and_energy(&mut app);
        assert!(
            energy <= initial + 1e-3,
            "l'énergie totale ne doit jamais dépasser l'apport initial ({initial}), vue : {energy}"
        );
    }

    let (population, energy) = population_and_energy(&mut app);
    assert_eq!(population, 2, "le fondateur s'est reproduit une fois");
    assert!(
        (energy - initial).abs() < 1e-3,
        "énergie conservée : {energy} ≈ {initial}"
    );
}

/// Cas pathologique que le garde neutralise : `offspring_energy > réserve`. Sans le
/// garde, le parent paierait plus qu'il n'a (réserve négative → mort), mais l'enfant
/// emporterait la pleine `offspring_energy` → énergie créée. Avec le garde, la
/// reproduction est simplement refusée : population et énergie restent figées.
#[test]
fn reproduction_is_refused_when_offspring_costs_more_than_reserve() {
    // seuil franchi (réserve de départ = 50), coût > réserve : impossible à payer.
    let config = repro_world(50.0, 40.0, 80.0);
    let initial = 50.0;
    let mut app = stepping_app(config);

    for _ in 0..64 {
        app.update();
        let (population, energy) = population_and_energy(&mut app);
        // Jamais d'enfant (coût impayable), donc jamais d'énergie créée.
        assert_eq!(
            population, 1,
            "aucune reproduction : le coût dépasse la réserve"
        );
        assert!(
            (energy - initial).abs() < 1e-3,
            "énergie figée à {initial} (rien créé, rien mis en négatif) : {energy}"
        );
    }
}
