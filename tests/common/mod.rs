//! Helpers partagés par les drivers d'intégration.
//!
//! Chaque test d'intégration est son *propre* crate ; pour mutualiser du code on
//! l'inclut via `mod common;`, le fichier vivant dans `common/mod.rs` (et non
//! `common.rs`) pour que cargo ne le prenne pas lui-même pour un binaire de test.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::{SimConfig, SimPlugin};

/// App de sim en **pas-à-pas manuel** : un `app.update()` avance d'exactement un
/// tick fixe (`TimeUpdateStrategy::ManualDuration(1/tick_hz)`, cf. débit headless,
/// §6), avec les seuls `MinimalPlugins` (ni fenêtre ni rendu) et le `SimPlugin` du
/// scénario — le *même* monde que les deux binaires.
///
/// On déclenche soi-même `finish()`/`cleanup()` : en pas-à-pas on pompe la boucle
/// à la main, or Avian insère certaines de ses ressources dans ces hooks. Prend le
/// `SimConfig` par référence (et le clone) pour que l'appelant le garde sous la
/// main (graine, effectifs fondateurs, …).
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
