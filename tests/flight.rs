//! Fuite active — la proie voit et **fuit** son prédateur.
//!
//! Le miroir exact de `tests/hunter.rs` (item 16), côté répulsion : on teste de bout
//! en bout le canal « menace » de la perception + la fuite de [`Brain::Hunter`]. On
//! pose une proie à l'origine, cap +X, et un prédateur (une espèce qui peut agir
//! *sur* elle, via la table de relations) droit devant, dans sa portée de vision ;
//! on fait tourner le *vrai* monde de sim et on vérifie (1) que le prédateur
//! s'inscrit dans le canal « menace » de la proie, et (2) qu'elle s'en **éloigne**
//! franchement — la preuve que le même cerveau `Hunter`, lu par l'espèce-proie via la
//! relation *inverse*, produit une FUITE (le pendant de l'attraction « cible »).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::brain::BrainKind;
use teemlab::components::{Agent, Perception, Species};
use teemlab::config::{Archetype, Mutability, Relation};
use teemlab::genotype::Genotype;
use teemlab::spawn::spawn_agent;

mod common;

#[test]
fn prey_sees_and_flees_its_predator() {
    // Monde nu : pas de peuplement auto (on place tout à la main), pas de métabolisme
    // (la proie ne meurt pas pendant le test), une relation prédateur→proie à débit
    // NUL et portée courte — le prédateur est un **épouvantail stable** : perçu comme
    // menace, mais il ne mange jamais. Il est de plus IMMOBILE (max_speed 0) : un
    // point de menace fixe, comme la nourriture est un appât fixe dans `tests/hunter.rs`.
    let config = SimConfig {
        arena_half_extent: 400.0,
        archetypes: vec![
            // Espèce 0 : la proie (chasseur). Ici elle n'a aucune cible — seule la
            // menace la pilote → fuite pure. Vision portée à 260 pour voir l'épouvantail.
            Archetype {
                name: "Proie".into(),
                color: Archetype::default_color(0),
                count: 0,
                radius: 8.0,
                reserve_max: 100.0,
                genotype: Genotype {
                    vision_fov_deg: 120.0,
                    vision_range: 260.0,
                    ..Genotype::default()
                },
                brain: BrainKind::Hunter,
                mutable: Mutability::default(),
                source: None,
            },
            // Espèce 1 : le prédateur, immobile (max_speed 0) — l'épouvantail.
            Archetype {
                name: "Prédateur".into(),
                color: Archetype::default_color(1),
                count: 0,
                radius: 8.0,
                reserve_max: 100.0,
                genotype: Genotype {
                    max_speed: 0.0,
                    ..Genotype::default()
                },
                brain: BrainKind::Hunter,
                mutable: Mutability::default(),
                source: None,
            },
        ],
        // Le prédateur (espèce 1) peut agir SUR la proie (espèce 0) : la proie le perçoit
        // donc comme une MENACE (relation *inverse* du canal « cible »). Débit nul → il
        // ne fait que menacer.
        relations: vec![Relation {
            actor: 1,
            target: 0,
            transfer: true,
            rate: 0.0,
            range: 10.0,
        }],
        ..SimConfig::default()
    };

    // Un tick fixe pile par `update()` (cf. `common::stepping_app`).
    let mut app = common::stepping_app(&config);

    // Proie à l'origine (cap +X), prédateur droit devant à 120 u : dans la portée
    // (260) et assez PROCHE pour franchir le seuil de fuite (proximité ≈ 0.54 > 0.35).
    let predator_x = 120.0_f32;
    app.world_mut()
        .run_system_once(move |mut commands: Commands, config: Res<SimConfig>| {
            // Proie (espèce 0).
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(0),
                Species(0),
                Vec2::ZERO,
                0.0,
                0,
                config.reserve_max_of(0),
                0, // fondateur : génération 0.
            );
            // Prédateur immobile (espèce 1), droit devant.
            spawn_agent(
                &mut commands,
                &config,
                config.genotype_of(1),
                Species(1),
                Vec2::new(predator_x, 0.0),
                0.0,
                1,
                config.reserve_max_of(1),
                0,
            );
        })
        .expect("spawn ponctuel");

    // La broad-phase d'Avian a besoin de quelques ticks pour intégrer le prédateur ;
    // pendant ce temps la proie commence déjà à se détourner. On échantillonne donc le
    // canal « menace » sur les premiers ticks : il doit s'allumer AU MOINS une fois (la
    // fenêtre où le prédateur est intégré ET encore dans le cône de vision avant).
    let mut ever_saw_threat = false;
    for _ in 0..12 {
        app.update();
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Species, &Perception), With<Agent>>();
        for (species, perception) in q.iter(world) {
            if species.0 == 0 && perception.threat.iter().any(|&v| v > 0.0) {
                ever_saw_threat = true;
            }
        }
    }
    assert!(
        ever_saw_threat,
        "le prédateur droit devant doit apparaître dans le canal « menace » de la proie"
    );

    // On laisse courir : la proie doit s'être ÉLOIGNÉE du prédateur — fuite vers les x
    // négatifs, à l'opposé de l'épouvantail posé en +X.
    for _ in 0..80 {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Transform), With<Agent>>();
    let prey_x = q
        .iter(world)
        .find(|(s, _)| s.0 == 0)
        .map(|(_, t)| t.translation.x)
        .expect("la proie existe encore");
    assert!(
        prey_x < -50.0,
        "la proie doit avoir fui sa menace (x={prey_x:.1}, départ 0, prédateur en {predator_x})"
    );
}
