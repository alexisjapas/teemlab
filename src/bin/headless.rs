//! Point d'entrée **headless**.
//!
//! Ni fenêtre ni rendu : `ScheduleRunnerPlugin` pompe la boucle fixe aussi vite
//! que possible. Le *même* [`teemlab::SimPlugin`] → le *même* monde que le
//! build fenêtré. On compte les ticks dans le schedule FIXE (donc un nombre
//! exact, indépendant de la vitesse d'horloge) et on sort à la condition de fin.

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;
use std::time::Duration;
use teemlab::components::{Agent, Perception, Reserve};
use teemlab::genotype::Genotype;
use teemlab::{SimConfig, SimPlugin};

/// Condition de fin (P0) : nombre de ticks fixes à simuler. ~10 s à 64 Hz.
const TICKS: u64 = 640;

#[derive(Resource, Default)]
struct TickCounter(u64);

fn main() -> AppExit {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)))
        .add_plugins(SimPlugin::new(SimConfig::from_cli()))
        .init_resource::<TickCounter>()
        // Comptage dans le schedule FIXE → exact, après la physique du tick.
        .add_systems(FixedLast, tick_and_maybe_exit)
        .run()
}

fn tick_and_maybe_exit(
    mut counter: ResMut<TickCounter>,
    // Bevy 0.18 : les events bufferisés sont des « messages ».
    mut exit: MessageWriter<AppExit>,
    agents: Query<(&Perception, &Reserve, &Genotype), With<Agent>>,
) {
    counter.0 += 1;
    if counter.0 >= TICKS {
        let population = agents.iter().count();
        let n = population.max(1) as f32;
        // Smoke test du raycast sans fenêtre : proportion de rayons qui voient
        // quelque chose. > 0 prouve que la vision fonctionne aussi en headless.
        let (seen, total) = agents.iter().fold((0usize, 0usize), |(s, t), (p, _, _)| {
            (s + p.vision.iter().filter(|&&v| v > 0.0).count(), t + p.vision.len())
        });
        let ratio = seen as f32 / total.max(1) as f32;
        let mean_reserve = agents.iter().map(|(_, r, _)| r.current).sum::<f32>() / n;
        // Moyennes des gènes : leur écart aux valeurs fondatrices est la preuve
        // de la dérive évolutive (item 9).
        let mean_speed = agents.iter().map(|(_, _, g)| g.max_speed).sum::<f32>() / n;
        let mean_vision = agents.iter().map(|(_, _, g)| g.vision_range).sum::<f32>() / n;
        // `println!` (pas `info!`) : MinimalPlugins n'a pas de LogPlugin.
        println!(
            "headless: {} ticks, population = {}, réserve moy = {:.1}, \
             rayons occupés = {:.0}%, gènes moy : vitesse = {:.1}, vision = {:.1}",
            counter.0, population, mean_reserve, ratio * 100.0, mean_speed, mean_vision
        );
        exit.write(AppExit::Success);
    }
}
