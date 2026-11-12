//! A track editor: a closed loop of control points that the pointer pulls into shape, a road
//! swept along it, and land under it that the pointer raises into hills.
//!
//! The track is a `TrackDesign`, a closed loop through control points that each carry a height
//! and a road width. The road is a node of the view, swept along the design by `bevy_gradmesh`,
//! and the `world` module draws the land and the light around it; the editor adds what is
//! needed to edit the track: a gem on every control point, a width handle, a hills brush and a
//! radial menu. It is built on the same library pieces as the cozy castle: [`hud`](bevy_composable_dragging::hud),
//! [`radial`](bevy_composable_dragging::radial) and
//! [`handles`](bevy_composable_dragging::handles), with the land as a
//! [`heightmap`](bevy_composable_dragging::heightmap).
//!
//! - Move: pull a gem and the control point follows the pointer, which bends the curve of the
//!   road through it. Pressing a gem selects its control point.
//! - The hub, which rests in the corner of the screen, pops up over the selected point; press it
//!   for the ring of tools and choose Move, Width or Hills.
//! - Width: the handle at the edge of the road widens it when you pull it out and narrows it when
//!   you push it in.
//! - The land under the road is its own heightmap, not the road. Hills takes hold of the land
//!   where you press it and raises it when you drag up and lowers it when you drag down, in a
//!   round patch; a column shows the patch while you hold the press. The land never covers the
//!   road: it is cut down along the road, whether the hill or the road moved last, and a control
//!   point cannot be pulled off the map.
//! - The view is an architect's map in orthographic projection at 45 degrees from a corner, with
//!   a grid on the ground and a stilt from the ground to every control point. It shows the whole
//!   map at any shape of window. Pressing the land selects nothing.
//!
//! Run with `cargo run --example kart_racer_editor --features shapes-3d`. With `DSD_SCREENSHOT`
//! set it plays a scripted tour of all of this and records it.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_composable_dragging::hud::HudPlugin;
use bevy_composable_dragging::radial::RadialMenuPlugin;
use bevy_composable_dragging::{DragModel, DraggablePlugin};

mod blueprint;
mod camera;
#[cfg(test)]
mod clicks;
#[path = "../common/mod.rs"]
mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::tour::TourPlugin;
mod ground;
mod menu;
mod state;
mod tour;
mod track;
mod view;
mod world;

use camera::{EditorCamera, framing};
use common::spawn_hud_camera;
use ground::Ground;
use state::Editor;
use view::{theme, view};
use world::WorldPlugin;

/// How many world units the HUD camera shows vertically.
const HUD_HEIGHT: f32 = 720.0;

/// How many frames the tour is recorded for at most: thirty frames are a second.
const TOUR_FRAMES: u32 = 30 * 24;

/// Builds the land of the editor and keeps it in step with it: the `Ground` takes the land of
/// the editor whenever it changes.
///
/// It needs a `DragModel<Editor>`, which `editor` sets up.
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        if let Some(ground) = app
            .world()
            .get_resource::<DragModel<Editor>>()
            .map(|model| model.state().ground.clone())
        {
            app.insert_resource(ground);
        }
        app.add_plugins(WorldPlugin)
            .add_systems(Update, sync_ground);
    }
}

/// Gives the world the land of the editor, which the world draws chunk by chunk.
fn sync_ground(model: Res<DragModel<Editor>>, mut ground: ResMut<Ground>) {
    if *ground != model.state().ground {
        *ground = model.state().ground.clone();
    }
}

fn setup(mut commands: Commands, model: Res<DragModel<Editor>>) {
    spawn_hud_camera(&mut commands, HUD_HEIGHT);
    if std::env::var("DSD_SCREENSHOT").is_ok() {
        // The banner of the capture is drawn by a camera of its own, above the map.
        commands.spawn((
            Camera2d,
            RenderLayers::layer(common::OVERLAY_LAYER),
            Camera {
                order: 2,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 100.0),
        ));
    }
    let (transform, projection) = framing(&model.state().framing_points());
    commands.spawn((EditorCamera, Camera3d::default(), projection, transform));
}

/// Adds the editor to `app`: everything but the windowing and rendering plugins.
fn editor(app: &mut App) -> &mut App {
    app.insert_resource(ClearColor(Color::srgb(0.1, 0.15, 0.3)))
        .add_plugins(
            DraggablePlugin::new(Editor::initial(), view)
                .with_transition_duration(theme().tween_seconds),
        )
        .add_plugins(HudPlugin::<Editor>::new(HUD_HEIGHT).camera::<EditorCamera>())
        .add_plugins(RadialMenuPlugin::<Editor>::new(theme()))
        .add_plugins(EditorPlugin)
        .add_systems(Startup, setup)
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app.add_plugins(common::default_plugins("kart racer editor"));
    editor(app)
        .add_plugins(TourPlugin::new(tour::tour))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_kart_racer_editor.gif")
                .with_title("kart_racer_editor")
                .with_description("Pull, widen, sculpt")
                .animation(TOUR_FRAMES),
        );
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_gradmesh::GradMeshMaterial;

    use super::*;
    use crate::world::GroundChunk;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<GradMeshMaterial>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                16,
            )))
            .insert_resource(DragModel::new(Editor::initial(), view))
            .add_plugins(EditorPlugin);
        app.finish();
        app.cleanup();
        app.update();
        app.update();
        app
    }

    fn edit(app: &mut App, change: impl FnOnce(&Editor) -> Editor) {
        let mut model = app.world_mut().resource_mut::<DragModel<Editor>>();
        let next = change(model.state());
        model.set_state(next);
    }

    fn count<T: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&T>().iter(world).count()
    }

    #[test]
    fn the_plugin_builds_the_land() {
        let mut app = app();
        let chunks = app.world().resource::<Ground>().chunk_count();
        assert_eq!(count::<GroundChunk>(&mut app), chunks);
    }

    #[test]
    fn a_hill_builds_the_chunks_it_reaches_again_and_no_others() {
        let mut app = app();
        let before: Vec<(usize, u64)> = {
            let world = app.world_mut();
            world
                .query::<&GroundChunk>()
                .iter(world)
                .map(|chunk| (chunk.index, chunk.key))
                .collect()
        };
        edit(&mut app, |editor| {
            editor.sculpted(editor.ground.middle() + Vec2::new(250.0, 200.0), 6.0)
        });
        app.update();
        app.update();
        let world = app.world_mut();
        let after: Vec<(usize, u64)> = world
            .query::<&GroundChunk>()
            .iter(world)
            .map(|chunk| (chunk.index, chunk.key))
            .collect();
        let changed = after.iter().filter(|chunk| !before.contains(chunk)).count();
        assert!(changed > 0 && changed < before.len(), "{changed} chunks");
    }
}
