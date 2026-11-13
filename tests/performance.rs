//! Performance of a crowded interface made of `bevy_gradmesh` shapes: what a pointer move costs
//! while previewing, and what a headless app spends per frame, counting the meshes and the
//! `bevy_gradmesh` materials it builds. The render statistics of `bevy_gradmesh`
//! (`InstancedStats`) come from its render world, which a test without a GPU does not have.
//!
//! The limits are generous, because they must hold on a busy machine and in a debug build, but
//! they are far below what a regression such as a mesh rebuilt on every frame would cost.
//! Run `cargo test --release --features shapes-3d --test performance -- --nocapture` for the
//! numbers. The labels of a crowded interface add little to a frame while their nodes move, and
//! nothing while nothing moves.

use std::time::{Duration, Instant};

use bevy::asset::AssetPlugin;
use bevy::math::{Vec2, Vec3};
use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use bevy::window::WindowEvent;
use bevy_composable_dragging::label::{Label, LabelMarker, LabelPlugin, LabelsResource};
use bevy_composable_dragging::ops::{closest, state};
use bevy_composable_dragging::{
    DragContext, DragModel, DragSpec, DraggablePlugin, Pointer, View, ViewNode,
};
use bevy_gradmesh::{GradMeshMaterial, GradMeshMaterialPool};

/// Nodes of the interface, about the size of the kart editor's track.
const NODES: usize = 240;

/// Branches that `closest` evaluates for every pointer move.
const BRANCHES: usize = 20;

/// Budgets are ten times larger in a debug build, which is far slower.
fn budget(release: Duration) -> Duration {
    if cfg!(debug_assertions) {
        release * 10
    } else {
        release
    }
}

/// The state: how far the interface is shifted.
#[derive(Clone, Debug, PartialEq)]
struct Shift(usize);

fn view(shift: &Shift) -> View<Shift> {
    let mut view = View::new();
    for index in 0..NODES {
        let x = (index + shift.0) as f32 * 0.5;
        let node = if index % 3 == 0 {
            ViewNode::cuboid(format!("n{index}"), Vec3::splat(0.4), 0.05)
        } else {
            ViewNode::sphere(format!("n{index}"), 0.2)
        };
        view.push(node.at_xy(Vec2::new(x, (index % 7) as f32)));
    }
    view.push(
        ViewNode::sphere("held", 0.3)
            .at_xy(Vec2::new(shift.0 as f32 * 0.5, -2.0))
            .draggable(|| closest((0..BRANCHES).map(|index| state(Shift(index))))),
    );
    view
}

fn micros(duration: Duration, rounds: usize) -> f64 {
    duration.as_secs_f64() * 1e6 / rounds as f64
}

#[test]
fn a_pointer_move_over_many_branches_fits_in_a_frame() {
    const MOVES: usize = 50;
    let spec: DragSpec<Shift> = closest((0..BRANCHES).map(|index| state(Shift(index))));
    let context = DragContext::new(view, "held".into(), Shift(0));
    let mut behavior = spec.compile(&context);

    let start = Instant::now();
    let mut gap = 0.0;
    for index in 0..MOVES {
        let frame = behavior(Pointer::from(Vec2::new(index as f32 * 0.4, -2.0)));
        gap += frame.gap;
    }
    let elapsed = start.elapsed();
    println!(
        "{NODES} nodes, {BRANCHES} branches: {:.0} us per pointer move (gap sum {gap:.1})",
        micros(elapsed, MOVES)
    );
    assert!(
        elapsed / u32::try_from(MOVES).unwrap_or(u32::MAX) <= budget(Duration::from_millis(8)),
        "a pointer move took {:.0} us",
        micros(elapsed, MOVES)
    );
}

/// A headless app that draws `view` with `bevy_gradmesh` shapes, without a GPU. The plugin
/// registers the gradmesh materials and their pool itself when there is no renderer.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        bevy::input::InputPlugin,
        DraggablePlugin::new(Shift(0), view).with_transition_duration(0.0),
    ))
    .add_message::<WindowEvent>();
    app.finish();
    app.cleanup();
    app.update();
    app
}

/// What the shown interface holds: the nodes, the meshes that were built for them, and the
/// gradmesh materials that something still refers to.
#[derive(Debug, PartialEq, Eq)]
struct Census {
    nodes: usize,
    meshes: usize,
    live_materials: usize,
}

fn census(app: &mut App) -> Census {
    let world = app.world_mut();
    let nodes = world.query::<&Name>().iter(world).count();
    let meshes = world.resource::<Assets<Mesh>>().len();
    let live_materials = world
        .resource::<GradMeshMaterialPool>()
        .live_len(world.resource::<Assets<GradMeshMaterial>>());
    Census {
        nodes,
        meshes,
        live_materials,
    }
}

#[test]
fn a_shown_interface_builds_each_shape_once_and_keeps_its_frame_rate() {
    const FRAMES: usize = 120;
    let mut app = app();
    let warm = census(&mut app);
    println!("warm: {warm:?}");
    assert_eq!(warm.nodes, NODES + 1, "every node has an entity");
    assert_eq!(
        warm.meshes, 3,
        "the cuboid, the small sphere and the held sphere are built once and shared by all nodes of that shape"
    );

    let start = Instant::now();
    let mut worst = Duration::ZERO;
    for frame in 0..FRAMES {
        app.world_mut()
            .resource_mut::<DragModel<Shift>>()
            .set_state(Shift(frame % BRANCHES));
        let frame_start = Instant::now();
        app.update();
        worst = worst.max(frame_start.elapsed());
    }
    let elapsed = start.elapsed();
    let fps = FRAMES as f64 / elapsed.as_secs_f64();

    let after = census(&mut app);
    println!(
        "{NODES} nodes: {fps:.0} fps ({:.0} us per frame, worst {:.0} us); {after:?}",
        micros(elapsed, FRAMES),
        worst.as_secs_f64() * 1e6,
    );
    assert_eq!(
        after, warm,
        "moving the interface builds no mesh and no material, and keeps every entity"
    );
    assert!(
        elapsed / u32::try_from(FRAMES).unwrap_or(u32::MAX) <= budget(Duration::from_millis(16)),
        "{fps:.0} fps is below 60"
    );
}

/// A component that the app puts on every node with a scene.
#[derive(Component, Default, Clone)]
struct Tagged;

fn tagged_view(shift: &Shift) -> View<Shift> {
    let mut view = View::new();
    for index in 0..NODES {
        view.push(
            ViewNode::sphere(format!("n{index}"), 0.2)
                .at_xy(Vec2::new((index + shift.0) as f32 * 0.5, 0.0))
                .with_scene(|| bsn! { Tagged }),
        );
    }
    view
}

#[test]
fn scenes_are_built_when_a_node_appears_and_not_when_the_interface_moves() {
    const FRAMES: usize = 120;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        bevy::input::InputPlugin,
        DraggablePlugin::new(Shift(0), tagged_view).with_transition_duration(0.0),
    ))
    .add_message::<WindowEvent>();
    app.finish();
    app.cleanup();

    let start = Instant::now();
    app.update();
    app.update();
    let appearing = start.elapsed();
    let warm = census(&mut app);
    let tagged = app.world_mut().query::<&Tagged>().iter(app.world()).count();
    assert_eq!(tagged, NODES, "every node got its scene");

    let start = Instant::now();
    for frame in 0..FRAMES {
        app.world_mut()
            .resource_mut::<DragModel<Shift>>()
            .set_state(Shift(frame % BRANCHES));
        app.update();
    }
    let elapsed = start.elapsed();
    println!(
        "{NODES} scenes appear in {:.0} us; {:.0} us per frame while they move",
        appearing.as_secs_f64() * 1e6,
        micros(elapsed, FRAMES),
    );
    assert_eq!(census(&mut app), warm, "moving spawns and builds nothing");
    assert!(
        elapsed / u32::try_from(FRAMES).unwrap_or(u32::MAX) <= budget(Duration::from_millis(16)),
        "scenes make a frame slower than 16 ms"
    );
}

/// Labels that follow the nodes of the interface.
const LABELS: usize = 10_000;

fn labelled_view(shift: &Shift) -> View<Shift> {
    let mut view = View::new();
    for index in 0..LABELS {
        view.push(ViewNode::sphere(format!("n{index}"), 0.2).at_xy(Vec2::new(
            (index % 100 + shift.0) as f32 * 0.1,
            (index / 100) as f32 * 0.1,
        )));
    }
    view
}

fn labels(_state: &Shift, view: &View<Shift>) -> Vec<Label> {
    view.nodes()
        .iter()
        .map(|node| Label::new(format!("label-{}", node.id), "text", node.id.to_string()))
        .collect()
}

#[test]
fn many_labels_keep_the_frame_rate_while_idle_and_while_they_move() {
    const FRAMES: usize = 20;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        WindowPlugin::default(),
        bevy::input::InputPlugin,
        DraggablePlugin::new(Shift(0), labelled_view).with_transition_duration(0.0),
    ))
    .add_message::<WindowEvent>()
    .insert_resource(LabelsResource::<Shift>::new(labels, 3))
    .add_plugins(LabelPlugin::<Shift>::default());
    app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((
            Camera3d::default(),
            Projection::Orthographic(OrthographicProjection::default_3d()),
            Transform::from_xyz(5.0, 5.0, 10.0),
        ));
    });
    app.finish();
    app.cleanup();
    let start = Instant::now();
    app.update();
    app.update();
    let appearing = start.elapsed();
    let count = app
        .world_mut()
        .query::<&LabelMarker>()
        .iter(app.world())
        .count();
    assert_eq!(count, LABELS, "every label has an entity");

    let start = Instant::now();
    for _ in 0..FRAMES {
        app.update();
    }
    let idle = start.elapsed();

    let start = Instant::now();
    for frame in 0..FRAMES {
        app.world_mut()
            .resource_mut::<DragModel<Shift>>()
            .set_state(Shift(frame % BRANCHES));
        app.update();
    }
    let moving = start.elapsed();
    println!(
        "{LABELS} labels appear in {:.0} us; {:.0} us per idle frame, {:.0} us per frame while they move",
        appearing.as_secs_f64() * 1e6,
        micros(idle, FRAMES),
        micros(moving, FRAMES),
    );
    let frames = u32::try_from(FRAMES).unwrap_or(u32::MAX);
    assert!(
        idle / frames <= budget(Duration::from_millis(4)),
        "idle labels make a frame slower than 4 ms: {:.0} us",
        micros(idle, FRAMES)
    );
    assert!(
        moving / frames <= budget(Duration::from_millis(16)),
        "moving labels make a frame slower than 16 ms: {:.0} us",
        micros(moving, FRAMES)
    );
}
