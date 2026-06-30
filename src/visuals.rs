//! Rendering layer **shared** by the binaries that *display* the sim: the
//! windowed build (`main.rs`) and the headless video recorder (`bin/record.rs`).
//!
//! This is strictly rendering/observation — everything lives in `Update`,
//! **never** in `FixedUpdate` (cardinal invariant). Deliberately outside
//! [`crate::SimPlugin`], which stays render-agnostic: the "pure" headless
//! (`bin/headless.rs`) does not include it. Centralizing here avoids duplicating
//! the rendering between the live preview and the recording (item 14, §7: *fresh
//! re-render* of a run).

use crate::components::{Agent, Locomotion, Perception, Radius, Reserve, Species};
use crate::config::SimConfig;
use crate::nutrients::NutrientField;
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Adds the sim's rendering systems (entity meshes, reserve-based shading,
/// arena, heading indicator, inner/outer **backgrounds**). To be combined with a
/// camera provided by the binary (window for `main`, image target for `record`).
/// The detailed fan of vision rays is not part of it: it is reserved for the
/// inspected agent, on the windowed side.
///
/// The backgrounds ([`draw_play_area`]) live here — therefore **shared** by the
/// live preview and the video recording — so that a video renders exactly the
/// colors set in the editor (background-colors item), and not a frozen
/// background.
pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        // `ClearColor` drives the off-game area on the windowed side (the `main`
        // camera uses it); we ensure its presence so `draw_play_area` can write it
        // in both binaries (the recorder, for its part, sets the off-game area on
        // its image-camera).
        app.init_resource::<ClearColor>()
            // The view **layers** ("calques") toggles (cf. [`Layers`]). Present in
            // both binaries; the windowed build drives it via egui, the recorder
            // keeps the defaults (agents on, nutrient maps off → video unchanged).
            .init_resource::<Layers>()
            .add_systems(
                Update,
                (
                    attach_visuals,
                    shade_by_reserve,
                    draw_arena,
                    draw_heading,
                    draw_play_area,
                    render_nutrient_layers,
                    apply_agent_layer,
                ),
            );
    }
}

/// Toggleable rendering **layers** ("calques"). The agents are the *main* layer;
/// each nutrient concentration field is a *background* layer, **off by default**
/// (the windowed build toggles them, cf. `panels`). All toggleable. The nutrient
/// layers **share** an opacity budget — `N` active ⇒ `1/N` each (2 ⇒ 50 %) — so
/// stacking several heatmaps in the background stays readable.
#[derive(Resource)]
pub struct Layers {
    /// The agents layer (their meshes and heading indicator).
    pub agents: bool,
    /// One flag per nutrient field (T2: a single one), each a background heatmap.
    pub nutrients: Vec<bool>,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            agents: true,
            // T2 has one nutrient field; its heatmap is shown by default (the windowed
            // build). The video recorder sets its own `Layers` from `--nutrients`
            // (off), so existing videos are unchanged (cf. `bin/record`).
            nutrients: vec![true],
        }
    }
}

/// Display color of nutrient `index` (cyclic palette) — the hue of its heatmap.
pub fn nutrient_color(index: usize) -> Srgba {
    const PALETTE: [Srgba; 4] = [
        Srgba::new(1.00, 0.60, 0.20, 1.0), // amber
        Srgba::new(0.30, 0.80, 1.00, 1.0), // cyan
        Srgba::new(0.80, 0.45, 1.00, 1.0), // violet
        Srgba::new(0.55, 1.00, 0.55, 1.0), // green
    ];
    PALETTE[index % PALETTE.len()]
}

/// Marker of the background quad materializing the play area (inside of the arena).
#[derive(Component)]
pub struct PlayAreaBg;

/// Opaque `Color` from an sRGB triplet `[r, g, b]` of the scenario (background settings).
pub fn srgb3([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
}

/// Display color of an entity: that of **its archetype** (the index carried by
/// [`Species`]), falling back to the palette for an out-of-list index. This is
/// how the color chosen in the archetype editor shows on screen.
fn entity_color(config: &SimConfig, species: Species) -> Srgba {
    let [r, g, b] = config.color_of(species.0);
    Srgba::new(r, g, b, 1.0)
}

/// Rendering only: give a visible mesh to freshly spawned agents, tinted by their
/// archetype's color. Runs in `Update` because it manipulates render assets.
fn attach_visuals(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    new_agents: Query<(Entity, &Radius, &Species), (Added<Agent>, Without<Mesh2d>)>,
) {
    for (entity, radius, species) in &new_agents {
        commands.entity(entity).insert((
            Mesh2d(meshes.add(Circle::new(radius.0))),
            MeshMaterial2d(materials.add(Color::from(entity_color(&config, *species)))),
        ));
    }
}

/// Rendering only: darken an agent as its reserve drops, to *see* predation drain
/// its prey. Each agent owns its own material (created in `attach_visuals`),
/// which we modulate here by the reserve fraction.
fn shade_by_reserve(
    config: Res<SimConfig>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    agents: Query<(&MeshMaterial2d<ColorMaterial>, &Species, &Reserve)>,
) {
    for (handle, species, reserve) in &agents {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let dim = 0.25 + 0.75 * reserve.fraction();
            let base = entity_color(&config, *species);
            material.color = Color::srgb(base.red * dim, base.green * dim, base.blue * dim);
        }
    }
}

/// Rendering only: a short **heading indicator** for **mobile** agents — a line
/// from the center to the body's edge, along the heading, to read a moving
/// entity's orientation at a glance. It stops at the radius (`Radius`): it never
/// overflows the body. We re-read the heading already computed by the sim
/// (`Perception::heading`), recomputing nothing.
///
/// An **immobile** entity (flora / sessile source, [`Locomotion::is_immobile`])
/// gets **none**: its "heading" is only a fixed fallback (`+X`), not a gaze
/// direction — showing it would draw a misleading line over a bush.
///
/// The vision **detail** (the full fan of rays, occlusion at work) is NOT drawn
/// here: on every agent it would saturate the screen. It is the windowed binary
/// that draws it, for the single **inspected** agent (cf. `inspector`).
fn draw_heading(
    mut gizmos: Gizmos,
    layers: Res<Layers>,
    agents: Query<(&Transform, &Radius, &Perception, &Locomotion), With<Agent>>,
) {
    if !layers.agents {
        return; // agents layer hidden: no heading either.
    }
    for (transform, radius, perception, loco) in &agents {
        if loco.is_immobile() {
            continue; // flora: no useful heading to show.
        }
        let facing = perception.heading;
        if facing == Vec2::ZERO {
            continue; // no heading yet (1st tick): nothing to show.
        }
        let origin = transform.translation.truncate();
        gizmos.line_2d(
            origin,
            origin + facing * radius.0,
            Color::srgb(0.95, 0.95, 0.98),
        );
    }
}

/// Rendering only: draw the arena outline with gizmos.
fn draw_arena(mut gizmos: Gizmos, config: Res<crate::SimConfig>) {
    let h = config.arena_half_extent;
    let color = Color::srgb(0.40, 0.40, 0.46);
    gizmos.linestrip_2d(
        [
            Vec2::new(-h, -h),
            Vec2::new(h, -h),
            Vec2::new(h, h),
            Vec2::new(-h, h),
            Vec2::new(-h, -h),
        ],
        color,
    );
}

/// Rendering only: materializes the **two backgrounds** set by the scenario
/// ([`SimConfig::play_area_color`] / [`SimConfig::off_game_color`]).
///
/// - The **play area** (inside of the arena) is a quad painted **under** the
///   agents (negative z), following the arena's size (`arena_half_extent`, which
///   may change when a scenario is reloaded) and the inner color.
/// - The **off-game area** (beyond the walls) is driven by `ClearColor`, written
///   here from the outer color. The windowed camera (`main`) uses it as-is; the
///   recorder (`record`) sets the same color on its image-camera (which ignores
///   `ClearColor`).
///
/// Shared by the live preview and the video recording: a video therefore renders
/// exactly the chosen colors. Both tints are re-read **continuously** → a change
/// in the editor (or when loading a scenario) shows immediately, without a reset.
fn draw_play_area(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut clear_color: ResMut<ClearColor>,
    config: Res<SimConfig>,
    mut existing: Query<(&mut Transform, &MeshMaterial2d<ColorMaterial>), With<PlayAreaBg>>,
) {
    let side = 2.0 * config.arena_half_extent;
    clear_color.0 = srgb3(config.off_game_color);
    let play_color = srgb3(config.play_area_color);
    if let Ok((mut tf, material)) = existing.single_mut() {
        tf.scale = Vec3::new(side, side, 1.0);
        if let Some(mut mat) = materials.get_mut(&material.0) {
            mat.color = play_color;
        }
    } else {
        commands.spawn((
            PlayAreaBg,
            Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial2d(materials.add(play_color)),
            Transform::from_xyz(0.0, 0.0, -10.0).with_scale(Vec3::new(side, side, 1.0)),
        ));
    }
}

/// Rendering only: show/hide the **agents layer** (their meshes) per [`Layers`].
/// Toggling the main layer off leaves the background (play area + any nutrient
/// heatmaps) visible on their own.
fn apply_agent_layer(layers: Res<Layers>, mut agents: Query<&mut Visibility, With<Agent>>) {
    let target = if layers.agents {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in &mut agents {
        if *vis != target {
            *vis = target;
        }
    }
}

/// A nutrient **heatmap** quad (one per nutrient field). Holds the field index and
/// the grid resolution the texture was built for, so a scenario reload that changes
/// the grid rebuilds it.
#[derive(Component)]
struct NutrientLayer {
    index: usize,
    res: usize,
}

/// Paints `field`'s concentrations into `image` (res×res RGBA): the nutrient's hue
/// with **alpha ∝ concentration** (normalized to the field's current max), so empty
/// cells are transparent and whatever is behind shows through. World +Y is mapped to
/// the image's **top** row (vertical flip).
fn paint_nutrient_image(image: &mut Image, field: &NutrientField, color: Srgba) {
    let res = field.resolution();
    let cells = field.cells();
    let max = cells.iter().copied().fold(0.0_f32, f32::max).max(1e-6);
    for y in 0..res {
        let row = (res - 1 - y) as u32; // world +Y → image top row
        for x in 0..res {
            let a = (cells[y * res + x] / max).clamp(0.0, 1.0);
            let _ = image.set_color_at(
                x as u32,
                row,
                Color::srgba(color.red, color.green, color.blue, a),
            );
        }
    }
}

/// A fresh res×res heatmap image (linear-sampled → a smooth map, not blocky cells).
fn make_nutrient_image(field: &NutrientField, color: Srgba) -> Image {
    let res = field.resolution().max(1) as u32;
    let mut image = Image::new_fill(
        Extent3d {
            width: res,
            height: res,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    paint_nutrient_image(&mut image, field, color);
    image
}

/// Rendering only: the nutrient **heatmap layer(s)** (background, *behind* the
/// agents at `z = -5`, above the play-area at `z = -10`). Off by default; toggled
/// per nutrient via [`Layers`]. Active nutrient layers **share** an opacity budget
/// (`N` active ⇒ `1/N` each), so several stacked maps blend without saturating the
/// background. T2 has a single nutrient field ([`NutrientField`]); the body
/// generalizes to several fields in T3.
fn render_nutrient_layers(
    mut commands: Commands,
    layers: Res<Layers>,
    field: Res<NutrientField>,
    config: Res<SimConfig>,
    mut images: ResMut<Assets<Image>>,
    mut quads: Query<(
        &mut NutrientLayer,
        &mut Sprite,
        &mut Visibility,
        &mut Transform,
    )>,
) {
    // Shared opacity: a full budget split across the *active* nutrient layers.
    let active = layers.nutrients.iter().filter(|&&on| on).count().max(1);
    let opacity = 1.0 / active as f32;
    let side = 2.0 * config.arena_half_extent;

    // T2: a single nutrient field, index 0.
    let index = 0usize;
    let enabled = layers.nutrients.get(index).copied().unwrap_or(false);
    let color = nutrient_color(index);

    if let Some((mut layer, mut sprite, mut vis, mut tf)) =
        quads.iter_mut().find(|(l, ..)| l.index == index)
    {
        *vis = if enabled {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if !enabled {
            return; // hidden: skip the texture repaint.
        }
        sprite.color = Color::srgba(1.0, 1.0, 1.0, opacity);
        sprite.custom_size = Some(Vec2::splat(side));
        tf.translation.z = -5.0 - index as f32 * 0.1;
        if layer.res == field.resolution() {
            if let Some(mut img) = images.get_mut(&sprite.image) {
                paint_nutrient_image(&mut img, &field, color);
            }
        } else {
            // The grid changed (scenario reload): rebuild the texture to fit.
            sprite.image = images.add(make_nutrient_image(&field, color));
            layer.res = field.resolution();
        }
    } else if enabled {
        let handle = images.add(make_nutrient_image(&field, color));
        commands.spawn((
            NutrientLayer {
                index,
                res: field.resolution(),
            },
            Sprite {
                image: handle,
                custom_size: Some(Vec2::splat(side)),
                color: Color::srgba(1.0, 1.0, 1.0, opacity),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, -5.0 - index as f32 * 0.1),
        ));
    }
}
