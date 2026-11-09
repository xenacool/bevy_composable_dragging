//! Common utilities shared across examples.
//!
//! Every example compiles this module on its own and uses a subset of it.
#![allow(dead_code)]

use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy_composable_dragging::GIZMO_LAYER;

// Adapted from `bevy_gradmesh`, so its lint findings are allowed instead of edited.
#[allow(clippy::pedantic, clippy::unnecessary_cast)]
pub mod screenshot;

pub mod script;

pub mod tour;

#[cfg(test)]
pub mod mouse;

/// The default plugins with a 1280x720 window and synchronous shader compilation, so screenshots
/// never capture a frame that is still compiling.
pub fn default_plugins(title: &str) -> impl PluginGroup {
    DefaultPlugins
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("bevy_composable_dragging: {title}"),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        })
}

/// The render layer of the screenshot title overlay, so its camera never draws the scene.
pub const OVERLAY_LAYER: usize = 1;

/// The render layer of the HUD, drawn by the camera of [`spawn_hud_camera`] over the scene.
pub const HUD_LAYER: usize = 2;

/// Marks the camera that draws the scene, as opposed to the one that draws the title overlay.
#[derive(Component)]
pub struct SceneCamera;

/// Marks the camera that draws the HUD over the scene.
#[derive(Component)]
pub struct HudCamera;

/// Spawns the camera that draws the screenshot title overlay.
///
/// It renders first and has a lower order than the scene camera, so the scene camera wins
/// picking and drags are projected through it.
fn spawn_overlay_camera(commands: &mut Commands) {
    commands.spawn((
        Camera2d,
        RenderLayers::layer(OVERLAY_LAYER),
        Camera {
            order: -1,
            ..default()
        },
        // The overlay text sits at z = 1000, which would be clipped for a camera at the origin.
        Transform::from_xyz(0.0, 0.0, 100.0),
    ));
}

/// Spawns an orthographic scene camera looking down the Z axis at the XY plane, showing
/// `viewport_height` world units vertically.
pub fn spawn_cameras(commands: &mut Commands, viewport_height: f32) {
    spawn_overlay_camera(commands);
    let layers = RenderLayers::from_layers(&[0, GIZMO_LAYER]);
    commands.spawn((
        SceneCamera,
        Camera3d::default(),
        layers,
        Camera {
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 10.0),
    ));
}

/// Spawns a `Camera2d` that shows `viewport_height` world units vertically, for scenes drawn
/// with `Rendering::Flat`.
pub fn spawn_cameras_2d(commands: &mut Commands, viewport_height: f32) {
    spawn_overlay_camera(commands);
    let layers = RenderLayers::from_layers(&[0, GIZMO_LAYER]);
    commands.spawn((
        SceneCamera,
        Camera2d,
        layers,
        Camera {
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// Spawns a `Camera2d` that draws the nodes on [`HUD_LAYER`] over the scene, showing
/// `viewport_height` world units vertically with the origin in the middle of the screen.
///
/// It renders after the scene camera without clearing, and its order is higher, so presses on
/// the HUD win over the scene below. Give the HUD nodes `ViewNode::flat` and
/// `ViewNode::on_layer(HUD_LAYER)`; the scene camera never draws or picks them.
pub fn spawn_hud_camera(commands: &mut Commands, viewport_height: f32) {
    commands.spawn((
        HudCamera,
        Camera2d,
        RenderLayers::layer(HUD_LAYER),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// Spawns a perspective scene camera at `eye` looking at `target`, and a light for lit materials.
pub fn spawn_perspective_scene(commands: &mut Commands, eye: Vec3, target: Vec3) {
    spawn_overlay_camera(commands);
    let layers = RenderLayers::from_layers(&[0, GIZMO_LAYER]);
    commands.spawn((
        SceneCamera,
        Camera3d::default(),
        layers,
        Camera {
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            ..default()
        },
        Transform::from_xyz(2.0, 6.0, 8.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
}

/// Returns `order` with the item at slot `from` moved to slot `to`.
pub fn move_item(order: &[usize], from: usize, to: usize) -> Vec<usize> {
    let mut moved = order.to_vec();
    let item = moved.remove(from);
    moved.insert(to, item);
    moved
}
