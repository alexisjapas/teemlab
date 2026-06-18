//! Item 18b (cœur) — le **driver** du cerveau MLP évolué.
//!
//! La falsification du §4 par l'expérience la plus robuste : la **cohabitation**
//! (cf. `tests/cohabitation.rs`). Deux espèces au même corps et à la même économie,
//! nourriture commune et limitée, ne diffèrent que par le cerveau — espèce 0 = MLP
//! (appris, parti de poids aléatoires), espèce 1 = errance (témoin naïf). À ressource
//! rare, le meilleur fourrageur exclut l'autre : si la neuroévolution apprend quoi que
//! ce soit d'utile, le MLP prend l'avantage sur l'errance. On le vérifie sur plusieurs
//! graines (une seule serait anecdotique).
//!
//! On fait tourner le *vrai* monde de sim (le même `SimPlugin` que les binaires), en
//! pas-à-pas manuel (cf. §6).

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::genotype::Genotype;

mod common;

const SCENARIO: &str = include_str!("../scenarios/cerveau_mlp.ron");
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];
const SECONDS: usize = 200;

/// Espèce 0 = MLP (appris), espèce 1 = errance (témoin naïf).
const MLP: u16 = 0;
const WANDER: u16 = 1;

/// Trajectoire d'une run : effectifs (MLP, errance) par seconde + vision moyenne
/// finale par espèce (la vision se maintient si le cerveau s'en sert).
struct Run {
    traj: Vec<(usize, usize)>,
    mlp_vision: f32,
    wander_vision: f32,
}

fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("scénario MLP valide");
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
        let (mut mlp, mut wander) = (0, 0);
        for s in q.iter(world) {
            match s.0 {
                MLP => mlp += 1,
                WANDER => wander += 1,
                _ => {}
            }
        }
        traj.push((mlp, wander));
    }

    let world = app.world_mut();
    let mut q = world.query::<(&Species, &Genotype)>();
    let (mut ms, mut mn, mut ws, mut wn) = (0.0f32, 0usize, 0.0f32, 0usize);
    for (s, g) in q.iter(world) {
        match s.0 {
            MLP => {
                ms += g.vision_range;
                mn += 1;
            }
            WANDER => {
                ws += g.vision_range;
                wn += 1;
            }
            _ => {}
        }
    }
    let mean = |sum: f32, n: usize| if n == 0 { f32::NAN } else { sum / n as f32 };
    Run {
        traj,
        mlp_vision: mean(ms, mn),
        wander_vision: mean(ws, wn),
    }
}

#[test]
fn mlp_outforages_wanderer_across_seeds() {
    let mut failures = Vec::new();
    eprintln!("  seed         | MLP(moy 2e moitié) | errance(moy 2e moitié) | vision MLP/errance");
    for seed in SEEDS {
        let run = run_seed(seed);
        // 2ᵉ moitié : on laisse passer le transitoire (croissance, premières
        // générations le temps que la neuroévolution démarre).
        let back = &run.traj[SECONDS / 2..];
        let mean = |f: &dyn Fn(&(usize, usize)) -> usize| -> f32 {
            back.iter().map(f).sum::<usize>() as f32 / back.len() as f32
        };
        let mlp_mean = mean(&|&(m, _)| m);
        let wander_mean = mean(&|&(_, w)| w);
        let sampled: Vec<String> = run
            .traj
            .iter()
            .step_by(25)
            .map(|&(m, w)| format!("{m}/{w}"))
            .collect();
        eprintln!(
            "  {seed:#012x} | {mlp_mean:>6.1}             | {wander_mean:>6.1}                 | {:.0} / {:.0}",
            run.mlp_vision, run.wander_vision
        );
        eprintln!("               t=0,25,..: {}", sampled.join("  "));

        // Le MLP appris doit DOMINER l'errance (fourrager bien mieux) : à départ égal
        // et corps identique, c'est la preuve qu'il a appris (§4). On exige une
        // domination nette (≥ 2×), pas une coudée — et que le MLP lui-même prospère
        // (sinon « les deux se sont effondrés » passerait à tort).
        if mlp_mean < 50.0 {
            failures.push(format!(
                "seed {seed:#x} : le MLP ne prospère pas ({mlp_mean:.1}) — il n'a pas appris à fourrager"
            ));
        } else if mlp_mean <= 2.0 * wander_mean {
            failures.push(format!(
                "seed {seed:#x} : le MLP ne domine pas l'errance ({mlp_mean:.1} vs {wander_mean:.1}) — apprentissage insuffisant"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "le MLP n'a pas su battre le témoin d'errance :\n  {}",
        failures.join("\n  ")
    );
}
