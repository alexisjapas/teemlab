//! Flore évolutive (Phase 3) — driver : une population de plantes sessiles **s'auto-régule**.
//!
//! Falsification du cœur de l'item : une *flore* (cerveau [`Brain::Sessile`], énergie par
//! photosynthèse, semis local) est une entité de plein droit qui (a) **croît** fortement
//! depuis quelques fondateurs (photosynthèse + semis fonctionnent), (b) reste **bornée bien
//! en deçà de la saturation physique** de l'arène — la compétition intraspécifique (relation
//! Plante→Plante, primitive d'interaction §3 : lumière/espace disputés) freine la croissance
//! en une onde spatiale au lieu de remplir l'arène —, et (c) **persiste** à un effectif
//! soutenu, le tout robustement sur plusieurs graines.
//!
//! C'est une **rétroaction négative** (densité haute → drain de compétition → moins de
//! semis / mortalité), donc robuste — pas le couplage oscillant *knife-edge* de
//! proie-prédateur. On fait tourner le *vrai* monde de sim (même `SimPlugin` que les
//! binaires), en pas-à-pas.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};

mod common;

/// Le scénario versionné, chargé tel quel.
const SCENARIO: &str = include_str!("../scenarios/flore.ron");

/// Quatre mondes indépendants : une bande qui tient pour tous n'est pas un coup de chance.
const SEEDS: [u64; 4] = [0x00C0_FFEE, 0x1234, 0x9999, 0xBEEF];

const SECONDS: usize = 120;

/// Effectif de plantes échantillonné chaque seconde de sim, pour une graine.
fn population_trajectory(seed: u64) -> Vec<usize> {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("scénario flore valide");
    config.seed = seed;
    let tick_hz = config.tick_hz as usize;

    let mut app = common::stepping_app(&config);
    let mut traj = Vec::with_capacity(SECONDS);
    for _ in 0..SECONDS {
        for _ in 0..tick_hz {
            app.update();
        }
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Species, With<Agent>>();
        traj.push(q.iter(world).count());
    }
    traj
}

#[test]
fn flora_grows_and_self_regulates_across_seeds() {
    let founders = SimConfig::from_ron_str(SCENARIO).unwrap().archetypes[0].count;

    let mut failures = Vec::new();
    eprintln!("  seed         | fondateurs={founders} | pic | bande 2ᵉ moitié (min..max)");
    for seed in SEEDS {
        let traj = population_trajectory(seed);
        let peak = *traj.iter().max().unwrap();
        let back = &traj[SECONDS / 2..];
        let lo = *back.iter().min().unwrap();
        let hi = *back.iter().max().unwrap();
        let sampled: Vec<String> = traj.iter().step_by(20).map(|n| n.to_string()).collect();
        eprintln!(
            "  {seed:#012x} | pic {peak:>4} | {lo:>4}..{hi:<4}  t=0,20,..: {}",
            sampled.join("  ")
        );

        // (a) a CRÛ fortement (≫ fondateurs) → photosynthèse + semis fonctionnent.
        if peak < 200 {
            failures.push(format!(
                "seed {seed:#x} : croissance trop faible (pic {peak}, fondateurs {founders})"
            ));
        }
        // (b) bornée LOIN de la saturation physique de l'arène (~4500 corps pour
        //     rayon 6, demi-arène 360) → la compétition freine, l'arène ne se remplit pas.
        if peak > 2000 {
            failures.push(format!(
                "seed {seed:#x} : la compétition ne borne pas (pic {peak})"
            ));
        }
        // (c) PERSISTE à un effectif soutenu sur la 2ᵉ moitié (pas d'effondrement).
        if lo < 100 {
            failures.push(format!(
                "seed {seed:#x} : effectif non soutenu (creux {lo})"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "auto-régulation non robuste :\n  {}",
        failures.join("\n  ")
    );
}
