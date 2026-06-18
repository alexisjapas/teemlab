//! Item 16 — le chasseur voit et poursuit sa cible.
//!
//! Test de bout en bout du canal « cible » de la perception + du réflexe
//! [`Brain::Hunter`] : on pose un chasseur à l'origine, cap +X, et une nourriture
//! droit devant, dans sa portée de vision ; on fait tourner le *vrai* monde de sim
//! et on vérifie (1) que la cible s'inscrit dans son canal de perception, et (2)
//! qu'il s'en rapproche franchement — preuve que percevoir→décider→agir est devenu
//! PORTEUR (et que la sélection de cerveau par scénario fonctionne).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Perception, Species};
use teemlab::ecology::spawn_food;
use teemlab::genotype::Genotype;
use teemlab::spawn::spawn_agent;

mod common;

#[test]
fn hunter_sees_and_chases_its_target() {
    // Monde nu : pas de peuplement auto (on place tout à la main), pas de
    // métabolisme (le chasseur ne meurt pas pendant le test), une relation à débit
    // NUL — la nourriture reste un appât stable : visée (donc « cible »), jamais
    // consommée. C'est `brain: Hunter` qu'on met à l'épreuve.
    let config = SimConfig::from_ron_str(
        "(
            arena_half_extent: 400.0,
            agent_count: 0,
            food_count: 0,
            reserve_max: 100.0,
            brain: Hunter,
            vision_rays: 7,
            vision_fov_deg: 120.0,
            vision_range: 260.0,
            food_species: 1,
            relations: [(actor: 0, target: 1, transfer: true, rate: 0.0, range: 10.0)],
        )",
    )
    .expect("config valide");

    // Un tick fixe pile par `update()` (cf. `common::stepping_app`).
    let mut app = common::stepping_app(&config);

    // Chasseur à l'origine (cap +X), nourriture droit devant à 200 u (< portée).
    let food_x = 200.0_f32;
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            let genotype = Genotype::base(&config);
            spawn_agent(
                &mut commands,
                &config,
                genotype,
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max,
                0, // fondateur : génération 0.
            );
            spawn_food(&mut commands, &config, Vec2::new(food_x, 0.0));
        })
        .expect("spawn ponctuel");

    // Quelques ticks pour que la broad-phase d'Avian intègre la nourriture, puis on
    // vérifie qu'elle s'inscrit dans le canal « cible » du chasseur.
    for _ in 0..10 {
        app.update();
    }
    let world = app.world_mut();
    let mut perceptions = world.query_filtered::<&Perception, With<Agent>>();
    let saw_target = perceptions
        .iter(world)
        .any(|p| p.target.iter().any(|&v| v > 0.0));
    assert!(
        saw_target,
        "la nourriture droit devant doit apparaître dans le canal « cible »"
    );

    // On laisse courir : le chasseur doit se rapprocher franchement de sa cible.
    for _ in 0..80 {
        app.update();
    }
    let world = app.world_mut();
    let mut transforms = world.query_filtered::<&Transform, With<Agent>>();
    let x = transforms
        .iter(world)
        .next()
        .expect("le chasseur existe encore")
        .translation
        .x;
    assert!(
        x > 100.0,
        "le chasseur doit avoir foncé vers sa cible (x={x:.1}, départ 0, cible {food_x})"
    );
}
