//! Low-poly polyhedra that snap face-to-face, the 3D counterpart of
//! `tactile_tessellations`.
//!
//! Drag a tile out of the palette shelf. Near other tiles it snaps face-to-face: the library
//! computes a convex polyhedron for each shape, finds compatible faces by vertex count,
//! aligns the face normals, and places the tile so the faces meet flush. Far from every snap
//! the tile follows the pointer freely, varying position and yaw, and never into another tile.
//! Dropping a tile on the trash bin deletes it.
//!
//! A drag is a `substate` zipper on the one tile: its spec only sees the tile's pose, or `None`
//! once it is dropped on the bin, and every other tile stays as it was, in view and draggable.
//! Snapping uses pure geometry on one polyhedron per kind of tile, built once for a drag. The
//! drag space is [`DragSpace::Screen`](bevy_composable_dragging::DragSpace::Screen), so the
//! pointer plane always faces the camera.
//!
//! Run with `cargo run --example tactile_tessellations_3d --features shapes-3d`.

use std::collections::HashMap;

use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
use bevy::prelude::*;
use bevy_composable_dragging::ops::{
    Lens, closest, custom, drop_target, state, substate, switch_to_state_and_follow,
    vary_constrained,
};
use bevy_composable_dragging::snapping::{self, Polyhedron, Tile3D};
use bevy_composable_dragging::{
    Constraint, DragSpace, DragSpec, DraggablePlugin, Param, View, ViewNode, param,
};
use bevy_gradmesh::{Shape, ShapeDetail, Style};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

/// The kind of low-poly 3D shape a tile draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ShapeKind {
    /// Tetrahedron-like pyramid (4 triangular faces).
    Tetrahedron,
    /// Square pyramid (4 triangular faces + 1 quad base).
    Pyramid,
    /// Cube (6 square faces).
    Cube,
    /// Pentagonal star (2 pentagonal faces).
    Star,
}

impl ShapeKind {
    const ALL: [ShapeKind; 4] = [
        ShapeKind::Tetrahedron,
        ShapeKind::Pyramid,
        ShapeKind::Cube,
        ShapeKind::Star,
    ];
}

/// The id of the trash bin node.
const TRASH: &str = "trash-bin";
/// The ground plane sits at Y = 0.
const GROUND_Y: f32 = 0.0;
/// How high the dragged tile lifts above the ground.
const LIFT: f32 = 0.5;
/// Maximum distance from a snap before free-drag mode takes over.
const SNAP_RANGE: f32 = 1.5;

/// Where a tile is and how it is turned.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    pos: Vec3,
    rotation: Quat,
}

#[derive(Clone, Debug, PartialEq)]
struct Tile {
    id: u32,
    kind: ShapeKind,
    pose: Pose,
}

#[derive(Clone, Debug, PartialEq)]
struct Board {
    tiles: Vec<Tile>,
    next_id: u32,
}

impl Board {
    fn tile(&self, id: u32) -> &Tile {
        self.tiles
            .iter()
            .find(|tile| tile.id == id)
            .expect("the dragged tile exists")
    }

    /// The board with the tile `id` posed as `pose`, or without it for `None`.
    fn with_tile(&self, id: u32, pose: Option<Pose>) -> Self {
        let mut board = self.clone();
        match pose {
            Some(pose) => {
                if let Some(tile) = board.tiles.iter_mut().find(|tile| tile.id == id) {
                    tile.pose = pose;
                }
            }
            None => board.tiles.retain(|tile| tile.id != id),
        }
        board
    }

    /// The board with a new tile of the given `kind` at `pos`, and the id of the tile.
    fn with_new(&self, kind: ShapeKind, pos: Vec3) -> (Self, u32) {
        let mut board = self.clone();
        let id = board.next_id;
        board.next_id += 1;
        let pose = Pose {
            pos,
            rotation: Quat::IDENTITY,
        };
        board.tiles.push(Tile { id, kind, pose });
        (board, id)
    }

    /// The tiles as the shapes that the snapping reads.
    fn tiles_3d(&self, polyhedra: &HashMap<ShapeKind, Polyhedron>) -> Vec<Tile3D> {
        self.tiles
            .iter()
            .map(|tile| Tile3D {
                id: tile.id,
                pos: tile.pose.pos,
                rotation: tile.pose.rotation,
                poly: polyhedra[&tile.kind].clone(),
            })
            .collect()
    }
}

/// One polyhedron for each kind of tile, however many tiles there are of it.
fn polyhedra() -> HashMap<ShapeKind, Polyhedron> {
    ShapeKind::ALL
        .into_iter()
        .map(|kind| {
            let poly = snapping::build_polyhedron(&tile_shape(kind))
                .expect("every kind of tile is a polyhedron");
            (kind, poly)
        })
        .collect()
}

/// A pyramid at the origin with a cube beside it.
fn initial() -> Board {
    let at = |x| Pose {
        pos: Vec3::new(x, TILE_Y, 0.0),
        rotation: Quat::IDENTITY,
    };
    let tile = |id, kind, pose| Tile { id, kind, pose };
    Board {
        tiles: vec![
            tile(0, ShapeKind::Pyramid, at(0.0)),
            tile(1, ShapeKind::Cube, at(1.2)),
        ],
        next_id: 2,
    }
}

// ---------------------------------------------------------------------------------------------
// Drag specs
// ---------------------------------------------------------------------------------------------

/// Makes the center of the node follow the pointer instead of the point that was grabbed.
fn anchored_at_center(spec: DragSpec<Board>) -> DragSpec<Board> {
    custom(move |context| {
        let offset = context.grab_offset();
        let mut behavior = spec.compile(&context.clone().with_grab_offset(Vec3::ZERO));
        Box::new(move |pointer| behavior(pointer.moved_to(pointer.point + offset)))
    })
}

/// The yaw of a pose, which is the only way a tile turns while it is dragged.
fn yaw() -> Param<Pose> {
    Param::new(
        |pose: &Pose| pose.rotation.to_euler(EulerRot::YXZ).0,
        |pose: &mut Pose, value| {
            let (_, pitch, roll) = pose.rotation.to_euler(EulerRot::YXZ);
            pose.rotation = Quat::from_euler(EulerRot::YXZ, value, pitch, roll);
        },
    )
    .with_scale(360.0)
}

/// The drag of a tile: its pose snaps face-to-face, it is dropped on the bin to delete it, or it
/// follows the pointer freely on the ground plane when far from any snap.
fn tile_spec(board: &Board, id: u32) -> DragSpec<Board> {
    let tile = board.tile(id);
    let (start, polyhedra) = (tile.pose, polyhedra());
    let poly = polyhedra[&tile.kind].clone();
    let tiles = board.tiles_3d(&polyhedra);
    let snaps = snapping::compute_snaps_3d(&poly, &tiles, Some(id));
    let others: Vec<Tile3D> = tiles.into_iter().filter(|other| other.id != id).collect();

    let no_overlap = Constraint::new(move |pose: &Pose| {
        if snapping::overlaps_any_3d(&poly, pose.pos, pose.rotation, &others, None) {
            100.0
        } else {
            0.0
        }
    });
    // Pitch and roll stay at zero so tiles don't spin wildly.
    let free = vary_constrained(
        start,
        [param!(Pose, pos.x), param!(Pose, pos.z), yaw()],
        no_overlap,
    );
    let pose = Lens::new(
        move |tile: &Option<Pose>| tile.unwrap_or(start),
        |_, pose| Some(pose),
    );
    let branches = snaps
        .into_iter()
        .map(|snap| {
            state(Some(Pose {
                pos: snap.pos,
                rotation: snap.rotation,
            }))
        })
        .chain([drop_target(TRASH, None)])
        .collect::<Vec<_>>();
    let tile = Lens::new(
        move |board: &Board| Some(board.tile(id).pose),
        move |board: &Board, pose| board.with_tile(id, pose),
    );
    let node = tile_id(id);
    anchored_at_center(substate(
        tile,
        closest(branches).when_far(substate(pose, free), SNAP_RANGE),
    ))
    .change_result(move |mut frame| {
        if let Some(dragged) = frame.view.get_mut(&node) {
            dragged.transform.translation.y = TILE_Y + LIFT;
        }
        frame
    })
}

// ---------------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------------
const TILE_Y: f32 = 0.3;

fn tile_id(id: u32) -> String {
    format!("shape-{id}")
}

fn fill(kind: ShapeKind) -> Color {
    match kind {
        ShapeKind::Tetrahedron => Color::srgb(0.95, 0.55, 0.55),
        ShapeKind::Pyramid => Color::srgb(0.45, 0.65, 0.95),
        ShapeKind::Cube => Color::srgb(0.40, 0.85, 0.55),
        ShapeKind::Star => Color::srgb(0.95, 0.85, 0.40),
    }
}

/// The 3D mesh for a tile kind - a true low-poly polyhedron.
fn tile_shape(kind: ShapeKind) -> Shape {
    let detail = ShapeDetail::Low;
    match kind {
        ShapeKind::Tetrahedron => Shape::pyramid(3, 0.5, 0.6, detail),
        ShapeKind::Pyramid => Shape::pyramid(4, 0.5, 0.6, detail),
        ShapeKind::Cube => Shape::cuboid(Vec3::new(0.7, 0.7, 0.7), 0.0, detail),
        ShapeKind::Star => Shape::star(5, 0.5, 0.25, detail),
    }
}

/// A tile shape with gradient style.
fn tile_node(id: impl Into<bevy_composable_dragging::NodeId>, kind: ShapeKind) -> ViewNode<Board> {
    ViewNode::grad(id, tile_shape(kind)).with_style(Style::dual_directional_up(
        fill(kind).darker(0.3),
        fill(kind),
    ))
}

/// The position of the palette item for a shape kind on the palette shelf.
fn palette_position(kind: ShapeKind) -> Vec3 {
    let x = match kind {
        ShapeKind::Tetrahedron => -4.8,
        ShapeKind::Pyramid => -2.6,
        ShapeKind::Cube => -0.4,
        ShapeKind::Star => 1.8,
    };
    let y = match kind {
        ShapeKind::Star => 0.25, // star is flatter
        _ => TILE_Y,
    };
    Vec3::new(x, y, 5.5)
}

fn view(board: &Board) -> View<Board> {
    let mut view = View::new();

    // Ground plane
    view.push(
        ViewNode::rectangle("ground", Vec2::new(12.0, 12.0))
            .at(Vec3::new(0.0, GROUND_Y, 0.0))
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
            .with_color(Color::srgb(0.15, 0.17, 0.22)),
    );

    // Palette shelf
    view.push(
        ViewNode::cuboid("palette", Vec3::new(9.6, 0.08, 3.0), 0.04)
            .at(Vec3::new(-1.2, 0.04, 5.5))
            .with_color(Color::srgb(0.92, 0.92, 0.95)),
    );

    // Trash bin
    view.push(
        ViewNode::cuboid(TRASH, Vec3::new(0.9, 0.45, 0.9), 0.08)
            .at(Vec3::new(5.3, 0.225, 5.5))
            .with_color(Color::srgb(0.95, 0.85, 0.85)),
    );
    view.push(
        ViewNode::cuboid("trash-bin/lid", Vec3::new(0.45, 0.05, 0.07), 0.015)
            .with_parent(TRASH)
            .at(Vec3::new(0.0, 0.25, 0.0))
            .with_color(Color::srgb(0.55, 0.55, 0.55)),
    );

    // Palette items
    for kind in ShapeKind::ALL {
        let (created, id) = board.with_new(kind, palette_position(kind));
        view.push(
            tile_node(format!("palette-{kind:?}"), kind)
                .at(palette_position(kind))
                .draggable(move || switch_to_state_and_follow(created.clone(), tile_id(id)))
                .in_space(DragSpace::screen()),
        );
    }

    // Board tiles
    for tile in &board.tiles {
        let id = tile.id;
        let snapshot = board.clone();
        let kind = tile.kind;
        view.push(
            tile_node(tile_id(id), kind)
                .at(tile.pose.pos)
                .with_rotation(tile.pose.rotation)
                .draggable(move || tile_spec(&snapshot, id))
                .in_space(DragSpace::screen()),
        );
    }

    view
}

/// Adds colliders to every tile entity and the ground plane so avian can detect overlap.
fn spawn_tile_colliders(mut commands: Commands, tiles: Query<(Entity, &Name), Without<RigidBody>>) {
    for (entity, name) in &tiles {
        let name_str = name.as_str();
        if name_str.starts_with("shape-") {
            commands
                .entity(entity)
                .insert((RigidBody::Static, Collider::sphere(0.4)));
        } else if name_str == "ground" {
            // Make the floor a solid surface: a half-space collider whose top face is at Y=0,
            // so tiles rest on it and cannot sink below.
            commands
                .entity(entity)
                .insert((RigidBody::Static, Collider::half_space(Vec3::Y)));
        }
    }
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let target = Vec3::new(-2.6, 0.0, 0.2);
    app.add_plugins(common::default_plugins("tactile_tessellations_3d"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(initial(), view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(
            palette_position(ShapeKind::Pyramid).xz(),
            target.xz(),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations_3d.gif")
                .with_title("tactile_tessellations_3d")
                .with_description("substate: one tile's pose snaps face-to-face"),
        )
        .add_systems(Startup, |mut commands: Commands| {
            spawn_scene(&mut commands);
        });
}

fn spawn_scene(commands: &mut Commands) {
    use bevy::light::GlobalAmbientLight;
    // Camera in the corner, looking down at the scene from above
    let camera_pos = Vec3::new(10.0, 8.0, 10.0);
    let camera_target = Vec3::new(0.0, 0.0, 2.0);
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(camera_pos).looking_at(camera_target, Vec3::Y),
        Msaa::Sample4,
    ));
    // Ambient light for even illumination
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.6, 0.65, 0.7),
        brightness: 300.0,
        ..default()
    });
    // Key light from above-right
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_translation(Vec3::new(5.0, 8.0, 3.0)).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Fill light from above-left
    commands.spawn((
        DirectionalLight {
            illuminance: 3_000.0,
            ..default()
        },
        Transform::from_translation(Vec3::new(-4.0, 6.0, -2.0)).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, Pointer};

    use super::*;

    fn drag(board: &Board, id: u32, pointer: Vec3) -> bevy_composable_dragging::Frame<Board> {
        let context =
            DragContext::new(view, tile_id(id).into(), board.clone()).with_space(DragSpace::Screen);
        tile_spec(board, id).compile(&context)(Pointer::at(pointer))
    }

    #[test]
    fn every_kind_has_one_polyhedron_with_faces() {
        let polyhedra = polyhedra();
        assert_eq!(polyhedra.len(), ShapeKind::ALL.len());
        assert!(polyhedra.values().all(|poly| !poly.faces.is_empty()));
    }

    #[test]
    fn the_start_board_has_a_cube_beside_a_pyramid() {
        let board = initial();
        assert_eq!(board.tiles.len(), 2);
        let polyhedra = polyhedra();
        let tiles = board.tiles_3d(&polyhedra);
        let cube = &board.tiles[1];
        assert!(!snapping::overlaps_any_3d(
            &polyhedra[&cube.kind],
            cube.pose.pos,
            cube.pose.rotation,
            &tiles[..1],
            None,
        ));
    }

    #[test]
    fn dragging_a_tile_changes_only_its_own_pose() {
        let board = initial();
        let far = Vec3::new(-4.0, TILE_Y, -4.0);
        let frame = drag(&board, 1, far);
        assert_eq!(frame.drop_state.tiles.len(), 2);
        assert_eq!(frame.drop_state.tiles[0], board.tiles[0]);
        assert_eq!(frame.drop_state.tiles[1].kind, board.tiles[1].kind);
        assert_ne!(frame.drop_state.tiles[1].pose, board.tiles[1].pose);
        assert!((frame.drop_state.tiles[1].pose.pos.y - board.tiles[1].pose.pos.y).abs() < 1e-4);
    }

    #[test]
    fn the_other_tiles_stay_draggable_while_one_is_dragged() {
        let board = initial();
        let frame = drag(&board, 1, Vec3::new(-4.0, TILE_Y, -4.0));
        assert!(frame.view.get(&tile_id(0)).unwrap().drag.is_some());
        assert!(frame.view.get(&tile_id(1)).unwrap().drag.is_some());
    }

    #[test]
    fn a_tile_never_comes_to_rest_inside_another() {
        let board = initial();
        let polyhedra = polyhedra();
        let others = board.tiles_3d(&polyhedra);
        let pyramid = board.tiles[0].pose.pos;
        let frame = drag(&board, 1, pyramid + Vec3::new(0.05, 0.0, 0.05));
        let pose = frame.drop_state.tile(1).pose;
        assert!(!snapping::overlaps_any_3d(
            &polyhedra[&ShapeKind::Cube],
            pose.pos,
            pose.rotation,
            &others[..1],
            None,
        ));
    }

    #[test]
    fn dropping_a_tile_on_the_bin_deletes_only_that_tile() {
        let board = initial();
        let frame = drag(&board, 1, Vec3::new(5.3, 0.225, 5.5));
        assert_eq!(frame.drop_state.tiles, vec![board.tiles[0].clone()]);
    }

    #[test]
    fn a_new_tile_gets_the_next_id() {
        let board = initial();
        assert_eq!(board.with_new(ShapeKind::Star, Vec3::ZERO).1, board.next_id);
    }

    #[test]
    fn the_view_places_tiles_at_their_pose() {
        let board = initial();
        let view = view(&board);
        let node = view.get(&tile_id(0)).unwrap();
        assert!((node.transform.translation.y - TILE_Y).abs() < 1e-4);
        assert_eq!(node.drag_space, Some(DragSpace::Screen));
    }
}
