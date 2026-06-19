//! Item 17 — le **driver** du scénario proie-prédateur co-évolutif.
//!
//! Le critère de falsification de l'item 17 a trois volets : (1) **bande de
//! population** — les deux lignées coexistent sans qu'aucune s'éteigne ni
//! n'explose, et ce **robustement** (sur plusieurs graines, pas par chance) ;
//! (2) **dérive attendue** — la vision se MAINTIENT (le chasseur s'en sert), au
//! lieu de s'effondrer comme sous l'errance ; (3) **« scénario en donnée + un
//! driver, zéro édition de `movement`/`interaction`/`ecology` »** — ce fichier de
//! test EST ce driver, et ces trois systèmes moteur n'ont pas bougé d'une ligne
//! (l'addition « effectif par espèce » de l'item 17 vit dans `config`/`spawn`).
//!
//! On fait tourner le *vrai* monde de sim (le même `SimPlugin` que les deux
//! binaires), en pas-à-pas manuel (cf. débit headless, §6), et on échantillonne la
//! population par espèce au fil du temps, **pour plusieurs graines** : la
//! coexistence d'une seule graine serait anecdotique, pas une bande.

use bevy::prelude::*;
use teemlab::SimConfig;
use teemlab::components::{Agent, Species};
use teemlab::genotype::Genotype;

mod common;

/// Le scénario versionné, chargé tel quel : le driver mesure CE que lancent les
/// binaires, pas une variante de test.
const SCENARIO: &str = include_str!("../scenarios/proie_predateur.ron");

/// Graines d'expérience (cf. §5 : on rejoue une *config*, pas le bit-à-bit). Cinq
/// mondes indépendants : si la coexistence tient pour tous, ce n'est pas un coup
/// de chance mais une propriété de l'économie calibrée.
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];

const SECONDS: usize = 120;

/// Trajectoire d'une run : effectifs (prédateurs = espèce 0, proies = espèce 1)
/// échantillonnés chaque seconde de sim, + la vision moyenne finale par espèce.
struct Run {
    traj: Vec<(usize, usize)>,
    pred_vision: f32,
    prey_vision: f32,
    /// Vision moyenne sur **tous** les agents vivants en fin de run (robuste à
    /// une espèce momentanément vide, où la moyenne par espèce serait NaN).
    all_vision: f32,
}

/// Fait tourner le scénario `SECONDS` secondes pour une graine donnée.
fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("scénario proie-prédateur valide");
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
        let (mut pred, mut prey) = (0, 0);
        for s in q.iter(world) {
            match s.0 {
                0 => pred += 1,
                1 => prey += 1,
                _ => {}
            }
        }
        traj.push((pred, prey));
    }

    let world = app.world_mut();
    let mut q = world.query::<(&Species, &Genotype)>();
    let (mut pred_sum, mut pred_n) = (0.0f32, 0usize);
    let (mut prey_sum, mut prey_n) = (0.0f32, 0usize);
    for (s, g) in q.iter(world) {
        match s.0 {
            0 => {
                pred_sum += g.vision_range;
                pred_n += 1;
            }
            1 => {
                prey_sum += g.vision_range;
                prey_n += 1;
            }
            _ => {}
        }
    }
    let mean = |sum: f32, n: usize| if n == 0 { f32::NAN } else { sum / n as f32 };
    Run {
        traj,
        pred_vision: mean(pred_sum, pred_n),
        prey_vision: mean(prey_sum, prey_n),
        all_vision: mean(pred_sum + prey_sum, pred_n + prey_n),
    }
}

#[test]
fn predator_prey_coexists_in_a_band_across_seeds() {
    let founder_vision = SimConfig::from_ron_str(SCENARIO)
        .unwrap()
        .genotype_of(0)
        .vision_range;

    let mut failures = Vec::new();
    eprintln!(
        "  seed       | préd(min..max) | proie(min..max) | vision préd/proie (fond {founder_vision:.0})"
    );
    for seed in SEEDS {
        let run = run_seed(seed);
        // On juge sur la 2ᵉ moitié : on laisse passer le transitoire initial
        // (pic des fondateurs, premier ajustement) et on regarde le régime établi.
        let back = &run.traj[SECONDS / 2..];
        let pred_min = back.iter().map(|&(p, _)| p).min().unwrap();
        let pred_max = back.iter().map(|&(p, _)| p).max().unwrap();
        let prey_min = back.iter().map(|&(_, q)| q).min().unwrap();
        let prey_max = back.iter().map(|&(_, q)| q).max().unwrap();
        let peak = run.traj.iter().map(|&(p, q)| p + q).max().unwrap();
        eprintln!(
            "  {seed:#012x} | {pred_min:>4}..{pred_max:<4}    | {prey_min:>4}..{prey_max:<4}     | {:.0} / {:.0}",
            run.pred_vision, run.prey_vision
        );
        // Trajectoire grossière (préd/proie tous les 20 s) — la forme du régime.
        let sampled: Vec<String> = run
            .traj
            .iter()
            .step_by(20)
            .map(|&(p, q)| format!("{p}/{q}"))
            .collect();
        eprintln!("              t=0,20,..: {}", sampled.join("  "));

        // --- Volet 1 : bande de population (coexistence bornée), pour CETTE graine. ---
        if pred_min == 0 {
            failures.push(format!(
                "seed {seed:#x} : prédateurs éteints (chaîne non soutenue)"
            ));
        }
        if prey_min == 0 {
            failures.push(format!("seed {seed:#x} : proies éteintes (surprédation)"));
        }
        if peak > 600 {
            failures.push(format!("seed {seed:#x} : explosion (pic {peak})"));
        }

        // --- Volet 2 : dérive attendue. Sous un chasseur, la vision SERT — elle se
        // maintient bien au-dessus du plancher (borne basse 30) vers lequel
        // l'errance la ferait fondre (cf. evolution.ron). On ne vise pas une valeur
        // précise (dérive stochastique en petite population), seulement le contraste
        // qualitatif : elle n'a pas fondu.
        if run.all_vision < 90.0 {
            failures.push(format!(
                "seed {seed:#x} : vision effondrée ({:.0}) — un chasseur devrait la maintenir",
                run.all_vision
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "coexistence non robuste :\n  {}",
        failures.join("\n  ")
    );
}
