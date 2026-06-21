//! Helpers shared by the integration drivers.
//!
//! Each integration test is its *own* crate; to share code we include it via `mod
//! common;`, the file living in `common/mod.rs` (and not `common.rs`) so cargo
//! does not itself take it for a test binary.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::{SimConfig, SimPlugin};

/// A sim app in **manual single-stepping**: one `app.update()` advances by exactly
/// one fixed tick (`TimeUpdateStrategy::ManualDuration(1/tick_hz)`, cf. headless
/// throughput, §6), with only `MinimalPlugins` (no window, no rendering) and the
/// scenario's `SimPlugin` — the *same* world as both binaries.
///
/// We trigger `finish()`/`cleanup()` ourselves: in single-stepping we pump the
/// loop by hand, yet Avian inserts some of its resources in these hooks. Takes the
/// `SimConfig` by reference (and clones it) so the caller keeps it on hand (seed,
/// founding counts, …).
pub fn stepping_app(config: &SimConfig) -> App {
    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config.clone()));
    app.finish();
    app.cleanup();
    app
}
