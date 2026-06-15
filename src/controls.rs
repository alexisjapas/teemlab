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

/// Vitesses proposées (× temps réel). Le §6 vise « x0.5 – x8 ».
const SPEEDS: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];

/// Quels panneaux dockés sont affichés. Tous les menus sont *dockés* (panneaux
/// d'arête, jamais flottants) ; comme ils ne tiennent pas tous de front, on les
/// montre/cache depuis le bandeau de contrôles. Le rendu de chaque panneau est
/// conditionné par ce drapeau.
#[derive(Resource)]
pub struct PanelVisibility {
    pub editor: bool,
    pub palette: bool,
    pub hud: bool,
    pub inspector: bool,
    pub runs: bool,
    pub recorder: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        // À l'ouverture : les outils de placement (éditeur + palette) et les
        // courbes ; les autres à la demande, pour laisser de la place à l'aire.
        Self {
            editor: true,
            palette: true,
            hud: true,
            inspector: false,
            runs: false,
            recorder: false,
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
            ui.label("Vitesse :");
            for s in SPEEDS {
                let selected = (controls.speed - s).abs() < 1e-3;
                if ui.selectable_label(selected, format!("{s}×")).clicked() {
                    controls.speed = s;
                    vtime.set_relative_speed(s);
                }
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
        // Affichage des panneaux dockés (tous attachés aux arêtes ; on choisit
        // lesquels montrer pour ne pas saturer la fenêtre).
        ui.horizontal(|ui| {
            ui.label("Panneaux :");
            ui.toggle_value(&mut vis.editor, "Éditeur");
            ui.toggle_value(&mut vis.palette, "Palette");
            ui.toggle_value(&mut vis.hud, "Courbes");
            ui.toggle_value(&mut vis.inspector, "Inspecteur");
            ui.toggle_value(&mut vis.runs, "Runs");
            ui.toggle_value(&mut vis.recorder, "Enregistrement");
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
