//! Menu d'enregistrement vidéo du build fenêtré.
//!
//! Module du *binaire* fenêtré uniquement (comme [`crate::editor`], …). Il ne
//! fait **pas** le rendu vidéo lui-même : il **pilote le binaire headless
//! `record`** (P3, item 14) en sous-processus. On écrit le `SimConfig` courant
//! (édits de l'éditeur compris) dans un fichier temporaire, puis on lance
//! `record` dessus → un *re-render frais* **propre** (sans l'overlay egui),
//! conforme au §7. L'UI ne fait que configurer et lancer ; un système `Update`
//! surveille la fin du process.
//!
//! Invariant cardinal : aucune logique de sim ici, juste de l'orchestration
//! d'outil — comme l'éditeur, c'est de l'action manuelle hors `FixedUpdate`.

use bevy::prelude::*;
use bevy_egui::egui;
use std::path::PathBuf;
use std::process::{Child, Command};
use teemlab::SimConfig;
use teemlab::selection::SelectionRoll;

/// État du panneau « Enregistrement » + process `record` en cours, le cas échéant.
#[derive(Resource)]
pub struct RecorderPanel {
    out: String,
    fps: f64,
    seconds: f64,
    width: u32,
    height: u32,
    /// Mode de **sélection automatique** d'un agent pendant la vidéo (pour montrer ses
    /// rayons aux spectateurs). `Off` = vidéo inchangée.
    select: SelectionRoll,
    /// Intervalle (s) entre deux changements de sélection (modes « à timer »).
    select_interval: f32,
    /// Incruster le **visualiseur natif** (stats / courbes / inspecteur) en composition 9:16.
    hud: bool,
    /// Intervalle (s) de rotation des sections du visualiseur (courbes ↔ inspecteur).
    hud_interval: f32,
    status: String,
    /// Le sous-process `record` tant qu'il tourne (sinon `None`).
    child: Option<Child>,
    /// Lancement demandé par l'UI, traité au prochain `Update`.
    launch_requested: bool,
}

impl Default for RecorderPanel {
    fn default() -> Self {
        Self {
            out: "outputs/run.mp4".into(),
            fps: 30.0,
            seconds: 61.0,
            // Portrait 9:16 par défaut : le visualiseur est incrusté (arène carrée en
            // haut, stats/courbes/inspecteur en bas). Décocher « HUD » et choisir
            // 1080×1080 pour l'ancienne vidéo carrée de la seule arène.
            width: 1080,
            height: 1920,
            // Doyen par défaut : on suit le survivant (calme, change peu) → les rayons
            // sont visibles dans la vidéo sans réglage. « Aucune » désactive.
            select: SelectionRoll::Eldest,
            select_interval: 4.0,
            // Visualiseur incrusté par défaut (cf. `record --hud`).
            hud: true,
            hud_interval: 6.0,
            status: String::new(),
            child: None,
            launch_requested: false,
        }
    }
}

/// Chemin du binaire `record` : à côté de l'exécutable courant (cas `cargo run` →
/// `target/debug/record`), sinon on s'en remet au `PATH`.
fn record_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(if cfg!(windows) {
            "record.exe"
        } else {
            "record"
        });
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("record")
}

/// La section « Enregistrement », rendue dans le panneau de droite (item dock). Ne
/// fait que lire/écrire son propre état et poser `launch_requested` ; le lancement et
/// le suivi vivent dans [`drive_recorder`].
pub(crate) fn recorder_section(ui: &mut egui::Ui, panel: &mut RecorderPanel) {
    let recording = panel.child.is_some();
    ui.small(
        "Ré-exécute le scénario courant à frais (rendu headless propre, \
         sans cette interface) et l'encode en vidéo via ffmpeg.",
    );
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Fichier :");
        ui.text_edit_singleline(&mut panel.out);
    });
    ui.add(egui::Slider::new(&mut panel.seconds, 1.0..=120.0).text("Durée (s)"));
    ui.add(egui::Slider::new(&mut panel.fps, 24.0..=60.0).text("FPS"));
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut panel.width).range(320..=3840))
            .on_hover_text("Largeur (px)");
        ui.label("×");
        ui.add(egui::DragValue::new(&mut panel.height).range(240..=2160))
            .on_hover_text("Hauteur (px)");
    });

    ui.separator();
    // Sélection automatique : montrer en continu les rayons d'un agent dans la vidéo,
    // sans intervention. Le « mode de roulement » choisit comment l'agent mis en avant
    // change au fil du temps.
    ui.horizontal(|ui| {
        ui.label("Sélection auto :");
        egui::ComboBox::from_id_salt("rec_select")
            .selected_text(panel.select.label())
            .show_ui(ui, |ui| {
                for m in SelectionRoll::ALL {
                    ui.selectable_value(&mut panel.select, m, m.label());
                }
            });
    })
    .response
    .on_hover_text(
        "Garde un agent mobile mis en avant pendant la vidéo (anneau + rayons de vision), \
         pour que les spectateurs voient les raycasts. « Aucune » = vidéo inchangée.",
    );
    if panel.select.rolls() {
        ui.add(egui::Slider::new(&mut panel.select_interval, 0.5..=30.0).text("intervalle (s)"))
            .on_hover_text("Temps entre deux changements d'agent mis en avant.");
    }

    ui.separator();
    // Visualiseur natif incrusté : compose la vidéo en 9:16 (arène + stats/courbes/
    // inspecteur), identique au mode présentation (F1) du fenêtré.
    ui.checkbox(
        &mut panel.hud,
        "HUD incrusté (stats / courbes / inspecteur)",
    )
    .on_hover_text(
        "Compose la vidéo en 9:16 : arène en haut, visualiseur natif en bas (identique \
             au mode présentation F1). Décoché : vidéo de la seule arène (choisir alors \
             1080×1080).",
    );
    if panel.hud {
        ui.add(
            egui::Slider::new(&mut panel.hud_interval, 1.0..=30.0).text("rotation sections (s)"),
        )
        .on_hover_text("Temps entre deux sections affichées (courbes ↔ inspecteur).");
    }

    ui.separator();
    if recording {
        ui.add_enabled(false, egui::Button::new("⏺ Enregistrement…"));
        ui.spinner();
    } else if ui
        .button("⏺ Lancer l'enregistrement")
        .on_hover_text("Lance le binaire headless `record` en sous-processus.")
        .clicked()
    {
        panel.launch_requested = true;
    }

    if !panel.status.is_empty() {
        ui.weak(&panel.status);
    }
}

/// `Update` : surveille la fin du process `record` et, si l'UI l'a demandé,
/// écrit le `SimConfig` courant dans un fichier temporaire puis lance `record`
/// dessus. Pas de logique de sim — de l'orchestration de process.
pub fn drive_recorder(mut panel: ResMut<RecorderPanel>, config: Res<SimConfig>) {
    // Suivi du process en cours : on relève sa fin sans bloquer (`try_wait`).
    if panel.child.is_some() {
        match panel.child.as_mut().unwrap().try_wait() {
            Ok(Some(status)) => {
                panel.child = None;
                panel.status = if status.success() {
                    format!("Vidéo écrite → {}", panel.out)
                } else {
                    format!("record a échoué ({status}). Voir la console.")
                };
            }
            Ok(None) => {} // toujours en cours
            Err(e) => {
                panel.child = None;
                panel.status = format!("Suivi du process impossible : {e}");
            }
        }
    }

    // Lancement demandé et rien en cours : on écrit le scénario courant puis on
    // lance `record`. On n'autorise qu'un enregistrement à la fois.
    if !panel.launch_requested || panel.child.is_some() {
        return;
    }
    panel.launch_requested = false;

    // Le scénario courant (édits de l'éditeur compris), capturé dans un RON
    // temporaire pour que `record` re-render exactement ce qu'on voit configuré.
    let scenario = std::env::temp_dir().join("teemlab_record_scenario.ron");
    if let Err(e) = config.save_ron_file(&scenario) {
        panel.status = format!("Échec d'écriture du scénario temporaire : {e}");
        return;
    }

    let (out, fps, seconds, width, height) = (
        panel.out.clone(),
        panel.fps,
        panel.seconds,
        panel.width,
        panel.height,
    );
    // Sélection automatique + HUD passés en arguments (réglages de rendu, pas du scénario)
    // → `record` les pilote sans toucher au RON temporaire.
    let (select, select_interval) = (panel.select.cli(), panel.select_interval.to_string());
    let hud_interval = panel.hud_interval.to_string();
    let mut cmd = Command::new(record_binary());
    cmd.arg(&scenario).args([
        "--out",
        &out,
        "--fps",
        &fps.to_string(),
        "--seconds",
        &seconds.to_string(),
        "--width",
        &width.to_string(),
        "--height",
        &height.to_string(),
        "--select",
        select,
        "--select-interval",
        &select_interval,
        "--hud-interval",
        &hud_interval,
    ]);
    // HUD activé par défaut côté `record` : on ne passe `--no-hud` que s'il est décoché.
    if !panel.hud {
        cmd.arg("--no-hud");
    }
    match cmd.spawn() {
        Ok(child) => {
            panel.child = Some(child);
            panel.status = format!("Enregistrement en cours → {out}");
        }
        Err(e) => {
            panel.status =
                format!("Lancement impossible ({e}). `record` et `ffmpeg` sont-ils présents ?");
        }
    }
}
