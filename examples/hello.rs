//! A static scene that exercises the screenshot pipeline shared by every example.

use bevy::prelude::*;

mod common;
use common::screenshot::AutoScreenshotPlugin;

fn main() {
    App::new()
        .add_plugins(common::default_plugins("hello"))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_hello.png")
                .with_title("hello")
                .with_description("Static scene")
                .wait_frames(10),
        )
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    common::spawn_cameras(&mut commands, 6.0);

    let track = meshes.add(Rectangle::new(4.0, 0.3));
    let knob = meshes.add(Circle::new(0.5));
    let track_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.27, 0.32),
        unlit: true,
        ..default()
    });
    let knob_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.6, 0.2),
        unlit: true,
        ..default()
    });

    commands.spawn((
        Mesh3d(track),
        MeshMaterial3d(track_material),
        Transform::default(),
    ));
    commands.spawn((
        Mesh3d(knob),
        MeshMaterial3d(knob_material),
        Transform::from_xyz(-2.0, 0.0, 0.1),
    ));
}
