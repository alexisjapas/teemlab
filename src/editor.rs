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
//! Disposition : un panneau d'**archétypes** à droite (on y pioche par
//! glisser-déposer), un bandeau de **statistiques** sous l'aire de jeu.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::components::{Agent, Food, Reserve, Species};
use teemlab::ecology::spawn_food;
use teemlab::genotype::Genotype;
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
    for species in 0..config.species_count.max(1) {
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

/// Le panneau d'archétypes (droite), le bandeau de stats (bas), et la résolution
/// du glisser-déposer. Tourne dans `EguiPrimaryContextPass`.
#[allow(clippy::too_many_arguments)]
pub fn editor_ui(
    mut contexts: EguiContexts,
    mut palette: ResMut<Palette>,
    mut commands: Commands,
    mut config: ResMut<SimConfig>,
    vis: Res<crate::controls::PanelVisibility>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    agents: Query<(&Reserve, &Genotype), With<Agent>>,
    food: Query<(), With<Food>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    // — Panneau de droite : la liste des archétypes (glisser pour poser, cliquer
    //   pour éditer). —
    if vis.palette {
    egui::SidePanel::right("palette")
        .default_width(190.0)
        .show(ctx, |ui| {
            ui.heading("Archétypes");
            ui.label("Glisse dans l'aire pour poser ; clique pour éditer.");
            ui.separator();
            let mut started = None;
            let mut clicked = None;
            for (i, arch) in palette.items.iter().enumerate() {
                let mark = if palette.selected == Some(i) { "▶ " } else { "⬤ " };
                let label =
                    egui::RichText::new(format!("{mark}{}", arch.name)).color(arch.color);
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
        });
    }

    // — Panneau de gauche : éditeur d'archétype + save/load RON. —
    if vis.editor {
        editor_panel(ctx, &mut palette, &mut config);
    }

    // — Bandeau du bas : statistiques en direct. —
    egui::TopBottomPanel::bottom("stats").show(ctx, |ui| {
        let population = agents.iter().count();
        let n = population.max(1) as f32;
        let mean_reserve = agents.iter().map(|(r, _)| r.current).sum::<f32>() / n;
        let mean_speed = agents.iter().map(|(_, g)| g.max_speed).sum::<f32>() / n;
        let mean_vision = agents.iter().map(|(_, g)| g.vision_range).sum::<f32>() / n;
        let food_count = food.iter().count();
        ui.horizontal(|ui| {
            ui.label(format!("Population : {population}"));
            ui.separator();
            ui.label(format!("Nourriture : {food_count}"));
            ui.separator();
            ui.label(format!("Réserve moy. : {mean_reserve:.0}"));
            ui.separator();
            ui.label(format!("Gènes moy. — vitesse : {mean_speed:.0}"));
            ui.separator();
            ui.label(format!("vision : {mean_vision:.0}"));
        });
    });

    // — Résolution du glisser-déposer. —
    if let Some(i) = palette.dragging {
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
    }

    Ok(())
}

/// Panneau de gauche : édition des gènes de l'archétype sélectionné + save/load
/// RON. Rend explicite la distinction **archétype** (le modèle édité ici) /
/// **génome** (la copie héritée par chaque instance, qui mute ensuite seule).
fn editor_panel(ctx: &egui::Context, palette: &mut Palette, config: &mut SimConfig) {
    egui::SidePanel::left("editor")
        .default_width(250.0)
        .show(ctx, |ui| {
            ui.heading("Éditeur d'archétype");
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
                        // Bornes copiées pour ne pas garder `config` emprunté.
                        let (sb, ab, rb, fb) = (
                            config.speed_bounds,
                            config.agility_bounds,
                            config.vision_range_bounds,
                            config.vision_fov_bounds,
                        );
                        if let Some(Archetype {
                            kind: ArchetypeKind::Agent { genotype, .. },
                            ..
                        }) = palette.items.get_mut(i)
                        {
                            ui.add(
                                egui::Slider::new(&mut genotype.max_speed, sb.min..=sb.max)
                                    .text("Vitesse max"),
                            );
                            ui.add(
                                egui::Slider::new(&mut genotype.agility, ab.min..=ab.max)
                                    .text("Agilité"),
                            );
                            ui.add(
                                egui::Slider::new(&mut genotype.vision_range, rb.min..=rb.max)
                                    .text("Portée vision"),
                            );
                            let mut fov_deg = genotype.vision_fov.to_degrees();
                            if ui
                                .add(
                                    egui::Slider::new(&mut fov_deg, fb.min..=fb.max)
                                        .text("Champ vision (°)"),
                                )
                                .changed()
                            {
                                genotype.vision_fov = fov_deg.to_radians();
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
        config.vision_fov_deg = g.vision_fov.to_degrees();
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
