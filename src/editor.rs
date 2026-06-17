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
use teemlab::brain::BrainKind;
use teemlab::components::{Agent, Food, Reserve, Species};
use teemlab::config::Relation;
use teemlab::ecology::spawn_food;
use teemlab::genotype::{Genotype, TRAITS};
use teemlab::spawn::spawn_agent;
use teemlab::SimConfig;

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

/// Couleur d'une espèce, en `egui::Color32` (miroir de la palette du rendu).
/// Partagée avec le HUD (item 10) pour que courbes et entités s'accordent.
pub(crate) fn species_color32(species: u16) -> egui::Color32 {
    const PALETTE: [(u8, u8, u8); 4] = [
        (77, 179, 255),  // bleu
        (255, 115, 89),  // corail
        (140, 230, 115), // vert
        (242, 204, 77),  // ambre
    ];
    let (r, g, b) = PALETTE[species as usize % PALETTE.len()];
    egui::Color32::from_rgb(r, g, b)
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

/// Les fenêtres flottantes d'archétypes : le **sélecteur** (où l'on pioche par
/// glisser-déposer) et l'**éditeur** de l'archétype choisi. Tourne dans
/// `EguiPrimaryContextPass`. La résolution du glisser-déposer vit dans
/// [`resolve_drag`] (système distinct, ordonné **après** toutes les fenêtres) et
/// les statistiques dans [`stats_ui`].
pub fn editor_ui(
    mut contexts: EguiContexts,
    mut palette: ResMut<Palette>,
    mut config: ResMut<SimConfig>,
    mut vis: ResMut<crate::controls::PanelVisibility>,
) -> Result {
    use crate::controls::{WindowSlot, tidy_pos};
    let tidy = vis.tidy_windows;
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();

    if vis.palette {
        let mut w = egui::Window::new("Archétypes")
            .open(&mut vis.palette)
            .default_pos([1180.0, 84.0])
            .default_width(220.0)
            .resizable(true);
        if tidy {
            w = w.current_pos(tidy_pos(screen, WindowSlot::Archetypes));
        }
        w.show(ctx, |ui| selector_section(ui, &mut palette));
    }
    if vis.editor {
        let mut w = egui::Window::new("Éditeur d'archétype")
            .open(&mut vis.editor)
            .default_pos([1180.0, 400.0])
            .default_width(250.0)
            .resizable(true)
            .vscroll(true);
        if tidy {
            w = w.current_pos(tidy_pos(screen, WindowSlot::Editor));
        }
        w.show(ctx, |ui| editor_section(ui, &mut palette, &mut config));
    }

    Ok(())
}

/// Résolution du glisser-déposer d'un archétype dans l'aire de jeu. Système
/// **distinct**, ordonné après tous les panneaux egui : `is_pointer_over_area`
/// connaît alors toutes les arêtes, sinon un dépôt au-dessus d'un panneau (bas ou
/// gauche) poserait une entité cachée sous l'UI. `viewport_to_world_2d` tient
/// compte de l'offset du viewport (sim centrée, cf. `set_sim_viewport`) → le
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
/// pour éditer). Rendue en haut du panneau de droite.
fn selector_section(ui: &mut egui::Ui, palette: &mut Palette) {
    ui.label("Glisse dans l'aire pour poser ; clique pour éditer ; Suppr (curseur sur une entité) pour retirer.");
    ui.separator();
    let mut started = None;
    let mut clicked = None;
    for (i, arch) in palette.items.iter().enumerate() {
        let mark = if palette.selected == Some(i) { "▶ " } else { "⬤ " };
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
fn editor_section(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) {
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
                    // « héritable » par caractéristique. Ajouter un trait n'ajoute
                    // pas une ligne ici — c'est la falsification de l'item 15
                    // contre la pluralité de traits existante.
                    for t in &TRAITS {
                        let bounds = (t.bounds)(config);
                        let mut value = (t.get)(genotype);
                        if ui
                            .add(egui::Slider::new(&mut value, bounds.min..=bounds.max).text(t.name))
                            .changed()
                        {
                            (t.set)(genotype, value);
                        }
                        let mut heritable = (t.heritable)(&config.heritable);
                        if ui
                            .checkbox(&mut heritable, "héritable")
                            .on_hover_text("Décoché : ce gène reste figé à l'archétype, il ne mute pas.")
                            .changed()
                        {
                            (t.set_heritable)(&mut config.heritable, heritable);
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
            } else {
                ui.label("La nourriture n'a pas de gènes éditables.");
            }
        }
        None => {
            ui.label("Clique un archétype dans la palette pour l'éditer.");
        }
    }

    brain_selector(ui, config);

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

/// Sélecteur de cerveau (item 15) : le **type** de cerveau des agents + les
/// paramètres **propres au variant** choisi. Global au scénario en v1, comme le
/// génotype fondateur (cf. [`sync_config_from_palette`]) — édité directement sur le
/// [`SimConfig`], donc persisté par « Sauver » sans passe de synchro.
///
/// Le choix se fait par *kind* (et non par valeur) : éditer un paramètre ne doit
/// pas désélectionner le type. Le `match` est exhaustif → ajouter un variant de
/// `Brain` force à exposer ses paramètres ici. C'est la contrepartie *hétérogène*
/// (des paramètres propres à chaque cerveau) de la table `TRAITS`, elle homogène.
fn brain_selector(ui: &mut egui::Ui, config: &mut SimConfig) {
    ui.separator();
    ui.strong("Cerveau (auteur de la décision)");
    ui.small(
        "Le type de cerveau des agents, global au scénario en v1. Chaque variant \
         expose ses propres paramètres d'archétype.",
    );
    egui::ComboBox::from_label("type")
        .selected_text(config.brain.name())
        .show_ui(ui, |ui| {
            // Sélection par KIND : on ne réécrit le config que si l'on change de
            // type, pour ne pas réinitialiser les paramètres du variant courant.
            let is_wander = matches!(config.brain, BrainKind::Wander { .. });
            if ui.selectable_label(is_wander, "Errance").clicked() && !is_wander {
                config.brain = BrainKind::default();
            }
            let is_hunter = matches!(config.brain, BrainKind::Hunter);
            if ui.selectable_label(is_hunter, "Chasseur").clicked() && !is_hunter {
                config.brain = BrainKind::Hunter;
            }
        });
    match &mut config.brain {
        BrainKind::Wander { turn_rate } => {
            ui.add(egui::Slider::new(turn_rate, 0.0..=1.0).text("vivacité du virage"))
                .on_hover_text("Amplitude max de la dérive de cap à chaque tick (rad).");
        }
        BrainKind::Hunter => {
            ui.weak("Réflexe déterministe — aucun paramètre.");
        }
    }
}

/// La fenêtre flottante « Monde » : les paramètres de **scénario** — l'arène, la
/// population, l'économie de nourriture, la table d'interactions — par opposition à
/// l'éditeur d'**archétype** (le génotype d'une espèce). Édités directement sur le
/// [`SimConfig`], donc persistés par « Sauver ». Tourne dans `EguiPrimaryContextPass`.
pub fn world_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<SimConfig>,
    mut vis: ResMut<crate::controls::PanelVisibility>,
) -> Result {
    if !vis.world {
        return Ok(());
    }
    use crate::controls::{WindowSlot, tidy_pos};
    let tidy = vis.tidy_windows;
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let mut w = egui::Window::new("Monde")
        .open(&mut vis.world)
        .default_pos([520.0, 130.0])
        .default_width(280.0)
        .resizable(true)
        .vscroll(true);
    if tidy {
        w = w.current_pos(tidy_pos(screen, WindowSlot::World));
    }
    w.show(ctx, |ui| world_section(ui, &mut config));
    Ok(())
}

/// Le contenu de la fenêtre « Monde » : lecture/écriture directe du [`SimConfig`].
/// Certains champs ne prennent effet qu'à la (ré)génération du monde (annotés
/// *reset*) ; les autres — nourriture maintenue, relations — sont lus en continu
/// par la sim et agissent **à chaud**.
fn world_section(ui: &mut egui::Ui, config: &mut SimConfig) {
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
    ui.add(
        egui::DragValue::new(&mut config.vision_rays)
            .range(1..=21)
            .prefix("rayons de vision (reset) : "),
    );
    ui.add(egui::DragValue::new(&mut config.seed).speed(1.0).prefix("graine (reset) : "));

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
            ui.add(egui::DragValue::new(&mut rel.actor).range(0..=8).prefix("acteur "));
            ui.add(egui::DragValue::new(&mut rel.target).range(0..=8).prefix("→ cible "));
            if ui.button("🗑").on_hover_text("Retirer cette relation").clicked() {
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

/// Fenêtre de statistiques en direct. Lecture seule du monde : c'est de
/// l'observation pour affichage, pas de la logique de sim.
pub fn stats_ui(
    mut contexts: EguiContexts,
    mut vis: ResMut<crate::controls::PanelVisibility>,
    agents: Query<(&Reserve, &Genotype), With<Agent>>,
    food: Query<(), With<Food>>,
) -> Result {
    if !vis.stats {
        return Ok(());
    }
    let tidy = vis.tidy_windows;
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let mut window = egui::Window::new("Stats")
        .open(&mut vis.stats)
        .default_pos([560.0, 84.0])
        .resizable(false);
    if tidy {
        window = window.current_pos(crate::controls::tidy_pos(screen, crate::controls::WindowSlot::Stats));
    }
    window.show(ctx, |ui| {
        let population = agents.iter().count();
        let n = population.max(1) as f32;
        let mean_reserve = agents.iter().map(|(r, _)| r.current).sum::<f32>() / n;
        let food_count = food.iter().count();
        ui.horizontal(|ui| {
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
    });
    Ok(())
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
            );
        }
        ArchetypeKind::Food => spawn_food(commands, config, world),
    }
}
