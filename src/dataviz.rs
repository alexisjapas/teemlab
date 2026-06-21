//! **Visualiseur natif Bevy** des stats / courbes / inspecteur — le pendant *rendu*
//! (non-egui) des panneaux d'observation, pour qu'ils apparaissent **dans la vidéo**
//! (l'overlay egui, lui, n'est jamais filmé : §7).
//!
//! Couche de rendu partagée (comme [`crate::visuals`]) : tout vit dans `Update`, jamais
//! de logique de sim. La *donnée* (stats, courbes) vient de [`crate::metrics`] — donc
//! **exactement** les mêmes nombres/polylignes que les versions egui ; ici on ne fait que
//! les *tracer* en Bevy (Text2d + Sprite + gizmos).
//!
//! ## Composition 9:16 (fixe)
//! Quand le visualiseur est **actif**, la cible de rendu est recomposée en portrait
//! 9:16 : l'**arène carrée** occupe le carré du haut, le **visualiseur** la bande du
//! bas (largeur × 7/9). Deux caméras supplémentaires (une de fond/lettre-box, une pour
//! le visualiseur) encadrent la caméra de sim, dont on règle le `viewport`. Le mode
//! « présentation » du fenêtré et le rendu vidéo empruntent **le même** chemin → l'aperçu
//! éditeur est strictement identique à la vidéo.
//!
//! ## Rotation des sections
//! La bande du bas est de taille fixe : on y montre les **stats** en permanence (en haut)
//! et on fait **tourner** le reste — courbes puis inspecteur — toutes les `interval`
//! secondes (configurable).

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode, Viewport};
use bevy::prelude::*;
use bevy::sprite::{Anchor, Text2d};

use crate::brain::{Brain, MlpBrain};
use crate::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use crate::config::SimConfig;
use crate::genotype::{Genotype, TRAITS};
use crate::metrics::{Curve, History, live_stats, population_curves, trait_curves};
use crate::selection::Selection;

/// Layer de rendu du visualiseur (Text2d / Sprite / gizmos `VizGizmos`).
const VIZ_LAYER: usize = 1;
/// Layer de la caméra de fond (lettre-box) : aucune entité, elle ne fait que *nettoyer*.
const LETTERBOX_LAYER: usize = 2;
/// Toile logique du visualiseur (ratio 9:7 = la bande du bas d'un cadre 9:16). On dessine
/// dans ce repère pixel virtuel (origine coin haut-gauche), indépendant de la résolution
/// réelle → mise en page identique entre l'aperçu éditeur et la vidéo.
const VIZ_W: f32 = 900.0;
const VIZ_H: f32 = 700.0;
/// Marge de respiration autour de l'arène dans le carré du haut.
const ARENA_MARGIN: f32 = 1.08;
/// Nombre de pages tournantes (0 = courbes, 1 = inspecteur).
const PAGES: usize = 2;

/// Groupe de gizmos dédié au visualiseur, restreint au [`VIZ_LAYER`] (cf. `build`).
#[derive(Default, Reflect, GizmoConfigGroup)]
struct VizGizmos;

/// État du visualiseur : actif ou non, et la rotation des sections.
#[derive(Resource)]
pub struct DataViz {
    /// Le visualiseur recompose-t-il la vue (9:16) et dessine-t-il ?
    pub active: bool,
    /// Intervalle de rotation des sections (secondes simulées).
    pub interval: f32,
    elapsed: f32,
    page: usize,
    was_active: bool,
}

/// Ajoute le visualiseur natif. `enabled` = état initial (vidéo : `true` ; éditeur :
/// `false`, basculé par une touche). À combiner avec [`crate::metrics::MetricsPlugin`]
/// (la donnée des courbes) et une caméra de sim fournie par le binaire.
pub struct DataVizPlugin {
    pub enabled: bool,
    pub interval: f32,
}

impl Plugin for DataVizPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<VizGizmos>()
            .insert_resource(DataViz {
                active: self.enabled,
                interval: self.interval.max(0.5),
                elapsed: 0.0,
                page: 0,
                was_active: false,
            })
            .add_systems(Startup, load_viz_font)
            .add_systems(
                Update,
                (
                    manage_viz_cameras,
                    advance_page,
                    compose_viewports,
                    draw_viz,
                )
                    .chain(),
            );

        // Les gizmos du visualiseur ne doivent paraître que dans sa caméra (sa bande),
        // pas par-dessus la sim : on les restreint à son layer.
        let mut store = app.world_mut().resource_mut::<GizmoConfigStore>();
        let (config, _) = store.config_mut::<VizGizmos>();
        config.render_layers = RenderLayers::layer(VIZ_LAYER);
    }
}

/// Police du visualiseur. La police Bevy par défaut est ASCII-only (pas d'accents) ; on
/// embarque DejaVu Sans (libre) pour rendre le français — noms de gènes compris — comme
/// dans l'aperçu egui.
#[derive(Resource)]
struct VizFont(Handle<Font>);

/// `Startup` : charge la police du visualiseur depuis `assets/fonts/`.
fn load_viz_font(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(VizFont(assets.load("fonts/DejaVuSans.ttf")));
}

/// Marqueur de la caméra du visualiseur (bande du bas).
#[derive(Component)]
struct VizCamera;
/// Marqueur de la caméra de fond/lettre-box (plein cadre, nettoie seulement).
#[derive(Component)]
struct LetterboxCamera;
/// Marqueur des entités d'affichage recréées à chaque frame (texte, barres).
#[derive(Component)]
struct VizEntity;

/// `Update` : crée les deux caméras d'encadrement **à l'activation** et les détruit au
/// retour en mode normal. Crucial pour le fenêtré : en mode normal, il ne doit rester
/// qu'une seule `Camera2d`, sinon bevy_egui ne résout plus son contexte primaire (plus
/// aucun panneau) et les requêtes `single()` de caméra échouent. On les crée donc
/// seulement en présentation/vidéo, ciblant le même rendu que la caméra de sim.
fn manage_viz_cameras(
    mut commands: Commands,
    viz: Res<DataViz>,
    existing: Query<Entity, Or<(With<VizCamera>, With<LetterboxCamera>)>>,
    sim: Query<
        Option<&RenderTarget>,
        (With<Camera2d>, Without<VizCamera>, Without<LetterboxCamera>),
    >,
) {
    let present = !existing.is_empty();
    if !viz.active {
        // Retour en mode normal : on retire les caméras d'encadrement (egui revient).
        if present {
            for e in &existing {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    if present {
        return; // déjà en place ; `compose_viewports` règle leurs viewports.
    }

    // Activation : on crée les caméras (inactives ; `compose_viewports` les allumera une
    // fois leurs viewports posés, le frame suivant — pas de flash plein cadre).
    let target = sim.single().ok().flatten().cloned();

    // Fond / lettre-box : plein cadre (pas de viewport), nettoie en noir, ne rend rien.
    let mut letterbox = commands.spawn((
        Camera2d,
        Camera {
            order: -1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Projection::from(OrthographicProjection::default_2d()),
        RenderLayers::layer(LETTERBOX_LAYER),
        LetterboxCamera,
    ));
    if let Some(t) = &target {
        letterbox.insert(t.clone());
    }

    // Visualiseur : toile logique fixe 900×700 (origine au centre), au-dessus de la sim.
    let mut viz = commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.06, 0.06, 0.09)),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: VIZ_W,
                height: VIZ_H,
            },
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(VIZ_LAYER),
        VizCamera,
    ));
    if let Some(t) = &target {
        viz.insert(t.clone());
    }
}

/// `Update` : fait tourner la page affichée toutes les `interval` secondes (temps simulé,
/// donc figé en pause). Inactif → ne touche à rien.
fn advance_page(time: Res<Time<Virtual>>, mut viz: ResMut<DataViz>) {
    if !viz.active {
        return;
    }
    viz.elapsed += time.delta_secs();
    if viz.elapsed >= viz.interval {
        viz.elapsed = 0.0;
        viz.page = (viz.page + 1) % PAGES;
    }
}

/// `Update` : recompose les viewports en 9:16 quand actif, et restaure le plein cadre à la
/// désactivation. Le carré du haut reçoit la sim (arène cadrée), la bande du bas le
/// visualiseur ; les marges (fenêtre non 9:16) sont noircies par la caméra de fond.
fn compose_viewports(
    mut viz: ResMut<DataViz>,
    config: Res<SimConfig>,
    mut sim: Query<
        (&mut Camera, &mut Projection, &mut Transform),
        (With<Camera2d>, Without<VizCamera>, Without<LetterboxCamera>),
    >,
    mut viz_cam: Query<&mut Camera, (With<VizCamera>, Without<LetterboxCamera>)>,
    mut letterbox: Query<&mut Camera, With<LetterboxCamera>>,
) {
    let active = viz.active;
    if active {
        let Ok((mut sim_cam, mut sim_proj, mut sim_tf)) = sim.single_mut() else {
            return;
        };
        let Some(target) = sim_cam.physical_target_size() else {
            return;
        };
        let (tw, th) = (target.x as f32, target.y as f32);
        // Plus grand rectangle 9:16 inscrit dans la cible (lettre-box au besoin).
        const ASPECT: f32 = 9.0 / 16.0;
        let (rw, rh) = if tw / th > ASPECT {
            (th * ASPECT, th)
        } else {
            (tw, tw / ASPECT)
        };
        let ox = ((tw - rw) * 0.5).max(0.0);
        let oy = ((th - rh) * 0.5).max(0.0);
        let square = rw; // carré du haut = pleine largeur du cadre 9:16

        // Caméra de sim : viewport = carré du haut, arène cadrée (AutoMin), centrée.
        sim_cam.viewport = Some(Viewport {
            physical_position: UVec2::new(ox as u32, oy as u32),
            physical_size: UVec2::new(square.max(1.0) as u32, square.max(1.0) as u32),
            ..default()
        });
        sim_cam.is_active = true;
        let span = 2.0 * config.arena_half_extent * ARENA_MARGIN;
        match &mut *sim_proj {
            Projection::Orthographic(o) => {
                o.scaling_mode = ScalingMode::AutoMin {
                    min_width: span,
                    min_height: span,
                };
            }
            other => {
                *other = Projection::from(OrthographicProjection {
                    scaling_mode: ScalingMode::AutoMin {
                        min_width: span,
                        min_height: span,
                    },
                    ..OrthographicProjection::default_2d()
                });
            }
        }
        sim_tf.translation.x = 0.0;
        sim_tf.translation.y = 0.0;

        if let Ok(mut vc) = viz_cam.single_mut() {
            vc.viewport = Some(Viewport {
                physical_position: UVec2::new(ox as u32, (oy + square) as u32),
                physical_size: UVec2::new(rw.max(1.0) as u32, (rh - square).max(1.0) as u32),
                ..default()
            });
            vc.is_active = true;
        }
        if let Ok(mut lc) = letterbox.single_mut() {
            lc.viewport = None;
            lc.is_active = true;
        }
    } else if viz.was_active {
        // Désactivation : on rend le plein cadre à la sim (le fenêtré reprend son cadrage
        // egui via `set_sim_camera`) et on coupe les caméras d'encadrement.
        if let Ok((mut sim_cam, mut sim_proj, _)) = sim.single_mut() {
            sim_cam.viewport = None;
            *sim_proj = Projection::from(OrthographicProjection::default_2d());
        }
        if let Ok(mut vc) = viz_cam.single_mut() {
            vc.is_active = false;
        }
        if let Ok(mut lc) = letterbox.single_mut() {
            lc.is_active = false;
        }
    }
    viz.was_active = active;
}

// ---------------------------------------------------------------------------
// Tracé (immédiat : on recrée texte/barres à chaque frame ; les gizmos le sont déjà)
// ---------------------------------------------------------------------------

/// Coin haut-gauche px (0..VIZ_W, 0..VIZ_H, y vers le bas) → monde de la toile (origine
/// au centre, y vers le haut).
fn p(x: f32, y: f32) -> Vec2 {
    Vec2::new(x - VIZ_W * 0.5, VIZ_H * 0.5 - y)
}

fn srgb([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
}

/// Spawne un texte aligné en haut-gauche à la position px de la toile (police DejaVu pour
/// les accents).
fn text(
    commands: &mut Commands,
    font: &Handle<Font>,
    s: impl Into<String>,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
) {
    let pos = p(x, y);
    commands.spawn((
        Text2d::new(s.into()),
        TextFont {
            font: font.clone(),
            font_size: size,
            ..default()
        },
        TextColor(color),
        Anchor::TOP_LEFT,
        Transform::from_xyz(pos.x, pos.y, 0.3),
        RenderLayers::layer(VIZ_LAYER),
        VizEntity,
    ));
}

/// Spawne un rectangle plein, coin haut-gauche à (x, y) px, taille (w, h) px.
fn rect(commands: &mut Commands, x: f32, y: f32, w: f32, h: f32, color: Color, z: f32) {
    let pos = p(x, y);
    commands.spawn((
        Sprite::from_color(color, Vec2::new(w.max(0.0), h)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(pos.x, pos.y, z),
        RenderLayers::layer(VIZ_LAYER),
        VizEntity,
    ));
}

/// Une barre de progression (fond + remplissage), `frac` ∈ [0, 1].
fn bar(commands: &mut Commands, x: f32, y: f32, w: f32, h: f32, frac: f32, fill: Color) {
    rect(commands, x, y, w, h, Color::srgb(0.16, 0.16, 0.20), 0.1);
    rect(commands, x, y, w * frac.clamp(0.0, 1.0), h, fill, 0.2);
}

/// `Update` : redessine tout le visualiseur. Recrée les entités de texte/barres (les
/// précédentes sont retirées) ; les gizmos sont déjà immédiats.
#[allow(clippy::too_many_arguments)]
fn draw_viz(
    mut commands: Commands,
    viz: Res<DataViz>,
    old: Query<Entity, With<VizEntity>>,
    mut gizmos: Gizmos<VizGizmos>,
    font: Res<VizFont>,
    config: Res<SimConfig>,
    history: Res<History>,
    selection: Res<Selection>,
    stats_q: Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
    inspect_q: Query<
        (
            &Species,
            &Reserve,
            &Genotype,
            &Vision,
            &Perception,
            &Action,
            &Brain,
            &Generation,
            &Age,
        ),
        With<Agent>,
    >,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    if !viz.active {
        return;
    }
    let font = &font.0;

    draw_stats(&mut commands, font, &stats_q);

    match viz.page {
        0 => draw_curves(&mut commands, &mut gizmos, font, &history, &config),
        _ => draw_inspector(&mut commands, &mut gizmos, font, &selection, &inspect_q),
    }

    // Indicateur de page (coin bas-droit).
    let label = if viz.page == 0 {
        "courbes"
    } else {
        "inspecteur"
    };
    text(
        &mut commands,
        font,
        format!("● {label}"),
        VIZ_W - 150.0,
        VIZ_H - 24.0,
        15.0,
        Color::srgb(0.5, 0.5, 0.55),
    );
}

/// Stats globales, toujours en haut de la bande. Mêmes nombres que `editor::stats_section`.
fn draw_stats(
    commands: &mut Commands,
    font: &Handle<Font>,
    agents: &Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
) {
    let s = live_stats(agents);
    let ink = Color::srgb(0.92, 0.92, 0.95);
    text(
        commands,
        font,
        format!(
            "Pop {}    Nourriture {}    Réserve {:.0}",
            s.population, s.food, s.mean_reserve
        ),
        24.0,
        16.0,
        26.0,
        ink,
    );
    let genes: Vec<String> = TRAITS
        .iter()
        .zip(&s.mean_traits)
        .map(|(t, m)| format!("{} {:.*}", t.name, t.decimals as usize, m))
        .collect();
    text(
        commands,
        font,
        format!("gènes moy. — {}", genes.join("   ")),
        24.0,
        52.0,
        15.0,
        Color::srgb(0.65, 0.65, 0.7),
    );
}

/// Page « courbes » : population (haut) et dérive des gènes (bas), via [`crate::metrics`].
fn draw_curves(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    history: &History,
    config: &SimConfig,
) {
    if history.is_empty() {
        text(
            commands,
            font,
            "(en attente de données…)",
            24.0,
            120.0,
            18.0,
            Color::srgb(0.6, 0.6, 0.65),
        );
        return;
    }

    let (pop, y_max) = population_curves(history, config);
    text(
        commands,
        font,
        "Population par espèce",
        24.0,
        100.0,
        18.0,
        Color::WHITE,
    );
    plot(
        commands,
        gizmos,
        font,
        &pop,
        0.0,
        y_max * 1.1,
        (24.0, 126.0, 852.0, 198.0),
    );

    text(
        commands,
        font,
        "Dérive des gènes (0–1)",
        24.0,
        356.0,
        18.0,
        Color::WHITE,
    );
    let traits = trait_curves(history);
    plot(
        commands,
        gizmos,
        font,
        &traits,
        0.0,
        1.0,
        (24.0, 382.0, 852.0, 198.0),
    );
}

/// Trace un cadre, ses polylignes et une légende, dans `(x, y, w, h)` px de la toile.
fn plot(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    curves: &[Curve],
    y_min: f32,
    y_max: f32,
    (x, y, w, h): (f32, f32, f32, f32),
) {
    // Cadre.
    let center = p(x + w * 0.5, y + h * 0.5);
    gizmos.rect_2d(center, Vec2::new(w, h), Color::srgb(0.3, 0.3, 0.35));

    if curves.is_empty() {
        return;
    }
    let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
    for c in curves {
        for q in &c.pts {
            x_min = x_min.min(q[0]);
            x_max = x_max.max(q[0]);
        }
    }
    if !x_max.is_finite() || x_max <= x_min {
        return;
    }
    let y_span = (y_max - y_min).max(1e-6);
    // Point donnée → px de la toile (haut = grande valeur), puis monde.
    let map = |t: f32, v: f32| {
        let px = x + (t - x_min) / (x_max - x_min) * w;
        let py = y + h - (v - y_min) / y_span * h;
        p(px, py)
    };
    for c in curves {
        let col = srgb(c.color);
        for win in c.pts.windows(2) {
            gizmos.line_2d(map(win[0][0], win[0][1]), map(win[1][0], win[1][1]), col);
        }
    }

    // Légende : pastille + nom, en ligne sous le cadre (pas de chevauchement au-delà de 6).
    let mut lx = x;
    let ly = y + h + 4.0;
    for c in curves {
        rect(commands, lx, ly + 4.0, 10.0, 10.0, srgb(c.color), 0.2);
        text(
            commands,
            font,
            &c.name,
            lx + 14.0,
            ly,
            13.0,
            Color::srgb(0.8, 0.8, 0.85),
        );
        lx += 130.0;
    }
}

/// Page « inspecteur » : l'agent sélectionné (mêmes informations que `inspector_section`,
/// hors bouton « Capturer » qui est une interaction, pas une donnée).
fn draw_inspector(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    selection: &Selection,
    agents: &Query<
        (
            &Species,
            &Reserve,
            &Genotype,
            &Vision,
            &Perception,
            &Action,
            &Brain,
            &Generation,
            &Age,
        ),
        With<Agent>,
    >,
) {
    let dim = Color::srgb(0.6, 0.6, 0.65);
    let Some(entity) = selection.0 else {
        text(
            commands,
            font,
            "Aucun agent sélectionné.",
            24.0,
            120.0,
            18.0,
            dim,
        );
        return;
    };
    let Ok((species, reserve, genotype, vision, perception, action, brain, generation, age)) =
        agents.get(entity)
    else {
        text(
            commands,
            font,
            "L'agent inspecté n'existe plus.",
            24.0,
            120.0,
            18.0,
            dim,
        );
        return;
    };

    let immobile = genotype.locomotion().is_immobile();
    let ink = Color::srgb(0.9, 0.9, 0.93);
    let key = Color::srgb(0.7, 0.7, 0.75);

    // --- Colonne gauche : identité, énergie, gènes, action ---
    text(
        commands,
        font,
        format!("Espèce {}", species.0),
        24.0,
        100.0,
        18.0,
        ink,
    );
    text(
        commands,
        font,
        format!("Cerveau : {}", brain.name()),
        24.0,
        124.0,
        16.0,
        key,
    );
    text(
        commands,
        font,
        format!("Génération {} · âge {:.1} s", generation.0, age.0),
        24.0,
        146.0,
        16.0,
        key,
    );

    text(commands, font, "Énergie / réserve", 24.0, 176.0, 16.0, ink);
    bar(
        commands,
        24.0,
        198.0,
        410.0,
        20.0,
        reserve.fraction(),
        Color::srgb(0.35, 0.75, 0.45),
    );
    text(
        commands,
        font,
        format!("{:.0} / {:.0}", reserve.current, reserve.max),
        30.0,
        200.0,
        14.0,
        Color::WHITE,
    );

    text(
        commands,
        font,
        "Génotype (gènes hérités)",
        24.0,
        232.0,
        16.0,
        ink,
    );
    let mut gy = 256.0;
    for t in &TRAITS {
        if immobile && t.inert_when_immobile {
            continue;
        }
        text(
            commands,
            font,
            format!("{}: {:.*}", t.name, t.decimals as usize, (t.get)(genotype)),
            30.0,
            gy,
            14.0,
            key,
        );
        gy += 18.0;
    }
    if !immobile {
        text(
            commands,
            font,
            format!("coût vision/s: {:.3}", vision.metabolic_cost()),
            30.0,
            gy,
            14.0,
            key,
        );
        gy += 18.0;
    }

    // Action (sortie du cerveau).
    let heading_deg = if action.dir.length_squared() > 1e-6 {
        action.dir.to_angle().to_degrees()
    } else {
        0.0
    };
    text(
        commands,
        font,
        "Action (sortie du cerveau)",
        24.0,
        gy + 8.0,
        16.0,
        ink,
    );
    text(
        commands,
        font,
        format!("cap désiré {heading_deg:+.0}°"),
        30.0,
        gy + 30.0,
        14.0,
        key,
    );
    bar(
        commands,
        24.0,
        gy + 50.0,
        410.0,
        16.0,
        action.throttle,
        Color::srgb(0.4, 0.6, 0.9),
    );
    text(
        commands,
        font,
        format!("accélérateur {:.2}", action.throttle),
        30.0,
        gy + 50.0,
        13.0,
        Color::WHITE,
    );

    // --- Colonne droite : cerveau MLP + perception ---
    if let Brain::Mlp(mlp) = brain {
        text(
            commands,
            font,
            "Cerveau MLP (activations)",
            470.0,
            100.0,
            16.0,
            ink,
        );
        let acts = mlp.forward_activations(perception);
        draw_mlp(
            commands,
            gizmos,
            font,
            mlp,
            &acts,
            (470.0, 124.0, 406.0, 280.0),
        );
    } else {
        text(commands, font, brain.name(), 470.0, 100.0, 16.0, ink);
    }

    if !immobile {
        text(
            commands,
            font,
            format!("Perception — {} rayons", vision.ray_count),
            470.0,
            424.0,
            16.0,
            ink,
        );
        text(
            commands,
            font,
            "obstacle · cible · menace",
            470.0,
            446.0,
            13.0,
            dim,
        );
        // Une ligne par rayon (cap à ce qui tient dans la bande).
        let row_h = 14.0;
        let max_rows = (((VIZ_H - 30.0) - 468.0) / row_h).floor() as usize;
        for (i, &prox) in perception.vision.iter().take(max_rows).enumerate() {
            let target = perception.target.get(i).copied().unwrap_or(0.0);
            let threat = perception.threat.get(i).copied().unwrap_or(0.0);
            let ry = 468.0 + i as f32 * row_h;
            bar(
                commands,
                470.0,
                ry,
                120.0,
                row_h - 3.0,
                prox,
                Color::srgb(0.6, 0.6, 0.62),
            );
            bar(
                commands,
                596.0,
                ry,
                120.0,
                row_h - 3.0,
                target,
                Color::srgb(0.86, 0.51, 0.16),
            );
            bar(
                commands,
                722.0,
                ry,
                120.0,
                row_h - 3.0,
                threat,
                Color::srgb(0.82, 0.24, 0.24),
            );
        }
    }
}

/// Couleur d'un nœud selon son activation (froid<0<chaud), mêmes teintes que l'éditeur.
fn activation_color(v: f32) -> Color {
    let t = v.clamp(-1.0, 1.0).abs();
    let base = 0.24;
    let lerp = |to: f32| base + (to - base) * t;
    if v >= 0.0 {
        Color::srgb(lerp(0.94), lerp(0.59), lerp(0.16)) // chaud
    } else {
        Color::srgb(lerp(0.24), lerp(0.55), lerp(0.94)) // froid
    }
}

/// Dessine le MLP (arêtes teintées par poids, nœuds par activation, taille ∝ |biais|),
/// dans `(x, y, w, h)` px. Reprend la géométrie de l'éditeur mais en gizmos.
fn draw_mlp(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    mlp: &MlpBrain,
    acts: &[Vec<f32>],
    (x, y, w, h): (f32, f32, f32, f32),
) {
    let sizes = mlp.layer_sizes();
    if sizes.len() < 2 {
        return;
    }
    let cols = sizes.len();
    let widest = *sizes.iter().max().unwrap_or(&1);
    const LABEL_W: f32 = 34.0;
    let (ix, iw) = (x + LABEL_W, w - 2.0 * LABEL_W);

    // Position px (toile) du nœud `node` de la colonne `col`.
    let node_px = |col: usize, node: usize, n: usize| -> (f32, f32) {
        let px = if cols == 1 {
            ix + iw * 0.5
        } else {
            ix + iw * col as f32 / (cols - 1) as f32
        };
        let py = if n == 1 {
            y + h * 0.5
        } else {
            y + h * (node as f32 + 0.5) / n as f32
        };
        (px, py)
    };

    // Arêtes d'abord (sous les nœuds, ordre de tracé du groupe gizmo).
    for col in 0..cols - 1 {
        let (from_n, to_n) = (sizes[col], sizes[col + 1]);
        let weights = (col < mlp.weight_layers()).then(|| mlp.layer_weights(col));
        for o in 0..to_n {
            for i in 0..from_n {
                let col_c = match weights {
                    Some((wts, fan_in, _)) if o * fan_in + i < wts.len() => {
                        let wt = wts[o * fan_in + i];
                        let a = (wt.abs() * 0.9).clamp(0.05, 0.9);
                        if wt >= 0.0 {
                            Color::srgba(0.9, 0.59, 0.24, a)
                        } else {
                            Color::srgba(0.27, 0.55, 0.9, a)
                        }
                    }
                    _ => Color::srgba(0.3, 0.3, 0.3, 0.3),
                };
                let (ax, ay) = node_px(col, i, from_n);
                let (bx, by) = node_px(col + 1, o, to_n);
                gizmos.line_2d(p(ax, ay), p(bx, by), col_c);
            }
        }
    }

    // Nœuds : rayon de référence rétréci par |biais| (normalisé au plus grand biais).
    let base_r = (h / (widest as f32 * 2.2)).clamp(2.5, 9.0);
    let max_bias = (0..mlp.weight_layers())
        .flat_map(|l| mlp.layer_biases(l).iter().copied())
        .fold(0.0_f32, |acc, b| acc.max(b.abs()));
    for (col, &n) in sizes.iter().enumerate() {
        let biases = (col >= 1 && col - 1 < mlp.weight_layers()).then(|| mlp.layer_biases(col - 1));
        for node in 0..n {
            let act = acts
                .get(col)
                .and_then(|l| l.get(node))
                .copied()
                .unwrap_or(0.0);
            let radius = match (biases, max_bias > 1e-6) {
                (Some(b), true) => {
                    let t = (b.get(node).copied().unwrap_or(0.0).abs() / max_bias).clamp(0.0, 1.0);
                    base_r * (0.35 + 0.65 * t)
                }
                _ => base_r,
            };
            let (px, py) = node_px(col, node, n);
            gizmos.circle_2d(p(px, py), radius, activation_color(act));
        }
    }

    // Étiquettes des groupes d'entrée (vis/cib/men) et des sorties (avant/côté).
    let n_in = sizes[0];
    if n_in.is_multiple_of(3) {
        let rays = n_in / 3;
        for (g, name) in ["vis", "cib", "men"].iter().enumerate() {
            let (_, py) = node_px(0, g * rays + rays / 2, n_in);
            text(
                commands,
                font,
                *name,
                x - 2.0,
                py - 6.0,
                11.0,
                Color::srgb(0.6, 0.6, 0.65),
            );
        }
    }
    let last = cols - 1;
    if sizes[last] == MlpBrain::OUTPUTS {
        for (o, name) in ["avant", "côté"].iter().enumerate() {
            let (px, py) = node_px(last, o, MlpBrain::OUTPUTS);
            text(
                commands,
                font,
                *name,
                px + 8.0,
                py - 6.0,
                11.0,
                Color::srgb(0.6, 0.6, 0.65),
            );
        }
    }
}
