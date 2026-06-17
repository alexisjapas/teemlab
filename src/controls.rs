//! Contrôles de simulation du build fenêtré : **pause / vitesse / pas-à-pas /
//! reset** (item 11).
//!
//! Module du *binaire* fenêtré uniquement (comme [`crate::editor`] et
//! [`crate::hud`]). Le pilotage du temps passe par `Time<Virtual>` — l'horloge
//! fixe le suit (§6), donc la pause fige la sim *et* le HUD pendant que le rendu
//! continue, et l'accéléré change la cadence d'évolution sans toucher au rendu.
//!
//! Invariant cardinal respecté : on ne touche jamais la *logique* de sim, on ne
//! fait que régler son horloge ou, pour le reset, **reconstruire le monde**
//! depuis le `SimConfig` — l'équivalent d'un nouveau `Startup`, déclenché à la
//! main (comme le placement de l'éditeur, c'est de l'édition, pas de la sim).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::components::{Agent, Food, Wall};
use teemlab::ecology::{FoodRegen, SimRng};
use teemlab::spawn;

use crate::hud::History;

/// Quelles fenêtres d'outils sont ouvertes. Chaque outil est une `egui::Window`
/// **flottante** au-dessus de la sim plein cadre ; ce drapeau pilote son
/// ouverture (bascule du bandeau ou croix de la fenêtre). Seul le bandeau de
/// contrôles reste docké (chrome de l'appli).
#[derive(Resource)]
pub struct PanelVisibility {
    pub editor: bool,
    pub palette: bool,
    pub world: bool,
    pub hud: bool,
    pub inspector: bool,
    pub runs: bool,
    pub recorder: bool,
    pub stats: bool,
    /// Demande de rangement des fenêtres (replacées par [`tidy_pos`] cette frame).
    pub tidy_windows: bool,
}

/// Emplacement d'une fenêtre d'outil pour le rangement automatique.
#[derive(Clone, Copy)]
pub enum WindowSlot {
    Runs,
    Recorder,
    Hud,
    Archetypes,
    Editor,
    Inspector,
    World,
    Stats,
}

/// Position de rangement d'une fenêtre, calculée depuis la taille **courante** de
/// l'écran egui — donc adaptée à la fenêtre réelle, contrairement à un `default_pos`
/// figé (qui ne suit pas un compositeur agrandissant la fenêtre après coup).
/// Disposition : colonne gauche (Runs · Enregistrement · Courbes), colonne droite
/// (Archétypes · Éditeur · Inspecteur), Stats en haut au centre — le centre reste
/// à la simulation.
pub fn tidy_pos(screen: egui::Rect, slot: WindowSlot) -> egui::Pos2 {
    const COL_W: f32 = 232.0;
    let top = screen.top() + 48.0;
    let left = screen.left() + 8.0;
    let right = (screen.right() - COL_W - 8.0).max(left + COL_W);
    let h = screen.height();
    match slot {
        WindowSlot::Runs => egui::pos2(left, top),
        WindowSlot::Recorder => egui::pos2(left, top + h * 0.28),
        WindowSlot::Hud => egui::pos2(left, top + h * 0.56),
        WindowSlot::Archetypes => egui::pos2(right, top),
        WindowSlot::Editor => egui::pos2(right, top + h * 0.30),
        WindowSlot::Inspector => egui::pos2(right, top + h * 0.62),
        WindowSlot::Stats => egui::pos2(screen.center().x - COL_W * 0.5, top),
        // Colonne centrale, sous les Stats : le monde est l'édition « globale »,
        // distincte des archétypes (colonne droite).
        WindowSlot::World => egui::pos2(screen.center().x - COL_W * 0.5, top + h * 0.10),
    }
}

/// Range les fenêtres une fois, peu après le lancement (quand la fenêtre a atteint
/// sa taille définitive). Le bouton « Ranger » fait la même chose à la demande.
pub fn auto_tidy(mut vis: ResMut<PanelVisibility>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame == 60 {
        vis.tidy_windows = true;
    }
}

/// Consomme le drapeau de rangement après que toutes les fenêtres l'ont lu (dernier
/// maillon de la chaîne des fenêtres).
pub fn clear_tidy(mut vis: ResMut<PanelVisibility>) {
    vis.tidy_windows = false;
}

impl Default for PanelVisibility {
    fn default() -> Self {
        // À l'ouverture : toutes les fenêtres d'outils ouvertes (on ferme à la
        // demande — croix de la fenêtre ou bascule du bandeau).
        Self {
            editor: true,
            palette: true,
            world: true,
            hud: true,
            inspector: true,
            runs: true,
            recorder: true,
            stats: true,
            tidy_windows: false,
        }
    }
}

/// État des contrôles : vitesse choisie, pas en attente, reset demandé. Les
/// boutons (en `EguiPrimaryContextPass`, trop tard pour la boucle fixe de la
/// frame) ne font qu'écrire ici ; ce sont [`drive_steps`] et [`apply_reset`], en
/// `PreUpdate`, qui agissent **avant** que la boucle fixe ne tourne.
#[derive(Resource)]
pub struct SimControls {
    /// Vitesse relative active (appliquée à `Time<Virtual>` quand on n'est pas en
    /// pause).
    pub speed: f32,
    /// Nombre de ticks fixes à jouer un par un pendant la pause.
    pub steps_pending: u32,
    /// Reset demandé ce frame.
    pub reset_requested: bool,
}

impl Default for SimControls {
    fn default() -> Self {
        Self {
            speed: 1.0,
            steps_pending: 0,
            reset_requested: false,
        }
    }
}

/// `Startup` : la sim démarre **en pause**, pour qu'on puisse placer/éditer et
/// préparer une run avant qu'elle ne tourne. On ne fige que l'horloge
/// (`Time<Virtual>`) — l'horloge fixe la suit (§6) ; le rendu, lui, continue.
pub fn pause_at_launch(mut vtime: ResMut<Time<Virtual>>) {
    vtime.pause();
}

/// Le bandeau de contrôles (haut de l'écran). N'agit que sur `Time<Virtual>`
/// (pause/vitesse) ou pose un drapeau (pas, reset).
pub fn controls_ui(
    mut contexts: EguiContexts,
    mut controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    mut vis: ResMut<PanelVisibility>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::top("controls").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let paused = vtime.is_paused();
            if ui
                .button(if paused { "▶ Lecture" } else { "⏸ Pause" })
                .clicked()
            {
                if paused {
                    vtime.unpause();
                } else {
                    vtime.pause();
                }
            }
            // Le pas-à-pas n'a de sens qu'à l'arrêt.
            ui.add_enabled_ui(paused, |ui| {
                if ui.button("⏭ Pas").clicked() {
                    controls.steps_pending += 1;
                }
            });

            ui.separator();
            // Slider à échelle logarithmique : réglage fin du x0.1 au x100 sur une
            // seule poignée (remplace les anciens presets discrets).
            if ui
                .add(
                    egui::Slider::new(&mut controls.speed, 0.1..=100.0)
                        .logarithmic(true)
                        .text("Vitesse ×"),
                )
                .changed()
            {
                vtime.set_relative_speed(controls.speed);
            }

            ui.separator();
            if ui.button("⟲ Réinitialiser").clicked() {
                controls.reset_requested = true;
            }
            if paused {
                ui.separator();
                ui.weak("en pause");
            }
        });
        // Ouverture des fenêtres d'outils (flottantes au-dessus de la sim).
        ui.horizontal(|ui| {
            ui.label("Fenêtres :");
            if ui
                .button("⊞ Ranger")
                .on_hover_text("Replace les fenêtres le long des bords")
                .clicked()
            {
                vis.tidy_windows = true;
            }
            ui.separator();
            ui.toggle_value(&mut vis.palette, "Archétypes");
            ui.toggle_value(&mut vis.editor, "Éditeur");
            ui.toggle_value(&mut vis.world, "Monde");
            ui.toggle_value(&mut vis.runs, "Runs");
            ui.toggle_value(&mut vis.recorder, "Enregistrement");
            ui.toggle_value(&mut vis.hud, "Courbes");
            ui.toggle_value(&mut vis.inspector, "Inspecteur");
            ui.toggle_value(&mut vis.stats, "Stats");
        });
    });
    Ok(())
}

/// Pas-à-pas : pendant la pause, avancer `Time<Virtual>` d'**exactement un
/// `timestep`** par pas demandé. Tourne en `PreUpdate` (après la mise à jour du
/// temps, avant la boucle fixe) pour qu'un seul tick fixe soit joué cette frame.
/// Hors pause, les pas en attente sont abandonnés (le déroulé normal reprend).
pub fn drive_steps(
    mut controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    fixed: Res<Time<Fixed>>,
) {
    if !vtime.is_paused() {
        controls.steps_pending = 0;
        return;
    }
    if controls.steps_pending == 0 {
        return;
    }
    // Un timestep injecté à la main : la boucle fixe l'accumulera et exécutera
    // pile un tick. (`advance_by` écrit le delta même sur une horloge en pause —
    // la pause ne fait que mettre le delta calculé par Bevy à zéro.)
    vtime.advance_by(fixed.timestep());
    controls.steps_pending -= 1;
}

/// Reset à chaud : reconstruire le monde depuis le `SimConfig`. Despawn de tout
/// ce qui est simulé (agents, nourriture, murs), re-peuplement, et remise à zéro
/// des ressources de sim (RNG, reliquat de repousse) et du HUD. En `PreUpdate` :
/// les commandes s'appliquent avant la boucle fixe, donc la frame repart déjà sur
/// le monde neuf.
pub fn apply_reset(
    mut controls: ResMut<SimControls>,
    mut commands: Commands,
    config: Res<SimConfig>,
    mut sim_rng: ResMut<SimRng>,
    mut regen: ResMut<FoodRegen>,
    mut history: ResMut<History>,
    simulated: Query<Entity, Or<(With<Agent>, With<Food>, With<Wall>)>>,
) {
    if !controls.reset_requested {
        return;
    }
    controls.reset_requested = false;

    for entity in &simulated {
        commands.entity(entity).despawn();
    }
    spawn::populate(&mut commands, &config);

    *sim_rng = SimRng::from_config(&config);
    regen.0 = 0.0;
    history.clear();
}
