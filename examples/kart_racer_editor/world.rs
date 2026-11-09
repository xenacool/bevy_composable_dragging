//! Building the scene around the view: the light and the land.
//!
//! [`WorldPlugin`] owns both. It builds the land from the [`Ground`] resource, one entity per
//! chunk and again for the chunks that change. Everything it spawns is invisible to the pointer:
//! only the view of the editor takes presses.

use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy_gradmesh::{GradMeshMaterial, Style};

use crate::ground::Ground;

/// Marks a chunk of the ground, with the key of the ground it was built from, so only the
/// chunks that changed are built again.
#[derive(Component)]
pub struct GroundChunk {
    /// The index of the chunk in the ground.
    pub index: usize,
    /// What the mesh of the chunk was built from, see [`Ground::chunk_key`].
    pub key: u64,
}

/// Builds the light and the land from the `Ground` resource.
///
/// It needs the default plugins and `GradMeshPlugin`, and builds no land until the app inserts
/// the resource.
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_light).add_systems(
            Update,
            sync_ground.run_if(resource_exists_and_changed::<Ground>),
        );
    }
}

fn spawn_light(mut commands: Commands) {
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

/// Builds the chunks of the ground that are new or changed since they were last built. The look
/// of the ground is one material that is kept for as long as the ground looks the same.
fn sync_ground(
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
        commands.spawn((
            GroundChunk { index, key },
            Mesh3d(meshes.add(ground.chunk_shape(index).build())),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(ground.chunk_center(index)),
            Pickable::IGNORE,
        ));
    }
}
