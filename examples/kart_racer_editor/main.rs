//! A track editor for the kart racer, with a track you can drive on while you edit it.
//!
//! The track is a `TrackDesign`, a closed loop through control points that each carry a height,
//! a road width and a banking. The editor draws the road with the kart world in its `world`,
//! `kart`, `race` and `hud` modules, which are a port of the kart racer example of
//! `bevy_gradmesh`, and adds what is needed to edit it: gems, cones, a gizmo, a width handle and
//! a roll ring on the control points, a radial menu and a map camera. It is built on the same
//! library pieces as the cozy castle: [`hud`](bevy_composable_dragging::hud), [`radial`](bevy_composable_dragging::radial),
//! [`handles`](bevy_composable_dragging::handles) and [`gizmo`](bevy_composable_dragging::gizmo).
//!
//! - The editor starts with no track. Choose Insert and press the ground to lay control points one
//!   after the other; the road appears as soon as there are enough of them to drive on.
//! - Press a gem to select a control point. The hub, which rests in the corner of the screen,
//!   pops up over the selected point; press it for the tools, the presets of the road and the
//!   precision of the handles.
//! - Move, Raise, Width, Roll and Insert show their handles; Delete removes the point you press.
//! - The land under the road is its own heightmap, not the road. Hills takes hold of the land
//!   where you press it and raises it when you drag up and lowers it when you drag down, in a
//!   round patch whose size follows the precision; a column shows the patch while you hold the
//!   press. Holes cuts a hole where you press, or fills one in when you press a hole, and
//!   dragging carries on over the land it crosses. The land never covers the road: it is cut
//!   down along the road, whether the hill or the road moved last, and a control point cannot
//!   be pulled off the edge of the land.
//! - A test drive takes the land with it. The kart drives on the road while it is on it and on
//!   the land beside it, and falls through a hole or off the edge of the map. A kart that falls
//!   below the lowest land goes back to the last checkpoint it passed, or to the start line if
//!   it has passed none, standing.
//! - Test drive puts the kart on the track: W and S accelerate, A and D steer, R restarts and
//!   Escape goes back to editing.
//! - Test drive appears in the Tools ring only once the track can be driven.
//! - The view is an architect's map in orthographic projection: a grid on the ground, a stilt
//!   from the ground to every control point and the middle of every segment, and while the track
//!   cannot be driven a ribbon along the curve. It starts at 45 degrees from a corner. The right
//!   mouse button turns it and tilts it, from a side view that shows the heights to a plan
//!   looking straight down, Q and E turn it to the next corner, Home goes back to the start,
//!   the middle button slides it, and the wheel or the up and down keys zoom. Zooming in comes
//!   in on the selected control point, and the wheel zooms out past the track to the whole
//!   space of the map. Pressing the terrain, which selects nothing, zooms out to the whole
//!   track. The drive has the chase camera of the kart racer.
//!
//! A track that cannot be driven stays on the screen with the reason in the status panel; the
//! road is built again when the track can be driven.
//!
//! Run with `cargo run --example kart_racer_editor --features shapes-3d`.

#![expect(
    clippy::disallowed_types,
    reason = "the views and the drags of the library are shared through `Arc`; the guidelines ban shared ownership, and this expectation warns once the last `Arc` of the example is gone"
)]

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_composable_dragging::gizmo::GIZMO_LAYER;
use bevy_composable_dragging::hud::HudPlugin;
use bevy_composable_dragging::radial::{RadialHost, RadialMenuPlugin};
use bevy_composable_dragging::{DragModel, DraggablePlugin};

mod blueprint;
mod camera;
#[cfg(test)]
mod clicks;
#[path = "../common/mod.rs"]
mod common;
use common::screenshot::{AutoScreenshotPlugin, ScreenshotState};
mod ground;
mod hud;
mod kart;
mod menu;
mod plugin;
mod race;
mod state;
mod track;
mod view;
mod world;

use camera::{EditorCamera, MapCamera, map_camera};
use common::spawn_hud_camera;
use ground::Ground;
use hud::HudPart;
use kart::{Kart, KartCamera};
use plugin::{DrivingPlugin, TrackWorldPlugin, start_driving};
use state::Editor;
use track::TrackDesign;
use view::{theme, view};

/// How many world units the HUD camera shows vertically.
const HUD_HEIGHT: f32 = 720.0;

/// What exists only while the track is driven: the kart, its camera and the HUD of the drive.
type DrivingParts = Or<(With<Kart>, With<KartCamera>, With<HudPart>)>;

/// Builds the track of the editor and keeps it in step with the editor: the `TrackDesign` takes
/// the design of the editor whenever the track can be driven, and a drive starts and ends with
/// the editor.
///
/// It needs a `DragModel<Editor>` and the camera of the editor, which `main` sets up.
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        if let Some((design, ground)) = app
            .world()
            .get_resource::<DragModel<Editor>>()
            .map(|model| (model.state().design.clone(), model.state().ground.clone()))
        {
            app.insert_resource(design).insert_resource(ground);
        }
        app.add_plugins((TrackWorldPlugin, DrivingPlugin))
            .add_systems(
                Update,
                (sync_design, sync_ground, sync_driving, leave_drive).before(plugin::rebuild_track),
            )
            .add_systems(Update, ignore_scenery)
            .add_systems(Update, open_menu_for_screenshot);
    }
}

/// When screenshotting, opens the radial menu after the first shot is saved so the
/// second screenshot shows the menu open.
fn open_menu_for_screenshot(
    mut model: ResMut<DragModel<Editor>>,
    state: Option<ResMut<ScreenshotState>>,
    frame_count: Res<bevy::diagnostic::FrameCount>,
) {
    let Some(mut state) = state else {
        return;
    };
    if state.saved && !state.ready_for_second_shot {
        let editor = model.state();
        if editor.menu.is_none() {
            let editor = RadialHost::with_menu(editor, Some(menu::Ring::Root));
            model.set_state(editor);
        }
        // Signal that the menu is open and record the frame so the second shot waits
        // until the menu is actually rendered. Only set once.
        state.ready_for_second_shot = true;
        state.ready_frame = frame_count.0;
    }
}

/// Makes the scenery of the kart world invisible to the pointer. Every node of the view carries
/// a `Pickable`, so a mesh without one is the world, which only has to be looked at: scenery in
/// front of a handle would take the press meant for it.
fn ignore_scenery(
    mut commands: Commands,
    scenery: Query<Entity, (Added<Mesh3d>, Without<Pickable>)>,
) {
    for entity in &scenery {
        commands.entity(entity).try_insert(Pickable::IGNORE);
    }
}

/// Gives the world the design of the editor, unless the track cannot be driven, which keeps the
/// road that is there.
fn sync_design(model: Res<DragModel<Editor>>, mut design: ResMut<TrackDesign>) {
    let editor = model.state();
    if editor.error().is_none() && *design != editor.design {
        *design = editor.design.clone();
    }
}

/// Gives the world the land of the editor, which the world draws chunk by chunk and the kart
/// drives on.
fn sync_ground(model: Res<DragModel<Editor>>, mut ground: ResMut<Ground>) {
    if *ground != model.state().ground {
        *ground = model.state().ground.clone();
    }
}

/// Puts the kart on the track while the editor is testing, and takes it off again afterwards.
/// The camera of the editor sleeps meanwhile, as the kart has its own.
fn sync_driving(
    model: Res<DragModel<Editor>>,
    mut commands: Commands,
    karts: Query<(), With<Kart>>,
    driving: Query<Entity, DrivingParts>,
    mut editor_camera: Single<&mut Camera, With<EditorCamera>>,
    mut design: ResMut<TrackDesign>,
) {
    let testing = model.state().testing;
    let present = !karts.is_empty();
    if testing && !present {
        commands.run_system_cached(start_driving);
        editor_camera.is_active = false;
        design.set_changed();
    } else if !testing && present {
        for entity in &driving {
            commands.entity(entity).try_despawn();
        }
        editor_camera.is_active = true;
    }
}

/// Ends the drive with Escape.
fn leave_drive(keys: Res<ButtonInput<KeyCode>>, mut model: ResMut<DragModel<Editor>>) {
    if model.state().testing && keys.just_pressed(KeyCode::Escape) {
        let editor = model.state().with_testing(false);
        model.set_state(editor);
    }
}

/// The aspect of the window the map is framed for, which the real window replaces as soon as it
/// has a size.
const FRAMING_ASPECT: f32 = 16.0 / 9.0;

fn setup(mut commands: Commands, model: Res<DragModel<Editor>>) {
    spawn_hud_camera(&mut commands, HUD_HEIGHT);
    let map = MapCamera::framing(&model.state().framing_points(), FRAMING_ASPECT);
    let layers = RenderLayers::from_layers(&[0, GIZMO_LAYER]);
    commands.spawn((
        EditorCamera,
        map,
        Camera3d::default(),
        layers,
        map.projection(),
        map.transform(),
    ));
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
        .add_systems(
            Update,
            map_camera.run_if(|model: Res<DragModel<Editor>>| !model.state().testing),
        )
}

fn main() {
    let mut app = App::new();
    app.add_plugins(common::default_plugins("kart racer editor"));
    editor(&mut app)
        .add_plugins(
            AutoScreenshotPlugin::new("screenshots/screenshot_kart_racer_editor.png")
                .with_title("kart_racer_editor")
                .with_description("A kart track editor with gradient meshes")
                .with_second_shot("screenshots/screenshot_kart_racer_editor_menu.png")
                .wait_frames(3),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_composable_dragging::radial::RadialHost;
    use bevy_gradmesh::GradMeshMaterial;

    use super::*;
    use crate::kart::KartState;
    use crate::race::{Checkpoints, Coin, Race};
    use crate::track::BakedTrack;
    use crate::world::TrackEntity;

    fn app() -> App {
        app_of(Editor::upstream())
    }

    fn app_of(editor: Editor) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<GradMeshMaterial>()
            .init_asset::<bevy_gradmesh::SdfMaterial2d>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                16,
            )))
            .insert_resource(DragModel::new(editor, view))
            .add_plugins(EditorPlugin);
        app.finish();
        app.cleanup();
        app.world_mut().spawn((EditorCamera, Camera::default()));
        app.update();
        app
    }

    fn edit(app: &mut App, change: impl FnOnce(&Editor) -> Editor) {
        let mut model = app.world_mut().resource_mut::<DragModel<Editor>>();
        let next = change(model.state());
        model.set_state(next);
    }

    fn track_entities(app: &mut App) -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<TrackEntity>>()
            .iter(app.world())
            .collect()
    }

    fn count<T: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&T>().iter(world).count()
    }

    #[test]
    fn the_plugin_builds_the_track_without_a_kart() {
        let mut app = app();
        assert!(app.world().contains_resource::<BakedTrack>());
        assert_eq!(count::<Coin>(&mut app), 24);
        assert_eq!(count::<Kart>(&mut app), 0);
        assert_eq!(count::<KartCamera>(&mut app), 0);
        assert_eq!(count::<HudPart>(&mut app), 0);
        assert!(!track_entities(&mut app).is_empty());
    }

    #[test]
    fn an_edit_that_can_be_driven_builds_the_road_again() {
        let mut app = app();
        let before = track_entities(&mut app);
        edit(&mut app, |editor| editor.raised(3, 2.0));
        app.update();
        app.update();
        let design = app.world().resource::<TrackDesign>();
        assert!((design.control_points[3].position.y - 2.5).abs() < 1.1);
        assert_eq!(
            design,
            &app.world().resource::<DragModel<Editor>>().state().design
        );
        let after = track_entities(&mut app);
        assert!(!after.is_empty());
        assert!(after.iter().all(|entity| !before.contains(entity)));
    }

    #[test]
    fn a_width_and_a_roll_reach_the_road() {
        let mut app = app();
        edit(&mut app, |editor| editor.widened(2, 1.0).rolled(2, 0.2));
        app.update();
        let design = app.world().resource::<TrackDesign>();
        assert!(design.control_points[2].width > 1.0);
        assert!((design.control_points[2].roll - 0.2).abs() < 1e-5);
        let baked = app.world().resource::<BakedTrack>();
        let banked = baked
            .frames()
            .iter()
            .map(|frame| frame.roll.abs())
            .fold(0.0, f32::max);
        assert!(banked > 0.1, "the baked road leans: {banked}");
    }

    #[test]
    fn a_track_that_cannot_be_driven_keeps_the_road_and_the_design_of_the_world() {
        let mut app = app();
        let mut before = track_entities(&mut app);
        before.sort();
        let good = app.world().resource::<TrackDesign>().clone();
        edit(&mut app, |editor| editor.raised(3, 500.0));
        app.update();
        app.update();
        let editor = app.world().resource::<DragModel<Editor>>().state().clone();
        assert!(editor.error().is_some());
        assert!(editor.status().contains("Cannot be driven"));
        assert_eq!(app.world().resource::<TrackDesign>(), &good);
        let mut now = track_entities(&mut app);
        now.sort();
        assert_eq!(now, before, "the road is the same entities");
        edit(&mut app, |editor| editor.raised(3, -500.0));
        app.update();
        app.update();
        assert_eq!(app.world().resource::<TrackDesign>(), &good);
    }

    #[test]
    fn testing_puts_the_kart_on_the_track_and_escape_takes_it_off() {
        let mut app = app();
        edit(&mut app, |editor| editor.with_testing(true));
        app.update();
        app.update();
        assert_eq!(count::<Kart>(&mut app), 1);
        assert_eq!(count::<KartCamera>(&mut app), 1);
        assert!(count::<HudPart>(&mut app) > 0);
        let world = app.world_mut();
        let camera = world
            .query_filtered::<&Camera, With<EditorCamera>>()
            .single(world)
            .unwrap();
        assert!(!camera.is_active, "the camera of the editor sleeps");
        assert_eq!(count::<Coin>(&mut app), 24, "the coins are back");

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.update();
        assert!(!app.world().resource::<DragModel<Editor>>().state().testing);
        assert_eq!(count::<Kart>(&mut app), 0);
        assert_eq!(count::<KartCamera>(&mut app), 0);
        assert_eq!(count::<HudPart>(&mut app), 0);
        let world = app.world_mut();
        let camera = world
            .query_filtered::<&Camera, With<EditorCamera>>()
            .single(world)
            .unwrap();
        assert!(camera.is_active);
    }

    #[test]
    fn a_kart_that_follows_the_edited_road_completes_a_lap() {
        let mut app = app();
        edit(&mut app, |editor| editor.widened(0, 2.0).banked(1, 2.0));
        edit(&mut app, |editor| editor.with_testing(true));
        app.add_systems(
            Update,
            kart::autopilot_keys
                .before(kart::drive_kart)
                .run_if(any_with_component::<Kart>),
        );
        app.update();
        app.update();
        let mut laps = 0;
        for _ in 0..12_000 {
            app.update();
            laps = app.world().resource::<race::Race>().laps;
            if laps >= 1 {
                break;
            }
        }
        assert!(laps >= 1, "a lap on the edited track");
        let state = *app.world().resource::<kart::KartState>();
        assert!(state.coins > 0, "coins were collected on the way");
        assert!(!state.off_road());
    }

    #[test]
    fn the_kart_drives_on_the_edited_track() {
        let mut app = app();
        edit(&mut app, |editor| editor.widened(0, 2.0).banked(1, 2.0));
        edit(&mut app, |editor| editor.with_testing(true));
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..30 {
            app.update();
        }
        let state = *app.world().resource::<kart::KartState>();
        assert!(state.speed > 5.0);
        assert_eq!(app.world().resource::<Checkpoints>().0.len(), 8);
    }

    #[test]
    fn an_empty_editor_builds_nothing_until_the_track_can_be_driven() {
        let mut app = app_of(Editor::initial());
        assert!(!app.world().contains_resource::<BakedTrack>());
        assert!(
            app.world()
                .resource::<TrackDesign>()
                .control_points
                .is_empty()
        );
        assert!(track_entities(&mut app).is_empty());
        assert_eq!(count::<Coin>(&mut app), 0);

        edit(&mut app, |editor| {
            editor
                .placed(Vec3::new(-40.0, 0.0, -40.0))
                .placed(Vec3::new(40.0, 0.0, -40.0))
        });
        app.update();
        app.update();
        assert!(
            !app.world().contains_resource::<BakedTrack>(),
            "two points are no loop"
        );
        assert!(track_entities(&mut app).is_empty());

        edit(&mut app, |_| Editor::upstream());
        app.update();
        app.update();
        assert!(app.world().contains_resource::<BakedTrack>());
        assert_eq!(count::<Coin>(&mut app), 24);
    }

    /// An app that is already testing the upstream track, with the kart on the start line.
    fn driving() -> App {
        let mut app = app();
        edit(&mut app, |editor| editor.with_testing(true));
        app.update();
        app.update();
        app
    }

    #[test]
    fn coins_raise_the_top_speed_only_a_little_and_only_five_times() {
        let mut app = driving();
        let coins: Vec<Vec3> = {
            let world = app.world_mut();
            world
                .query::<&Coin>()
                .iter(world)
                .map(|coin| coin.rest.translation)
                .collect()
        };
        assert!(coins.len() > 6);
        let mut tops = Vec::new();
        for coin in coins.iter().take(7) {
            app.world_mut().resource_mut::<KartState>().position = *coin;
            app.world_mut().resource_mut::<Race>().last = Some(*coin);
            app.update();
            tops.push(kart::top_speed(app.world().resource::<KartState>().coins));
        }
        assert_eq!(app.world().resource::<KartState>().coins, 7);
        assert!(tops.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(tops[4] > tops[0]);
        assert_eq!(tops[5], tops[4]);
        assert_eq!(tops[6], tops[4]);
    }

    #[test]
    fn restarting_puts_the_kart_back_at_the_start() {
        let mut app = driving();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..20 {
            app.update();
        }
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::KeyW);
        keys.clear();
        keys.press(KeyCode::KeyR);
        app.update();
        let state = *app.world().resource::<KartState>();
        let start = KartState::at_start(app.world().resource::<BakedTrack>());
        assert_eq!(state.position, start.position);
        assert_eq!(state.speed, 0.0);
    }

    #[test]
    fn a_kart_that_misses_a_checkpoint_is_counted_down_and_put_back_before_it_standing() {
        let mut app = driving();
        let track = app.world().resource::<BakedTrack>().clone();
        let gate = app.world().resource::<Checkpoints>().0[1];
        {
            let mut race = app.world_mut().resource_mut::<Race>();
            race.started = true;
            race.next = 1;
            // The kart is put down, not driven, so the way it drove is measured from here.
            race.last = None;
        }
        let missed = KartState {
            coins: 2,
            ..KartState::at_distance(&track, gate.distance + 25.0)
        };
        kart::put(&mut app, missed);

        let dial = |app: &mut App| {
            let world = app.world_mut();
            *world
                .query_filtered::<&Visibility, With<hud::MissedDial>>()
                .single(world)
                .unwrap()
        };
        let mut shown = Vec::new();
        for frame in 0..900 {
            app.update();
            let countdown = app.world().resource::<Race>().countdown;
            if let Some(left) = countdown {
                shown.push(left);
                assert_eq!(dial(&mut app), Visibility::Inherited, "frame {frame}");
            }
            if frame < 250 {
                assert_eq!(countdown, None, "five seconds of grace: frame {frame}");
                assert_eq!(dial(&mut app), Visibility::Hidden);
            }
        }
        assert!(
            shown.len() > 200,
            "the countdown runs for five seconds: {}",
            shown.len()
        );
        assert!(
            shown.windows(2).all(|pair| pair[1] < pair[0]),
            "it counts down"
        );
        assert!(shown[0] <= race::MISSED_COUNTDOWN && shown[0] > race::MISSED_COUNTDOWN - 0.1);

        let state = *app.world().resource::<KartState>();
        assert!(state.speed.abs() < 0.1, "standing: {}", state.speed);
        assert_eq!(state.coins, 2, "the coins stay");
        let before = gate.distance - kart::KART_WIDTH;
        assert!(
            (state.distance - before).abs() < 0.5,
            "{} against {before}",
            state.distance
        );
        assert!(app.world().resource::<Race>().started);
        assert_eq!(
            app.world().resource::<Race>().next,
            1,
            "it still wants that gate"
        );
        assert_eq!(app.world().resource::<Race>().countdown, None);
        assert_eq!(dial(&mut app), Visibility::Hidden);
    }

    /// Runs the drive with the kart put off the road over a hole, after the race has passed
    /// `passed` checkpoints, until it has fallen and been put back, and returns the kart and the
    /// distance along the road of the last checkpoint it passed.
    fn fall_after(passed: usize) -> (KartState, f32) {
        let mut app = driving();
        let frame = app.world().resource::<BakedTrack>().sample(0.0);
        let spot = frame.surface_point(frame.width / 2.0 + 30.0).xz();
        edit(&mut app, |editor| {
            editor.with_ground(editor.ground.with_holes(spot, 60.0, true))
        });
        app.update();
        app.world_mut().resource_mut::<Race>().next = passed;
        let gate = app
            .world()
            .resource::<Checkpoints>()
            .0
            .get(passed.wrapping_sub(1))
            .map_or(0.0, |gate| gate.distance);
        kart::put(
            &mut app,
            KartState {
                position: Vec3::new(spot.x, ground::BASE + 0.5, spot.y),
                ..KartState::default()
            },
        );
        let mut fell = false;
        for _ in 0..4000 {
            app.update();
            let state = app.world().resource::<KartState>();
            fell |= state.falling() && state.position.y < ground::BASE - 50.0;
            if fell && state.position.y > ground::BASE - 1.0 {
                break;
            }
        }
        assert!(fell, "the kart fell through the hole");
        (*app.world().resource::<KartState>(), gate)
    }

    #[test]
    fn a_kart_that_falls_is_put_back_at_the_last_checkpoint_it_passed_or_the_start_line() {
        let track = TrackDesign::upstream().bake();
        for passed in [0, 1, 3, 6] {
            let (state, gate) = fall_after(passed);
            let frame = track.sample_at_distance(gate);
            assert!(
                state.position.distance(frame.position) < 2.0,
                "{passed}: {} against {}",
                state.position,
                frame.position
            );
            assert!(state.speed.abs() < 0.1, "{passed}: {}", state.speed);
            assert!(!state.falling(), "{passed}");
        }
    }
}
