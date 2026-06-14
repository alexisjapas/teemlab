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

/// La palette d'archétypes + l'état du glisser-déposer en cours.
#[derive(Resource, Default)]
pub struct Palette {
    pub items: Vec<Archetype>,
    /// Index de l'archétype actuellement glissé, le cas échéant.
    pub dragging: Option<usize>,
    /// Graine roulante pour donner un flux distinct au cerveau de chaque agent
    /// posé à la main.
    pub next_seed: u64,
}

/// Couleur d'une espèce, en `egui::Color32` (miroir de la palette du rendu).
fn species_color32(species: u16) -> egui::Color32 {
    const PALETTE: [(u8, u8, u8); 4] = [
        (77, 179, 255),  // bleu
        (255, 115, 89),  // corail
        (140, 230, 115), // vert
        (242, 204, 77),  // ambre
    ];
    let (r, g, b) = PALETTE[species as usize % PALETTE.len()];
    egui::Color32::from_rgb(r, g, b)
}

/// Construit la palette à partir du scénario chargé : une entrée par espèce
/// d'agent (avec le génotype fondateur) + la nourriture. Tourne au `Startup`,
/// après l'insertion de [`SimConfig`] par le plugin de sim.
pub fn build_palette(mut commands: Commands, config: Res<SimConfig>) {
    let mut items = Vec::new();
    let base = Genotype::base(&config);
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
    commands.insert_resource(Palette {
        items,
        dragging: None,
        next_seed: config.seed ^ 0xED17,
    });
}

/// Le panneau d'archétypes (droite), le bandeau de stats (bas), et la résolution
/// du glisser-déposer. Tourne dans `EguiPrimaryContextPass`.
#[allow(clippy::too_many_arguments)]
pub fn editor_ui(
    mut contexts: EguiContexts,
    mut palette: ResMut<Palette>,
    mut commands: Commands,
    config: Res<SimConfig>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    agents: Query<(&Reserve, &Genotype), With<Agent>>,
    food: Query<(), With<Food>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    // — Panneau de droite : la liste des archétypes, chacun glissable. —
    egui::SidePanel::right("palette")
        .default_width(190.0)
        .show(ctx, |ui| {
            ui.heading("Archétypes");
            ui.label("Glisse un élément dans l'aire de jeu pour le placer.");
            ui.separator();
            let mut started = None;
            for (i, arch) in palette.items.iter().enumerate() {
                let label = egui::RichText::new(format!("⬤ {}", arch.name)).color(arch.color);
                let resp = ui.add_sized(
                    [ui.available_width(), 28.0],
                    egui::Button::new(label).sense(egui::Sense::click_and_drag()),
                );
                if resp.drag_started() {
                    started = Some(i);
                }
            }
            if started.is_some() {
                palette.dragging = started;
            }
            if palette.dragging.is_some() {
                ui.separator();
                ui.weak("Relâche au-dessus de l'aire pour déposer.");
            }
        });

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
