//! Point d'entrée **fenêtré** (direct).
//!
//! `DefaultPlugins` pilote la fenêtre, le rendu et la présentation. Tout ce
//! qu'on ajoute ici vit dans `Update` et ne touche QUE le rendu / l'UI — jamais
//! l'état de simulation, qui appartient à [`teemlab::SimPlugin`].

mod controls;
mod editor;
mod hud;
mod inspector;
mod recorder;
mod runs;

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use teemlab::visuals::VisualsPlugin;
use teemlab::{SimConfig, SimPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "teemlab".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(SimPlugin::new(SimConfig::from_cli()))
        // Rendu de la sim partagé avec l'enregistreur vidéo (item 14).
        .add_plugins(VisualsPlugin)
        .init_resource::<hud::History>()
        .init_resource::<controls::SimControls>()
        .init_resource::<inspector::Selection>()
        .init_resource::<recorder::RecorderPanel>()
        .init_resource::<controls::PanelVisibility>()
        // La sim démarre **en pause** (on prépare la run avant de la lancer).
        .add_systems(
            Startup,
            (
                setup_camera,
                editor::build_palette,
                runs::build_runs_panel,
                controls::pause_at_launch,
            ),
        )
        // PILOTAGE DU TEMPS / RESET / RUNS (items 11, 13) — pas de logique de sim :
        // on règle l'horloge, recharge un scénario, sauve/restaure une run, ou
        // reconstruit le monde, le tout avant la boucle fixe de la frame.
        // `apply_scenario_load` précède `apply_reset` : il pose le drapeau que ce
        // dernier consomme pour reconstruire le monde avec le nouveau scénario.
        .add_systems(
            PreUpdate,
            (
                controls::drive_steps,
                runs::apply_scenario_load,
                controls::apply_reset,
                runs::save_snapshot,
                runs::apply_snapshot_load,
            )
                .chain(),
        )
        // RENDU / OBSERVATION UNIQUEMENT — jamais de logique de sim ici.
        // Le rendu de la sim (mesh, arène, vision) vit dans `VisualsPlugin` ;
        // ici, l'observation propre au build fenêtré : `hud::sample_history` ne
        // fait que *lire* l'état pour les courbes, l'inspecteur surligne la sélection.
        .add_systems(
            Update,
            (
                hud::sample_history,
                inspector::highlight_selection,
                recorder::drive_recorder,
            ),
        )
        // UI egui — **tout est docké** (panneaux d'arête, pas de fenêtres
        // flottantes). Ordre = empilement : controls en bandeau haut ; à gauche
        // éditeur · runs · enregistrement ; à droite palette · courbes · inspecteur ;
        // stats en bandeau bas (dans `editor_ui`).
        .add_systems(
            EguiPrimaryContextPass,
            (
                controls::controls_ui,
                editor::editor_ui,
                runs::runs_ui,
                recorder::recorder_ui,
                hud::hud_ui,
                (inspector::pick_agent, inspector::inspector_ui).chain(),
            ),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
