//! **Headless** entry point.
//!
//! No window, no rendering: `ScheduleRunnerPlugin` pumps the fixed loop as fast
//! as possible. The *same* [`teemlab::SimPlugin`] → the *same* world as the
//! windowed build. We count the ticks in the FIXED schedule (hence an exact
//! number, independent of clock speed) and exit at the end condition.

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;
use std::time::Duration;
use teemlab::components::{Agent, Perception, Reserve};
use teemlab::genotype::Genotype;
use teemlab::{SimConfig, SimPlugin};

/// Default end condition (P0): number of fixed ticks to simulate. ~10 s at 64 Hz.
/// Override with the `TEEMLAB_TICKS` env var to run longer — e.g. under a profiler
/// (`flame` / cargo-flamegraph), where a short run yields too few samples. Only the
/// iteration *count* changes; per-tick work and the RNG stream are untouched (DEV
/// Rule 3), so the default run stays byte-identical.
const DEFAULT_TICKS: u64 = 640;

/// Fixed ticks to run before exiting (default [`DEFAULT_TICKS`], or `TEEMLAB_TICKS`).
#[derive(Resource)]
struct TickTarget(u64);

#[derive(Resource, Default)]
struct TickCounter(u64);

fn main() -> AppExit {
    let target = std::env::var("TEEMLAB_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TICKS);
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)))
        .add_plugins(SimPlugin::new(SimConfig::from_cli()))
        .init_resource::<TickCounter>()
        .insert_resource(TickTarget(target))
        // Counting in the FIXED schedule → exact, after the tick's physics.
        .add_systems(FixedLast, tick_and_maybe_exit)
        .run()
}

fn tick_and_maybe_exit(
    mut counter: ResMut<TickCounter>,
    target: Res<TickTarget>,
    // Bevy 0.18: buffered events are "messages".
    mut exit: MessageWriter<AppExit>,
    agents: Query<(&Perception, &Reserve, &Genotype), With<Agent>>,
) {
    counter.0 += 1;
    if counter.0 >= target.0 {
        let population = agents.iter().count();
        let n = population.max(1) as f32;
        // Raycast smoke test without a window: proportion of rays that see
        // something. > 0 proves vision also works headless.
        let (seen, total) = agents.iter().fold((0usize, 0usize), |(s, t), (p, _, _)| {
            (
                s + p.vision.iter().filter(|&&v| v > 0.0).count(),
                t + p.vision.len(),
            )
        });
        let ratio = seen as f32 / total.max(1) as f32;
        let mean_reserve = agents.iter().map(|(_, r, _)| r.current).sum::<f32>() / n;
        // Gene means: their deviation from the founding values is the proof of
        // evolutionary drift (item 9).
        let mean_speed = agents.iter().map(|(_, _, g)| g.max_speed).sum::<f32>() / n;
        let mean_vision = agents.iter().map(|(_, _, g)| g.vision_range).sum::<f32>() / n;
        // `println!` (not `info!`): MinimalPlugins has no LogPlugin.
        println!(
            "headless: {} ticks, population = {}, mean reserve = {:.1}, \
             rays occupied = {:.0}%, mean genes: speed = {:.1}, vision = {:.1}",
            counter.0,
            population,
            mean_reserve,
            ratio * 100.0,
            mean_speed,
            mean_vision
        );
        exit.write(AppExit::Success);
    }
}
