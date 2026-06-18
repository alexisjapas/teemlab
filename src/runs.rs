//! Gestion de runs & scénarios à chaud, build fenêtré (item 13).
//!
//! Module du *binaire* fenêtré uniquement (comme [`crate::editor`], …). Trois
//! services :
//!
//! - **sélecteur de scénario** : la liste des `scenarios/*.ron`, à choisir et
//!   **recharger dans le monde vivant** sans relancer le binaire ;
//! - **sauvegarde** de l'état d'une run vers un snapshot RON ;
//! - **restauration** d'une run depuis un snapshot.
//!
//! Comme l'éditeur et le reset (item 11), c'est de l'édition déclenchée à la main
//! (hors `FixedUpdate`) : les boutons ne posent qu'une *action en attente* ; ce
//! sont les systèmes `PreUpdate` ci-dessous qui l'appliquent **avant** la boucle
//! fixe de la frame. Recharger un scénario réutilise le reset (item 11) pour
//! reconstruire le monde — pas de logique de peuplement dupliquée ici.

use bevy::prelude::*;
use bevy_egui::egui;
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Agent, Food, Reserve, Species, Wall};
use teemlab::ecology::{FoodRegen, SimRng};
use teemlab::genotype::Genotype;
use teemlab::snapshot::{AgentSnap, FoodSnap, Snapshot};
use teemlab::spawn;

use crate::controls::SimControls;
use crate::editor::{self, Palette};
use crate::hud::History;

/// Une action demandée par l'UI, appliquée au prochain `PreUpdate`.
enum RunAction {
    /// Recharger ce scénario dans le monde vivant (config + reset).
    LoadScenario(String),
    /// Sauver l'état courant vers ce fichier snapshot.
    SaveSnapshot(String),
    /// Restaurer une run depuis ce fichier snapshot.
    LoadSnapshot(String),
}

/// État du panneau « Runs & scénarios ».
#[derive(Resource)]
pub struct RunsPanel {
    /// Chemins des `scenarios/*.ron` trouvés au lancement.
    scenarios: Vec<String>,
    /// Index du scénario sélectionné dans la liste.
    selected: Option<usize>,
    /// Chemin de sauvegarde/chargement de snapshot.
    snapshot_path: String,
    /// Dernier message (succès/échec).
    status: String,
    /// Action en attente d'application en `PreUpdate`.
    pending: Option<RunAction>,
}

/// Liste les scénarios RON présents dans `scenarios/`, triés.
fn scan_scenarios() -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir("scenarios") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ron")
                && let Some(s) = path.to_str()
            {
                found.push(s.to_string());
            }
        }
    }
    found.sort();
    found
}

/// Construit le panneau au `Startup`.
pub fn build_runs_panel(mut commands: Commands) {
    commands.insert_resource(RunsPanel {
        scenarios: scan_scenarios(),
        selected: None,
        snapshot_path: "run.ron".to_string(),
        status: String::new(),
        pending: None,
    });
}

/// La fenêtre flottante « Runs & scénarios ». Tourne dans `EguiPrimaryContextPass`.
/// La section « Runs & scénarios », rendue dans le panneau de gauche (item dock).
/// Ne fait que lire/écrire son propre état et poser une action en attente — aucun
/// accès au monde ici (cf. systèmes `PreUpdate`).
pub(crate) fn runs_section(ui: &mut egui::Ui, panel: &mut RunsPanel) {
    // On travaille sur des copies locales pour que la fermeture egui ne capture
    // pas `panel` (évite les emprunts croisés dans le combo).
    let scenarios = panel.scenarios.clone();
    let status = panel.status.clone();
    let mut selected = panel.selected;
    let mut snapshot_path = panel.snapshot_path.clone();
    let mut pending = None;
    let mut rescan = false;

    ui.strong("Scénario");
    ui.horizontal(|ui| {
        let label = selected
            .and_then(|i| scenarios.get(i))
            .map(String::as_str)
            .unwrap_or("(choisir…)");
        egui::ComboBox::from_id_salt("scenario_combo")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (i, path) in scenarios.iter().enumerate() {
                    ui.selectable_value(&mut selected, Some(i), path);
                }
            });
        if ui.button("↻").on_hover_text("Rescanner scenarios/").clicked() {
            rescan = true;
        }
    });
    if ui
        .add_enabled(
            selected.is_some(),
            egui::Button::new("⟲ Recharger dans le monde"),
        )
        .on_hover_text("Charge le scénario et redémarre la run.")
        .clicked()
        && let Some(path) = selected.and_then(|i| scenarios.get(i))
    {
        pending = Some(RunAction::LoadScenario(path.clone()));
    }

    ui.separator();
    ui.strong("État de la run (snapshot)");
    ui.text_edit_singleline(&mut snapshot_path);
    ui.horizontal(|ui| {
        if ui.button("💾 Sauver la run").clicked() {
            pending = Some(RunAction::SaveSnapshot(snapshot_path.clone()));
        }
        if ui.button("📂 Charger la run").clicked() {
            pending = Some(RunAction::LoadSnapshot(snapshot_path.clone()));
        }
    });

    if !status.is_empty() {
        ui.weak(&status);
    }

    // Report des copies locales vers la ressource.
    panel.selected = selected;
    panel.snapshot_path = snapshot_path;
    if rescan {
        panel.scenarios = scan_scenarios();
        panel.selected = None;
    }
    if pending.is_some() {
        panel.pending = pending;
    }
}

/// Recharge un scénario dans le monde vivant : remplace le `SimConfig`, resynchro
/// la palette de l'éditeur, puis **délègue la reconstruction au reset** (item 11)
/// en levant son drapeau. Doit tourner avant `controls::apply_reset` (chaîné).
pub fn apply_scenario_load(
    mut panel: ResMut<RunsPanel>,
    mut config: ResMut<SimConfig>,
    mut palette: ResMut<Palette>,
    mut controls: ResMut<SimControls>,
) {
    if !matches!(panel.pending, Some(RunAction::LoadScenario(_))) {
        return;
    }
    let Some(RunAction::LoadScenario(path)) = panel.pending.take() else {
        return;
    };
    panel.status = match SimConfig::from_ron_file(&path) {
        Ok(loaded) => {
            *config = loaded;
            palette.items = editor::make_items(&config);
            palette.selected = None;
            palette.dragging = None;
            controls.reset_requested = true;
            format!("Scénario rechargé ← {path}")
        }
        Err(e) => format!("Échec : {e}"),
    };
}

/// Sauve l'état vivant vers un snapshot RON. Lecture seule du monde.
pub fn save_snapshot(
    mut panel: ResMut<RunsPanel>,
    config: Res<SimConfig>,
    sim_rng: Res<SimRng>,
    regen: Res<FoodRegen>,
    agents: Query<(&Transform, &Genotype, &Reserve, &Species, &Brain), With<Agent>>,
    food: Query<(&Transform, &Reserve), With<Food>>,
) {
    if !matches!(panel.pending, Some(RunAction::SaveSnapshot(_))) {
        return;
    }
    let Some(RunAction::SaveSnapshot(path)) = panel.pending.take() else {
        return;
    };

    let snapshot = Snapshot {
        config: config.clone(),
        sim_rng: sim_rng.0.clone(),
        food_regen: regen.0,
        agents: agents
            .iter()
            .map(|(transform, genotype, reserve, species, brain)| AgentSnap {
                pos: transform.translation.truncate().to_array(),
                genotype: *genotype,
                reserve: reserve.current,
                species: species.0,
                brain: brain.clone(),
            })
            .collect(),
        food: food
            .iter()
            .map(|(transform, reserve)| FoodSnap {
                pos: transform.translation.truncate().to_array(),
                reserve: reserve.current,
            })
            .collect(),
    };

    panel.status = match snapshot.save_ron_file(&path) {
        Ok(()) => format!("Run sauvée ({} agents) → {path}", snapshot.agents.len()),
        Err(e) => format!("Échec : {e}"),
    };
}

/// Restaure une run depuis un snapshot : despawn de tout le simulé, puis arène +
/// agents (cerveaux exacts) + nourriture rejoués depuis le fichier, et ressources
/// de sim/HUD remises dans l'état sauvegardé. En `PreUpdate` : le monde neuf est
/// en place avant la boucle fixe de la frame.
#[allow(clippy::too_many_arguments)]
pub fn apply_snapshot_load(
    mut panel: ResMut<RunsPanel>,
    mut commands: Commands,
    mut config: ResMut<SimConfig>,
    mut sim_rng: ResMut<SimRng>,
    mut regen: ResMut<FoodRegen>,
    mut history: ResMut<History>,
    mut palette: ResMut<Palette>,
    simulated: Query<Entity, Or<(With<Agent>, With<Food>, With<Wall>)>>,
) {
    if !matches!(panel.pending, Some(RunAction::LoadSnapshot(_))) {
        return;
    }
    let Some(RunAction::LoadSnapshot(path)) = panel.pending.take() else {
        return;
    };
    let snapshot = match Snapshot::from_ron_file(&path) {
        Ok(s) => s,
        Err(e) => {
            panel.status = format!("Échec : {e}");
            return;
        }
    };

    for entity in &simulated {
        commands.entity(entity).despawn();
    }

    *config = snapshot.config.clone();
    spawn::spawn_arena(&mut commands, &config);
    for agent in &snapshot.agents {
        spawn::spawn_agent_with_brain(
            &mut commands,
            &config,
            agent.genotype,
            Species(agent.species),
            Vec2::from(agent.pos),
            agent.brain.clone(),
            agent.reserve,
        );
    }
    for source in &snapshot.food {
        teemlab::ecology::spawn_food_with_energy(
            &mut commands,
            &config,
            Vec2::from(source.pos),
            source.reserve,
        );
    }

    *sim_rng = SimRng(snapshot.sim_rng.clone());
    regen.0 = snapshot.food_regen;
    palette.items = editor::make_items(&config);
    palette.selected = None;
    palette.dragging = None;
    history.clear();

    panel.status = format!("Run chargée ({} agents) ← {path}", snapshot.agents.len());
}
