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
use crate::config::{SimConfig, Source};
use crate::nutrients::{Field, Fields};
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
            // The fill value for newly-appearing nutrient layers (cf. `sync_layer_flags`).
            // Default `true` (windowed: show every component); the recorder overrides it.
            .init_resource::<NewLayerVisible>()
            .add_systems(
                Update,
                (
                    attach_visuals,
                    shade_by_reserve,
                    draw_arena,
                    draw_sources,
                    render_source_bodies,
                    draw_heading,
                    draw_play_area,
                    sync_layer_flags,
                    render_nutrient_layers,
                    apply_agent_layer,
                    crate::decor::render_decor,
                    crate::decor::style_entity_decor,
                    render_source_decor,
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
            // Left empty on purpose: the per-component flags are grown to the scenario's
            // field count by `sync_layer_flags`, which fills them from `NewLayerVisible`
            // (the windowed default → every component shown). The recorder sets its own
            // `Layers` from `--nutrients` (cf. `bin/record`).
            nutrients: Vec::new(),
        }
    }
}

/// Visibility handed to a nutrient layer that **appears** when the field count grows
/// (a scenario load / reset) — the fill value used by [`sync_layer_flags`]. The
/// windowed build wants every declared component shown by default (`true`, "see the
/// whole substrate"); the video recorder overrides it to `false` so a `--nutrients`
/// render only shows the field it explicitly asked for, keeping existing videos
/// byte-identical.
#[derive(Resource)]
pub struct NewLayerVisible(pub bool);

impl Default for NewLayerVisible {
    fn default() -> Self {
        Self(true)
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

/// Dark rim behind a body (decor entity style): the pixel-art "liseré".
pub const DECOR_OUTLINE_COLOR: Color = Color::srgba(4.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0, 0.5);
/// Extra radius of the rim beyond the body, in world units.
pub const DECOR_OUTLINE_GROW: f32 = 0.8;
/// White glint offset up-left on a body (decor entity style): the "reflet".
pub const DECOR_HIGHLIGHT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.4);

/// Rendering only: give a visible mesh to freshly spawned agents, tinted by their
/// archetype's color — plus the decor's outline/highlight discs as **children**
/// (they ride the sim transform, despawn with the agent, and are invisible to
/// [`shade_by_reserve`], which only dims the parent's own material).
fn attach_visuals(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    new_agents: Query<(Entity, &Radius, &Species), (Added<Agent>, Without<Mesh2d>)>,
) {
    for (entity, radius, species) in &new_agents {
        let decor_visibility = if config.decor.enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        commands
            .entity(entity)
            .insert((
                Mesh2d(meshes.add(Circle::new(radius.0))),
                MeshMaterial2d(materials.add(Color::from(entity_color(&config, *species)))),
            ))
            .with_children(|parent| {
                parent.spawn((
                    crate::decor::DecorOutline,
                    Mesh2d(meshes.add(Circle::new(radius.0 + DECOR_OUTLINE_GROW))),
                    MeshMaterial2d(materials.add(DECOR_OUTLINE_COLOR)),
                    Transform::from_xyz(0.0, 0.0, -0.1),
                    decor_visibility,
                ));
                parent.spawn((
                    crate::decor::DecorHighlight,
                    Mesh2d(meshes.add(Circle::new(0.4 * radius.0))),
                    MeshMaterial2d(materials.add(DECOR_HIGHLIGHT_COLOR)),
                    Transform::from_xyz(-0.35 * radius.0, 0.35 * radius.0, 0.1),
                    decor_visibility,
                ));
            });
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

/// Rendering only: draw the arena outline with gizmos. Skipped when the decor is
/// on: the basin's bank already marks the walls, and a gray box floating over
/// the water reads as debug clutter.
fn draw_arena(mut gizmos: Gizmos, config: Res<crate::SimConfig>) {
    if config.decor.enabled {
        return;
    }
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

/// Rendering only: **outline** each scenario **source** (a substrate feature — a vent or
/// a rock) with a gizmo circle at its position, in its color. Sources are non-`Agent`
/// entities with no mesh, so — like [`draw_arena`] — the ring is drawn straight from the
/// config; this is the *only* on-screen trace of an otherwise invisible feature, and the
/// sole way a **solid** rock that emits nothing (`rate 0`, no field heatmap) is visible at
/// all. A **solid** source (a rock / obstacle) is additionally *filled* by
/// [`render_source_bodies`] (a disc mesh) so it reads as a tangible body — this ring then
/// crisps its edge; an intangible emitter (a vent) is left as the bare outline (its reach
/// shows through its field's heatmap layer).
/// With the decor on, a **solid** source keeps only its filled disc (plus the
/// decor outline/highlight — [`render_source_bodies`]): its gizmo ring would
/// float above the water film (gizmos always render on top). The intangible
/// emitters keep the ring — it is their only trace.
fn draw_sources(mut gizmos: Gizmos, config: Res<crate::SimConfig>) {
    for source in &config.sources {
        if config.decor.enabled && source.solid {
            continue;
        }
        let pos = Vec2::from(source.pos);
        gizmos.circle_2d(pos, source.radius, srgb3(source.color));
    }
}

/// A **filled body** for a solid source (a rock): a disc mesh tinted the source's color,
/// under the agents (`z = -4`, above the play-area and the component heatmaps). One per
/// **solid** [`Source`](crate::config::Source), reconciled against the config every frame
/// — mirroring [`render_nutrient_layers`] — so editing a source (moving, resizing,
/// toggling `solid`) shows live. It carries the index of the source it mirrors so a
/// reconcile can find it again.
///
/// A unit circle scaled by the radius (never rebuilt on a resize). The gizmo ring
/// ([`draw_sources`]) still crisps its edge; this fills the interior so a rock reads as a
/// solid body rather than a hollow outline.
#[derive(Component)]
struct SourceBody {
    /// Index into `config.sources` this disc mirrors.
    source: usize,
}

/// Rendering only: keep one filled disc ([`SourceBody`]) per **solid** source, tinted and
/// placed from the config. Reconciles like [`render_nutrient_layers`]: update the discs
/// that still map to a solid source, hide those whose source vanished or turned
/// intangible, and spawn a disc for any solid source that lacks one. No solid source
/// (every scenario before rocks) → nothing spawned.
fn render_source_bodies(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut bodies: Query<(
        &SourceBody,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial2d<ColorMaterial>,
    )>,
) {
    let mut covered = vec![false; config.sources.len()];
    for (body, mut tf, mut vis, material) in &mut bodies {
        match config.sources.get(body.source) {
            Some(src) if src.solid => {
                covered[body.source] = true;
                *vis = Visibility::Visible;
                tf.translation = Vec2::from(src.pos).extend(-4.0);
                tf.scale = Vec3::splat(src.radius);
                if let Some(mut mat) = materials.get_mut(&material.0) {
                    mat.color = srgb3(src.color);
                }
            }
            // The source was removed or its `solid` was turned off: hide the disc (kept,
            // to reuse if it becomes solid again — as the heatmap layers do).
            _ => *vis = Visibility::Hidden,
        }
    }
    for (index, src) in config.sources.iter().enumerate() {
        if src.solid && !covered[index] {
            commands.spawn((
                SourceBody { source: index },
                Mesh2d(meshes.add(Circle::new(1.0))),
                MeshMaterial2d(materials.add(srgb3(src.color))),
                Transform::from_translation(Vec2::from(src.pos).extend(-4.0))
                    .with_scale(Vec3::splat(src.radius)),
            ));
        }
    }
}

/// A rock's decor part (outline or highlight disc), keyed like [`SourceBody`]
/// to the source it dresses. A **sibling** of the disc, not a child: the disc's
/// scale is the radius on *all* axes, so a child's z-offset would be multiplied
/// by it and land among the heatmaps.
#[derive(Component)]
struct SourceDecor {
    /// Index into `config.sources` this part dresses.
    source: usize,
    /// `false` = the dark outline behind the disc, `true` = the up-left glint.
    highlight: bool,
}

/// Rendering only: the decor outline + highlight of each **solid** source,
/// reconciled against the config every frame exactly like
/// [`render_source_bodies`]. Hidden while the decor is off (the agents' twin
/// toggle is `decor::style_entity_decor`).
fn render_source_decor(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut parts: Query<(&SourceDecor, &mut Transform, &mut Visibility)>,
) {
    let mut covered = vec![[false; 2]; config.sources.len()];
    for (part, mut tf, mut vis) in &mut parts {
        match config.sources.get(part.source) {
            Some(src) if src.solid && config.decor.enabled => {
                covered[part.source][part.highlight as usize] = true;
                *vis = Visibility::Visible;
                place_source_decor(&mut tf, src, part.highlight);
            }
            _ => *vis = Visibility::Hidden,
        }
    }
    if !config.decor.enabled {
        return;
    }
    for (index, src) in config.sources.iter().enumerate() {
        if !src.solid {
            continue;
        }
        for highlight in [false, true] {
            if covered[index][highlight as usize] {
                continue;
            }
            let mut tf = Transform::default();
            place_source_decor(&mut tf, src, highlight);
            commands.spawn((
                SourceDecor {
                    source: index,
                    highlight,
                },
                Mesh2d(meshes.add(Circle::new(1.0))),
                MeshMaterial2d(materials.add(if highlight {
                    DECOR_HIGHLIGHT_COLOR
                } else {
                    DECOR_OUTLINE_COLOR
                })),
                tf,
            ));
        }
    }
}

/// Place one rock-decor part from its source: the outline slightly larger,
/// behind the disc (z −4.2 < −4); the glint up-left, in front (z −3.8 > −4).
fn place_source_decor(tf: &mut Transform, src: &Source, highlight: bool) {
    let pos = Vec2::from(src.pos);
    if highlight {
        tf.translation = (pos + Vec2::new(-0.35, 0.35) * src.radius).extend(-3.8);
        tf.scale = Vec3::splat(0.4 * src.radius);
    } else {
        tf.translation = pos.extend(-4.2);
        tf.scale = Vec3::splat(src.radius + DECOR_OUTLINE_GROW);
    }
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
/// Purely a **rendering** artifact (a background sprite), not part of the simulated
/// world — so the windowed build's hot reset despawns it explicitly:
/// [`render_nutrient_layers`] only ever touches indices that still exist in
/// [`Fields`], so a reload into a scenario with **fewer** fields would otherwise leave
/// the dropped layers orphaned (frozen on their last texture).
#[derive(Component)]
pub struct NutrientLayer {
    index: usize,
    res: usize,
}

/// Paints `field`'s concentrations into `image` (res×res RGBA): the nutrient's hue
/// with **alpha ∝ concentration** (normalized to the field's current max), so empty
/// cells are transparent and whatever is behind shows through. World +Y is mapped to
/// the image's **top** row (vertical flip).
fn paint_nutrient_image(image: &mut Image, field: &Field, color: Srgba) {
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
fn make_nutrient_image(field: &Field, color: Srgba) -> Image {
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

/// Rendering only: keep the per-component visibility flags ([`Layers::nutrients`]) sized
/// to the actual number of fields — a scenario with `N` components needs `N` toggles.
/// New components are filled from [`NewLayerVisible`]: `true` in the windowed build (every
/// declared component is shown by default), `false` in the recorder (a `--nutrients` video
/// shows only the field it asked for → byte-identical). Sizing these flags also makes the
/// extra components *reachable* — the View ▸ Layers menu iterates them, so a field at
/// index ≥ 1 (pheromone / toxicity / detritus) that had no flag would be both force-hidden
/// **and** absent from the menu.
fn sync_layer_flags(
    mut layers: ResMut<Layers>,
    fields: Res<Fields>,
    new_visible: Res<NewLayerVisible>,
) {
    if layers.nutrients.len() != fields.len() {
        layers.nutrients.resize(fields.len(), new_visible.0);
    }
}

/// Rendering only: the component **heatmap layers** (background, *behind* the agents
/// at `z = -5`, above the play-area at `z = -10`). Off by default; toggled per
/// component via [`Layers`]. Active layers **share** an opacity budget (`N` active ⇒
/// `1/N` each), so several stacked maps blend without saturating the background. One
/// quad ([`NutrientLayer`]) per [`Fields`] entry, hued by [`nutrient_color`].
fn render_nutrient_layers(
    mut commands: Commands,
    layers: Res<Layers>,
    fields: Res<Fields>,
    config: Res<SimConfig>,
    mut images: ResMut<Assets<Image>>,
    mut quads: Query<(
        &mut NutrientLayer,
        &mut Sprite,
        &mut Visibility,
        &mut Transform,
    )>,
) {
    // Shared opacity: a full budget split across the *active* layers.
    let active = layers.nutrients.iter().filter(|&&on| on).count().max(1);
    let opacity = 1.0 / active as f32;
    let side = 2.0 * config.arena_half_extent;

    for (index, field) in fields.iter().enumerate() {
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
                continue; // hidden: skip the texture repaint.
            }
            sprite.color = Color::srgba(1.0, 1.0, 1.0, opacity);
            sprite.custom_size = Some(Vec2::splat(side));
            tf.translation.z = -5.0 - index as f32 * 0.1;
            if layer.res == field.resolution() {
                if let Some(mut img) = images.get_mut(&sprite.image) {
                    paint_nutrient_image(&mut img, field, color);
                }
            } else {
                // The grid changed (scenario reload): rebuild the texture to fit.
                sprite.image = images.add(make_nutrient_image(field, color));
                layer.res = field.resolution();
            }
        } else if enabled {
            let handle = images.add(make_nutrient_image(field, color));
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
}
