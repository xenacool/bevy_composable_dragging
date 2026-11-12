//! A drag-and-drop editor for physics scenarios built from gradmesh shapes, simulated with avian.
//!
//! Drag a shape chip from the palette onto a free cell, or a placed piece to another cell. The
//! radial menu in the corner chooses how the pieces are simulated: the collider built from each
//! mesh, the detail of the meshes, the timestep and substeps, the scale of the pieces and the
//! restitution and friction. Play turns the pieces into avian bodies, and while the simulation
//! runs a body can be grabbed with the cursor. A grabbed body is driven by a speed that is
//! capped from its own size, so no flick of the cursor can carry it through a thin wall.
//!
//! The same shapes, colliders and rigs are measured headlessly by the tests of this example,
//! which write tables of where each one holds and where it breaks to `target/physics_lab`.
//!
//! Run with `cargo run --example physics_lab --features shapes-3d`.

use avian3d::prelude::*;
use bevy::camera::visibility::RenderLayers;
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_composable_dragging::hud::{HudPlugin, Screen, Theme};
use bevy_composable_dragging::radial::RadialMenuPlugin;
use bevy_composable_dragging::{DragModel, DraggablePlugin, GIZMO_LAYER};

#[cfg(test)]
mod clicks;
mod drag;
mod editor;
#[cfg(all(test, feature = "fuzz"))]
mod fuzz;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod mesh_check;
mod scenario;
#[cfg(test)]
mod sweeps;

#[path = "../common/mod.rs"]
mod common;

use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;
use common::{HUD_LAYER, SceneCamera, spawn_hud_camera};
use editor::{Lab, parts, tint, view};

/// The height of the HUD camera in HUD units.
const HUD_HEIGHT: f32 = 720.0;
const ASPECT: f32 = 16.0 / 9.0;

/// Marks a body of the running simulation.
#[derive(Component)]
struct LabBody;

/// The fastest a body may be dragged, from its smallest dimension and the step.
#[derive(Component)]
struct SpeedLimit(f32);

/// The body that is held by the cursor.
#[derive(Resource, Default)]
struct Held(Option<Entity>);

/// Marks the physics info label of the held body.
#[derive(Component)]
struct PhysicsLabel;

fn theme() -> Theme {
    Theme::new(HUD_LAYER)
}

fn scene_camera() -> Transform {
    Transform::from_xyz(0.0, 4.5, 17.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y)
}

fn setup(mut commands: Commands) {
    spawn_hud_camera(&mut commands, HUD_HEIGHT);
    let layers = RenderLayers::from_layers(&[0, GIZMO_LAYER]);
    commands.spawn((SceneCamera, Camera3d::default(), layers, scene_camera()));
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-5.0, 9.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 300.0,
        ..default()
    });
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(400.0, 2.0, 400.0),
        Restitution::new(0.0),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
}

/// Marks a wall of the simulation.
#[derive(Component)]
struct LabWall;

/// Turns the pieces into bodies when the simulation starts and takes the bodies away when it
/// stops, with the timestep and the substeps that were chosen.
fn sync_bodies(
    model: Res<DragModel<Lab>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bodies: Query<Entity, With<LabBody>>,
    walls: Query<Entity, With<LabWall>>,
    mut fixed: ResMut<Time<Fixed>>,
    mut substeps: ResMut<SubstepCount>,
    mut gravity: ResMut<Gravity>,
    mut was_running: Local<bool>,
) {
    let lab = model.state();
    if lab.running == *was_running {
        return;
    }
    *was_running = lab.running;
    for body in &bodies {
        commands.entity(body).despawn();
    }
    for wall in &walls {
        commands.entity(wall).despawn();
    }
    if !lab.running {
        return;
    }
    fixed.set_timestep_seconds(f64::from(lab.setup.step()));
    substeps.0 = lab.setup.substeps();
    gravity.0 = Vec3::NEG_Y * lab.setup.gravity();
    for piece in &lab.pieces {
        let Some(parts) = parts(piece, &lab.setup) else {
            continue;
        };
        commands
            .spawn((
                LabBody,
                SpeedLimit(parts.speed_limit),
                parts.body,
                parts.collider,
                LockedAxes::new().lock_translation_z(),
                Restitution::new(parts.restitution),
                Friction::new(parts.friction),
                ColliderDensity(lab.setup.density()),
                Mesh3d(meshes.add(parts.mesh)),
                // The meshes of gradmesh face inward by winding, so both sides are drawn.
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: tint(piece.primitive),
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })),
                Transform::from_translation(parts.position),
            ))
            .observe(grab);
    }
    // Spawn walls
    for wall in &lab.walls {
        commands.spawn((
            LabWall,
            RigidBody::Static,
            Collider::cuboid(wall.size.x, wall.size.y, wall.size.z),
            Mesh3d(
                meshes.add(
                    bevy_gradmesh::Shape::cuboid(
                        Vec3::new(wall.size.x * 2.0, wall.size.y * 2.0, wall.size.z * 2.0),
                        0.05,
                        bevy_gradmesh::ShapeDetail::Low,
                    )
                    .build(),
                ),
            ),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.55, 0.45, 0.75),
                ..default()
            })),
            Transform::from_translation(wall.position),
        ));
    }
}

/// Holds the body that was pressed.
fn grab(press: On<PointerPress>, bodies: Query<(), With<LabBody>>, mut held: ResMut<Held>) {
    if press.button == PointerButton::Primary && bodies.contains(press.entity) {
        held.0 = Some(press.entity);
    }
}

/// Drives the held body toward the cursor at a speed it cannot tunnel at.
fn drive_held(
    mut held: ResMut<Held>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<SceneCamera>>,
    fixed: Res<Time<Fixed>>,
    mut bodies: Query<(&Position, &SpeedLimit, &mut LinearVelocity)>,
) {
    let Some(entity) = held.0 else {
        return;
    };
    if !buttons.pressed(MouseButton::Left) {
        held.0 = None;
        return;
    }
    let (camera, transform) = *camera;
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(transform, cursor) else {
        return;
    };
    let Some(target) = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z))
    else {
        return;
    };
    let Ok((position, limit, mut velocity)) = bodies.get_mut(entity) else {
        held.0 = None;
        return;
    };
    velocity.0 = drag::drive(position.0, target, fixed.timestep().as_secs_f32(), limit.0);
}

/// Shows the physics properties of the held body as a label above it.
fn show_physics_label(
    held: Res<Held>,
    camera: Single<(&Camera, &GlobalTransform), With<SceneCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    bodies: Query<(&Position, &LinearVelocity, &ComputedMass)>,
    mut labels: Query<(Entity, &mut Transform, &mut Text2d), With<PhysicsLabel>>,
    mut commands: Commands,
) {
    let Some(entity) = held.0 else {
        // Despawn any existing label when nothing is held
        for (label_entity, _, _) in &mut labels {
            commands.entity(label_entity).despawn();
        }
        return;
    };

    let Ok((position, velocity, mass)) = bodies.get(entity) else {
        return;
    };

    let (camera, cam_transform) = *camera;
    let size = window.size();

    // Project 3D position to 2D viewport coordinates
    let Ok(screen_pos) = camera.world_to_viewport(cam_transform, position.0) else {
        return;
    };

    // Convert viewport (pixel) coordinates to HUD coordinates (centered origin)
    let hud_x = screen_pos.x - size.x / 2.0;
    let hud_y = screen_pos.y - size.y / 2.0;
    let label_pos = Vec3::new(hud_x, hud_y + 2.0, 0.0);

    let text = format!(
        "pos: ({:+.1}, {:+.1})\nvel: {:.2} m/s\nmass: {:.2} kg",
        position.0.x,
        position.0.y,
        velocity.0.length(),
        mass.value(),
    );

    // Update existing label or create a new one
    let mut label_exists = false;
    for (_, mut label_transform, mut text_comp) in &mut labels {
        if label_transform.translation.z > -1e6 {
            label_transform.translation = label_pos;
            text_comp.0.clone_from(&text);
            label_exists = true;
            break;
        }
    }

    if !label_exists {
        commands.spawn((
            PhysicsLabel,
            Text2d::new(text),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.95, 0.95, 0.9)),
            Transform::from_translation(label_pos),
            RenderLayers::layer(HUD_LAYER),
        ));
    }
}

/// Keeps gravity in sync with the lab setup even while editing.
fn sync_gravity(model: Res<DragModel<Lab>>, mut gravity: ResMut<Gravity>) {
    gravity.0 = Vec3::NEG_Y * model.state().setup.gravity();
}

/// Moves the camera to match the chosen distance and height.
fn sync_camera(
    model: Res<DragModel<Lab>>,
    mut camera: Single<&mut Transform, With<SceneCamera>>,
    mut prev: Local<Option<(usize, usize)>>,
) {
    let lab = model.state();
    let key = (lab.setup.camera_distance, lab.setup.camera_height);
    if Some(key) == *prev {
        return;
    }
    *prev = Some(key);
    let distance = lab.setup.camera_distance();
    let height = lab.setup.camera_height();
    camera.translation = Vec3::new(0.0, height, distance);
    camera.rotation = Transform::from_xyz(0.0, height, distance)
        .looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y)
        .rotation;
}

/// Adds the lab to `app`: everything but the windowing and rendering plugins.
fn lab(app: &mut App) -> &mut App {
    let screen = Screen::for_camera(HUD_HEIGHT, scene_camera(), ASPECT);
    let theme = theme();
    let view_theme = theme;
    app.add_plugins(PhysicsPlugins::default())
        .add_plugins(
            DraggablePlugin::new(Lab::new(screen), move |lab| view(lab, &view_theme))
                .with_transition_duration(theme.tween_seconds),
        )
        .add_plugins(HudPlugin::<Lab>::new(HUD_HEIGHT).camera::<SceneCamera>())
        .add_plugins(RadialMenuPlugin::<Lab>::new(theme))
        .init_resource::<Held>()
        .add_systems(Startup, setup)
        .add_systems(Update, sync_bodies)
        .add_systems(Update, sync_gravity)
        .add_systems(Update, sync_camera)
        .add_systems(Update, show_physics_label)
        .add_systems(FixedUpdate, drive_held)
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app.add_plugins(common::default_plugins("physics_lab"));
    lab(app)
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(-7.7, 0.5),
            Vec2::new(0.0, 2.6),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_physics_lab.gif")
                .with_title("physics_lab")
                .with_description("3D physics shapes dragged onto a grid, simulated with avian")
                .animation(600),
        );
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}
