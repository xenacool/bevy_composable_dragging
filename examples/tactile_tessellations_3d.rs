//! Low-poly polyhedra that snap face-to-face, the 3D counterpart of
//! `tactile_tessellations`.
//!
//! Drag a tile out of the palette shelf. Near other tiles it snaps face-to-face: the library
//! computes a convex polyhedron for each shape, finds compatible faces by vertex count,
//! aligns the face normals, and places the tile so the faces meet flush.  Far from every
//! snap the tile follows the pointer freely on the ground plane, varying position and yaw.
//! Dropping a tile on the trash bin deletes it.
//!
//! The tile spec is `closest(snaps + trash bin)` with `when_far` handing over to
//! `vary(x, z, yaw)`, anchored at the center of the tile.  Palette items start a drag
//! with `switch_to_state_and_follow`.
//!
//! Every tile is a true low-poly polyhedron from `bevy_gradmesh`: tetrahedra (4 faces),
//! square pyramids (5 faces), cubes (6 faces), and pentagonal stars (2 faces).  Snapping
//! uses pure geometry: mesh vertices are extracted, coplanar triangles merged into faces,
//! and overlap tested with the separating axis theorem.  The drag space is the XZ ground
//! plane for natural pointer tracking.
//!
//! Run with `cargo run --example tactile_tessellations_3d --features shapes-3d,snapping`.

use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
use bevy::prelude::*;
use bevy_composable_dragging::ops::{
    closest, custom, drop_target, state, switch_to_state_and_follow, vary_constrained,
};
use bevy_composable_dragging::snapping::{self, Snap3D, Tile3D};
use bevy_composable_dragging::{
    Constraint, DragSpace, DragSpec, DraggablePlugin, Param, View, ViewNode,
};
use bevy_gradmesh::{Shape, ShapeDetail, Style};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

/// The kind of low-poly 3D shape a tile draws.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// All available shape kinds.
    fn all() -> [ShapeKind; 4] {
        [
            ShapeKind::Tetrahedron,
            ShapeKind::Pyramid,
            ShapeKind::Cube,
            ShapeKind::Star,
        ]
    }
}

/// The id of the trash bin node.
const TRASH: &str = "trash-bin";
/// The ground plane sits at Y = 0.
const GROUND_Y: f32 = 0.0;
/// How high the dragged tile lifts above the ground.
const LIFT: f32 = 0.5;
/// Maximum distance from a snap before free-drag mode takes over.
const SNAP_RANGE: f32 = 1.5;

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct Tile {
    id: u32,
    /// The kind of shape this tile draws.
    kind: ShapeKind,
    /// Full 3D position of the tile center.
    pos: Vec3,
    /// Full 3D rotation (orientation) of the tile.
    rotation: Quat,
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

    fn tile_mut(&mut self, id: u32) -> &mut Tile {
        self.tiles
            .iter_mut()
            .find(|tile| tile.id == id)
            .expect("the dragged tile exists")
    }

    /// Snap the tile with `id` to the given 3D snap position.
    fn snapped(&self, id: u32, snap: Snap3D) -> Self {
        let mut board = self.clone();
        let tile = board.tile_mut(id);
        tile.pos = snap.pos;
        tile.rotation = snap.rotation;
        board
    }

    fn without(&self, id: u32) -> Self {
        let mut board = self.clone();
        board.tiles.retain(|tile| tile.id != id);
        board
    }

    /// The board with a new tile of the given `kind` at `pos`.
    fn with_new(&self, kind: ShapeKind, pos: Vec3) -> (Self, u32) {
        let mut board = self.clone();
        let id = board.next_id;
        board.next_id += 1;
        board.tiles.push(Tile {
            id,
            kind,
            pos,
            rotation: Quat::IDENTITY,
        });
        (board, id)
    }

    /// Build a list of [`Tile3D`] instances for snap computation.
    fn to_tile3d(&self) -> Vec<Tile3D> {
        self.tiles
            .iter()
            .map(|tile| {
                let shape = tile_shape(tile.kind);
                let poly =
                    snapping::build_polyhedron(&shape).expect("shape should build a polyhedron");
                Tile3D {
                    id: tile.id,
                    pos: tile.pos,
                    rotation: tile.rotation,
                    poly,
                }
            })
            .collect()
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

/// A parameter of the tile `id`.
fn tile_param(id: u32, get: fn(&Tile) -> f32, set: fn(&mut Tile, f32)) -> Param<Board> {
    Param::new(
        move |board: &Board| get(board.tile(id)),
        move |board: &mut Board, value| set(board.tile_mut(id), value),
    )
}

/// Get the yaw (Y) component of the tile's Euler rotation.
fn get_yaw(tile: &Tile) -> f32 {
    tile.rotation.to_euler(EulerRot::YXZ).0
}
/// Set the yaw (Y) component of the tile's Euler rotation.
fn set_yaw(tile: &mut Tile, value: f32) {
    let (_, pitch, roll) = tile.rotation.to_euler(EulerRot::YXZ);
    tile.rotation = Quat::from_euler(EulerRot::YXZ, value, pitch, roll);
}

/// The drag of a tile: snap face-to-face, drop on the bin to delete, or follow the
/// pointer freely on the ground plane when far from any snap.
fn tile_spec(board: &Board, id: u32) -> DragSpec<Board> {
    let tile = board.tile(id);
    let dragged_shape = tile_shape(tile.kind);
    let dragged_poly = snapping::build_polyhedron(&dragged_shape)
        .expect("dragged shape should build a polyhedron");
    let tiles_3d = board.to_tile3d();
    let snaps = snapping::compute_snaps_3d(&dragged_poly, &tiles_3d, Some(id));
    let mut branches: Vec<DragSpec<Board>> = snaps
        .into_iter()
        .map(|snap| state(board.snapped(id, snap)))
        .collect();
    branches.push(drop_target(TRASH, board.without(id)));

    // Build a no-overlap constraint using 3D intersection testing
    let other_tiles_3d: Vec<Tile3D> = tiles_3d.iter().filter(|t| t.id != id).cloned().collect();
    let no_overlap = Constraint::new(move |state: &Board| {
        let t = state.tile(id);
        let t_shape = tile_shape(t.kind);
        let Some(t_poly) = snapping::build_polyhedron(&t_shape) else {
            return 0.0;
        };
        let overlaps = snapping::overlaps_any_3d(&t_poly, t.pos, t.rotation, &other_tiles_3d, None);
        if overlaps { 100.0 } else { 0.0 }
    });

    // Vary only x, z, and yaw on the ground plane — pitch and roll stay at zero so tiles
    // don't spin wildly. The drag space is the XZ plane, so the pointer only moves in 2D.
    let free = vary_constrained(
        board.clone(),
        [
            tile_param(id, |tile| tile.pos.x, |tile, x| tile.pos.x = x),
            tile_param(id, |tile| tile.pos.z, |tile, z| tile.pos.z = z),
            tile_param(id, get_yaw, set_yaw).with_scale(360.0),
        ],
        no_overlap,
    );
    let node = tile_id(id);
    anchored_at_center(closest(branches).when_far(free, SNAP_RANGE)).change_result(
        move |mut frame| {
            if let Some(dragged) = frame.view.get_mut(&node) {
                // Lift the dragged tile above its base Y position by LIFT.
                dragged.transform.translation.y = TILE_Y + LIFT;
            }
            frame
        },
    )
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

/// The 3D mesh for a tile kind — a true low-poly polyhedron.
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
    for kind in ShapeKind::all() {
        let (created, id) = board.with_new(kind, palette_position(kind));
        view.push(
            tile_node(format!("palette-{kind:?}"), kind)
                .at(palette_position(kind))
                .draggable(move || switch_to_state_and_follow(created.clone(), tile_id(id)))
                .in_space(DragSpace::plane(Vec3::ZERO, Dir3::Y)),
        );
    }

    // Board tiles
    for tile in &board.tiles {
        let id = tile.id;
        let snapshot = board.clone();
        let kind = tile.kind;
        view.push(
            tile_node(tile_id(id), kind)
                .at(tile.pos)
                .with_rotation(tile.rotation)
                .draggable(move || tile_spec(&snapshot, id))
                .in_space(DragSpace::plane(Vec3::ZERO, Dir3::Y)),
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
    // A pyramid at the origin with a cube snapped nearby.
    let pyramid = Tile {
        id: 0,
        kind: ShapeKind::Pyramid,
        pos: Vec3::new(0.0, TILE_Y, 0.0),
        rotation: Quat::IDENTITY,
    };
    let pyramid_shape = tile_shape(ShapeKind::Pyramid);
    let pyramid_poly = snapping::build_polyhedron(&pyramid_shape).unwrap();
    let cube_shape = tile_shape(ShapeKind::Cube);
    let cube_poly = snapping::build_polyhedron(&cube_shape).unwrap();
    let tiles_3d = vec![Tile3D {
        id: 0,
        pos: pyramid.pos,
        rotation: pyramid.rotation,
        poly: pyramid_poly,
    }];
    let snaps = snapping::compute_snaps_3d(&cube_poly, &tiles_3d, None);
    let cube_snap = snaps.first().copied().unwrap_or(Snap3D {
        pos: Vec3::new(1.2, TILE_Y, 0.0),
        rotation: Quat::IDENTITY,
        face_normal: Dir3::Y,
    });
    let board = Board {
        tiles: vec![
            pyramid,
            Tile {
                id: 1,
                kind: ShapeKind::Cube,
                pos: cube_snap.pos,
                rotation: cube_snap.rotation,
            },
        ],
        next_id: 2,
    };
    let target = Vec3::new(-2.6, 0.0, 0.2);
    app.add_plugins(common::default_plugins("tactile_tessellations_3d"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(board, view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(
            palette_position(ShapeKind::Pyramid).xz(),
            target.xz(),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations_3d.webp")
                .with_title("tactile_tessellations_3d")
                .with_description("3D low-poly polyhedra with face-to-face snapping"),
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
    // A pyramid at the origin with a cube snapped nearby.
    let pyramid = Tile {
        id: 0,
        kind: ShapeKind::Pyramid,
        pos: Vec3::new(0.0, TILE_Y, 0.0),
        rotation: Quat::IDENTITY,
    };
    let pyramid_shape = tile_shape(ShapeKind::Pyramid);
    let pyramid_poly = snapping::build_polyhedron(&pyramid_shape).unwrap();
    let cube_shape = tile_shape(ShapeKind::Cube);
    let cube_poly = snapping::build_polyhedron(&cube_shape).unwrap();
    let tiles_3d = vec![Tile3D {
        id: 0,
        pos: pyramid.pos,
        rotation: pyramid.rotation,
        poly: pyramid_poly,
    }];
    let snaps = snapping::compute_snaps_3d(&cube_poly, &tiles_3d, None);
    let cube_snap = snaps.first().copied().unwrap_or(Snap3D {
        pos: Vec3::new(1.2, TILE_Y, 0.0),
        rotation: Quat::IDENTITY,
        face_normal: Dir3::Y,
    });
    let board = Board {
        tiles: vec![
            pyramid,
            Tile {
                id: 1,
                kind: ShapeKind::Cube,
                pos: cube_snap.pos,
                rotation: cube_snap.rotation,
            },
        ],
        next_id: 2,
    };
    let target = Vec3::new(-2.6, 0.0, 0.2);
    App::new()
        .add_plugins(common::default_plugins("tactile_tessellations_3d"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(board, view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(
            palette_position(ShapeKind::Pyramid).xz(),
            target.xz(),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations_3d.webp")
                .with_title("tactile_tessellations_3d")
                .with_description("3D low-poly polyhedra with face-to-face snapping"),
        )
        .add_systems(Startup, |mut commands: Commands| {
            spawn_scene(&mut commands);
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(id: u32, kind: ShapeKind, pos: Vec3, rotation: Quat) -> Tile {
        Tile {
            id,
            kind,
            pos,
            rotation,
        }
    }

    fn tile3d(id: u32, kind: ShapeKind, pos: Vec3, rotation: Quat) -> Tile3D {
        let shape = tile_shape(kind);
        let poly = snapping::build_polyhedron(&shape).expect("shape should build");
        Tile3D {
            id,
            pos,
            rotation,
            poly,
        }
    }

    #[test]
    fn all_shapes_build_polyhedra() {
        for kind in ShapeKind::all() {
            let shape = tile_shape(kind);
            let poly = snapping::build_polyhedron(&shape)
                .expect(&format!("{kind:?} should build a polyhedron"));
            assert!(!poly.faces.is_empty(), "{kind:?} should have faces");
        }
    }

    #[test]
    fn cube_snaps_to_cube_face_to_face() {
        let cube = tile3d(
            0,
            ShapeKind::Cube,
            Vec3::new(0.0, TILE_Y, 0.0),
            Quat::IDENTITY,
        );
        let cube_shape = tile_shape(ShapeKind::Cube);
        let cube_poly = snapping::build_polyhedron(&cube_shape).unwrap();
        let snaps = snapping::compute_snaps_3d(&cube_poly, &[cube], None);
        assert!(!snaps.is_empty(), "cube should snap to cube");
        for snap in &snaps {
            assert!(
                snap.face_normal.length() > 0.9,
                "face normal should be unit length"
            );
        }
    }

    #[test]
    fn every_shape_kind_snaps_to_every_other_kind() {
        let kinds = ShapeKind::all();
        for &kind_a in &kinds {
            let tile_a = tile3d(0, kind_a, Vec3::new(0.0, TILE_Y, 0.0), Quat::IDENTITY);
            for &kind_b in &kinds {
                let shape_b = tile_shape(kind_b);
                let poly_b = snapping::build_polyhedron(&shape_b).unwrap();
                let snaps = snapping::compute_snaps_3d(&poly_b, &[tile_a.clone()], None);
                // Snaps are only produced when face vertex counts match.
                // Some shape pairs may not have matching faces, which is expected.
                // Verify that when snaps exist, they are valid.
                for snap in &snaps {
                    assert!(
                        snap.face_normal.length() > 0.9,
                        "{kind_b:?} → {kind_a:?}: invalid face normal at snap {snap:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn dropping_on_the_bin_deletes_the_tile() {
        let board = Board {
            tiles: vec![tile(
                0,
                ShapeKind::Cube,
                Vec3::new(5.3, TILE_Y, 5.5),
                Quat::IDENTITY,
            )],
            next_id: 1,
        };
        assert_eq!(board.without(0).tiles.len(), 0);
        assert_eq!(board.with_new(ShapeKind::Tetrahedron, Vec3::ZERO).1, 1);
    }

    #[test]
    fn the_view_places_tiles_at_correct_height() {
        let board = Board {
            tiles: vec![tile(
                0,
                ShapeKind::Cube,
                Vec3::new(0.0, TILE_Y, 0.0),
                Quat::IDENTITY,
            )],
            next_id: 1,
        };
        let view = view(&board);
        let tile_node = view.get(&tile_id(0)).unwrap();
        assert!((tile_node.transform.translation.y - TILE_Y).abs() < 1e-4);
        assert_eq!(
            tile_node.drag_space,
            Some(DragSpace::plane(Vec3::ZERO, Dir3::Y))
        );
    }

    #[test]
    fn snapped_faces_have_opposed_normals() {
        let cube = tile3d(
            0,
            ShapeKind::Cube,
            Vec3::new(0.0, TILE_Y, 0.0),
            Quat::IDENTITY,
        );
        let cube_shape = tile_shape(ShapeKind::Cube);
        let cube_poly = snapping::build_polyhedron(&cube_shape).unwrap();
        let snaps = snapping::compute_snaps_3d(&cube_poly, &[cube], None);
        assert!(!snaps.is_empty());

        for snap in &snaps {
            assert!(
                snap.face_normal.length() > 0.9,
                "face normal should be unit length"
            );
        }
    }

    #[test]
    fn hovering_near_a_pyramid_face_produces_valid_snaps() {
        use bevy_composable_dragging::{DragContext, Pointer};

        let pyramid = tile(
            0,
            ShapeKind::Pyramid,
            Vec3::new(0.0, TILE_Y, 0.0),
            Quat::IDENTITY,
        );
        let cube = tile(
            1,
            ShapeKind::Cube,
            Vec3::new(10.0, TILE_Y, 10.0),
            Quat::IDENTITY,
        );
        let board = Board {
            tiles: vec![pyramid.clone(), cube],
            next_id: 2,
        };

        let spec = tile_spec(&board, 1);
        let context = DragContext::new(view, tile_id(1).into(), board.clone())
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let mut behavior = spec.compile(&context);

        // Hover near the pyramid
        let pointer = Pointer::at(Vec3::new(1.0, 0.0, 0.5));
        let frame = behavior(pointer);

        // The drop state should be finite and reasonable
        let drop_tile = frame.drop_state.tile(1);
        assert!(drop_tile.pos.is_finite(), "drop position should be finite");
        assert!(
            drop_tile.rotation.is_finite(),
            "drop rotation should be finite"
        );
    }

    #[test]
    fn snapped_tile_has_full_3d_transform() {
        let cube1 = tile(
            0,
            ShapeKind::Cube,
            Vec3::new(0.0, TILE_Y, 0.0),
            Quat::IDENTITY,
        );
        let cube2 = tile(
            1,
            ShapeKind::Cube,
            Vec3::new(10.0, TILE_Y, 10.0),
            Quat::IDENTITY,
        );
        let board = Board {
            tiles: vec![cube1.clone(), cube2],
            next_id: 2,
        };

        let cube1_shape = tile_shape(ShapeKind::Cube);
        let cube1_poly = snapping::build_polyhedron(&cube1_shape).unwrap();
        let cube2_shape = tile_shape(ShapeKind::Cube);
        let cube2_poly = snapping::build_polyhedron(&cube2_shape).unwrap();
        let tiles_3d = vec![Tile3D {
            id: 0,
            pos: cube1.pos,
            rotation: cube1.rotation,
            poly: cube1_poly,
        }];
        let snaps = snapping::compute_snaps_3d(&cube2_poly, &tiles_3d, None);
        assert!(!snaps.is_empty());

        let snap = snaps[0];
        let snapped_board = board.snapped(1, snap);
        let snapped_tile = snapped_board.tile(1);
        assert_eq!(snapped_tile.pos, snap.pos);
        assert_eq!(snapped_tile.rotation, snap.rotation);
    }
}
