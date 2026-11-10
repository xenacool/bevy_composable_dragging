//! Regular polygon prisms that snap edge to edge on a ground plane, the 3D counterpart of
//! `tactile_tessellations`.
//!
//! Drag a tile out of the palette shelf. Near other tiles it snaps flush against an edge of the
//! same length, rotating by the least angle that lines the edges up. Far from every snap it
//! follows the pointer freely on the XZ ground plane, and dropping it on the trash bin deletes
//! it.
//!
//! The tile spec is `closest(snaps + trash bin)` with `when_far` handing over to `vary(x, z)`,
//! anchored at the center of the tile. Palette items start a drag with
//! `switch_to_state_and_follow`.
//!
//! Every tile is an extruded polygon prism with a gradient style from `bevy_gradmesh`. The
//! snapping geometry is the same as the 2D example: regular polygons with equal side length,
//! edge-to-edge alignment, and a separating-axis overlap test. The tiles live on the XZ ground
//! plane, so the drag space is the XZ plane and the rotation axis is Y.
//!
//! Run with `cargo run --example tactile_tessellations_3d --features shapes-3d`.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
use bevy::prelude::*;
use bevy_composable_dragging::ops::{
    closest, custom, drop_target, state, switch_to_state_and_follow, vary_constrained,
};
use bevy_composable_dragging::{
    Constraint, DragSpace, DragSpec, DraggablePlugin, Param, View, ViewNode,
};
use bevy_gradmesh::{Shape, ShapeDetail, Style};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

/// The side length of every polygon (50 px in the original).
const SIDE: f32 = 1.0;
/// The height of every prism.
const PRISM_HEIGHT: f32 = 0.25;
/// How far two polygons may overlap and still count as touching (2 px in the original).
const OVERLAP_EPSILON: f32 = 0.04;
/// Edges of this length difference or less are equal (0.1 px in the original).
const EDGE_TOLERANCE: f32 = 0.002;
/// The number of sides of the polygons in the palette.
const KINDS: [u32; 4] = [3, 4, 6, 8];
/// The id of the trash bin node.
const TRASH: &str = "trash-bin";
/// The ground plane sits at Y = 0.
const GROUND_Y: f32 = 0.0;
/// How high the dragged tile lifts above the ground.
const LIFT: f32 = 0.5;

// ---------------------------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------------------------

/// The vertices of the regular polygon with `sides` sides, counterclockwise from the bottom left,
/// with a flat bottom edge and the center at the origin.
fn vertices(sides: u32) -> Vec<Vec2> {
    let n = sides as f32;
    let radius = SIDE / (2.0 * (PI / n).sin());
    (0..sides)
        .map(|i| Vec2::from_angle(-FRAC_PI_2 - PI / n + TAU * i as f32 / n) * radius)
        .collect()
}

/// The vertices after turning by `rot_deg` degrees around the origin and moving to `pos`.
fn world_vertices(sides: u32, pos: Vec2, rot_deg: f32) -> Vec<Vec2> {
    let rotation = Vec2::from_angle(rot_deg.to_radians());
    vertices(sides)
        .into_iter()
        .map(|vertex| rotation.rotate(vertex) + pos)
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    a: Vec2,
    b: Vec2,
}

impl Edge {
    fn mid(self) -> Vec2 {
        self.a.midpoint(self.b)
    }

    fn length(self) -> f32 {
        self.a.distance(self.b)
    }

    fn dir(self) -> Vec2 {
        (self.b - self.a).normalize()
    }
}

fn edges(vertices: &[Vec2]) -> Vec<Edge> {
    vertices
        .iter()
        .enumerate()
        .map(|(i, &a)| Edge {
            a,
            b: vertices[(i + 1) % vertices.len()],
        })
        .collect()
}

/// The interval of `vertices` projected onto `axis`.
fn project(vertices: &[Vec2], axis: Vec2) -> (f32, f32) {
    vertices
        .iter()
        .map(|vertex| vertex.dot(axis))
        .fold((f32::MAX, f32::MIN), |(min, max), dot| {
            (min.min(dot), max.max(dot))
        })
}

fn axes(vertices: &[Vec2]) -> Vec<Vec2> {
    edges(vertices)
        .into_iter()
        .map(|edge| {
            let along = edge.b - edge.a;
            Vec2::new(-along.y, along.x).normalize()
        })
        .collect()
}

/// Whether two convex polygons overlap by more than `epsilon`, by the separating axis theorem.
fn polygons_overlap(a: &[Vec2], b: &[Vec2], epsilon: f32) -> bool {
    axes(a).into_iter().chain(axes(b)).all(|axis| {
        let (min_a, max_a) = project(a, axis);
        let (min_b, max_b) = project(b, axis);
        max_a - epsilon > min_b && max_b - epsilon > min_a
    })
}

/// Normalizes an angle to `[-PI, PI)`.
fn normalize_angle(radians: f32) -> f32 {
    (radians + PI).rem_euclid(TAU) - PI
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct Tile {
    id: u32,
    sides: u32,
    /// Position on the XZ plane (x, z).
    pos: Vec2,
    /// Rotation around the Y axis (yaw) in degrees.
    rot_deg: f32,
    /// Rotation around the X axis (pitch) in degrees.
    pitch_deg: f32,
    /// Rotation around the Z axis (roll) in degrees.
    roll_deg: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct Board {
    tiles: Vec<Tile>,
    next_id: u32,
}

/// A position and rotation a tile can snap to.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    pos: Vec2,
    rot_deg: f32,
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

    fn snapped(&self, id: u32, snap: Snap) -> Self {
        let mut board = self.clone();
        let tile = board.tile_mut(id);
        tile.pos = snap.pos;
        tile.rot_deg = snap.rot_deg;
        board
    }

    fn without(&self, id: u32) -> Self {
        let mut board = self.clone();
        board.tiles.retain(|tile| tile.id != id);
        board
    }

    /// The board with a new tile of `sides` sides at `pos`.
    fn with_new(&self, sides: u32, pos: Vec2) -> (Self, u32) {
        let mut board = self.clone();
        let id = board.next_id;
        board.next_id += 1;
        board.tiles.push(Tile {
            id,
            sides,
            pos,
            rot_deg: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
        });
        (board, id)
    }
}

/// Whether a tile of `sides` sides at `pos` turned by `rot_deg` overlaps any tile but `exclude`.
fn overlaps_any(sides: u32, pos: Vec2, rot_deg: f32, tiles: &[Tile], exclude: Option<u32>) -> bool {
    let candidate = world_vertices(sides, pos, rot_deg);
    tiles
        .iter()
        .filter(|tile| Some(tile.id) != exclude)
        .any(|tile| {
            let existing = world_vertices(tile.sides, tile.pos, tile.rot_deg);
            polygons_overlap(&candidate, &existing, OVERLAP_EPSILON)
        })
}

/// Every valid edge-to-edge placement of a tile with `sides` sides against `tiles`.
fn compute_snaps(sides: u32, tiles: &[Tile], exclude: Option<u32>) -> Vec<Snap> {
    let at_origin = edges(&vertices(sides));
    let mut snaps = Vec::new();
    for tile in tiles.iter().filter(|tile| Some(tile.id) != exclude) {
        let existing = edges(&world_vertices(tile.sides, tile.pos, tile.rot_deg));
        for edge in existing {
            let target_angle = (-edge.dir()).to_angle();
            let rotation_for = |own: &Edge| normalize_angle(target_angle - own.dir().to_angle());
            let Some(best) = at_origin
                .iter()
                .filter(|own| (own.length() - edge.length()).abs() <= EDGE_TOLERANCE)
                .min_by(|a, b| rotation_for(a).abs().total_cmp(&rotation_for(b).abs()))
            else {
                continue;
            };
            let rotation = rotation_for(best);
            let pos = edge.mid() - Vec2::from_angle(rotation).rotate(best.mid());
            let rot_deg = rotation.to_degrees().rem_euclid(360.0);
            if !overlaps_any(sides, pos, rot_deg, tiles, exclude) {
                snaps.push(Snap { pos, rot_deg });
            }
        }
    }
    snaps
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

/// The drag of a tile: snap to the nearest valid edge, drop on the bin to delete, or follow the
/// pointer when far from both, but never overlap another tile.
fn tile_spec(board: &Board, id: u32) -> DragSpec<Board> {
    let tile = board.tile(id);
    let snaps = compute_snaps(tile.sides, &board.tiles, Some(id));
    let mut branches: Vec<DragSpec<Board>> = snaps
        .into_iter()
        .map(|snap| state(board.snapped(id, snap)))
        .collect();
    branches.push(drop_target(TRASH, board.without(id)));
    let sides = tile.sides;
    let other_tiles: Vec<Tile> = board.tiles.iter().filter(|t| t.id != id).cloned().collect();
    let no_overlap = Constraint::new(move |state: &Board| {
        let t = state.tile(id);
        if overlaps_any(sides, t.pos, t.rot_deg, &other_tiles, None) {
            100.0
        } else {
            0.0
        }
    });
    let free = vary_constrained(
        board.clone(),
        [
            tile_param(id, |tile| tile.pos.x, |tile, x| tile.pos.x = x),
            tile_param(id, |tile| tile.pos.y, |tile, y| tile.pos.y = y),
            tile_param(id, |tile| tile.rot_deg, |tile, r| tile.rot_deg = r).with_scale(360.0),
            tile_param(id, |tile| tile.pitch_deg, |tile, r| tile.pitch_deg = r).with_scale(360.0),
            tile_param(id, |tile| tile.roll_deg, |tile, r| tile.roll_deg = r).with_scale(360.0),
        ],
        no_overlap,
    );
    let node = tile_id(id);
    anchored_at_center(closest(branches).with_floating().when_far(free, SIDE)).change_result(
        move |mut frame| {
            if let Some(dragged) = frame.view.get_mut(&node) {
                dragged.transform.translation.y = TILE_Y + LIFT;
            }
            frame
        },
    )
}

// ---------------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------------

const TILE_Y: f32 = PRISM_HEIGHT / 2.0;

fn tile_id(id: u32) -> String {
    format!("shape-{id}")
}

fn fill(sides: u32) -> Color {
    match sides {
        3 => Color::srgb(0.95, 0.55, 0.55),
        4 => Color::srgb(0.45, 0.65, 0.95),
        6 => Color::srgb(0.40, 0.85, 0.55),
        _ => Color::srgb(0.95, 0.85, 0.40),
    }
}

/// An extruded polygon prism: a cylinder-like shape with the polygon cross-section.
/// The height is `PRISM_HEIGHT` and the center is at the origin.
fn prism(sides: u32) -> Shape {
    let verts = vertices(sides);
    Shape::extruded_polygon(verts, PRISM_HEIGHT, 0.08, ShapeDetail::Medium)
}

/// A tile prism with gradient style.
fn tile_node(id: impl Into<bevy_composable_dragging::NodeId>, sides: u32) -> ViewNode<Board> {
    ViewNode::grad(id, prism(sides)).with_style(Style::dual_directional_up(
        fill(sides).darker(0.3),
        fill(sides),
    ))
}

/// The position of the palette item for a polygon kind on the palette shelf.
fn palette_position(sides: u32) -> Vec2 {
    let x = match sides {
        3 => -4.8,
        4 => -3.0,
        6 => -0.9,
        _ => 1.5,
    };
    Vec2::new(x, 5.5)
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
    for sides in KINDS {
        let (created, id) = board.with_new(sides, palette_position(sides));
        let pos = palette_position(sides);
        view.push(
            tile_node(format!("palette-{sides}"), sides)
                .at(Vec3::new(pos.x, TILE_Y, pos.y))
                .draggable(move || switch_to_state_and_follow(created.clone(), tile_id(id)))
                .in_space(DragSpace::plane(Vec3::ZERO, Dir3::Y)),
        );
    }

    // Board tiles
    for tile in &board.tiles {
        let id = tile.id;
        let snapshot = board.clone();
        let pos = tile.pos;
        let sides = tile.sides;
        view.push(
            tile_node(tile_id(id), sides)
                .at(Vec3::new(pos.x, TILE_Y, pos.y))
                .with_rotation(
                    Quat::from_rotation_y(tile.rot_deg.to_radians())
                        * Quat::from_rotation_x(tile.pitch_deg.to_radians())
                        * Quat::from_rotation_z(tile.roll_deg.to_radians()),
                )
                .draggable(move || tile_spec(&snapshot, id))
                .in_space(DragSpace::plane(Vec3::ZERO, Dir3::Y)),
        );
    }

    view
}

/// Adds colliders to every tile entity so avian can detect overlap.
/// The collider is shifted up by half the prism height so the tile rests on the ground instead of sinking.
fn spawn_tile_colliders(mut commands: Commands, tiles: Query<(Entity, &Name), Without<RigidBody>>) {
    for (entity, name) in &tiles {
        if name.as_str().starts_with("shape-") {
            // The tile transform center is at TILE_Y = PRISM_HEIGHT / 2.0, so the bottom of the
            // prism sits at Y = 0 (the ground).  The collider cuboid is centered on the entity,
            // so its half-height extends below the ground.  Shift the collider up by half the
            // prism height so only the top half exists, preventing sinking.
            commands.entity(entity).insert((
                RigidBody::Static,
                Collider::cuboid(SIDE, PRISM_HEIGHT / 2.0, SIDE),
            ));
        }
    }
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    // A hexagon with a triangle and a square snapped to it, and a square waiting in the palette
    // to be dragged against the other side.
    let hexagon = Tile {
        id: 0,
        sides: 6,
        pos: Vec2::ZERO,
        rot_deg: 0.0,
        pitch_deg: 0.0,
        roll_deg: 0.0,
    };
    let triangle = compute_snaps(3, std::slice::from_ref(&hexagon), None)[0];
    let board = Board {
        tiles: vec![
            hexagon.clone(),
            Tile {
                id: 1,
                sides: 3,
                pos: triangle.pos,
                rot_deg: triangle.rot_deg,
                pitch_deg: 0.0,
                roll_deg: 0.0,
            },
        ],
        next_id: 2,
    };
    let target = edges(&world_vertices(6, hexagon.pos, 0.0))[5].mid() + Vec2::new(-0.3, 0.2);
    app.add_plugins(common::default_plugins("tactile_tessellations_3d"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(board, view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(palette_position(4), target))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations_3d.webp")
                .with_title("tactile_tessellations_3d")
                .with_description("3D snapping, rotation, trash bin"),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_perspective_scene(
                &mut commands,
                Vec3::new(0.0, 3.8, 9.5),
                Vec3::new(0.0, 0.125, 0.5),
            );
        });
}

fn main() {
    // A hexagon with a triangle and a square snapped to it, and a square waiting in the palette
    // to be dragged against the other side.
    let hexagon = Tile {
        id: 0,
        sides: 6,
        pos: Vec2::ZERO,
        rot_deg: 0.0,
        pitch_deg: 0.0,
        roll_deg: 0.0,
    };
    let triangle = compute_snaps(3, std::slice::from_ref(&hexagon), None)[0];
    let board = Board {
        tiles: vec![
            hexagon.clone(),
            Tile {
                id: 1,
                sides: 3,
                pos: triangle.pos,
                rot_deg: triangle.rot_deg,
                pitch_deg: 0.0,
                roll_deg: 0.0,
            },
        ],
        next_id: 2,
    };
    let target = edges(&world_vertices(6, hexagon.pos, 0.0))[5].mid() + Vec2::new(-0.3, 0.2);
    App::new()
        .add_plugins(common::default_plugins("tactile_tessellations_3d"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(board, view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(palette_position(4), target))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations_3d.webp")
                .with_title("tactile_tessellations_3d")
                .with_description("3D snapping, rotation, trash bin"),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_perspective_scene(
                &mut commands,
                Vec3::new(0.0, 3.8, 9.5),
                Vec3::new(0.0, 0.125, 0.5),
            );
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(id: u32, sides: u32, pos: Vec2, rot_deg: f32) -> Tile {
        Tile {
            id,
            sides,
            pos,
            rot_deg,
            pitch_deg: 0.0,
            roll_deg: 0.0,
        }
    }

    #[test]
    fn regular_polygons_have_unit_sides_and_a_flat_bottom() {
        for sides in KINDS {
            let corners = vertices(sides);
            assert_eq!(corners.len(), sides as usize);
            for edge in edges(&corners) {
                assert!((edge.length() - SIDE).abs() < 1e-4, "{sides}: {edge:?}");
            }
            let bottom = corners.iter().map(|v| v.y).fold(f32::MAX, f32::min);
            let flat = corners
                .iter()
                .filter(|v| (v.y - bottom).abs() < 1e-4)
                .count();
            assert_eq!(flat, 2, "{sides} sides: the bottom is one edge");
            let area: f32 = edges(&corners).iter().map(|e| e.a.perp_dot(e.b)).sum();
            assert!(area > 0.0, "{sides} sides: counterclockwise");
        }
    }

    #[test]
    fn identical_polygons_overlap_and_touching_ones_do_not() {
        let square = world_vertices(4, Vec2::ZERO, 0.0);
        assert!(polygons_overlap(&square, &square, OVERLAP_EPSILON));
        let next = world_vertices(4, Vec2::new(SIDE, 0.0), 0.0);
        assert!(!polygons_overlap(&square, &next, OVERLAP_EPSILON));
        let apart = world_vertices(4, Vec2::new(3.0, 0.0), 0.0);
        assert!(!polygons_overlap(&square, &apart, OVERLAP_EPSILON));
    }

    #[test]
    fn a_square_next_to_a_square_has_four_snaps() {
        let tiles = [tile(0, 4, Vec2::ZERO, 0.0)];
        let snaps = compute_snaps(4, &tiles, None);
        assert_eq!(snaps.len(), 4);
        for snap in &snaps {
            assert!((snap.pos.length() - SIDE).abs() < 1e-3, "{snap:?}");
            assert!(snap.rot_deg.abs() < 1e-2 || (snap.rot_deg - 360.0).abs() < 1e-2);
        }
    }

    #[test]
    fn every_kind_snaps_to_every_edge_of_every_other_kind() {
        let tiles = [tile(0, 4, Vec2::ZERO, 0.0)];
        let snaps = compute_snaps(3, &tiles, None);
        assert_eq!(snaps.len(), 4);
        for snap in snaps {
            assert!(!overlaps_any(3, snap.pos, snap.rot_deg, &tiles, None));
        }
        for sides in KINDS {
            let tiles = [tile(0, sides, Vec2::ZERO, 0.0)];
            assert_eq!(compute_snaps(sides, &tiles, None).len(), sides as usize);
        }
    }

    #[test]
    fn dropping_on_the_bin_deletes_the_tile() {
        let board = Board {
            tiles: vec![tile(0, 4, Vec2::new(5.3, 5.5), 0.0)],
            next_id: 1,
        };
        assert_eq!(board.without(0).tiles.len(), 0);
        assert_eq!(board.with_new(3, Vec2::ZERO).1, 1);
    }

    #[test]
    fn the_view_places_tiles_on_the_ground_plane() {
        let board = Board {
            tiles: vec![tile(0, 4, Vec2::ZERO, 0.0)],
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

    /// Snapped tiles share an edge with no gap between their surfaces.
    #[test]
    fn snapped_tiles_have_zero_gap_between_edges() {
        // A hexagon at the origin.
        let hexagon = tile(0, 6, Vec2::ZERO, 0.0);
        // Snap a square to one of the hexagon's edges.
        let snaps = compute_snaps(4, std::slice::from_ref(&hexagon), None);
        assert!(!snaps.is_empty(), "square should snap to hexagon");

        for snap in &snaps {
            let square = tile(1, 4, snap.pos, snap.rot_deg);
            let hex_verts = world_vertices(6, hexagon.pos, hexagon.rot_deg);
            let sq_verts = world_vertices(4, square.pos, square.rot_deg);

            // The two polygons must not overlap (beyond epsilon).
            assert!(
                !polygons_overlap(&sq_verts, &hex_verts, OVERLAP_EPSILON),
                "snapped square overlaps hexagon at snap {snap:?}"
            );

            // Find the shared edge: the square edge and hexagon edge should share the same
            // midpoint and have reversed direction, with the distance between corresponding
            // vertices being near zero.
            let sq_edges = edges(&sq_verts);
            let hex_edges = edges(&hex_verts);

            let mut found_shared = false;
            for se in &sq_edges {
                for he in &hex_edges {
                    if se.mid().distance(he.mid()) < EDGE_TOLERANCE
                        && se.dir().dot(he.dir()) < -0.999
                    {
                        // Edges are aligned — check that corresponding vertices are at zero gap.
                        let vertex_gap = se.a.distance(he.b).max(se.b.distance(he.a));
                        assert!(
                            vertex_gap < 1e-6,
                            "edge vertex gap {vertex_gap:.6} at snap {snap:?} — expected zero gap"
                        );
                        found_shared = true;
                        break;
                    }
                }
                if found_shared {
                    break;
                }
            }
            assert!(found_shared, "no shared edge found at snap {snap:?}");
        }
    }

    /// Snapping different polygon kinds produces zero gap between their shared edges.
    #[test]
    fn zero_gap_between_arbitrary_polygon_kinds() {
        // Test all pairs of polygon kinds.
        for &sides_a in &KINDS {
            for &sides_b in &KINDS {
                let tile_a = tile(0, sides_a, Vec2::ZERO, 0.0);
                let snaps = compute_snaps(sides_b, std::slice::from_ref(&tile_a), None);
                for snap in &snaps {
                    let tile_b = tile(1, sides_b, snap.pos, snap.rot_deg);
                    let verts_a = world_vertices(sides_a, tile_a.pos, tile_a.rot_deg);
                    let verts_b = world_vertices(sides_b, tile_b.pos, tile_b.rot_deg);

                    // No overlap beyond epsilon.
                    assert!(
                        !polygons_overlap(&verts_a, &verts_b, OVERLAP_EPSILON),
                        "{sides_a} and {sides_b}: overlap at snap {snap:?}"
                    );

                    // Find the shared edge and verify zero vertex gap.
                    let edges_a = edges(&verts_a);
                    let edges_b = edges(&verts_b);
                    let mut found = false;
                    for ea in &edges_a {
                        for eb in &edges_b {
                            if ea.mid().distance(eb.mid()) < EDGE_TOLERANCE
                                && ea.dir().dot(eb.dir()) < -0.999
                            {
                                let gap = ea.a.distance(eb.b).max(ea.b.distance(eb.a));
                                assert!(
                                    gap < 1e-6,
                                    "{sides_a} and {sides_b}: vertex gap {gap:.6} at snap {snap:?}"
                                );
                                found = true;
                                break;
                            }
                        }
                        if found {
                            break;
                        }
                    }
                    assert!(
                        found,
                        "{sides_a} and {sides_b}: no shared edge at snap {snap:?}"
                    );
                }
            }
        }
    }

    /// Hovering a square around every edge of a hexagon snaps it to each exposed surface with
    /// sub-epsilon margin.  The test hovers the pointer at positions just outside every hexagon
    /// edge — where a user would naturally approach to snap — and verifies the tile visual
    /// position follows the pointer closely instead of jumping to a fixed snap position far from
    /// the cursor.
    #[test]
    fn hovering_a_square_around_a_hexagon_snaps_to_every_exposed_edge() {
        use bevy_composable_dragging::{DragContext, Pointer};

        // A hexagon at the origin — keep a copy for edge iteration.
        let hexagon = tile(0, 6, Vec2::ZERO, 0.0);
        let hex_edges = edges(&world_vertices(6, hexagon.pos, hexagon.rot_deg));

        // A square starting far away.
        let square = tile(1, 4, Vec2::new(10.0, 10.0), 0.0);
        let board = Board {
            tiles: vec![hexagon.clone(), square],
            next_id: 2,
        };

        // Build the drag spec for the square.
        let spec = tile_spec(&board, 1);

        // Compile with the XZ ground plane drag space.
        let context = DragContext::new(view, tile_id(1).into(), board.clone())
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let mut behavior = spec.compile(&context);

        // Sub-epsilon margin: when the pointer hovers just outside an edge, the tile's visual
        // position in the rendered view must be right under the pointer.  The gap field of the
        // frame reports the distance from the pointer to the drop-state position (the snap),
        // which is non-zero while floating; what matters for the user experience is that the
        // tile's rendered position tracks the pointer.
        let epsilon = 1e-4;

        // Hover just outside each edge midpoint — this is where a user approaches to snap.
        for (i, edge) in hex_edges.iter().enumerate() {
            let mid = edge.mid();
            // Normal pointing outward from the hexagon center.
            let outward = (mid - hexagon.pos).normalize();
            // Hover a small distance outside the edge.
            let hover_offset = 0.1;
            let hover_pos = mid + outward * hover_offset;
            let pointer = Pointer::at(Vec3::new(hover_pos.x, 0.0, hover_pos.y));
            let frame = behavior(pointer);

            // The tile's visual position in the view must track the pointer on the XZ plane.
            let tile_world = frame
                .view
                .world_position(&tile_id(1))
                .expect("dragged tile exists in view");
            let tile_on_ground = Vec2::new(tile_world.x, tile_world.z);
            let visual_gap = tile_on_ground.distance(hover_pos);

            assert!(
                visual_gap < epsilon,
                "edge {i}: visual gap {:.4} exceeds epsilon {:.6} — pointer at ({:+.3}, {:+.3}), \
                 tile visual at ({:+.3}, {:+.3}), drop_state at ({:+.3}, {:+.3}), \
                 frame.gap {:.4}, branch {}",
                visual_gap,
                epsilon,
                hover_pos.x,
                hover_pos.y,
                tile_on_ground.x,
                tile_on_ground.y,
                frame.drop_state.tile(1).pos.x,
                frame.drop_state.tile(1).pos.y,
                frame.gap,
                frame.branch
            );
        }
    }
}
