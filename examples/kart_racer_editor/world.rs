//! Building the scene: the light, the ground, the kart model and everything along the track.
//!
//! Ported from the `bevy_gradmesh` kart racer example. [`spawn_environment`] builds the light,
//! which does not depend on the track, [`sync_ground`] builds the land from the [`Ground`]
//! resource, one entity per chunk, [`spawn_driver`] builds the kart and the
//! camera that follows it, and [`spawn_track`] builds the road, curbs, guardrails, checkpoint
//! arches, light posts, coins and scenery from a [`TrackDesign`] and marks all of it with
//! [`TrackEntity`], so a rebuild only has to despawn the marked entities.

use avian3d::prelude::{Collider, RigidBody};
use bevy::camera::visibility::Visibility;
use bevy::color::LinearRgba;
use bevy::ecs::system::ResMut;
use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy_gradmesh::{
    BatchRecorder, DeformationParams, GradMeshMaterial, MixSpace, Shape, ShapeDetail, Style,
    additive, dual_directional, dual_radial_alpha, opaque, transparent,
};

use crate::ground::Ground;
use crate::kart::{Kart, KartCamera, body};
use crate::race::Coin;
use crate::track::{BakedTrack, DecorationKind, DecorationSite, TrackDesign};

/// How many curb stripes stand along each side of the road.
const CURB_COUNT: usize = 60;
/// How many guardrail posts stand along each side of the road.
const POST_COUNT: usize = 30;
/// How many light posts stand along the road, alternating sides.
const LIGHT_POST_COUNT: usize = 20;

/// Marks everything that is built from the [`TrackDesign`]; it is despawned and spawned again
/// when the design changes.
#[derive(Component)]
pub struct TrackEntity;

/// Marks the road surface, whose mesh is in world space.
#[derive(Component)]
pub struct RoadSurface;

/// Marks a chunk of the ground, with the key of the ground it was built from, so only the
/// chunks that changed are built again.
#[derive(Component)]
pub struct GroundChunk {
    /// The index of the chunk in the ground.
    pub index: usize,
    /// What the mesh of the chunk was built from, see [`Ground::chunk_key`].
    pub key: u64,
}

/// Spawns the camera that follows the kart and the kart model.
pub fn spawn_driver(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
) {
    commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        KartCamera,
        Transform::from_xyz(0.0, 12.0, -20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    spawn_kart(commands, meshes, materials);
}

/// The surfaces a kart drives on: the road and the chunks of the ground.
type Surface = Or<(With<GroundChunk>, With<RoadSurface>)>;

/// Gives the road and the ground a collider from their meshes while a kart exists, to the ones
/// that have none, and takes the colliders away when there is no kart. Building a collider is
/// slow and the surfaces are built again on every step of an edit, so they only get one for a
/// drive.
pub fn sync_surface_colliders(
    mut commands: Commands,
    karts: Query<(), With<Kart>>,
    surfaces: Query<(Entity, &Mesh3d, Has<Collider>), Surface>,
    meshes: Res<Assets<Mesh>>,
) {
    let driving = !karts.is_empty();
    for (entity, mesh, has_collider) in &surfaces {
        match (driving, has_collider) {
            (true, false) => {
                if let Some(collider) = meshes.get(&mesh.0).and_then(Collider::trimesh_from_mesh) {
                    commands
                        .entity(entity)
                        .insert((RigidBody::Static, collider));
                }
            }
            (false, true) => {
                commands.entity(entity).remove::<(RigidBody, Collider)>();
            }
            _ => {}
        }
    }
}

/// Spawns the light. The ground is built from the [`Ground`] resource by [`sync_ground`].
pub fn spawn_environment(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.92, 0.78),
            illuminance: 15_000.0,
            ..default()
        },
        Transform::from_xyz(200.0, 300.0, 100.0).looking_at(Vec3::new(-100.0, 0.0, -50.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.25, 0.35, 0.7),
            illuminance: 3_000.0,
            ..default()
        },
        Transform::from_xyz(-200.0, 200.0, -100.0)
            .looking_at(Vec3::new(-100.0, 0.0, -50.0), Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.2, 0.25, 0.4),
        brightness: 400.0,
        affects_lightmapped_meshes: false,
    });
}

/// Builds the chunks of the ground that are new or changed since they were last built, and
/// removes the ones that are gone or are nothing but holes now. The look of the ground is one
/// material that is kept for as long as the ground looks the same.
pub fn sync_ground(
    mut commands: Commands,
    ground: Res<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GradMeshMaterial>>,
    chunks: Query<(Entity, &GroundChunk)>,
    mut look: Local<Option<(Style, Handle<GradMeshMaterial>)>>,
) {
    let style = ground.style();
    if look.as_ref().is_none_or(|(known, _)| *known != style) {
        let material = style.build_material(&mut materials);
        *look = Some((style, material));
    }
    let Some((_, material)) = look.as_ref() else {
        return;
    };
    let mut built = vec![None; ground.chunk_count()];
    for (entity, chunk) in &chunks {
        match built.get_mut(chunk.index) {
            Some(slot @ None) => *slot = Some((entity, chunk.key)),
            _ => commands.entity(entity).try_despawn(),
        }
    }
    for (index, slot) in built.into_iter().enumerate() {
        let key = ground.chunk_key(index);
        if let Some((entity, built)) = slot {
            if built == key {
                continue;
            }
            commands.entity(entity).try_despawn();
        }
        if let Some(shape) = ground.chunk_shape(index) {
            commands.spawn((
                GroundChunk { index, key },
                Mesh3d(meshes.add(shape.build())),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(ground.chunk_center(index)),
            ));
        }
    }
}

/// Spawns the road, curbs, guardrails, checkpoint arches, light posts, coins and scenery of
/// `design`, all marked with [`TrackEntity`].
pub fn spawn_track(
    commands: &mut Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: &mut Assets<GradMeshMaterial>,
    design: &TrackDesign,
    baked: &BakedTrack,
) {
    spawn_road(commands, &mut meshes, materials, design);
    spawn_curbs(commands, &mut meshes, materials, baked);
    spawn_guardrails(commands, &mut meshes, materials, baked);
    spawn_gates(commands, &mut meshes, materials, design, baked);
    spawn_light_posts(commands, &mut meshes, materials, baked);
    spawn_coins(commands, &mut meshes, materials, design, baked);
    spawn_decorations(commands, meshes, &design.decoration_sites(baked));
}

fn spawn_road(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    design: &TrackDesign,
) {
    let road_shape = design.road_shape();
    let mut road = dual_directional([0.45, 0.45, 0.47], [0.55, 0.55, 0.57], [0.0, 1.0, 0.0]);
    road.base.metallic = 0.0;
    road.base.perceptual_roughness = 0.85;
    road.base.cull_mode = None;
    commands.spawn((
        Mesh3d(meshes.add(road_shape.build())),
        MeshMaterial3d(materials.add(road)),
        RoadSurface,
        TrackEntity,
    ));
}

fn spawn_curbs(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    baked: &BakedTrack,
) {
    let curb = meshes.add(Shape::cuboid(Vec3::new(1.5, 0.12, 0.3), 0.02, ShapeDetail::Low).build());
    let red = materials.add(dual_directional(
        [0.7, 0.05, 0.05],
        [1.0, 0.2, 0.2],
        [0.0, 1.0, 0.0],
    ));
    let white = materials.add(dual_directional(
        [0.8, 0.8, 0.8],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
    ));
    for i in 0..CURB_COUNT {
        let frame = baked.sample(i as f32 / CURB_COUNT as f32);
        let material = if i.is_multiple_of(2) { &red } else { &white };
        for side in [-1.0, 1.0] {
            let offset = frame.width / 2.0 + 0.15;
            commands.spawn((
                Mesh3d(curb.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(frame.surface_point(offset * side) + frame.up * 0.06)
                    .with_rotation(frame.rotation()),
                TrackEntity,
            ));
        }
    }
}

fn spawn_guardrails(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    baked: &BakedTrack,
) {
    let post = meshes.add(Shape::tapered_cylinder(0.08, 0.15, 2.0, ShapeDetail::Low).build());
    let mut metal = dual_directional([0.4, 0.4, 0.45], [0.7, 0.7, 0.75], [0.0, 1.0, 0.0]);
    metal.base.metallic = 0.5;
    metal.base.perceptual_roughness = 0.3;
    let metal = materials.add(metal);
    for i in 0..POST_COUNT {
        let frame = baked.sample(i as f32 / POST_COUNT as f32);
        for side in [-1.0, 1.0] {
            let offset = frame.width / 2.0 + 1.0;
            commands.spawn((
                Mesh3d(post.clone()),
                MeshMaterial3d(metal.clone()),
                Transform::from_translation(frame.surface_point(offset * side) + Vec3::Y),
                TrackEntity,
            ));
        }
    }
}

fn spawn_gates(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    design: &TrackDesign,
    baked: &BakedTrack,
) {
    let pillar = meshes.add(Shape::tapered_cylinder(0.15, 0.25, 4.0, ShapeDetail::Medium).build());
    let mut green = dual_directional([0.1, 0.5, 0.15], [0.3, 1.0, 0.4], [0.0, 1.0, 0.0]);
    green.base.metallic = 0.3;
    green.base.perceptual_roughness = 0.4;
    let green = materials.add(green);
    let mut glow_material = additive(0.2, 1.0, 0.3);
    glow_material.base.emissive = LinearRgba::rgb(0.2, 1.0, 0.3) * 20_000.0;
    let glow_material = materials.add(glow_material);
    let glow = meshes.add(Shape::sphere(0.3, ShapeDetail::Low).build());

    for gate in design.checkpoint_gates(baked) {
        let pillar_offset = gate.width / 2.0 + 0.5;
        for side in [-1.0, 1.0] {
            commands.spawn((
                Mesh3d(pillar.clone()),
                MeshMaterial3d(green.clone()),
                gate.transform
                    .mul_transform(Transform::from_xyz(pillar_offset * side, 2.0, 0.0)),
                TrackEntity,
            ));
        }
        let beam = meshes.add(
            Shape::cuboid(
                Vec3::new(gate.width + 2.0, 0.2, 0.3),
                0.03,
                ShapeDetail::Low,
            )
            .build(),
        );
        commands.spawn((
            Mesh3d(beam),
            MeshMaterial3d(green.clone()),
            gate.transform
                .mul_transform(Transform::from_xyz(0.0, 4.0, 0.0)),
            TrackEntity,
        ));
        commands.spawn((
            Mesh3d(glow.clone()),
            MeshMaterial3d(glow_material.clone()),
            gate.transform
                .mul_transform(Transform::from_xyz(0.0, 4.5, 0.0)),
            TrackEntity,
        ));
    }
}

fn spawn_light_posts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    baked: &BakedTrack,
) {
    let post = meshes.add(Shape::tapered_cylinder(0.06, 0.12, 4.0, ShapeDetail::Low).build());
    let mut metal = dual_directional([0.3, 0.3, 0.35], [0.6, 0.6, 0.65], [0.0, 1.0, 0.0]);
    metal.base.metallic = 0.7;
    metal.base.perceptual_roughness = 0.2;
    let metal = materials.add(metal);
    let glow = meshes.add(Shape::sphere(0.25, ShapeDetail::Low).build());
    let mut lamp = additive(1.0, 0.9, 0.5);
    lamp.base.emissive = LinearRgba::rgb(1.0, 0.9, 0.5) * 40_000.0;
    let lamp = materials.add(lamp);

    for i in 0..LIGHT_POST_COUNT {
        let frame = baked.sample(i as f32 / LIGHT_POST_COUNT as f32);
        let side = if i.is_multiple_of(2) { 1.0 } else { -1.0 };
        let base = frame.surface_point((frame.width / 2.0 + 2.5) * side);
        commands.spawn((
            Mesh3d(post.clone()),
            MeshMaterial3d(metal.clone()),
            Transform::from_translation(base + Vec3::Y * 2.0),
            TrackEntity,
        ));
        commands.spawn((
            Mesh3d(glow.clone()),
            MeshMaterial3d(lamp.clone()),
            Transform::from_translation(base + Vec3::Y * 4.1),
            TrackEntity,
        ));
    }
}

fn spawn_coins(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    design: &TrackDesign,
    baked: &BakedTrack,
) {
    let coin = meshes.add(Shape::cylinder(0.3, 0.08, ShapeDetail::Medium).build());
    let mut gold = dual_radial_alpha([1.0, 0.85, 0.2, 1.0], [0.8, 0.5, 0.1, 1.0]);
    gold.base.metallic = 0.9;
    gold.base.perceptual_roughness = 0.1;
    gold.base.emissive = LinearRgba::rgb(1.0, 0.85, 0.2) * 5_000.0;
    let gold = materials.add(gold);
    for (id, rest) in (0..).zip(design.coin_transforms(baked)) {
        commands.spawn((
            Mesh3d(coin.clone()),
            MeshMaterial3d(gold.clone()),
            rest,
            Coin {
                id,
                collected: false,
                rest,
            },
            TrackEntity,
        ));
    }
}

/// Records every tree and bush into instanced batches and spawns them; the wobble and the pulse
/// are animated by the shader.
fn spawn_decorations(
    commands: &mut Commands,
    meshes: ResMut<Assets<Mesh>>,
    sites: &[DecorationSite],
) {
    let mut recorder = BatchRecorder::new();
    recorder.begin();
    let (mut trees, mut bushes) = (0, 0);
    for site in sites {
        match site.kind {
            DecorationKind::Tree => {
                record_tree(&mut recorder, site.position, trees);
                trees += 1;
            }
            DecorationKind::Bush => {
                record_bush(&mut recorder, site.position, bushes);
                bushes += 1;
            }
        }
    }
    recorder.end();
    for entity in recorder.render_batches(commands, meshes, None) {
        commands.entity(entity).insert(TrackEntity);
    }
}

fn record_tree(recorder: &mut BatchRecorder, position: Vec3, index: usize) {
    const TRUNK: [f32; 4] = [0.4, 0.25, 0.12, 1.0];
    const LEAVES: [([f32; 3], [f32; 3]); 3] = [
        ([0.05, 0.25, 0.05], [0.15, 0.55, 0.15]),
        ([0.02, 0.15, 0.04], [0.10, 0.35, 0.10]),
        ([0.10, 0.30, 0.05], [0.25, 0.60, 0.12]),
    ];
    let scale = 0.7 + (index % 3) as f32 * 0.3;
    let (dark, light) = LEAVES[index % LEAVES.len()];
    let canopy_radius = if index.is_multiple_of(2) { 2.5 } else { 1.8 };

    recorder.set_position(position + Vec3::Y * 2.0 * scale);
    recorder.set_scale(Vec3::splat(scale));
    recorder.set_rotation(Quat::IDENTITY);
    recorder.cone(0.3, 4.0, ShapeDetail::Low, TRUNK);

    recorder.set_position(position + Vec3::Y * 5.5 * scale);
    recorder.set_scale(Vec3::splat(scale));
    recorder.set_rotation(Quat::IDENTITY);
    recorder.set_deformation(DeformationParams::wobble(0.15, 1.0, index as f32 * 0.3));
    recorder.sphere_dual_directional(
        canopy_radius,
        ShapeDetail::Medium,
        dark,
        light,
        [0.0, 1.0, 0.0],
        MixSpace::default(),
    );
}

fn record_bush(recorder: &mut BatchRecorder, position: Vec3, index: usize) {
    const LEAVES: [([f32; 3], [f32; 3]); 2] = [
        ([0.03, 0.18, 0.04], [0.12, 0.40, 0.10]),
        ([0.04, 0.20, 0.06], [0.14, 0.45, 0.12]),
    ];
    let (dark, light) = LEAVES[index % LEAVES.len()];
    recorder.set_position(position + Vec3::Y * 0.5);
    recorder.set_scale(Vec3::ONE);
    recorder.set_rotation(Quat::IDENTITY);
    recorder.set_deformation(DeformationParams::pulse(0.1, 1.5, index as f32 * 0.5));
    recorder.sphere_dual_directional(
        0.8,
        ShapeDetail::Low,
        dark,
        light,
        [0.0, 1.0, 0.0],
        MixSpace::default(),
    );
}

fn spawn_kart(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
) {
    let root = commands
        .spawn((Transform::default(), Visibility::default(), Kart, body()))
        .id();
    let part = |commands: &mut Commands,
                mesh: Handle<Mesh>,
                material: Handle<GradMeshMaterial>,
                transform: Transform| {
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            ChildOf(root),
        ));
    };

    let mut paint = dual_directional([0.6, 0.05, 0.05], [1.0, 0.25, 0.2], [0.0, 1.0, 0.0]);
    paint.base.metallic = 0.8;
    paint.base.perceptual_roughness = 0.2;
    part(
        commands,
        meshes.add(Shape::cuboid(Vec3::new(1.0, 0.4, 2.0), 0.12, ShapeDetail::Medium).build()),
        materials.add(paint),
        Transform::from_xyz(0.0, 0.5, 0.0),
    );

    let mut cabin = dual_directional([0.15, 0.05, 0.08], [0.35, 0.12, 0.15], [0.0, 1.0, 0.0]);
    cabin.base.metallic = 0.6;
    cabin.base.perceptual_roughness = 0.15;
    part(
        commands,
        meshes.add(Shape::cuboid(Vec3::new(0.7, 0.35, 0.9), 0.15, ShapeDetail::Medium).build()),
        materials.add(cabin),
        Transform::from_xyz(0.0, 0.88, -0.15),
    );

    let mut glass = transparent(0.4, 0.6, 0.8, 0.3);
    glass.base.metallic = 0.0;
    glass.base.perceptual_roughness = 0.05;
    part(
        commands,
        meshes.add(Shape::cuboid(Vec3::new(0.65, 0.3, 0.05), 0.0, ShapeDetail::Low).build()),
        materials.add(glass),
        Transform::from_xyz(0.0, 0.85, 0.32).with_rotation(Quat::from_rotation_x(-0.3)),
    );

    let wheel = meshes.add(Shape::cylinder(0.22, 0.18, ShapeDetail::Medium).build());
    let mut rubber = opaque(0.12, 0.12, 0.14);
    rubber.base.perceptual_roughness = 0.8;
    let rubber = materials.add(rubber);
    let rim = meshes.add(Shape::ring(0.18, 0.08, ShapeDetail::Medium).build());
    let mut steel = dual_directional([0.5, 0.5, 0.55], [0.9, 0.9, 0.95], [0.0, 1.0, 0.0]);
    steel.base.metallic = 0.95;
    steel.base.perceptual_roughness = 0.1;
    let steel = materials.add(steel);
    for position in [
        Vec3::new(0.6, 0.22, 0.7),
        Vec3::new(-0.6, 0.22, 0.7),
        Vec3::new(0.6, 0.22, -0.7),
        Vec3::new(-0.6, 0.22, -0.7),
    ] {
        let axle = Transform::from_translation(position)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        let wheel_root = commands
            .spawn((axle, Visibility::default(), ChildOf(root)))
            .id();
        commands.spawn((
            Mesh3d(wheel.clone()),
            MeshMaterial3d(rubber.clone()),
            ChildOf(wheel_root),
        ));
        for side in [-1.0, 1.0] {
            commands.spawn((
                Mesh3d(rim.clone()),
                MeshMaterial3d(steel.clone()),
                Transform::from_xyz(0.0, 0.0, side * 0.09),
                ChildOf(wheel_root),
            ));
        }
    }

    let mut orange = dual_directional([0.8, 0.25, 0.0], [1.0, 0.55, 0.1], [0.0, 1.0, 0.0]);
    orange.base.metallic = 0.7;
    orange.base.perceptual_roughness = 0.25;
    let orange = materials.add(orange);
    part(
        commands,
        meshes.add(Shape::cuboid(Vec3::new(1.2, 0.06, 0.35), 0.02, ShapeDetail::Low).build()),
        orange.clone(),
        Transform::from_xyz(0.0, 0.75, -0.95),
    );
    let support = meshes.add(Shape::cylinder(0.03, 0.3, ShapeDetail::Low).build());
    for side in [-0.4, 0.4] {
        part(
            commands,
            support.clone(),
            orange.clone(),
            Transform::from_xyz(side, 0.6, -0.95),
        );
    }

    let lamp = meshes.add(Shape::sphere(0.08, ShapeDetail::Low).build());
    let mut headlight = additive(1.0, 1.0, 0.4);
    headlight.base.emissive = LinearRgba::rgb(1.0, 1.0, 0.4) * 50_000.0;
    let headlight = materials.add(headlight);
    let mut taillight = additive(0.9, 0.1, 0.1);
    taillight.base.emissive = LinearRgba::rgb(0.9, 0.1, 0.1) * 30_000.0;
    let taillight = materials.add(taillight);
    for side in [-0.3, 0.3] {
        part(
            commands,
            lamp.clone(),
            headlight.clone(),
            Transform::from_xyz(side, 0.25, 1.02),
        );
    }
    for side in [-0.35, 0.35] {
        part(
            commands,
            lamp.clone(),
            taillight.clone(),
            Transform::from_xyz(side, 0.25, -1.02),
        );
    }
}
