//! Snapshot de run : capturer l'état vivant, le sérialiser, puis le restaurer
//! dans un autre monde — et retrouver la même population (item 13).
//!
//! Le test rejoue ce que fait le binaire (`runs.rs`) en réutilisant les briques
//! du cœur (`Snapshot`, `spawn_agent_with_brain`, `spawn_arena`). Il couvre surtout le
//! chemin **restauration**, le plus risqué. Depuis la Phase 3b, les sources de
//! nourriture sont des agents sessiles : elles sont capturées/restaurées dans la même
//! liste, sans chemin `food` séparé.

// Les requêtes Bevy (tuples + filtres) déclenchent `type_complexity` par nature.
#![allow(clippy::type_complexity)]

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Age, Agent, Generation, Reserve, Species, Wall};
use teemlab::ecology::SimRng;
use teemlab::genotype::Genotype;
use teemlab::snapshot::{AgentSnap, Snapshot};
use teemlab::spawn::{spawn_agent_with_brain, spawn_arena};

mod common;

/// Réceptacle de la capture (résultat de `capture_system`).
#[derive(Resource, Default)]
struct Captured(Option<Snapshot>);

/// Capture l'état vivant en un [`Snapshot`] — réplique de `runs::save_snapshot`.
fn capture_system(
    mut captured: ResMut<Captured>,
    config: Res<SimConfig>,
    sim_rng: Res<SimRng>,
    agents: Query<
        (
            &Transform,
            &Genotype,
            &Reserve,
            &Species,
            &Brain,
            &Generation,
            &Age,
        ),
        With<Agent>,
    >,
) {
    captured.0 = Some(Snapshot {
        config: config.clone(),
        sim_rng: sim_rng.0.clone(),
        agents: agents
            .iter()
            .map(|(t, g, r, s, b, generation, age)| AgentSnap {
                pos: t.translation.truncate().to_array(),
                genotype: *g,
                reserve: r.current,
                species: s.0,
                generation: generation.0,
                age: age.0,
                brain: b.clone(),
            })
            .collect(),
    });
}

/// Le snapshot à restaurer (entrée de `restore_system`).
#[derive(Resource)]
struct ToRestore(Snapshot);

/// Restaure une run — réplique de `runs::apply_snapshot_load`.
fn restore_system(
    mut commands: Commands,
    to: Res<ToRestore>,
    existing: Query<Entity, Or<(With<Agent>, With<Wall>)>>,
) {
    let snap = &to.0;
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    spawn_arena(&mut commands, &snap.config);
    for a in &snap.agents {
        spawn_agent_with_brain(
            &mut commands,
            &snap.config,
            a.genotype,
            Species(a.species),
            Vec2::from(a.pos),
            a.brain.clone(),
            a.reserve,
            a.generation,
            a.age,
        );
    }
}

#[test]
fn run_survives_snapshot_roundtrip() {
    let config =
        SimConfig::from_ron_file("scenarios/evolution.ron").expect("scénario evolution.ron");

    // — Source : on laisse vivre la run, puis on capture. —
    let mut source = common::stepping_app(&config);
    for _ in 0..400 {
        source.update();
    }
    source.world_mut().insert_resource(Captured::default());
    source
        .world_mut()
        .run_system_once(capture_system)
        .expect("capture");
    let snapshot = source
        .world()
        .resource::<Captured>()
        .0
        .clone()
        .expect("snapshot capturé");
    assert!(!snapshot.agents.is_empty(), "rien à capturer — run éteinte");

    // — Passage par le disque (RON) : ce que le binaire écrit/relit. —
    let text = snapshot.to_ron_string().expect("sérialisation");
    let snapshot = Snapshot::from_ron_str(&text).expect("relecture");
    // Les agents incluent les sources sessiles (Phase 3b) : un seul compte à vérifier.
    let expected_agents = snapshot.agents.len();

    // — Cible : un monde neuf, peuplé par le Startup, qu'on écrase par la run
    //   restaurée. —
    let mut target = common::stepping_app(&config);
    target.update(); // Startup peuple la population par défaut…
    target.world_mut().insert_resource(ToRestore(snapshot));
    target
        .world_mut()
        .run_system_once(restore_system)
        .expect("restauration");

    // La population restaurée (sources sessiles comprises) remplace exactement celle du
    // snapshot.
    let agents = target
        .world_mut()
        .query_filtered::<(), With<Agent>>()
        .iter(target.world())
        .count();
    assert_eq!(agents, expected_agents, "population restaurée incorrecte");

    // Et la run restaurée continue de tourner sainement (pas de panique, pop > 0).
    for _ in 0..100 {
        target.update();
    }
    let still_alive = target
        .world_mut()
        .query_filtered::<(), With<Agent>>()
        .iter(target.world())
        .count();
    assert!(still_alive > 0, "la run restaurée s'est éteinte aussitôt");
}
