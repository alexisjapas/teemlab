//! Disposition **dockée** du build fenêtré : quatre panneaux egui fixes autour de
//! la zone de simulation centrale.
//!
//! Module du *binaire* fenêtré uniquement. On n'invente rien : chaque panneau
//! appelle la `*_section(ui, …)` réutilisable déjà exposée par son module d'outil
//! (`controls`, `editor`, `runs`, `hud`, `recorder`, `inspector`). Le rôle de ces
//! systèmes est purement la **mise en page** — réserver les bords de l'écran egui.
//!
//! C'est ce qui rend la zone centrale auto-ajustée : les `SidePanel`/`TopBottomPanel`
//! *réservent* leur espace, donc `ctx.available_rect()` se réduit au centre, et
//! `main::set_sim_camera` (qui tourne en dernier du pass egui) y cadre la sim — d'où
//! une simulation toujours **centrée et entièrement visible**, quelle que soit la
//! taille des panneaux. Aucun `CentralPanel` : le centre reste « transparent » et
//! laisse transparaître le rendu Bevy.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{
    Action, Age, Agent, Food, Generation, Perception, Reserve, Species, Vision,
};
use teemlab::genotype::Genotype;

use crate::controls::{self, SimControls};
use crate::editor::{self, Palette};
use crate::hud::{self, History};
use crate::inspector::{self, Selection};
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};

/// Barre du haut, pleine largeur : **contrôles** (pause/pas/vitesse/réinit.) à
/// gauche, **stats** à droite, même ligne.
pub fn top_bar(
    mut contexts: EguiContexts,
    mut sim_controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    agents: Query<(&Reserve, &Genotype), With<Agent>>,
    food: Query<(), With<Food>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            controls::controls_section(ui, &mut sim_controls, &mut vtime);
            ui.separator();
            editor::stats_section(ui, &agents, &food);
        });
    });
    Ok(())
}

/// Colonne de gauche, redimensionnable : les quatre outils d'édition empilés de
/// haut en bas, chacun dans un en-tête repliable (une seule `ScrollArea` car la
/// colonne est haute).
pub fn left_tools(
    mut contexts: EguiContexts,
    mut runs_panel: ResMut<RunsPanel>,
    mut palette: ResMut<Palette>,
    mut config: ResMut<SimConfig>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::left("left_tools")
        .resizable(true)
        .default_width(300.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::CollapsingHeader::new("Runs & scénarios")
                    .default_open(true)
                    .show(ui, |ui| runs::runs_section(ui, &mut runs_panel));
                egui::CollapsingHeader::new("Monde")
                    .default_open(true)
                    .show(ui, |ui| editor::world_section(ui, &mut config));
                egui::CollapsingHeader::new("Archétypes")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::selector_section(ui, &mut palette, &mut config)
                    });
                egui::CollapsingHeader::new("Éditeur d'archétype")
                    .default_open(true)
                    .show(ui, |ui| {
                        editor::editor_section(ui, &mut palette, &mut config)
                    });
            });
        });
    Ok(())
}

/// Colonne de droite, redimensionnable : l'enregistrement vidéo.
pub fn right_panel(mut contexts: EguiContexts, mut panel: ResMut<RecorderPanel>) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::right("right_panel")
        .resizable(true)
        .default_width(240.0)
        .show(ctx, |ui| {
            ui.heading("Enregistrement");
            recorder::recorder_section(ui, &mut panel);
        });
    Ok(())
}

/// Bandeau du bas, redimensionnable : courbes d'évolution (gauche) et inspecteur
/// d'agent (droite), en deux colonnes.
pub fn bottom_panel(
    mut contexts: EguiContexts,
    mut history: ResMut<History>,
    selection: Res<Selection>,
    agents: Query<
        (
            &Species,
            &Reserve,
            &Genotype,
            &Vision,
            &Perception,
            &Action,
            &Brain,
            &Generation,
            &Age,
        ),
        With<Agent>,
    >,
) -> Result {
    /// Largeur minimale du panneau central sous laquelle on n'essaie plus de le
    /// scinder en deux colonnes. `egui::Ui::columns` calcule une largeur de colonne
    /// négative — et **panique** (`ui.rs:958`, « Negative width makes no sense ») —
    /// quand l'espace dispo est presque nul, ce qui arrive dès que les colonnes
    /// latérales (gauche + droite) mangent quasiment toute la fenêtre. En deçà, on
    /// empile les deux sections au lieu de planter (et d'emporter tout le pass egui,
    /// panneau d'enregistrement compris).
    const MIN_TWO_COLUMN_WIDTH: f32 = 160.0;

    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .default_height(220.0)
        .show(ctx, |ui| {
            // Courbes : on les enveloppe d'un défilement (elles n'en ont pas).
            let mut courbes = |ui: &mut egui::Ui| {
                egui::ScrollArea::vertical()
                    .id_salt("courbes")
                    .show(ui, |ui| {
                        ui.strong("Évolution — courbes");
                        hud::hud_section(ui, &mut history);
                    });
            };
            // Inspecteur : `inspector_section` porte déjà sa propre `ScrollArea`.
            let inspecteur = |ui: &mut egui::Ui| {
                ui.strong("Inspecteur d'agent");
                inspector::inspector_section(ui, &selection, &agents);
            };

            if ui.available_width() >= MIN_TWO_COLUMN_WIDTH {
                ui.columns(2, |cols| {
                    courbes(&mut cols[0]);
                    inspecteur(&mut cols[1]);
                });
            } else {
                // Panneau central trop étroit : empilé, chacun gardant son défilement.
                courbes(ui);
                ui.separator();
                inspecteur(ui);
            }
        });
    Ok(())
}
