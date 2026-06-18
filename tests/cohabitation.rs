//! Item 18a — le **driver** de la cohabitation témoin/appris.
//!
//! La couture « cerveau par espèce » + l'héritage du cerveau à la reproduction,
//! falsifiés avec les DEUX cerveaux déterministes existants (chasseur vs errance),
//! avant que le MLP (18b) n'arrive. Deux espèces partagent le même corps et la même
//! économie, ne diffèrent QUE par le cerveau, et broutent la même nourriture. Le
//! critère a trois volets, jugés sur plusieurs graines (la réussite d'une seule
//! serait anecdotique) :
//!
//! 1. **Invariant d'héritage** — tout agent vivant d'espèce 0 porte `Brain::Hunter`,
//!    tout agent d'espèce 1 porte `Brain::Wander`. Si la reproduction reconstruisait
//!    le cerveau depuis le `config` global au lieu d'hériter du parent, ce volet
//!    casserait : c'est la falsification directe de la couture.
//! 2. **Reproduction effective** — la population de chasseurs croît au-delà de ses
//!    fondateurs : sans cela, l'héritage ne serait pas exercé.
//! 3. **Domination du témoin compétent** (§4) — à ressource commune et limitée, le
//!    chasseur l'emporte sur l'errant (population). C'est le contraste qu'un cerveau
//!    appris devra, en 18b, au moins égaler.
//!
//! On fait tourner le *vrai* monde de sim (le même `SimPlugin` que les deux
//! binaires), en pas-à-pas manuel (cf. débit headless, §6).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::brain::Brain;
use teemlab::components::{Agent, Species};
use teemlab::{SimConfig, SimPlugin};

/// Le scénario versionné, chargé tel quel : le driver mesure CE que lancent les
/// binaires, pas une variante de test.
const SCENARIO: &str = include_str!("../scenarios/cohabitation.ron");

/// Graines d'expérience (cf. §5 : on rejoue une *config*, pas le bit-à-bit). Cinq
/// mondes indépendants : si la domination tient pour tous, ce n'est pas un coup de
/// chance mais une propriété du cerveau.
const SEEDS: [u64; 5] = [0x00C0_FFEE, 0x1234, 0x9999, 0xABCD, 0xBEEF];

const SECONDS: usize = 120;

/// Espèce 0 = chasseur (témoin compétent), espèce 1 = errance (témoin naïf).
const HUNTER: u16 = 0;
const WANDER: u16 = 1;

/// Trajectoire d'une run : effectifs (chasseurs, errants) par seconde de sim, + un
/// bilan de fin de run pour l'invariant d'héritage.
struct Run {
    traj: Vec<(usize, usize)>,
    /// Nombre d'agents vivants dont le cerveau NE correspond PAS à leur espèce
    /// (chasseur attendu pour 0, errance pour 1) — doit rester nul (volet 1).
    brain_mismatches: usize,
    /// Pic de chasseurs sur toute la run (volet 2 : croissance > fondateurs).
    hunter_peak: usize,
}

/// Fait tourner le scénario `SECONDS` secondes pour une graine donnée.
fn run_seed(seed: u64) -> Run {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("scénario cohabitation valide");
    config.seed = seed;
    let tick_hz = config.tick_hz as usize;

    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config));
    // Pas-à-pas manuel : finish()/cleanup() avant de pomper (Avian y insère ses
    // ressources), comme dans `tests/hunter.rs` et `tests/predator_prey.rs`.
    app.finish();
    app.cleanup();

    let mut traj = Vec::with_capacity(SECONDS);
    for _ in 0..SECONDS {
        for _ in 0..tick_hz {
            app.update();
        }
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Species, With<Agent>>();
        let (mut hunters, mut wanderers) = (0, 0);
        for s in q.iter(world) {
            match s.0 {
                HUNTER => hunters += 1,
                WANDER => wanderers += 1,
                _ => {}
            }
        }
        traj.push((hunters, wanderers));
    }

    // Bilan d'héritage : le cerveau de chaque agent vivant correspond-il à son espèce ?
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Species, &Brain), With<Agent>>();
    let brain_mismatches = q
        .iter(world)
        .filter(|(s, brain)| match s.0 {
            HUNTER => !matches!(brain, Brain::Hunter(_)),
            WANDER => !matches!(brain, Brain::Wander(_)),
            _ => false,
        })
        .count();

    let hunter_peak = traj.iter().map(|&(h, _)| h).max().unwrap_or(0);
    Run {
        traj,
        brain_mismatches,
        hunter_peak,
    }
}

#[test]
fn hunter_outforages_wanderer_across_seeds() {
    let founders = SimConfig::from_ron_str(SCENARIO).unwrap().agents_per_species;
    let hunter_founders = founders[HUNTER as usize];

    let mut failures = Vec::new();
    eprintln!("  seed         | chasseur(moy 2e moitié) | errance(moy 2e moitié) | pic chasseur (fond {hunter_founders})");
    for seed in SEEDS {
        let run = run_seed(seed);
        // On juge la domination sur la 2ᵉ moitié : on laisse passer le transitoire
        // (croissance depuis les fondateurs, premier ajustement de la compétition).
        let back = &run.traj[SECONDS / 2..];
        let mean = |f: &dyn Fn(&(usize, usize)) -> usize| -> f32 {
            back.iter().map(f).sum::<usize>() as f32 / back.len() as f32
        };
        let hunter_mean = mean(&|&(h, _)| h);
        let wander_mean = mean(&|&(_, w)| w);
        let sampled: Vec<String> = run
            .traj
            .iter()
            .step_by(20)
            .map(|&(h, w)| format!("{h}/{w}"))
            .collect();
        eprintln!(
            "  {seed:#012x} | {hunter_mean:>6.1}                 | {wander_mean:>6.1}                | {}",
            run.hunter_peak
        );
        eprintln!("               t=0,20,..: {}", sampled.join("  "));

        // --- Volet 1 : invariant d'héritage (la couture par espèce). ---
        if run.brain_mismatches > 0 {
            failures.push(format!(
                "seed {seed:#x} : {} agent(s) au cerveau incohérent avec leur espèce \
                 (l'héritage du cerveau a failli)",
                run.brain_mismatches
            ));
        }

        // --- Volet 2 : reproduction effective (les chasseurs ont essaimé). ---
        if run.hunter_peak <= hunter_founders {
            failures.push(format!(
                "seed {seed:#x} : les chasseurs n'ont pas crû au-delà des fondateurs \
                 (pic {} ≤ {hunter_founders}) — héritage non exercé",
                run.hunter_peak
            ));
        }

        // --- Volet 3 : domination du témoin compétent (§4). À ressource commune et
        // limitée, le chasseur (qui trouve la nourriture) doit l'emporter NETTEMENT
        // sur l'errant (qui ne la croise que par hasard). ---
        if hunter_mean <= wander_mean * 1.3 {
            failures.push(format!(
                "seed {seed:#x} : le chasseur ne domine pas l'errant ({hunter_mean:.1} vs \
                 {wander_mean:.1}) — le témoin compétent devrait fourrager bien mieux"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "cohabitation non concluante :\n  {}",
        failures.join("\n  ")
    );
}
