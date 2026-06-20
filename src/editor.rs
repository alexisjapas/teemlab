//! Éditeur du build fenêtré : **placement manuel** par drag-and-drop (item 4).
//!
//! Module du *binaire* fenêtré uniquement (jamais compilé dans le headless) :
//! tout ce qui touche egui, la caméra ou la fenêtre vit ici, à l'écart du cœur
//! render-agnostic. On respecte l'invariant cardinal — c'est de l'**édition
//! manuelle** déclenchée par l'utilisateur (comme retoucher le scénario à la
//! main), pas de la logique de simulation : ça peut donc vivre hors de
//! `FixedUpdate`. Les entités créées rejoignent ensuite la boucle de sim
//! normalement.
//!
//! Disposition : des **fenêtres flottantes** au-dessus de la sim plein cadre —
//! **sélecteur** d'archétypes (on y pioche par glisser-déposer), **éditeur** de
//! l'archétype choisi, et **statistiques** ([`stats_ui`]).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::{BrainKind, MlpBrain};
use teemlab::components::{Agent, Food, Reserve, Species};
use teemlab::config::{Archetype, ArchetypeKind, Relation};
use teemlab::ecology::spawn_food;
use teemlab::genotype::{Genotype, TRAITS};
use teemlab::spawn::spawn_agent;

/// La palette / l'état de l'éditeur. La **liste d'archétypes** vit désormais dans
/// [`SimConfig::archetypes`] (la donnée centrale) ; la palette ne garde que l'état
/// d'interaction. Éditer un archétype écrit donc *directement* dans le `SimConfig`,
/// sans copie ni passe de synchro.
#[derive(Resource, Default)]
pub struct Palette {
    /// Index (dans `config.archetypes`) de l'archétype actuellement glissé.
    pub dragging: Option<usize>,
    /// Index de l'archétype sélectionné pour édition.
    pub selected: Option<usize>,
    /// Graine roulante pour donner un flux distinct au cerveau de chaque agent
    /// posé à la main.
    pub next_seed: u64,
    /// Chemin de sauvegarde/chargement RON.
    pub save_path: String,
    /// Dernier message d'état (sauvegarde/chargement).
    pub status: String,
    /// Bibliothèque d'espèces : fichiers `species/*.ron` trouvés au dernier scan.
    pub species_files: Vec<String>,
    /// Index, dans [`species_files`](Self::species_files), de l'espèce choisie pour l'import.
    pub species_selected: Option<usize>,
}

/// Couleur d'une espèce, en `egui::Color32`, **dérivée** de l'unique palette du
/// rendu ([`teemlab::visuals::species_color`]) plutôt que recopiée : courbes du HUD,
/// pastilles de la palette et entités à l'écran ne peuvent plus diverger. Partagée
/// avec le HUD (item 10).
pub(crate) fn species_color32(species: u16) -> egui::Color32 {
    let c = teemlab::visuals::species_color(Species(species));
    let q = |channel: f32| (channel * 255.0).round() as u8;
    egui::Color32::from_rgb(q(c.red), q(c.green), q(c.blue))
}

/// Couleur egui d'un archétype, depuis sa couleur stockée (`[r, g, b]` ∈ [0, 1]).
fn archetype_color32(a: &Archetype) -> egui::Color32 {
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(a.color[0]), q(a.color[1]), q(a.color[2]))
}

/// Construit la palette au `Startup`, après l'insertion de [`SimConfig`] par le
/// plugin de sim.
pub fn build_palette(mut commands: Commands, config: Res<SimConfig>) {
    commands.insert_resource(Palette {
        dragging: None,
        selected: None,
        next_seed: config.seed ^ 0xED17,
        save_path: "scenarios/edited.ron".to_string(),
        status: String::new(),
        species_files: scan_species(),
        species_selected: None,
    });
}

/// Liste les espèces de la bibliothèque (`species/*.ron`), triées. Miroir du scan de
/// scénarios ([`crate::runs`]) ; un dossier absent donne simplement une liste vide.
fn scan_species() -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir("species") {
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

/// Résolution du glisser-déposer d'un archétype dans l'aire de jeu. Système
/// **distinct**, ordonné après tous les panneaux egui : `is_pointer_over_area`
/// connaît alors toutes les arêtes, sinon un dépôt au-dessus d'un panneau (bas ou
/// gauche) poserait une entité cachée sous l'UI. `viewport_to_world_2d` tient
/// compte de l'offset du viewport (sim centrée, cf. `set_sim_camera`) → le
/// curseur fenêtre reste la bonne entrée.
pub fn resolve_drag(
    mut contexts: EguiContexts,
    mut palette: ResMut<Palette>,
    mut commands: Commands,
    config: Res<SimConfig>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
) -> Result {
    let Some(i) = palette.dragging else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    if ctx.input(|input| input.pointer.any_released()) {
        // Déposé hors de tout panneau egui = au-dessus de l'aire de jeu.
        // Chaîne de `let` (edition 2024) : caméra, fenêtre, curseur, monde.
        if !ctx.is_pointer_over_area()
            && let Ok((camera, cam_tf)) = cameras.single()
            && let Ok(window) = windows.single()
            && let Some(cursor) = window.cursor_position()
            && let Ok(world) = camera.viewport_to_world_2d(cam_tf, cursor)
        {
            place(&mut commands, &config, &mut palette, i, world);
        }
        palette.dragging = None;
    }

    Ok(())
}

/// Section « sélecteur » : la liste des **archétypes du scénario** (glisser pour
/// poser, cliquer pour éditer), plus la création (agent / nourriture) et la
/// suppression. La liste *est* [`SimConfig::archetypes`] — créer ou supprimer ici
/// modifie donc le scénario directement.
pub(crate) fn selector_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    ui.label("Glisse dans l'aire pour poser ; clique pour éditer ; Suppr (curseur sur une entité) pour retirer.");
    ui.separator();
    let mut started = None;
    let mut clicked = None;
    for (i, arch) in config.archetypes.iter().enumerate() {
        let mark = if palette.selected == Some(i) {
            "▶ "
        } else {
            "⬤ "
        };
        let suffix = if arch.is_food() { " · nourriture" } else { "" };
        let label = egui::RichText::new(format!("{mark}{}{suffix}", arch.name))
            .color(archetype_color32(arch));
        let resp = ui.add_sized(
            [ui.available_width(), 28.0],
            egui::Button::new(label).sense(egui::Sense::click_and_drag()),
        );
        if resp.drag_started() {
            started = Some(i);
        }
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    if started.is_some() {
        palette.dragging = started;
    }
    if clicked.is_some() {
        palette.selected = clicked;
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("＋ Agent").clicked() {
            config
                .archetypes
                .push(Archetype::new_agent(config.archetypes.len()));
            palette.selected = Some(config.archetypes.len() - 1);
        }
        if ui.button("＋ Nourriture").clicked() {
            config
                .archetypes
                .push(Archetype::new_food(config.archetypes.len()));
            palette.selected = Some(config.archetypes.len() - 1);
        }
    });

    // Opérations sur l'archétype **sélectionné** : dupliquer et réordonner (comme la
    // suppression plus bas, elles agissent sur la sélection).
    if let Some(i) = palette.selected {
        let count = config.archetypes.len();
        ui.horizontal(|ui| {
            if ui
                .button("⧉ Dupliquer")
                .on_hover_text(
                    "Clone l'archétype sélectionné en fin de liste (relations non copiées).",
                )
                .clicked()
            {
                palette.selected = duplicate_archetype(config, i);
            }
            // Réordonner = échanger avec le voisin + transposer les index de relations.
            // Comme la suppression, le changement de structure prend pleinement effet à
            // la (ré)génération du monde (⟲ du bandeau) ; les relations, elles, sont
            // corrigées tout de suite (le scénario sauvé reste juste).
            if ui
                .add_enabled(i > 0, egui::Button::new("▲ Monter"))
                .on_hover_text("Échange avec l'archétype précédent (remappe les relations).")
                .clicked()
            {
                swap_archetypes(config, i, i - 1);
                palette.selected = Some(i - 1);
            }
            if ui
                .add_enabled(i + 1 < count, egui::Button::new("▼ Descendre"))
                .on_hover_text("Échange avec l'archétype suivant (remappe les relations).")
                .clicked()
            {
                swap_archetypes(config, i, i + 1);
                palette.selected = Some(i + 1);
            }
        });
    }

    if let Some(i) = palette.selected
        && config.archetypes.len() > 1
        && ui
            .button("🗑 Supprimer l'archétype sélectionné")
            .on_hover_text("Retire l'archétype et remappe la table de relations.")
            .clicked()
    {
        remove_archetype(config, i);
        palette.selected = None;
    }

    ui.separator();
    species_library_section(ui, palette, config);

    if palette.dragging.is_some() {
        ui.separator();
        ui.weak("Relâche au-dessus de l'aire pour déposer.");
    }
}

/// **Bibliothèque d'espèces** (item 4) : rendre une espèce réutilisable hors scénario.
///
/// - **Exporter** l'archétype sélectionné vers `species/<nom>.ron` (`source` effacé : le
///   fichier *est* la source).
/// - **Importer** une espèce : une **copie** rejoint le scénario (qui reste autonome,
///   §9), en retenant le fichier comme `source`.
/// - **Synchroniser** un archétype importé : recharge sa définition depuis `source`, en
///   gardant l'effectif local. Le choix « copie + lien de resynchro » de l'item 4.
fn species_library_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Bibliothèque d'espèces")
        .default_open(false)
        .show(ui, |ui| {
            // Export / synchro de l'archétype sélectionné.
            if let Some(i) = palette.selected.filter(|&i| i < config.archetypes.len()) {
                if ui
                    .button("📤 Exporter la sélection → species/")
                    .on_hover_text("Sauve l'archétype comme espèce réutilisable.")
                    .clicked()
                {
                    palette.status = export_species(&config.archetypes[i]);
                    palette.species_files = scan_species();
                }
                if let Some(src) = config.archetypes[i].source.clone()
                    && ui
                        .button("↻ Synchroniser depuis la source")
                        .on_hover_text(format!(
                            "Recharge la définition depuis {src} (garde l'effectif local)."
                        ))
                        .clicked()
                {
                    palette.status = sync_species(config, i);
                }
            } else {
                ui.weak("Sélectionne un archétype pour l'exporter / le synchroniser.");
            }

            ui.separator();
            // Import : combo des `species/*.ron`. On travaille sur des copies locales
            // pour ne pas emprunter `palette` à la fois en lecture (liste) et en écriture
            // (sélection) dans la fermeture du combo (cf. `crate::runs`).
            let files = palette.species_files.clone();
            let mut sel = palette.species_selected;
            let mut rescan = false;
            let mut to_import = None;
            ui.horizontal(|ui| {
                let label = sel
                    .and_then(|j| files.get(j))
                    .map(String::as_str)
                    .unwrap_or("(choisir une espèce…)");
                egui::ComboBox::from_id_salt("species_import")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (j, path) in files.iter().enumerate() {
                            ui.selectable_value(&mut sel, Some(j), path);
                        }
                    });
                if ui.button("↻").on_hover_text("Rescanner species/").clicked() {
                    rescan = true;
                }
            });
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("📥 Importer (copie)"))
                .on_hover_text(
                    "Ajoute une COPIE de l'espèce au scénario (réimporter pour resynchroniser).",
                )
                .clicked()
                && let Some(path) = sel.and_then(|j| files.get(j))
            {
                to_import = Some(path.clone());
            }

            palette.species_selected = sel;
            if rescan {
                palette.species_files = scan_species();
                palette.species_selected = None;
            }
            if let Some(path) = to_import {
                palette.status = import_species(config, &path);
                palette.selected = Some(config.archetypes.len().saturating_sub(1));
            }

            if !palette.status.is_empty() {
                ui.weak(&palette.status);
            }
        });
}

/// Retire l'archétype `i` et **remappe la table de relations** : les relations qui le
/// référencent (acteur ou cible) sont retirées, et tout index supérieur est décrémenté
/// — sinon les indices d'archétype pointeraient vers la mauvaise espèce.
fn remove_archetype(config: &mut SimConfig, i: usize) {
    config.archetypes.remove(i);
    let removed = i as u16;
    config
        .relations
        .retain(|r| r.actor != removed && r.target != removed);
    for r in &mut config.relations {
        if r.actor > removed {
            r.actor -= 1;
        }
        if r.target > removed {
            r.target -= 1;
        }
    }
}

/// Duplique l'archétype `i` : un clone **indépendant** ajouté **en fin** de liste —
/// donc sans décaler les index existants, et la table de relations reste intacte
/// (cf. [`swap_archetypes`] qui, lui, doit remapper). Le clone ne reprend **pas** les
/// relations de l'original (à recâbler à la main) ; son nom gagne « (copie) ».
/// Renvoie l'index du clone pour le sélectionner. `None` si `i` est hors-liste.
fn duplicate_archetype(config: &mut SimConfig, i: usize) -> Option<usize> {
    let mut clone = config.archetypes.get(i)?.clone();
    clone.name = format!("{} (copie)", clone.name);
    config.archetypes.push(clone);
    Some(config.archetypes.len() - 1)
}

/// Échange les archétypes `i` et `j` (réordonnancement) et **transpose** leurs index
/// dans la table de relations : l'index d'un archétype *est* son identité d'espèce
/// ([`Species`]), donc échanger deux archétypes sans toucher aux relations les ferait
/// pointer vers la mauvaise espèce. Pendant exact, pour le réordonnancement, du remap
/// que [`remove_archetype`] fait pour la suppression.
fn swap_archetypes(config: &mut SimConfig, i: usize, j: usize) {
    config.archetypes.swap(i, j);
    let (i, j) = (i as u16, j as u16);
    let transpose = |x: &mut u16| {
        if *x == i {
            *x = j;
        } else if *x == j {
            *x = i;
        }
    };
    for r in &mut config.relations {
        transpose(&mut r.actor);
        transpose(&mut r.target);
    }
}

/// Exporte un archétype comme **espèce réutilisable** vers `species/<nom>.ron`. Le champ
/// `source` est effacé : le fichier exporté *est* la source (pas d'auto-référence si on
/// ré-exporte une espèce importée). Renvoie un message d'état pour l'UI.
fn export_species(arch: &Archetype) -> String {
    let _ = std::fs::create_dir_all("species");
    let path = format!("species/{}.ron", sanitize_filename(&arch.name));
    let mut def = arch.clone();
    def.source = None;
    match def.save_ron_file(&path) {
        Ok(()) => format!("Espèce exportée → {path}"),
        Err(e) => format!("Échec export : {e}"),
    }
}

/// Importe une espèce : une **copie** rejoint le scénario (qui reste autonome, §9), en
/// retenant le fichier comme `source` (pour la resynchro). Ajoutée en fin de liste, donc
/// sans décaler les index de relations. Renvoie un message d'état.
fn import_species(config: &mut SimConfig, path: &str) -> String {
    match Archetype::from_ron_file(path) {
        Ok(mut arch) => {
            arch.source = Some(path.to_string());
            config.archetypes.push(arch);
            format!("Espèce importée (copie) ← {path}")
        }
        Err(e) => format!("Échec import : {e}"),
    }
}

/// Resynchronise l'archétype `i` depuis son fichier `source` : recharge la définition et
/// la réapplique en **gardant l'effectif local** (`count`). Renvoie un message d'état.
fn sync_species(config: &mut SimConfig, i: usize) -> String {
    let Some(src) = config.archetypes[i].source.clone() else {
        return "Cet archétype n'a pas de source à synchroniser.".to_string();
    };
    match Archetype::from_ron_file(&src) {
        Ok(loaded) => {
            merge_species_def(&mut config.archetypes[i], loaded, src.clone());
            format!("Espèce resynchronisée ← {src}")
        }
        Err(e) => format!("Échec synchro : {e}"),
    }
}

/// Réapplique une définition d'espèce `loaded` sur `target`, en **préservant l'effectif**
/// (`count`, propre au scénario) et en re-fixant le lien `source`. Le reste (corps,
/// cerveau, couleur, nom, mutabilité) vient de la définition. Pur (sans I/O) → testable ;
/// [`sync_species`] n'y ajoute que la lecture du fichier.
fn merge_species_def(target: &mut Archetype, loaded: Archetype, source: String) {
    let count = target.count;
    *target = loaded;
    target.count = count;
    target.source = Some(source);
}

/// Nettoie un nom d'espèce en un nom de fichier sûr : on garde lettres/chiffres, `-` et
/// `_`, tout le reste devient `_` ; un nom vide retombe sur « espece ».
fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "espece".to_string()
    } else {
        s
    }
}

/// Section « éditeur » : édition des gènes de l'archétype sélectionné + save/load
/// RON. Rendue sous le sélecteur. Rend explicite la distinction **archétype** (le
/// modèle édité ici) / **génome** (la copie héritée par chaque instance, qui mute
/// ensuite seule).
pub(crate) fn editor_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    match palette.selected {
        Some(i) if i < config.archetypes.len() => archetype_editor(ui, config, i),
        _ => {
            ui.label("Clique un archétype dans la palette pour l'éditer (ou crée-en un).");
        }
    }

    ui.separator();
    ui.label("Scénario (RON)");
    ui.text_edit_singleline(&mut palette.save_path);
    ui.horizontal(|ui| {
        if ui.button("💾 Sauver").clicked() {
            let path = palette.save_path.clone();
            palette.status = match config.save_ron_file(&path) {
                Ok(()) => format!("Sauvé → {path}"),
                Err(e) => format!("Échec : {e}"),
            };
        }
        if ui.button("📂 Charger").clicked() {
            let path = palette.save_path.clone();
            palette.status = match SimConfig::from_ron_file(&path) {
                Ok(loaded) => {
                    *config = loaded;
                    palette.selected = None;
                    palette.dragging = None;
                    format!("Chargé ← {path}")
                }
                Err(e) => format!("Échec : {e}"),
            };
        }
    });
    if !palette.status.is_empty() {
        ui.weak(&palette.status);
    }
}

/// Éditeur de **l'archétype `i`** : propriétés communes (nom, couleur, effectif,
/// taille, réserve) puis ce qui dépend du type — gènes + mutabilité (par espèce) +
/// cerveau pour un agent, repousse pour une nourriture. Écrit *directement* dans
/// `config.archetypes[i]` (persisté par « Sauver »).
fn archetype_editor(ui: &mut egui::Ui, config: &mut SimConfig, i: usize) {
    // Bornes (globales) capturées avant d'emprunter `config.archetypes` en mutable.
    let trait_bounds: Vec<_> = TRAITS.iter().map(|t| (t.bounds)(config)).collect();
    let arch = &mut config.archetypes[i];

    ui.horizontal(|ui| {
        ui.label("nom :");
        ui.text_edit_singleline(&mut arch.name);
    });
    ui.horizontal(|ui| {
        ui.label("couleur :");
        ui.color_edit_button_rgb(&mut arch.color);
    });
    ui.add(
        egui::DragValue::new(&mut arch.count)
            .range(0..=5000)
            .prefix("effectif au spawn (reset) : "),
    );
    ui.add(egui::Slider::new(&mut arch.radius, 2.0..=30.0).text("rayon du corps"));
    ui.add(egui::Slider::new(&mut arch.reserve_max, 10.0..=500.0).text("réserve max"));

    match &mut arch.kind {
        ArchetypeKind::Agent {
            genotype,
            brain,
            mutable,
        } => {
            ui.separator();
            ui.strong("Gènes (l'archétype)");
            ui.small(
                "Chaque agent posé reçoit une COPIE de ces gènes — son génome — qui mute \
                 ensuite seule. La case « mutable » gouverne, PAR ESPÈCE, le droit de muter.",
            );
            // Une seule boucle sur TRAITS : slider (valeur, bornes) + case « mutable »
            // par gène. Ajouter un trait n'ajoute pas une ligne ici (item 15).
            for (t, bounds) in TRAITS.iter().zip(&trait_bounds) {
                let mut value = (t.get)(genotype);
                if ui
                    .add(egui::Slider::new(&mut value, bounds.min..=bounds.max).text(t.name))
                    .changed()
                {
                    (t.set)(genotype, value);
                }
                let mut m = (t.mutable)(mutable);
                if ui
                    .checkbox(&mut m, "mutable")
                    .on_hover_text(
                        "Coché : ce gène mute à la reproduction (il dérive et se transmet \
                         avec variation). Décoché : il est quand même transmis, mais figé \
                         sur la valeur du fondateur.",
                    )
                    .changed()
                {
                    (t.set_mutable)(mutable, m);
                }
            }
            ui.separator();
            ui.strong("Cerveau (auteur de la décision)");
            brain_kind_editor(ui, brain, genotype.ray_count());
        }
        ArchetypeKind::Food { regen } => {
            ui.separator();
            ui.strong("Nourriture (source sessile)");
            ui.add(egui::Slider::new(regen, 0.0..=50.0).text("repousse/s"))
                .on_hover_text(
                    "Sources repoussées par seconde (0 = maintien instantané à l'effectif).",
                );
        }
    }
}

/// Édite **un** [`BrainKind`] : combo de type (sélection par *kind*, pour ne pas
/// réinitialiser les paramètres du variant courant), paramètres propres au variant,
/// puis sa description fonctionnelle. Le `match` exhaustif force tout futur variant
/// de `Brain` à exposer ses paramètres ici — la contrepartie *hétérogène* (des
/// paramètres propres à chaque cerveau) de la table `TRAITS`, elle homogène.
///
/// `vision_rays` ne sert qu'au MLP, pour afficher la taille (contrainte) de sa couche
/// d'entrée.
fn brain_kind_editor(ui: &mut egui::Ui, kind: &mut BrainKind, vision_rays: usize) {
    egui::ComboBox::from_label("type")
        .selected_text(kind.name())
        .show_ui(ui, |ui| {
            let is_wander = matches!(kind, BrainKind::Wander { .. });
            if ui.selectable_label(is_wander, "Errance").clicked() && !is_wander {
                *kind = BrainKind::default();
            }
            let is_hunter = matches!(kind, BrainKind::Hunter);
            if ui.selectable_label(is_hunter, "Chasseur").clicked() && !is_hunter {
                *kind = BrainKind::Hunter;
            }
            let is_sessile = matches!(kind, BrainKind::Sessile);
            if ui.selectable_label(is_sessile, "Sessile").clicked() && !is_sessile {
                *kind = BrainKind::Sessile;
            }
            let is_mlp = matches!(kind, BrainKind::Mlp { .. });
            if ui.selectable_label(is_mlp, "Réseau (MLP)").clicked() && !is_mlp {
                *kind = BrainKind::Mlp { hidden: vec![8] };
            }
        });
    match kind {
        BrainKind::Wander { turn_rate } => {
            ui.add(egui::Slider::new(turn_rate, 0.0..=1.0).text("vivacité du virage"))
                .on_hover_text("Amplitude max de la dérive de cap à chaque tick (rad).");
        }
        BrainKind::Hunter | BrainKind::Sessile => {}
        BrainKind::Mlp { hidden } => mlp_architecture_editor(ui, hidden, vision_rays),
    }
    ui.weak(kind.description());
}

/// Édition **numérique** de l'architecture d'un MLP (item 18b, cœur) : le nombre de
/// couches cachées et la largeur de chacune. L'entrée (`3 × rayons` : vision, cible,
/// menace) et la sortie (2) sont *contraintes* par le contrat et seulement affichées.
fn mlp_architecture_editor(ui: &mut egui::Ui, hidden: &mut Vec<usize>, vision_rays: usize) {
    ui.small(format!(
        "Entrée {} au fondateur (= 3 × {vision_rays} rayons : vision, cible, menace) → \
         sortie {} (contrat). La couche d'entrée s'adapte ensuite à la précision \
         visuelle de chaque individu (gène « Rayons »).",
        MlpBrain::input_size(vision_rays),
        MlpBrain::OUTPUTS,
    ));
    let mut remove = None;
    for (i, n) in hidden.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("cachée {i}"));
            ui.add(egui::DragValue::new(n));
            *n = (*n).clamp(1, 64); // au moins un neurone, plafond raisonnable.
            if ui
                .small_button("✕")
                .on_hover_text("retirer cette couche")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        hidden.remove(i); // 0 couche cachée = perceptron simple, valide.
    }
    if ui.button("+ couche cachée").clicked() {
        hidden.push(8);
    }

    // Aperçu structurel du réseau (item 18b-viz) : entrée → cachées → sortie. Pas
    // d'activations ici (on édite un *type*, pas un cerveau vivant) — c'est
    // l'inspecteur qui montre le réseau en action.
    let mut sizes = vec![MlpBrain::input_size(vision_rays)];
    sizes.extend_from_slice(hidden);
    sizes.push(MlpBrain::OUTPUTS);
    draw_mlp_graph(ui, &sizes, None, None);
}

/// Couleur d'un nœud selon son activation `v` : froid (bleu) pour négatif, chaud
/// (orange) pour positif, gris foncé au repos — l'échelle naturelle d'un `tanh`
/// (∈ [-1, 1] ; l'entrée ∈ [0, 1] tombe côté chaud). `None` → nœud neutre (aperçu
/// structurel, sans activation).
fn activation_color(v: Option<f32>) -> egui::Color32 {
    let Some(v) = v else {
        return egui::Color32::from_gray(110);
    };
    let t = v.clamp(-1.0, 1.0).abs();
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    let base = 60; // gris de repos
    if v >= 0.0 {
        egui::Color32::from_rgb(lerp(base, 240), lerp(base, 150), lerp(base, 40)) // chaud
    } else {
        egui::Color32::from_rgb(lerp(base, 60), lerp(base, 140), lerp(base, 240)) // froid
    }
}

/// Dessine un MLP en **graphe** (item 18b-viz) : une colonne de nœuds par couche
/// (entrée à gauche, sortie à droite), arêtes entre couches consécutives.
///
/// - `brain = Some(mlp)` (inspecteur) : arêtes teintées par le **signe/intensité du
///   poids** (bleu négatif, orange positif) — le réseau réel. Avec `activations`
///   (calculées à la demande par [`MlpBrain::forward_activations`]), les nœuds se
///   colorent par leur **activation courante** : le réseau « en action ».
/// - `brain = None` (éditeur) : aperçu structurel, nœuds neutres et arêtes ténues.
///
/// `sizes` donne le nombre de nœuds par colonne (entrée incluse). Séparer poids
/// (`brain`) et activations (`activations`) permet à `MlpBrain::think` de ne plus
/// porter d'état transitoire : c'est l'inspecteur qui rejoue la propagation.
pub(crate) fn draw_mlp_graph(
    ui: &mut egui::Ui,
    sizes: &[usize],
    brain: Option<&MlpBrain>,
    activations: Option<&[Vec<f32>]>,
) {
    use egui::{Pos2, Sense, Stroke, vec2};
    if sizes.len() < 2 {
        return;
    }
    let cols = sizes.len();
    let widest = *sizes.iter().max().unwrap_or(&1);
    // Hauteur proportionnée à la colonne la plus large, bornée pour rester compacte.
    let height = (widest as f32 * 16.0).clamp(60.0, 220.0);
    let width = ui.available_width().max(180.0);
    let (resp, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    // Marges horizontales réservées aux étiquettes d'entrée (gauche) et de sortie
    // (droite) ; petites marges verticales. Les colonnes de nœuds vivent dans `rect`.
    const LABEL_W: f32 = 42.0;
    let rect = egui::Rect::from_min_max(
        egui::pos2(resp.rect.left() + LABEL_W, resp.rect.top() + 8.0),
        egui::pos2(resp.rect.right() - LABEL_W, resp.rect.bottom() - 8.0),
    );

    // Position du nœud `node` (0-based) de la colonne `col`.
    let pos = |col: usize, node: usize, n: usize| -> Pos2 {
        let x = if cols == 1 {
            rect.center().x
        } else {
            rect.left() + rect.width() * col as f32 / (cols - 1) as f32
        };
        let y = if n == 1 {
            rect.center().y
        } else {
            rect.top() + rect.height() * (node as f32 + 0.5) / n as f32
        };
        Pos2::new(x, y)
    };

    // Arêtes d'abord (sous les nœuds). En mode live, teinte/épaisseur par poids.
    for col in 0..cols - 1 {
        let (from_n, to_n) = (sizes[col], sizes[col + 1]);
        let weights = brain.and_then(|m| (col < m.weight_layers()).then(|| m.layer_weights(col)));
        for o in 0..to_n {
            for i in 0..from_n {
                let stroke = match weights {
                    Some((w, fan_in, _)) if i + o * fan_in < w.len() => {
                        let wt = w[o * fan_in + i];
                        let a = (wt.abs() * 0.9).clamp(0.04, 0.9);
                        let c = if wt >= 0.0 {
                            egui::Color32::from_rgb(230, 150, 60)
                        } else {
                            egui::Color32::from_rgb(70, 140, 230)
                        };
                        Stroke::new(1.0, c.gamma_multiply(a))
                    }
                    _ => Stroke::new(0.5, egui::Color32::from_gray(80)),
                };
                painter.line_segment([pos(col, i, from_n), pos(col + 1, o, to_n)], stroke);
            }
        }
    }

    // Nœuds par-dessus, colorés par activation (live) ou neutres (aperçu).
    let radius = (rect.height() / (widest as f32 * 2.2)).clamp(2.5, 8.0);
    for (col, &n) in sizes.iter().enumerate() {
        for node in 0..n {
            let act = activations
                .and_then(|a| a.get(col))
                .and_then(|layer| layer.get(node))
                .copied();
            let center = pos(col, node, n);
            painter.circle_filled(center, radius, activation_color(act));
            painter.circle_stroke(
                center,
                radius,
                Stroke::new(0.6, egui::Color32::from_gray(25)),
            );
        }
    }

    // Étiquettes des canaux d'entrée / sortie — « ce à quoi ça correspond », déduit
    // du contrat d'E/S du MLP : l'entrée concatène les canaux *vision* (obstacle),
    // *cible* puis *menace* (cf. `MlpBrain::input_vector`) ; la sortie est le pilotage
    // en repère du corps (avant, côté). Dessinées dans les marges réservées de part et
    // d'autre, à hauteur de chaque nœud concerné.
    let font = egui::FontId::monospace(8.0);
    let ink = egui::Color32::from_gray(165);
    let n_in = sizes[0];
    let rays = n_in / 3; // entrée = 3 × rayons (vision ++ cible ++ menace)
    for node in 0..n_in {
        let text = if n_in.is_multiple_of(3) {
            match node / rays {
                0 => format!("vis {node}"),
                1 => format!("cib {}", node - rays),
                _ => format!("men {}", node - 2 * rays),
            }
        } else {
            format!("in {node}")
        };
        let p = pos(0, node, n_in);
        painter.text(
            egui::pos2(rect.left() - 4.0, p.y),
            egui::Align2::RIGHT_CENTER,
            text,
            font.clone(),
            ink,
        );
    }
    let last = cols - 1;
    let n_out = sizes[last];
    for node in 0..n_out {
        let text = match (n_out == MlpBrain::OUTPUTS).then_some(node) {
            Some(0) => "avant",
            Some(1) => "côté",
            _ => continue, // sortie non standard : pas d'étiquette devinable.
        };
        let p = pos(last, node, n_out);
        painter.text(
            egui::pos2(rect.right() + 4.0, p.y),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            ink,
        );
    }
}

/// Section « Monde » : les paramètres de **scénario** — arène, population, économie
/// de nourriture, table d'interactions — par opposition à l'éditeur d'**archétype**
/// (le génotype d'une espèce). Lecture/écriture directe du [`SimConfig`], donc
/// persistée par « Sauver ». Certains champs ne prennent effet qu'à la (ré)génération
/// du monde (annotés *reset*) ; les autres — nourriture maintenue, relations — sont
/// lus en continu par la sim et agissent **à chaud**.
pub(crate) fn world_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    ui.small(
        "Paramètres globaux. (reset) = à la (ré)génération du monde (⟲ du bandeau) ; \
         les relations agissent à chaud. Population, corps et cerveaux vivent dans les \
         archétypes (panneau « Archétypes »).",
    );

    ui.separator();
    ui.strong("Monde");
    ui.add(egui::Slider::new(&mut config.arena_half_extent, 100.0..=1000.0).text("demi-arène"));
    ui.add(
        egui::DragValue::new(&mut config.tick_hz)
            .range(8.0..=240.0)
            .speed(1.0)
            .prefix("cadence sim (Hz, reset) : "),
    )
    .on_hover_text("Pas fixe de la simulation. Prend effet à la (ré)génération du monde (⟲).");
    ui.add(
        egui::DragValue::new(&mut config.seed)
            .speed(1.0)
            .prefix("graine (reset) : "),
    );

    ui.separator();
    gene_bounds_section(ui, config);

    ui.separator();
    relations_section(ui, config);
}

/// Les **bornes des gènes** (`*_bounds` du scénario), éditées globalement : min/max de
/// chaque caractéristique. Elles bornent la **mutation** (cf. [`Genotype::mutate`]) ET
/// les curseurs de l'éditeur d'archétype. Boucle sur [`TRAITS`] via `bounds_mut`, donc
/// ajouter un gène l'expose ici sans toucher cette section (item 15/3). Repliée par
/// défaut (réglage avancé, rarement touché — d'où « hors UI » jusqu'ici).
fn gene_bounds_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Bornes des gènes")
        .default_open(false)
        .show(ui, |ui| {
            ui.small(
                "Min/max de chaque gène : bornent la mutation et les curseurs de \
                 l'éditeur d'archétype. Globales (partagées par tous les archétypes).",
            );
            egui::Grid::new("gene_bounds_grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("gène");
                    ui.strong("min");
                    ui.strong("max");
                    ui.end_row();
                    for t in &TRAITS {
                        // Pas de glissé adapté à l'échelle du gène (fin pour l'agilité,
                        // grossier pour la vitesse), via ses décimales d'affichage.
                        let speed = 10f64.powi(-(t.decimals as i32));
                        let b = (t.bounds_mut)(config);
                        ui.label(t.name);
                        ui.add(egui::DragValue::new(&mut b.min).speed(speed));
                        ui.add(egui::DragValue::new(&mut b.max).speed(speed));
                        // Garde min ≤ max : un span négatif ferait paniquer le clamp de
                        // la mutation (`f32::clamp` exige min ≤ max).
                        b.max = b.max.max(b.min);
                        ui.end_row();
                    }
                });
        });
}

/// La table de relations, **adressée par archétype** : chaque acteur/cible se choisit
/// dans un menu d'archétypes (nom + couleur). Plus de numéros nus ni de collision
/// possible avec la nourriture — qui est un archétype à part entière, avec son index.
fn relations_section(ui: &mut egui::Ui, config: &mut SimConfig) {
    ui.strong("Relations (qui agit sur qui)");
    ui.small(
        "Un acteur réduit la réserve d'une cible à portée. C'est ce qui fait d'un \
         archétype une CIBLE (ce que poursuit Brain::Hunter). transfert = prédation \
         (l'acteur gagne l'énergie) ; sinon simple destruction.",
    );
    // Instantané (nom, couleur) des archétypes pour les menus — capturé avant
    // d'emprunter `config.relations` en mutable.
    let archs: Vec<(String, egui::Color32)> = config
        .archetypes
        .iter()
        .map(|a| (a.name.clone(), archetype_color32(a)))
        .collect();
    if archs.len() < 2 {
        ui.weak("Crée au moins deux archétypes pour définir une relation.");
        return;
    }
    let mut to_remove = None;
    for (i, rel) in config.relations.iter_mut().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            archetype_combo(ui, ("rel_actor", i), &mut rel.actor, &archs);
            ui.label("→");
            archetype_combo(ui, ("rel_target", i), &mut rel.target, &archs);
            if ui
                .button("🗑")
                .on_hover_text("Retirer cette relation")
                .clicked()
            {
                to_remove = Some(i);
            }
        });
        ui.checkbox(&mut rel.transfer, "transfert (prédation)");
        ui.add(egui::Slider::new(&mut rel.rate, 0.0..=400.0).text("débit/s"));
        ui.add(egui::Slider::new(&mut rel.range, 1.0..=100.0).text("portée"));
    }
    if let Some(i) = to_remove {
        config.relations.remove(i);
    }
    if ui.button("＋ Ajouter une relation").clicked() {
        // Défaut : le premier agent mange la première nourriture (le cas courant).
        let actor = config
            .archetypes
            .iter()
            .position(|a| a.is_agent())
            .unwrap_or(0) as u16;
        let target = config
            .archetypes
            .iter()
            .position(|a| a.is_food())
            .unwrap_or(0) as u16;
        config.relations.push(Relation {
            actor,
            target,
            transfer: true,
            rate: 100.0,
            range: 20.0,
        });
    }
}

/// Un menu déroulant qui choisit un **archétype** (par index `u16`) parmi `archs`,
/// affichant son nom dans sa couleur. `id` désambiguïse les combos d'une même frame.
fn archetype_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    value: &mut u16,
    archs: &[(String, egui::Color32)],
) {
    let cur = *value as usize;
    let selected_text = archs
        .get(cur)
        .map(|(n, _)| n.clone())
        .unwrap_or_else(|| format!("#{value}"));
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            for (i, (name, color)) in archs.iter().enumerate() {
                let label = egui::RichText::new(name).color(*color);
                if ui.selectable_label(cur == i, label).clicked() {
                    *value = i as u16;
                }
            }
        });
}

/// Statistiques en direct, rendues **à droite de la barre du haut** (dock fixe) par
/// [`crate::panels::top_bar`]. Lecture seule du monde : de l'observation pour
/// affichage, pas de la logique de sim. En `horizontal_wrapped` pour passer à la
/// ligne plutôt que déborder quand la fenêtre est étroite.
pub(crate) fn stats_section(
    ui: &mut egui::Ui,
    agents: &Query<(&Reserve, &Genotype), With<Agent>>,
    food: &Query<(), With<Food>>,
) {
    let population = agents.iter().count();
    let n = population.max(1) as f32;
    let mean_reserve = agents.iter().map(|(r, _)| r.current).sum::<f32>() / n;
    let food_count = food.iter().count();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Population : {population}"));
        ui.separator();
        ui.label(format!("Nourriture : {food_count}"));
        ui.separator();
        ui.label(format!("Réserve moy. : {mean_reserve:.0}"));
        ui.separator();
        ui.label("Gènes moy. —");
        // Une moyenne par caractéristique de TRAITS, sans champ codé en dur.
        for t in &TRAITS {
            let mean = agents.iter().map(|(_, g)| (t.get)(g)).sum::<f32>() / n;
            ui.label(format!("{} {:.*}", t.name, t.decimals as usize, mean));
        }
    });
}

/// Compile l'archétype `i` vers une entité vivante, posée en `world` : un agent
/// (génotype/cerveau de l'archétype) ou une source de nourriture, selon son type. Son
/// `Species` est son **index d'archétype** — l'identité que cible la table de relations.
fn place(
    commands: &mut Commands,
    config: &SimConfig,
    palette: &mut Palette,
    i: usize,
    world: Vec2,
) {
    let Some(arch) = config.archetypes.get(i) else {
        return;
    };
    let species = i as u16;
    match arch.kind {
        ArchetypeKind::Agent { .. } => {
            let seed = palette.next_seed;
            palette.next_seed = palette.next_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            spawn_agent(
                commands,
                config,
                config.genotype_of(species),
                Species(species),
                world,
                0.0,
                seed,
                config.reserve_max_of(species),
                0, // posé à la main : génération 0 (fondateur).
            );
        }
        ArchetypeKind::Food { .. } => spawn_food(commands, config, species, world),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper : une relation `actor → target` (débit/portée triviaux, sans intérêt ici).
    fn rel(actor: u16, target: u16) -> Relation {
        Relation {
            actor,
            target,
            transfer: true,
            rate: 1.0,
            range: 1.0,
        }
    }

    /// Réordonner échange deux archétypes ET **transpose** leurs index dans les
    /// relations (l'index EST l'identité d'espèce). En échangeant les archétypes 0 et 1 :
    /// une relation `0→2` devient `1→2`, et `1→0` devient `0→1` ; un index tiers (2) ne
    /// bouge pas.
    #[test]
    fn swap_transposes_relation_indices() {
        let mut config = SimConfig {
            archetypes: vec![
                Archetype::new_agent(0),
                Archetype::new_agent(1),
                Archetype::new_food(2),
            ],
            relations: vec![rel(0, 2), rel(1, 0)],
            ..SimConfig::default()
        };
        swap_archetypes(&mut config, 0, 1);
        // L'archétype jadis en 0 (« Espèce 0 ») est maintenant en 1, et inversement.
        assert_eq!(config.archetypes[0].name, "Espèce 1");
        assert_eq!(config.archetypes[1].name, "Espèce 0");
        // 0→2 ⇒ 1→2 (la cible tierce 2 est intacte) ; 1→0 ⇒ 0→1.
        assert_eq!(
            (config.relations[0].actor, config.relations[0].target),
            (1, 2)
        );
        assert_eq!(
            (config.relations[1].actor, config.relations[1].target),
            (0, 1)
        );
    }

    /// Dupliquer ajoute un clone **en fin** (sans décaler les index existants → relations
    /// intactes), de même corps que l'original, nommé « … (copie) ».
    #[test]
    fn duplicate_appends_a_clone_without_touching_relations() {
        let mut config = SimConfig {
            archetypes: vec![Archetype::new_agent(0), Archetype::new_food(1)],
            relations: vec![rel(0, 1)],
            ..SimConfig::default()
        };
        let new = duplicate_archetype(&mut config, 0).expect("clone d'un index valide");
        assert_eq!(new, 2, "le clone est ajouté en fin");
        assert_eq!(config.archetypes.len(), 3);
        assert_eq!(config.archetypes[2].name, "Espèce 0 (copie)");
        // Même corps que l'original (tout sauf le nom).
        assert_eq!(config.archetypes[2].kind, config.archetypes[0].kind);
        // Relations inchangées : le clone est en fin, aucun index n'a glissé.
        assert_eq!(config.relations.len(), 1);
        assert_eq!(
            (config.relations[0].actor, config.relations[0].target),
            (0, 1)
        );
    }

    /// Un index hors-liste ne duplique rien.
    #[test]
    fn duplicate_out_of_range_is_none() {
        let mut config = SimConfig::default();
        let n = config.archetypes.len();
        assert_eq!(duplicate_archetype(&mut config, 99), None);
        assert_eq!(config.archetypes.len(), n, "rien ajouté");
    }

    /// Resynchroniser une espèce importée **préserve l'effectif local** (`count`) et
    /// re-fixe le lien `source` ; tout le reste (corps, nom…) vient de la définition.
    #[test]
    fn merge_species_preserves_local_count_and_relinks() {
        let mut target = Archetype::new_agent(0);
        target.count = 50; // effectif propre à CE scénario
        target.name = "renommé localement".into();
        // La définition rechargée : autre corps (nourriture), autre nom, autre effectif.
        let mut loaded = Archetype::new_food(1);
        loaded.count = 7;
        loaded.name = "depuis le fichier".into();

        merge_species_def(&mut target, loaded, "species/x.ron".into());

        assert_eq!(target.count, 50, "l'effectif local est préservé");
        assert_eq!(
            target.name, "depuis le fichier",
            "le reste vient de la définition"
        );
        assert!(target.is_food(), "le corps vient de la définition");
        assert_eq!(
            target.source.as_deref(),
            Some("species/x.ron"),
            "le lien de resynchro est re-fixé"
        );
    }

    /// La sanitisation d'un nom d'espèce en nom de fichier : caractères sûrs gardés, le
    /// reste en `_`, un nom vide retombe sur un défaut.
    #[test]
    fn sanitize_filename_keeps_safe_chars() {
        assert_eq!(sanitize_filename("Loup gris/2"), "Loup_gris_2");
        assert_eq!(sanitize_filename("alpha-1_b"), "alpha-1_b");
        assert_eq!(sanitize_filename(""), "espece");
    }
}
