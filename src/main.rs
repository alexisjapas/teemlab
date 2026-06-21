//! Point d'entrée **fenêtré** (direct).
//!
//! `DefaultPlugins` pilote la fenêtre, le rendu et la présentation. Tout ce
//! qu'on ajoute ici vit dans `Update` et ne touche QUE le rendu / l'UI — jamais
//! l'état de simulation, qui appartient à [`teemlab::SimPlugin`].

// Cf. `lib.rs` : les requêtes Bevy déclenchent `type_complexity` par nature.
#![allow(clippy::type_complexity)]

mod controls;
mod editor;
mod hud;
mod inspector;
mod panels;
mod recorder;
mod runs;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use teemlab::metrics::MetricsPlugin;
use teemlab::selection::SelectionRenderPlugin;
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
        // Sans argument, le fenêtré démarre sur une **arène vide** (la toile de
        // l'éditeur) ; un scénario explicite l'emporte. Le headless, lui, garde le
        // défaut peuplé (`from_cli`).
        .add_plugins(SimPlugin::new(SimConfig::from_cli_or(SimConfig::empty())))
        // Rendu de la sim partagé avec l'enregistreur vidéo (item 14) — y compris les
        // **fonds** (aire de jeu + hors-jeu) : `VisualsPlugin::draw_play_area` lit leurs
        // couleurs dans le scénario et pilote `ClearColor`, donc l'aperçu live et la vidéo
        // rendent les mêmes teintes (cf. cette fonction).
        .add_plugins(VisualsPlugin)
        // Surbrillance + rayons de l'agent sélectionné — rendu **partagé** avec
        // l'enregistreur (qui, lui, pilote la sélection automatiquement). Ici la cible
        // vient du picking souris (cf. `inspector`). Fournit aussi la ressource `Selection`.
        .add_plugins(SelectionRenderPlugin)
        // Échantillonnage des courbes (ressource `History` + `sample_history`), partagé
        // avec l'enregistreur vidéo : l'aperçu live et la vidéo tracent la même donnée.
        .add_plugins(MetricsPlugin)
        // NB : le **visualiseur natif Bevy** ([`teemlab::dataviz`]) n'est PAS monté ici. Il
        // n'existe que pour le rendu **vidéo** (`bin/record`) : dans le fenêtré, bevy_egui rend
        // egui via la caméra de sim, donc recomposer la vue casserait l'UI (cf. mémoire).
        .init_resource::<controls::SimControls>()
        .init_resource::<recorder::RecorderPanel>()
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
            )
                .chain(),
        )
        // RENDU / OBSERVATION UNIQUEMENT — jamais de logique de sim ici.
        // Le rendu de la sim (mesh, arène, vision) vit dans `VisualsPlugin` ;
        // l'échantillonnage des courbes dans `MetricsPlugin` (lib, partagé) ; ici, le
        // seul observateur propre au binaire est le pilote d'enregistrement vidéo.
        .add_systems(Update, recorder::drive_recorder)
        // UI egui — **panneaux dockés fixes** autour de la zone de simulation
        // centrale (cf. `panels`). L'ordre est **chaîné** et compte : les panneaux
        // d'abord (ils réservent les bords), puis les interactions APRÈS — sinon
        // `is_pointer_over_area` lit un état périmé (un clic sur un panneau
        // désélectionnerait l'agent, un dépôt au-dessus poserait une entité cachée).
        // `set_sim_camera` clôt le pass : `available_rect` reflète alors tous les
        // panneaux, donc la sim est cadrée pile dans la zone centrale.
        .add_systems(
            EguiPrimaryContextPass,
            (
                panels::top_bar,
                panels::left_tools,
                panels::right_panel,
                panels::bottom_panel,
                panels::bottom_bar,
                inspector::pick_agent,
                inspector::delete_under_cursor,
                editor::resolve_drag,
                toggle_pause_key,
                set_sim_camera,
            )
                .chain(),
        )
        .run();
}

/// Raccourci **Espace** : play/pause de la simulation (pilote `Time<Virtual>`, comme le
/// bouton des contrôles). On respecte le focus clavier d'egui : si une saisie texte (chemin
/// RON, etc.) a le focus, l'espace lui revient et ne déclenche pas la pause.
fn toggle_pause_key(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    mut vtime: ResMut<Time<Virtual>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if ctx.wants_keyboard_input() {
        return Ok(());
    }
    if keys.just_pressed(KeyCode::Space) {
        if vtime.is_paused() {
            vtime.unpause();
        } else {
            vtime.pause();
        }
    }
    Ok(())
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Cadre la simulation dans la zone centrale laissée libre par les **panneaux
/// dockés** (cf. `panels`) : l'arène **entière** est visible et centrée (cadrage
/// « tout voir », petite marge), quelle que soit la fenêtre. Les panneaux réservent
/// les bords, donc `available_rect` se réduit à ce centre et la sim s'y ajuste. Le
/// hors-jeu autour de l'arène (sur le côté le plus long) est grisé par `ClearColor`
/// + `draw_play_area`, donc ne paraît pas vide.
///
/// On **zoome et déplace la caméra** plutôt que de redimensionner son viewport :
/// sous bevy_egui la surface egui est calée sur le viewport de la caméra, donc le
/// rétrécir relancerait une mise en page → vibration. En gardant le viewport plein
/// écran, la surface egui est stable. Rendu uniquement — ne touche jamais l'état de
/// sim. Tourne en dernier du pass egui : `available_rect` reflète alors le bandeau.
/// Le picking reste correct (`viewport_to_world_2d` lit l'échelle et la translation).
fn set_sim_camera(
    mut contexts: EguiContexts,
    config: Res<SimConfig>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) -> Result {
    /// Marge de respiration autour de l'arène (1.0 = collée aux bords).
    const VIEW_MARGIN: f32 = 1.06;

    let ctx = contexts.ctx_mut()?;
    let rect = ctx.available_rect();
    let (Ok(window), Ok((mut transform, mut projection))) =
        (windows.single(), cameras.single_mut())
    else {
        return Ok(());
    };

    let (wc, hc) = (rect.width(), rect.height());
    let arena = 2.0 * config.arena_half_extent; // côté de l'arène carrée, en unités monde
    if wc < 1.0 || hc < 1.0 || arena <= 0.0 {
        return Ok(());
    }

    // Échelle = unités monde par pixel. Prendre le plus PETIT côté de la zone donne
    // la plus grande échelle qui fait **entrer l'arène entière** (le grand côté
    // garde des marges, grisées en hors-jeu) ; la marge ajoute un peu d'air autour.
    // Projection par défaut de Camera2d : ScalingMode::WindowSize, origine au centre.
    let s = arena / wc.min(hc) * VIEW_MARGIN;
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = s;
    }

    // Déplacement : l'origine monde (centre de l'arène) se projette au centre `c`
    // de la zone (Y écran vers le bas ↔ Y monde vers le haut), à l'échelle `s`.
    let c = rect.center();
    transform.translation.x = (window.width() * 0.5 - c.x) * s;
    transform.translation.y = (c.y - window.height() * 0.5) * s;
    Ok(())
}
