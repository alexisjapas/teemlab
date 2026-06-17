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
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};
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
        // Rendu de la sim partagé avec l'enregistreur vidéo (item 14).
        .add_plugins(VisualsPlugin)
        // Fond « hors-jeu » : avec le cadrage « tout voir », la zone derrière les
        // murs est visible ; on la grise (`ClearColor`) pour qu'elle ne paraisse
        // pas vide, et `draw_play_area` peint l'aire de jeu par-dessus. Côté
        // fenêtré seulement — l'enregistreur (`VisualsPlugin`) garde son rendu.
        .insert_resource(ClearColor(OFF_GAME_COLOR))
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
                draw_play_area,
            ),
        )
        // UI egui — un bandeau de contrôles docké en haut (chrome) ; tous les
        // outils sont des **fenêtres flottantes** au-dessus de la sim plein cadre.
        // L'ordre est **chaîné** et compte : `pick_agent` et `resolve_drag` tournent
        // APRÈS toutes les fenêtres, sinon `is_pointer_over_area` lit un
        // `available_rect`/un état de survol périmé — un clic sur une fenêtre
        // désélectionnerait l'agent, et un dépôt au-dessus d'une fenêtre poserait
        // une entité cachée. `set_sim_camera` clôt le pass, zone centrale connue.
        .add_systems(
            EguiPrimaryContextPass,
            (
                controls::controls_ui,
                controls::auto_tidy,
                editor::editor_ui,
                runs::runs_ui,
                recorder::recorder_ui,
                hud::hud_ui,
                editor::stats_ui,
                inspector::inspector_ui,
                controls::clear_tidy,
                inspector::pick_agent,
                editor::resolve_drag,
                set_sim_camera,
            )
                .chain(),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Cadre la simulation dans la zone sous le bandeau de contrôles : l'arène
/// **entière** est visible et centrée (cadrage « tout voir », petite marge), quelle
/// que soit la fenêtre. Les fenêtres d'outils flottent par-dessus sans réduire
/// cette zone. Le hors-jeu autour de l'arène (sur le côté le plus long) est grisé
/// par `ClearColor` + `draw_play_area`, donc ne paraît pas vide.
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

/// Marqueur du quad de fond matérialisant l'aire de jeu (intérieur de l'arène).
#[derive(Component)]
struct PlayAreaBg;

/// Couleur du hors-jeu (fond, derrière les murs) : un gris mat qui délimite l'arène
/// sans paraître vide.
const OFF_GAME_COLOR: Color = Color::Srgba(Srgba::new(0.17, 0.17, 0.19, 1.0));
/// Couleur de l'aire de jeu (intérieur de l'arène), plus sombre que le hors-jeu.
const PLAY_AREA_COLOR: Color = Color::Srgba(Srgba::new(0.07, 0.07, 0.09, 1.0));

/// Rendu seul (fenêtré) : un quad sombre matérialisant l'aire de jeu, peint **sous**
/// les agents (z négatif) pour que le hors-jeu grisé (`ClearColor`) ne couvre que
/// l'extérieur des murs. Le quad suit la taille de l'arène (`arena_half_extent`),
/// qui peut changer au rechargement d'un scénario.
fn draw_play_area(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    config: Res<SimConfig>,
    mut existing: Query<&mut Transform, With<PlayAreaBg>>,
) {
    let side = 2.0 * config.arena_half_extent;
    if let Ok(mut tf) = existing.single_mut() {
        tf.scale = Vec3::new(side, side, 1.0);
    } else {
        commands.spawn((
            PlayAreaBg,
            Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial2d(materials.add(PLAY_AREA_COLOR)),
            Transform::from_xyz(0.0, 0.0, -10.0).with_scale(Vec3::new(side, side, 1.0)),
        ));
    }
}
