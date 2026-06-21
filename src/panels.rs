//! Disposition **dockée** du build fenêtré : des panneaux egui fixes autour de la
//! zone de simulation centrale.
//!
//! Module du *binaire* fenêtré uniquement. On n'invente rien : chaque panneau
//! appelle la `*_section(ui, …)` réutilisable déjà exposée par son module d'outil
//! (`controls`, `editor`, `runs`, `hud`, `recorder`, `inspector`). Le rôle de ces
//! systèmes est purement la **mise en page** — réserver les bords de l'écran egui.
//!
//! Découpage **sémantique** : *monde* à gauche, *entités* à droite, *scénario +
//! enregistrement* en bande du haut, *contrôles + stats* puis *courbes + inspecteur*
//! en bas.
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
use teemlab::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use teemlab::config::Archetype;
use teemlab::genotype::Genotype;
use teemlab::metrics::History;
use teemlab::selection::Selection;

use crate::controls::{self, SimControls};
use crate::editor::{self, Palette};
use crate::hud;
use crate::inspector;
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};

/// Bande du haut, pleine largeur : **scénario** (choisir / recharger / sauver-charger)
/// à gauche, **enregistrement** vidéo à droite — tout l'IO « entrée/sortie » d'une run.
pub fn top_bar(
    mut contexts: EguiContexts,
    mut runs_panel: ResMut<RunsPanel>,
    mut config: ResMut<SimConfig>,
    mut recorder_panel: ResMut<RecorderPanel>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        // Deux groupes verticaux côte à côte : chacun empile ses propres lignes sans
        // que l'`horizontal` extérieur ne les mette bout à bout.
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.heading("Scénario");
                runs::scenario_section(ui, &mut runs_panel, &mut config);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.heading("Enregistrement");
                recorder::recorder_section(ui, &mut recorder_panel);
            });
        });
    });
    Ok(())
}

/// Colonne de gauche, redimensionnable : le **monde** — paramètres globaux de
/// scénario (arène, cadence, graine, fonds), bornes des gènes et table de relations.
pub fn left_tools(mut contexts: EguiContexts, mut config: ResMut<SimConfig>) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::left("left_tools")
        .resizable(true)
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.heading("Monde");
            egui::ScrollArea::vertical().show(ui, |ui| editor::world_section(ui, &mut config));
        });
    Ok(())
}

/// Colonne de droite, redimensionnable : les **entités** — palette d'archétypes
/// (sélecteur + bibliothèque d'espèces) et éditeur de l'archétype sélectionné.
pub fn right_panel(
    mut contexts: EguiContexts,
    mut palette: ResMut<Palette>,
    mut config: ResMut<SimConfig>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::SidePanel::right("right_panel")
        .resizable(true)
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.heading("Entités");
            egui::ScrollArea::vertical().show(ui, |ui| {
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

/// Bandeau du bas (juste sous la sim) : **contrôles** (pause/pas/vitesse/réinit.) à
/// gauche, **stats globales** à droite, même ligne.
pub fn bottom_bar(
    mut contexts: EguiContexts,
    mut sim_controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    agents: Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            controls::controls_section(ui, &mut sim_controls, &mut vtime);
            ui.separator();
            editor::stats_section(ui, &agents);
        });
    });
    Ok(())
}

/// Panneau du bas, redimensionnable : courbes d'évolution (gauche) et inspecteur
/// d'agent (droite), en deux colonnes.
pub fn bottom_panel(
    mut contexts: EguiContexts,
    mut history: ResMut<History>,
    selection: Res<Selection>,
    mut config: ResMut<SimConfig>,
    mut palette: ResMut<Palette>,
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
                        hud::hud_section(ui, &mut history, &config);
                    });
            };
            // Inspecteur : `inspector_section` porte déjà sa propre `ScrollArea`. Il ne
            // mute pas la sim : s'il y a une demande de capture, il **retourne**
            // l'archétype dérivé, qu'on ajoute à la config après les fermetures (pour ne
            // pas emprunter `config` en mutable pendant que les courbes le lisent).
            let mut capture_request: Option<Archetype> = None;
            let mut inspecteur = |ui: &mut egui::Ui| {
                ui.strong("Inspecteur d'agent");
                if let Some(arch) = inspector::inspector_section(ui, &selection, &config, &agents) {
                    capture_request = Some(arch);
                }
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

            // Capture validée : l'archétype dérivé rejoint la config et devient la
            // sélection de l'éditeur (les fermetures ci-dessus, qui empruntaient `config`
            // en partage, ne sont plus utilisées ici → emprunt mutable autorisé).
            if let Some(arch) = capture_request {
                let from = arch.captured_from.clone().unwrap_or_default();
                config.archetypes.push(arch);
                palette.selected = Some(config.archetypes.len() - 1);
                palette.status = format!("Archétype capturé depuis {from}.");
            }
        });
    Ok(())
}
