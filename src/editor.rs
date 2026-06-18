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
use teemlab::config::Relation;
use teemlab::ecology::spawn_food;
use teemlab::genotype::{Genotype, TRAITS};
use teemlab::spawn::spawn_agent;

/// Ce qu'un archétype produit une fois déposé. L'archétype est le *modèle*
/// éditable (item 5) ; le génome porté ici en est la valeur d'instance.
#[derive(Clone)]
pub enum ArchetypeKind {
    Agent { species: u16, genotype: Genotype },
    Food,
}

/// Une entrée « entité déjà définie » du panneau de droite.
#[derive(Clone)]
pub struct Archetype {
    pub name: String,
    pub kind: ArchetypeKind,
    pub color: egui::Color32,
}

/// La palette d'archétypes + l'état de l'éditeur.
#[derive(Resource, Default)]
pub struct Palette {
    pub items: Vec<Archetype>,
    /// Index de l'archétype actuellement glissé, le cas échéant.
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

/// Les archétypes déduits d'un scénario : une entrée par espèce d'agent (avec le
/// génotype fondateur, l'« archétype ») + la nourriture. Reconstruit aussi après
/// un chargement RON.
pub fn make_items(config: &SimConfig) -> Vec<Archetype> {
    let mut items = Vec::new();
    let base = Genotype::base(config);
    for species in 0..config.species_cardinality() {
        items.push(Archetype {
            name: format!("Agent · espèce {species}"),
            kind: ArchetypeKind::Agent {
                species,
                genotype: base,
            },
            color: species_color32(species),
        });
    }
    items.push(Archetype {
        name: "Nourriture".to_string(),
        kind: ArchetypeKind::Food,
        color: species_color32(config.food_species),
    });
    items
}

/// Construit la palette au `Startup`, après l'insertion de [`SimConfig`] par le
/// plugin de sim.
pub fn build_palette(mut commands: Commands, config: Res<SimConfig>) {
    commands.insert_resource(Palette {
        items: make_items(&config),
        dragging: None,
        selected: None,
        next_seed: config.seed ^ 0xED17,
        save_path: "scenarios/edited.ron".to_string(),
        status: String::new(),
    });
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

/// Section « sélecteur » : la liste des archétypes (glisser pour poser, cliquer
/// pour éditer). Rendue dans le panneau de gauche (item dock).
pub(crate) fn selector_section(ui: &mut egui::Ui, palette: &mut Palette) {
    ui.label("Glisse dans l'aire pour poser ; clique pour éditer ; Suppr (curseur sur une entité) pour retirer.");
    ui.separator();
    let mut started = None;
    let mut clicked = None;
    for (i, arch) in palette.items.iter().enumerate() {
        let mark = if palette.selected == Some(i) {
            "▶ "
        } else {
            "⬤ "
        };
        let label = egui::RichText::new(format!("{mark}{}", arch.name)).color(arch.color);
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
    if palette.dragging.is_some() {
        ui.separator();
        ui.weak("Relâche au-dessus de l'aire pour déposer.");
    }
}

/// Section « éditeur » : édition des gènes de l'archétype sélectionné + save/load
/// RON. Rendue sous le sélecteur. Rend explicite la distinction **archétype** (le
/// modèle édité ici) / **génome** (la copie héritée par chaque instance, qui mute
/// ensuite seule).
pub(crate) fn editor_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
    match palette.selected {
        Some(i) => {
            let is_agent = matches!(
                palette.items.get(i).map(|a| &a.kind),
                Some(ArchetypeKind::Agent { .. })
            );
            if is_agent {
                ui.label(format!("Édition : {}", palette.items[i].name));
                ui.small(
                    "Ces gènes sont l'ARCHÉTYPE (le modèle). Chaque agent posé \
                     en reçoit une COPIE — son génome — qui mute ensuite seule. \
                     L'évolution ne touche jamais l'archétype.",
                );
                ui.separator();
                if let Some(Archetype {
                    kind: ArchetypeKind::Agent { genotype, .. },
                    ..
                }) = palette.items.get_mut(i)
                {
                    // Une seule boucle sur TRAITS : slider (valeur, bornes) + case
                    // « mutable » par caractéristique. Ajouter un trait n'ajoute
                    // pas une ligne ici — c'est la falsification de l'item 15
                    // contre la pluralité de traits existante.
                    for t in &TRAITS {
                        let bounds = (t.bounds)(config);
                        let mut value = (t.get)(genotype);
                        if ui
                            .add(
                                egui::Slider::new(&mut value, bounds.min..=bounds.max).text(t.name),
                            )
                            .changed()
                        {
                            (t.set)(genotype, value);
                        }
                        let mut mutable = (t.mutable)(&config.mutable);
                        if ui
                            .checkbox(&mut mutable, "mutable")
                            .on_hover_text(
                                "Coché : ce gène mute à la reproduction (il dérive et se \
                                 transmet avec variation). Décoché : il est quand même \
                                 transmis, mais figé sur la valeur du fondateur — rien à \
                                 sélectionner.",
                            )
                            .changed()
                        {
                            (t.set_mutable)(&mut config.mutable, mutable);
                        }
                    }
                }
                if ui.button("↺ Réinitialiser au scénario").clicked() {
                    let base = Genotype::base(config);
                    if let Some(Archetype {
                        kind: ArchetypeKind::Agent { genotype, .. },
                        ..
                    }) = palette.items.get_mut(i)
                    {
                        *genotype = base;
                    }
                }
                // Cerveau de CETTE espèce uniquement : on lit l'espèce et la précision
                // fondatrice de l'archétype sélectionné (le `get_mut` ci-dessus est
                // refermé), puis on édite le cerveau correspondant.
                if let Some(ArchetypeKind::Agent { species, genotype }) =
                    palette.items.get(i).map(|a| &a.kind)
                {
                    let species = *species;
                    let arch_rays = genotype.ray_count();
                    species_brain_editor(ui, config, species, arch_rays);
                }
            } else {
                ui.label("La nourriture n'a pas de gènes éditables.");
            }
        }
        None => {
            ui.label("Clique un archétype dans la palette pour l'éditer.");
        }
    }

    ui.separator();
    ui.label("Scénario (RON)");
    ui.text_edit_singleline(&mut palette.save_path);
    ui.horizontal(|ui| {
        if ui.button("💾 Sauver").clicked() {
            sync_config_from_palette(config, palette);
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
                    palette.items = make_items(config);
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

/// Éditeur de cerveau (item 15) **ciblé sur l'espèce sélectionnée** : on ne voit ni
/// n'édite que le cerveau de l'archétype choisi, jamais ceux des autres espèces. Le
/// **type** de cerveau + ses paramètres **propres au variant** sont édités directement
/// sur le [`SimConfig`], donc persistés par « Sauver » sans passe de synchro.
///
/// À une seule espèce, on édite le [`SimConfig::brain`] uniforme ; sinon l'entrée
/// `species` de [`SimConfig::brains_per_species`] (item 18a, cohabitation
/// témoin/appris §4), matérialisée à la demande en clonant l'uniforme pour les
/// entrées manquantes. `arch_rays` = précision visuelle du fondateur, pour afficher
/// la taille (au fondateur) de la couche d'entrée du MLP. Chaque variant affiche en
/// plus sa **description fonctionnelle** (cf. [`BrainKind::description`]).
fn species_brain_editor(ui: &mut egui::Ui, config: &mut SimConfig, species: u16, arch_rays: usize) {
    ui.separator();
    ui.strong("Cerveau de cette espèce (auteur de la décision)");
    ui.small(
        "Le cerveau de l'espèce sélectionnée uniquement. Chaque variant expose ses \
         propres paramètres d'archétype.",
    );
    if config.species_cardinality() <= 1 {
        brain_kind_editor(ui, &mut config.brain, arch_rays);
    } else {
        let fallback = config.brain.clone();
        config
            .brains_per_species
            .resize(config.species_cardinality() as usize, fallback);
        brain_kind_editor(
            ui,
            &mut config.brains_per_species[species as usize],
            arch_rays,
        );
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
        BrainKind::Hunter => {}
        BrainKind::Mlp { hidden } => mlp_architecture_editor(ui, hidden, vision_rays),
    }
    ui.weak(kind.description());
}

/// Édition **numérique** de l'architecture d'un MLP (item 18b, cœur) : le nombre de
/// couches cachées et la largeur de chacune. L'entrée (`2 × rayons`) et la sortie (2)
/// sont *contraintes* par le contrat et seulement affichées. La visualisation en
/// graphe arrive à la tranche suivante.
fn mlp_architecture_editor(ui: &mut egui::Ui, hidden: &mut Vec<usize>, vision_rays: usize) {
    ui.small(format!(
        "Entrée {} au fondateur (= 2 × {vision_rays} rayons) → sortie {} (contrat). La \
         couche d'entrée s'adapte ensuite à la précision visuelle de chaque individu \
         (gène « Rayons »).",
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
    // du contrat d'E/S du MLP : l'entrée concatène les canaux *vision* (obstacle)
    // puis *cible* (cf. `MlpBrain::input_vector`) ; la sortie est le pilotage en
    // repère du corps (avant, côté). Dessinées dans les marges réservées de part et
    // d'autre, à hauteur de chaque nœud concerné.
    let font = egui::FontId::monospace(8.0);
    let ink = egui::Color32::from_gray(165);
    let n_in = sizes[0];
    let rays = n_in / 2; // entrée = 2 × rayons (vision ++ cible)
    for node in 0..n_in {
        let text = if n_in.is_multiple_of(2) {
            if node < rays {
                format!("vis {node}")
            } else {
                format!("cib {}", node - rays)
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
        "Paramètres de scénario. Les champs notés (reset) ne s'appliquent qu'à la \
         (ré)génération du monde (⟲ du bandeau) ; les autres agissent à chaud.",
    );

    ui.separator();
    ui.strong("Arène & population");
    ui.add(egui::Slider::new(&mut config.arena_half_extent, 100.0..=1000.0).text("demi-arène"));
    ui.add(
        egui::DragValue::new(&mut config.agent_count)
            .range(0..=5000)
            .prefix("agents au spawn (reset) : "),
    );
    ui.add(
        egui::DragValue::new(&mut config.species_count)
            .range(1..=8)
            .prefix("espèces (reset) : "),
    );
    ui.add(egui::Slider::new(&mut config.reserve_max, 10.0..=500.0).text("réserve max"));
    // Le nombre de rayons de vision est désormais le gène « Rayons (précision) » de
    // l'archétype (éditeur d'archétype), hérité et mutable — plus un réglage de monde.
    ui.add(
        egui::DragValue::new(&mut config.seed)
            .speed(1.0)
            .prefix("graine (reset) : "),
    );

    ui.separator();
    ui.strong("Nourriture");
    ui.add(
        egui::DragValue::new(&mut config.food_count)
            .range(0..=2000)
            .prefix("effectif maintenu : "),
    );
    ui.add(egui::Slider::new(&mut config.food_radius, 2.0..=30.0).text("rayon"));
    ui.add(egui::Slider::new(&mut config.food_energy, 5.0..=300.0).text("énergie"));
    ui.add(egui::Slider::new(&mut config.food_regen, 0.0..=50.0).text("repousse/s"));
    ui.add(
        egui::DragValue::new(&mut config.food_species)
            .range(0..=8)
            .prefix("espèce nourriture : "),
    );

    ui.separator();
    ui.strong("Relations (qui agit sur qui)");
    ui.small(
        "Une relation = un acteur réduit la réserve d'une cible à portée. C'est elle \
         qui fait d'une espèce une CIBLE comestible — ce que poursuit Brain::Hunter. \
         transfert = prédation (l'acteur gagne l'énergie) ; sinon simple destruction.",
    );
    let mut to_remove = None;
    for (i, rel) in config.relations.iter_mut().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut rel.actor)
                    .range(0..=8)
                    .prefix("acteur "),
            );
            ui.add(
                egui::DragValue::new(&mut rel.target)
                    .range(0..=8)
                    .prefix("→ cible "),
            );
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
        // Défaut « l'espèce 0 mange la nourriture » : le cas le plus courant, et ce
        // qu'il faut pour qu'un chasseur reconnaisse la nourriture comme cible.
        config.relations.push(Relation {
            actor: 0,
            target: config.food_species,
            transfer: true,
            rate: 100.0,
            range: 20.0,
        });
    }
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

/// Reporte les gènes de l'archétype d'agent (le premier) dans le génotype
/// fondateur du scénario, pour que la sauvegarde reflète l'édition.
///
/// Limite v1 assumée : `SimConfig` ne porte qu'un génotype fondateur ; si
/// plusieurs espèces d'agents ont été éditées séparément, seule la première est
/// persistée. Les scénarios actuels n'ont qu'une espèce.
fn sync_config_from_palette(config: &mut SimConfig, palette: &Palette) {
    let agent = palette.items.iter().find_map(|a| match &a.kind {
        ArchetypeKind::Agent { genotype, .. } => Some(genotype),
        ArchetypeKind::Food => None,
    });
    if let Some(g) = agent {
        config.max_speed = g.max_speed;
        config.agility = g.agility;
        config.vision_range = g.vision_range;
        config.vision_fov_deg = g.vision_fov_deg;
        config.reproduction_threshold = g.reproduction_threshold;
        config.offspring_energy = g.offspring_energy;
        config.mutation_rate = g.mutation_rate;
        config.base_metabolism = g.base_metabolism;
        config.move_cost = g.move_cost;
        // La précision visuelle (gène f32) repasse au fondateur (usize) du scénario.
        config.vision_rays = g.ray_count();
    }
}

/// Compile l'archétype `i` vers une entité vivante, posée en `world`.
fn place(
    commands: &mut Commands,
    config: &SimConfig,
    palette: &mut Palette,
    i: usize,
    world: Vec2,
) {
    match palette.items[i].kind.clone() {
        ArchetypeKind::Agent { species, genotype } => {
            let seed = palette.next_seed;
            palette.next_seed = palette.next_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            spawn_agent(
                commands,
                config,
                genotype,
                Species(species),
                world,
                0.0,
                seed,
                config.reserve_max,
                0, // posé à la main : génération 0 (fondateur).
            );
        }
        ArchetypeKind::Food => spawn_food(commands, config, world),
    }
}
