//! The plugins that build the kart racer world from a `TrackDesign` and drive on it.
//!
//! [`TrackWorldPlugin`] owns the light, the ground and the track: it builds the track from the
//! design resource, and again whenever the resource changes, and the ground from the `Ground`
//! resource. [`DrivingPlugin`] runs the kart, the race and the HUD, but only while a kart exists,
//! so a program that wants to drive now and then adds the kart with [`start_driving`] and
//! despawns everything marked `Kart`, `KartCamera` or `HudPart` to stop, as the editor does for a
//! test drive.

use avian3d::prelude::{Gravity, PhysicsPlugins};
use bevy::prelude::*;
use bevy_gradmesh::{GradMeshMaterial, SdfMaterial2d};

use crate::ground::Ground;
use crate::hud;
use crate::kart::{
    GRAVITY, Kart, KartState, PutKart, drive_kart, follow_camera, place_kart, read_kart,
};
use crate::race::{
    Checkpoint, Checkpoints, Race, animate_coins, check_checkpoints, collect_coins,
    put_back_after_fall, put_back_before_missed,
};
use crate::track::{BakedTrack, TrackDesign};
use crate::world::{
    TrackEntity, spawn_driver, spawn_environment, spawn_track, sync_ground, sync_surface_colliders,
};

/// Builds the light, the ground and the track from the `TrackDesign` resource.
///
/// It needs the default plugins and `GradMeshPlugin`, and builds nothing until the app inserts a
/// `TrackDesign` resource and a `Ground` resource: there is no default track and no default land.
/// The ground is built in chunks, and again for the chunks that change. Whenever the design
/// changes, everything marked with `TrackEntity` is despawned and built again, the checkpoints are placed anew and the kart and the race start
/// over. A design that cannot be driven is ignored with a warning, so the track stays as it was,
/// and a design without control points has nothing to build, so it is left alone quietly.
pub struct TrackWorldPlugin;

impl Plugin for TrackWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KartState>()
            .add_message::<PutKart>()
            .init_resource::<Race>()
            .init_resource::<Checkpoints>()
            .add_systems(Startup, setup_environment)
            .add_systems(
                Update,
                rebuild_track.run_if(resource_exists_and_changed::<TrackDesign>),
            )
            .add_systems(
                Update,
                sync_ground.run_if(resource_exists_and_changed::<Ground>),
            );
    }
}

/// Runs the kart, the race and the HUD on the track of [`TrackWorldPlugin`], while a kart exists
/// and the track is built.
///
/// Avian carries the kart: while one exists, the road and the ground are given colliders from
/// their meshes, and the kart rests on them.
pub struct DrivingPlugin;

impl Plugin for DrivingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            .insert_resource(Gravity(Vec3::NEG_Y * GRAVITY))
            .add_systems(Update, sync_surface_colliders)
            .add_systems(
                Update,
                (
                    read_kart,
                    drive_kart,
                    collect_coins,
                    check_checkpoints,
                    put_back_after_fall,
                    put_back_before_missed,
                    place_kart,
                    follow_camera,
                    hud::follow_hud_camera,
                    hud::update_speed_bar,
                    hud::update_hud_text,
                    hud::update_missed_dial,
                    animate_coins,
                )
                    .chain()
                    .after(rebuild_track)
                    .run_if(resource_exists::<BakedTrack>)
                    .run_if(resource_exists::<Ground>)
                    .run_if(any_with_component::<Kart>),
            );
    }
}

fn setup_environment(mut commands: Commands) {
    spawn_environment(&mut commands);
}

/// Spawns the kart, the camera that follows it and the HUD. Run it as a system once.
pub fn start_driving(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GradMeshMaterial>>,
    mut dials: ResMut<Assets<SdfMaterial2d>>,
) {
    spawn_driver(&mut commands, &mut meshes, &mut materials);
    hud::spawn_hud(&mut commands, &mut meshes, &mut materials, &mut dials);
}

/// Despawns the track and builds it again from the design, putting the kart and the race back
/// at the start.
pub fn rebuild_track(
    mut commands: Commands,
    design: Res<TrackDesign>,
    old: Query<Entity, With<TrackEntity>>,
    meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GradMeshMaterial>>,
    mut kart: ResMut<KartState>,
    mut race: ResMut<Race>,
    mut puts: MessageWriter<PutKart>,
) {
    if design.control_points.is_empty() {
        return;
    }
    if let Err(error) = design.validate() {
        warn!("the track design cannot be driven, keeping the track: {error}");
        return;
    }
    for entity in &old {
        commands.entity(entity).try_despawn();
    }
    let baked = design.bake();
    *kart = KartState::at_start(&baked);
    puts.write(PutKart(*kart));
    *race = Race::default();
    commands.insert_resource(Checkpoints(
        design
            .checkpoint_gates(&baked)
            .iter()
            .map(|gate| {
                Checkpoint::across(
                    gate.transform.translation,
                    gate.transform.rotation * Vec3::Z,
                    gate.width,
                )
                .at_distance(gate.distance)
            })
            .collect(),
    ));
    spawn_track(&mut commands, meshes, &mut materials, &design, &baked);
    commands.insert_resource(baked);
}
