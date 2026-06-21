//! Gestion des **scénarios** à chaud, build fenêtré (item 13).
//!
//! Module du *binaire* fenêtré uniquement (comme [`crate::editor`], …). Tout l'IO
//! de scénario est réuni ici :
//!
//! - **sélecteur de scénario** : la liste des `scenarios/*.ron`, à choisir et
//!   **recharger dans le monde vivant** sans relancer le binaire ;
//! - **sauvegarde / chargement par chemin** : écrire le `SimConfig` courant vers un
//!   `.ron`, ou en recharger un dans le monde vivant.
//!
//! Comme l'éditeur et le reset (item 11), recharger est de l'édition déclenchée à la
//! main (hors `FixedUpdate`) : le bouton ne pose qu'une *action en attente* ; c'est le
//! système `PreUpdate` [`apply_scenario_load`] qui l'applique **avant** la boucle fixe
//! de la frame, en réutilisant le reset (item 11) pour reconstruire le monde — pas de
//! logique de peuplement dupliquée. La *sauvegarde* (simple écriture de fichier, sans
//! mutation du monde) se fait, elle, en place dans la section UI.

use bevy::prelude::*;
use bevy_egui::egui;
use teemlab::SimConfig;

use crate::controls::SimControls;
use crate::editor::Palette;

/// Une action demandée par l'UI, appliquée au prochain `PreUpdate`.
enum RunAction {
    /// Recharger ce scénario dans le monde vivant (config + reset).
    LoadScenario(String),
}

/// État du panneau « Scénario ».
#[derive(Resource)]
pub struct RunsPanel {
    /// Chemins des `scenarios/*.ron` trouvés au lancement.
    scenarios: Vec<String>,
    /// Index du scénario sélectionné dans la liste.
    selected: Option<usize>,
    /// Chemin de sauvegarde/chargement RON par saisie libre.
    scenario_path: String,
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
        scenario_path: "scenarios/edited.ron".to_string(),
        status: String::new(),
        pending: None,
    });
}

/// La section « Scénario », rendue dans la bande du haut (item dock). Lit/écrit son
/// propre état, **sauve** directement le scénario courant (écriture de fichier, pas une
/// mutation du monde → en place), et **pose une action en attente** pour les
/// (re)chargements (qui, eux, reconstruisent le monde en `PreUpdate`).
pub(crate) fn scenario_section(ui: &mut egui::Ui, panel: &mut RunsPanel, config: &mut SimConfig) {
    // On travaille sur des copies locales pour que la fermeture egui du combo ne
    // capture pas `panel` (évite les emprunts croisés).
    let scenarios = panel.scenarios.clone();
    let mut selected = panel.selected;
    let mut scenario_path = panel.scenario_path.clone();
    let mut pending = None;
    let mut rescan = false;

    // Émis **inline** (pas de `ui.horizontal` propre) : `top_bar` enveloppe scénario +
    // enregistrement dans un même `horizontal_wrapped` → une seule ligne en haut.
    ui.strong("Scénario :");
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
    if ui
        .button("↻")
        .on_hover_text("Rescanner scenarios/")
        .clicked()
    {
        rescan = true;
    }
    if ui
        .add_enabled(selected.is_some(), egui::Button::new("⟲ Recharger"))
        .on_hover_text("Charge le scénario sélectionné et redémarre la run.")
        .clicked()
        && let Some(path) = selected.and_then(|i| scenarios.get(i))
    {
        pending = Some(RunAction::LoadScenario(path.clone()));
    }

    ui.separator();
    ui.label("RON :");
    ui.add(egui::TextEdit::singleline(&mut scenario_path).desired_width(140.0))
        .on_hover_text("Fichier de scénario (.ron)");
    if ui
        .button("💾")
        .on_hover_text("Sauver le scénario courant")
        .clicked()
    {
        panel.status = match config.save_ron_file(&scenario_path) {
            Ok(()) => format!("Sauvé → {scenario_path}"),
            Err(e) => format!("Échec : {e}"),
        };
    }
    if ui
        .button("📂")
        .on_hover_text("Charger ce fichier et redémarrer la run")
        .clicked()
    {
        pending = Some(RunAction::LoadScenario(scenario_path.clone()));
    }

    if !panel.status.is_empty() {
        ui.weak(&panel.status);
    }

    // Report des copies locales vers la ressource.
    panel.selected = selected;
    panel.scenario_path = scenario_path;
    if rescan {
        panel.scenarios = scan_scenarios();
        panel.selected = None;
    }
    if pending.is_some() {
        panel.pending = pending;
    }
}

/// Recharge un scénario dans le monde vivant : remplace le `SimConfig`, resynchro
/// la palette de l'éditeur, **met la sim en pause**, puis **délègue la reconstruction
/// au reset** (item 11) en levant son drapeau. Doit tourner avant
/// `controls::apply_reset` (chaîné).
///
/// La pause (sur `Time<Virtual>`, comme [`crate::controls`]) avant le reset : on
/// repart sur un monde neuf **figé**, pour le placer/éditer/inspecter avant de le
/// lancer — calqué sur le démarrage en pause (`controls::pause_at_launch`).
pub fn apply_scenario_load(
    mut panel: ResMut<RunsPanel>,
    mut config: ResMut<SimConfig>,
    mut palette: ResMut<Palette>,
    mut controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
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
            palette.selected = None;
            palette.dragging = None;
            // Pause avant la reconstruction : le monde neuf naît figé.
            vtime.pause();
            controls.reset_requested = true;
            format!("Scénario rechargé (en pause) ← {path}")
        }
        Err(e) => format!("Échec : {e}"),
    };
}
