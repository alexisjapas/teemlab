//! **Frosted-glass backdrop** for the HUD chips floating over the arena (the
//! comp's blurred pills: time read-out, Paused, Follow, zoom/fit — cf.
//! `panels::overlay_frame` / `panels::central_overlay`).
//!
//! egui cannot sample what sits behind it, so the "backdrop blur" is built the
//! Bevy way: a second camera ([`BlurCamera`]) re-renders the world each frame
//! into a small offscreen texture (1/[`DOWNSCALE`] of the window, linear-sampled
//! → a cheap, stable blur), registered with egui; each chip then paints the
//! sub-rect of that texture under its translucent fill. Same offscreen-camera
//! idiom as `bin/record` and `dataviz`; the primary egui context is untouched
//! (bevy_egui binds it to the *first* camera only). Rendering only — the sim
//! never knows (DEV Rule 1).

use bevy::camera::{RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy_egui::{EguiTextureHandle, EguiUserTextures, egui};
use teemlab::visuals::BlurCamera;

use crate::panels;

/// Window-to-texture downscale: the blur radius, effectively (linear sampling of
/// a 1/6 render ≈ a 6 px box blur at display size).
const DOWNSCALE: u32 = 6;

/// The blurred world snapshot the HUD chips paint under their fill. `texture`
/// spans the **whole window** (same framing as the sim camera), so a chip's
/// backdrop uv is simply its screen rect divided by [`Self::window`].
#[derive(Resource, Default)]
pub(crate) struct HudBlur {
    /// Strong handle backing the offscreen render target.
    handle: Option<Handle<Image>>,
    /// Physical size of the target (recreated when the window resizes).
    size: UVec2,
    /// The egui id of the texture — `None` until the first frame.
    pub(crate) texture: Option<egui::TextureId>,
    /// Logical window size backing the uv mapping of `texture`.
    pub(crate) window: egui::Vec2,
}

impl HudBlur {
    /// The uv sub-rect of the blurred snapshot under a screen-space `rect`.
    pub(crate) fn uv(&self, rect: egui::Rect) -> egui::Rect {
        let (w, h) = (self.window.x.max(1.0), self.window.y.max(1.0));
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x / w, rect.min.y / h),
            egui::pos2(rect.max.x / w, rect.max.y / h),
        )
    }
}

/// Keeps the offscreen camera mirroring the sim camera (same translation, same
/// world coverage) into a window-sized/[`DOWNSCALE`] texture. Runs right after
/// `set_sim_camera` in the egui pass, so the snapshot matches this frame's
/// framing. Inactive off-Observe (empty central rect) and in presentation mode
/// (extra dataviz cameras → the sim-camera lookup is ambiguous → we bail).
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_blur_camera(
    mut commands: Commands,
    windows: Query<&Window>,
    central: Res<panels::CentralRect>,
    mut images: ResMut<Assets<Image>>,
    mut user_textures: ResMut<EguiUserTextures>,
    mut blur: ResMut<HudBlur>,
    sim_cam: Query<(&Transform, &Projection), (With<Camera2d>, Without<BlurCamera>)>,
    mut blur_cam: Query<(Entity, &mut Camera, &mut Transform, &mut Projection), With<BlurCamera>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let rect = central.0;
    let active = rect.width() >= 1.0 && rect.height() >= 1.0;
    // Presentation mode spawns extra cameras → no unique sim camera → idle.
    let Ok((sim_tf, sim_proj)) = sim_cam.single() else {
        if let Ok((_, mut cam, ..)) = blur_cam.single_mut() {
            cam.is_active = false;
        }
        return;
    };
    let Projection::Orthographic(sim_ortho) = sim_proj else {
        return;
    };
    let s = sim_ortho.scale;

    // (Re)create the render target when the window size changes.
    let physical = UVec2::new(
        (window.physical_width() / DOWNSCALE).max(1),
        (window.physical_height() / DOWNSCALE).max(1),
    );
    let mut retargeted = false;
    if blur.handle.is_none() || blur.size != physical {
        if let Some(old) = blur.handle.take() {
            user_textures.remove_image(old.id());
            images.remove(old.id());
        }
        let mut image = Image::new_fill(
            Extent3d {
                width: physical.x,
                height: physical.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage =
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        let handle = images.add(image);
        blur.texture = Some(user_textures.add_image(EguiTextureHandle::Strong(handle.clone())));
        blur.handle = Some(handle);
        blur.size = physical;
        retargeted = true;
    }
    blur.window = egui::vec2(window.width(), window.height());

    // Mirror the sim framing: full-window world coverage at the sim's scale.
    let coverage = ScalingMode::Fixed {
        width: window.width() * s,
        height: window.height() * s,
    };
    if let Ok((entity, mut cam, mut tf, mut proj)) = blur_cam.single_mut() {
        cam.is_active = active;
        *tf = *sim_tf;
        if let Projection::Orthographic(ortho) = &mut *proj {
            ortho.scaling_mode = coverage;
        }
        if retargeted {
            commands
                .entity(entity)
                .insert(RenderTarget::from(blur.handle.clone().expect("just set")));
        }
    } else {
        commands.spawn((
            Camera2d,
            Camera {
                // Before the main camera; separate target, so purely cosmetic.
                order: -1,
                is_active: active,
                ..default()
            },
            RenderTarget::from(blur.handle.clone().expect("just set")),
            Projection::from(OrthographicProjection {
                scaling_mode: coverage,
                ..OrthographicProjection::default_2d()
            }),
            *sim_tf,
            BlurCamera,
        ));
    }
}
