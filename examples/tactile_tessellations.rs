//! Regular polygons that snap edge to edge, a port of Dragology's `tactile-tessellations` demo.
//!
//! Drag a polygon out of the palette. Near other polygons it snaps flush against an edge of the
//! same length, rotating by the least angle that lines the edges up. Far from every snap it
//! follows the pointer freely, and dropping it on the trash bin deletes it.
//!
//! The tile spec is the one of the original: `closest(snaps + trash bin)`, with `when_far`
//! handing over to `vary(x, y)`, anchored at the center of the tile. Palette items start a drag
//! with `switch_to_state_and_follow`.
//!
//! # Comparison with the original
//!
//! Geometry is ported from `tactile-tessellations.tsx` line by line, with the 50 px side length
//! becoming one world unit and the 2 px overlap tolerance becoming 0.04. The y axis points up
//! here, so the polygons are listed counterclockwise, and `rot_deg` turns counterclockwise.
//!
//! - **Snap candidates:** the same. For every edge of every other tile, the new tile's edge of
//!   equal length that needs the least rotation is aligned flush (reversed direction, matching
//!   midpoints), and candidates that overlap another tile (separating axis test) are dropped.
//!   Every polygon has the same side length, so all kinds snap to each other: a square next to a
//!   square has four snaps, and a triangle next to a square has four as well.
//! - **Rotation while blending:** Dragology interpolates rotation with `lerpDegrees`, the
//!   shortest arc. Quaternion blending here is the shortest arc too, so a tile turning from 350
//!   to 10 degrees passes through 0 in both.
//! - **`when_far`:** the same gap of 30 px (0.6 units) between the free `vary` drag and the
//!   snaps, with the tile center as the anchor of the drag (`with_init_context` in the
//!   original, `anchored_at_center` here, which resets the grab offset).
//! - **Appearing and leaving tiles:** a new tile fades in and a deleted one fades out, which is
//!   Dragology's default for elements that exist in one state only.
//! - **Dragged tile on top:** the original sets `dragologyZIndex` from the dragged id. A view
//!   function here does not know what is dragged, so the spec raises the dragged tile in its
//!   previews with `change_result`.
//! - **New tiles:** the original creates a tile at the origin of the canvas, which the `vary`
//!   branch moves to the pointer at once. Here it starts at its palette item, so the first
//!   frame of the drag does not fly in from a corner.
//! - **Outline:** the SVG stroke is a slightly larger polygon behind the fill, because meshes
//!   have no stroke.
//!
//! Run with `cargo run --example tactile_tessellations --features shapes-2d`.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use avian2d::prelude::{Collider, PhysicsPlugins, RigidBody};
use bevy::prelude::*;
use bevy_composable_dragging::ops::{
    Lens, closest, custom, drop_target, state, substate, switch_to_state_and_follow,
    vary_constrained,
};
use bevy_composable_dragging::{Constraint, DragSpec, DraggablePlugin, View, ViewNode, param};
use bevy_gradmesh::{Shape, ShapeDetail, Style};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

/// The side length of every polygon (50 px in the original).
const SIDE: f32 = 1.0;
/// How far two polygons may overlap and still count as touching (2 px in the original).
const OVERLAP_EPSILON: f32 = 0.04;
/// Edges of this length difference or less are equal (0.1 px in the original).
const EDGE_TOLERANCE: f32 = 0.002;
/// The distance from every snap beyond which a tile follows the pointer freely (30 px).
const GAP: f32 = 0.6;
/// The thickness of the outline of a tile.
const OUTLINE: f32 = 0.04;
/// The number of sides for each shape in the palette.
const KINDS: [u32; 4] = [3, 4, 6, 8];
/// The height of the canvas in world units (450 px in the original).
const CANVAS_HEIGHT: f32 = 9.0;
/// The id of the trash bin node.
const TRASH: &str = "trash-bin";

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

/// The number of sides of a regular polygon tile.
type TileKind = u32;

#[derive(Clone, Debug, PartialEq)]
struct Tile {
    id: u32,
    sides: u32,
    pos: Vec2,
    rot_deg: f32,
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

/// The position and rotation of a single tile, isolated from the rest of the board.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TilePose {
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

    /// The board with the tile `id` posed as `pose`, or without it for `None`.
    fn with_tile(&self, id: u32, pose: Option<TilePose>) -> Self {
        let mut board = self.clone();
        match pose {
            Some(pose) => {
                if let Some(tile) = board.tiles.iter_mut().find(|tile| tile.id == id) {
                    tile.pos = pose.pos;
                    tile.rot_deg = pose.rot_deg;
                }
            }
            None => board.tiles.retain(|tile| tile.id != id),
        }
        board
    }

    /// The board with a new tile of the given `sides` at `pos`.
    fn with_new(&self, sides: u32, pos: Vec2) -> (Self, u32) {
        let mut board = self.clone();
        let id = board.next_id;
        board.next_id += 1;
        board.tiles.push(Tile {
            id,
            sides,
            pos,
            rot_deg: 0.0,
        });
        (board, id)
    }
}

/// Whether a tile with `sides` at `pos` turned by `rot_deg` overlaps any tile but `exclude`.
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

/// Every valid edge-to-edge placement of a tile with `sides` against `tiles`.
///
/// Two edges snap when they have the same length and can be laid flush against each other with
/// reversed direction. The tile turns by the least angle that does so, and snaps whose result
/// overlaps another tile are dropped.
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

/// Makes the center of the node follow the pointer instead of the point that was grabbed, like
/// `withInitContext({ anchorPos: Vec2(0) })` in the original.
fn anchored_at_center(spec: DragSpec<Board>) -> DragSpec<Board> {
    custom(move |context| {
        let offset = context.grab_offset();
        let mut behavior = spec.compile(&context.clone().with_grab_offset(Vec3::ZERO));
        Box::new(move |pointer| behavior(pointer.moved_to(pointer.point + offset)))
    })
}

/// The drag of a tile: its pose snaps edge-to-edge, it is dropped on the bin to delete it, or it
/// follows the pointer freely on the canvas when far from any snap.
fn tile_spec(board: &Board, id: u32) -> DragSpec<Board> {
    let tile = board.tile(id);
    let sides = tile.sides;
    let start = TilePose {
        pos: tile.pos,
        rot_deg: tile.rot_deg,
    };
    let snaps = compute_snaps(sides, &board.tiles, Some(id));
    let other_tiles: Vec<Tile> = board.tiles.iter().filter(|t| t.id != id).cloned().collect();

    let no_overlap = Constraint::new(move |pose: &TilePose| {
        if overlaps_any(sides, pose.pos, pose.rot_deg, &other_tiles, None) {
            100.0
        } else {
            0.0
        }
    });
    let free = vary_constrained(
        start,
        [
            param!(TilePose, pos.x),
            param!(TilePose, pos.y),
            param!(TilePose, rot_deg).with_scale(360.0),
        ],
        no_overlap,
    );
    let pose = Lens::new(
        move |tile: &Option<TilePose>| tile.unwrap_or(start),
        |_, pose| Some(pose),
    );
    let branches = snaps
        .into_iter()
        .map(|snap| state(Some(TilePose { pos: snap.pos, rot_deg: snap.rot_deg })))
        .chain([drop_target(TRASH, None)])
        .collect::<Vec<_>>();
    let tile = Lens::new(
        move |board: &Board| Some(TilePose { pos: board.tile(id).pos, rot_deg: board.tile(id).rot_deg }),
        move |board: &Board, pose| board.with_tile(id, pose),
    );
    let node = tile_id(id);
    anchored_at_center(substate(
        tile,
        closest(branches).when_far(substate(pose, free), GAP),
    ))
    .change_result(move |mut frame| {
        if let Some(dragged) = frame.view.get_mut(&node) {
            dragged.transform.translation.z = TILE_Z + 1.0;
        }
        frame
    })
}

// ---------------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------------

const CANVAS_Z: f32 = -1.0;
const PALETTE_Z: f32 = -0.5;
const PALETTE_ITEM_Z: f32 = 0.0;
const TILE_Z: f32 = 0.2;

fn tile_id(id: u32) -> String {
    format!("shape-{id}")
}

fn fill(sides: u32) -> Color {
    match sides {
        3 => Color::srgb_u8(0xfc, 0xa5, 0xa5),
        4 => Color::srgb_u8(0x93, 0xc5, 0xfd),
        6 => Color::srgb_u8(0x86, 0xef, 0xac),
        8 => Color::srgb_u8(0xfd, 0xe6, 0x8a),
        _ => Color::srgb_u8(0xcc, 0xcc, 0xcc),
    }
}

fn stroke(sides: u32) -> Color {
    match sides {
        3 => Color::srgb_u8(0xef, 0x44, 0x44),
        4 => Color::srgb_u8(0x3b, 0x82, 0xf6),
        6 => Color::srgb_u8(0x16, 0xa3, 0x4a),
        8 => Color::srgb_u8(0xca, 0x8a, 0x04),
        _ => Color::srgb_u8(0x88, 0x88, 0x88),
    }
}

/// The gradmesh shape for a regular polygon.
fn tile_shape(sides: u32) -> Shape {
    Shape::arbitrary_polygon(vertices(sides), ShapeDetail::Medium)
}

/// A tile shape with its fill color.
fn tile_node<S>(
    id: impl Into<bevy_composable_dragging::NodeId>,
    sides: u32,
    color: Color,
) -> ViewNode<S> {
    ViewNode::grad(id, tile_shape(sides))
        .with_style(Style::shaded(color, 1.0))
}

/// A tile shape with its outline: the outline is a larger shape of the stroke color behind it.
fn outlined(id: &str, sides: u32, opacity: f32) -> [ViewNode<Board>; 2] {
    let apothem = SIDE / (2.0 * (PI / sides as f32).tan());
    let outline_scale = 1.0 + OUTLINE / apothem;
    [
        tile_node(id.to_owned(), sides, fill(sides)).with_opacity(opacity),
        tile_node(format!("{id}/outline"), sides, stroke(sides))
            .with_parent(id.to_owned())
            .with_opacity(opacity)
            .with_z(-0.01)
            .scaled_xy(Vec2::splat(outline_scale)),
    ]
}

/// The center of the palette item for a shape: the bottoms of all items rest on one line.
fn palette_position(sides: u32) -> Vec2 {
    const BASELINE: f32 = 1.8;
    let x = match sides {
        3 => -5.0,
        4 => -2.5,
        6 => 0.0,
        8 => 2.5,
        _ => 0.0,
    };
    let lowest = vertices(sides)
        .iter()
        .map(|vertex| vertex.y)
        .fold(f32::MAX, f32::min);
    Vec2::new(x, BASELINE - lowest)
}

fn view(board: &Board) -> View<Board> {
    let mut view = View::new();
    view.push(
        ViewNode::rectangle("canvas", Vec2::new(12.0, CANVAS_HEIGHT))
            .with_color(Color::WHITE)
            .with_z(CANVAS_Z),
    );
    view.push(
        ViewNode::rectangle("palette", Vec2::new(9.6, 3.0))
            .at_xy(Vec2::new(-1.2, 3.0))
            .with_color(Color::srgb_u8(0xf9, 0xfa, 0xfb))
            .with_z(PALETTE_Z),
    );
    view.push(
        ViewNode::rectangle(TRASH, Vec2::splat(1.0))
            .at_xy(Vec2::new(5.3, 3.5))
            .with_color(Color::srgb_u8(0xff, 0xee, 0xee))
            .with_z(PALETTE_Z),
    );
    view.push(
        ViewNode::rectangle("trash-bin/lid", Vec2::new(0.5, 0.08))
            .with_parent(TRASH)
            .at_xy(Vec2::new(0.0, 0.26))
            .with_color(Color::srgb_u8(0x99, 0x99, 0x99))
            .with_z(0.1),
    );
    view.push(
        ViewNode::rectangle("trash-bin/body", Vec2::new(0.4, 0.5))
            .with_parent(TRASH)
            .at_xy(Vec2::new(0.0, -0.05))
            .with_color(Color::srgb_u8(0x99, 0x99, 0x99))
            .with_z(0.1),
    );

    for sides in KINDS {
        let (created, id) = board.with_new(sides, palette_position(sides));
        let [item, outline] = outlined(&format!("palette-{sides}"), sides, 0.8);
        view.push(
            item.at_xy(palette_position(sides))
                .with_z(PALETTE_ITEM_Z)
                .draggable(move || switch_to_state_and_follow(created.clone(), tile_id(id))),
        );
        view.push(outline);
    }

    for tile in &board.tiles {
        let id = tile.id;
        let sides = tile.sides;
        let snapshot = board.clone();
        let [shape, outline] = outlined(&tile_id(id), sides, 1.0);
        view.push(
            shape.at_xy(tile.pos)
                .with_z(TILE_Z)
                .with_rotation_xy(tile.rot_deg.to_radians())
                .draggable(move || tile_spec(&snapshot, id)),
        );
        view.push(outline);
    }
    view
}

/// Adds colliders to every tile entity so avian can detect overlap.
fn spawn_tile_colliders(mut commands: Commands, tiles: Query<(Entity, &Name), Without<RigidBody>>) {
    for (entity, name) in &tiles {
        if name.as_str().starts_with("shape-") {
            commands
                .entity(entity)
                .insert((RigidBody::Static, Collider::rectangle(SIDE, SIDE)));
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
        pos: Vec2::new(-1.0, -1.2),
        rot_deg: 0.0,
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
            },
        ],
        next_id: 2,
    };
    let target = edges(&world_vertices(6, hexagon.pos, 0.0))[5].mid() + Vec2::new(-0.3, 0.2);
    app.add_plugins(common::default_plugins("tactile_tessellations"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(board, view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(palette_position(4), target))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations.gif")
                .with_title("tactile_tessellations")
                .with_description("snapping, rotation, trash bin")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, CANVAS_HEIGHT);
        });
}

fn main() {
    // A hexagon with a triangle and a square snapped to it, and a square waiting in the palette
    // to be dragged against the other side.
    let hexagon = Tile {
        id: 0,
        sides: 6,
        pos: Vec2::new(-1.0, -1.2),
        rot_deg: 0.0,
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
            },
        ],
        next_id: 2,
    };
    let target = edges(&world_vertices(6, hexagon.pos, 0.0))[5].mid() + Vec2::new(-0.3, 0.2);
    App::new()
        .add_plugins(common::default_plugins("tactile_tessellations"))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(DraggablePlugin::new(board, view))
        .add_systems(Startup, spawn_tile_colliders)
        .add_plugins(ScriptedDragPlugin::straight(palette_position(4), target))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tactile_tessellations.gif")
                .with_title("tactile_tessellations")
                .with_description("snapping, rotation, trash bin")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, CANVAS_HEIGHT);
        })
        .run();
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, Pointer};

    use super::*;

    fn tile(id: u32, sides: u32, pos: Vec2, rot_deg: f32) -> Tile {
        Tile {
            id,
            sides,
            pos,
            rot_deg,
        }
    }

    fn drag(board: &Board, id: u32, pointer: Vec2) -> bevy_composable_dragging::Frame<Board> {
        let context = DragContext::new(view, tile_id(id).into(), board.clone());
        tile_spec(board, id).compile(&context)(Pointer::at(pointer.extend(0.0)))
    }

    #[test]
    fn regular_polygons_have_unit_sides_and_a_flat_bottom() {
        for sides in KINDS {
            let corners = vertices(sides);
            assert_eq!(corners.len(), sides as usize);
            for edge in edges(&corners) {
                assert!((edge.length() - SIDE).abs() < 1e-4, "{}: {edge:?}", sides);
            }
            let bottom = corners.iter().map(|v| v.y).fold(f32::MAX, f32::min);
            let flat = corners
                .iter()
                .filter(|v| (v.y - bottom).abs() < 1e-4)
                .count();
            assert_eq!(flat, 2, "{}: the bottom is one edge", sides);
            let area: f32 = edges(&corners).iter().map(|e| e.a.perp_dot(e.b)).sum();
            assert!(area > 0.0, "{}: counterclockwise", sides);
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
    fn the_overlap_tolerance_allows_a_sliver() {
        let square = world_vertices(4, Vec2::ZERO, 0.0);
        let sliver = world_vertices(4, Vec2::new(SIDE - 0.03, 0.0), 0.0);
        assert!(!polygons_overlap(&square, &sliver, OVERLAP_EPSILON));
        let deep = world_vertices(4, Vec2::new(SIDE - 0.1, 0.0), 0.0);
        assert!(polygons_overlap(&square, &deep, OVERLAP_EPSILON));
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
        // All sides have the same length, so a triangle snaps to a square.
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
    fn snapped_tiles_share_an_edge_midpoint_with_reversed_direction() {
        let existing = tile(0, 6, Vec2::new(1.0, 2.0), 15.0);
        for snap in compute_snaps(3, std::slice::from_ref(&existing), None) {
            let placed = edges(&world_vertices(3, snap.pos, snap.rot_deg));
            let other = edges(&world_vertices(6, existing.pos, existing.rot_deg));
            let shared = placed.iter().any(|a| {
                other
                    .iter()
                    .any(|b| a.mid().distance(b.mid()) < 1e-3 && a.dir().dot(b.dir()) < -0.999)
            });
            assert!(shared, "{snap:?}");
        }
    }

    #[test]
    fn snap_rotations_are_normalized_and_the_least_rotation_is_used() {
        let existing = tile(0, 4, Vec2::ZERO, 0.0);
        for snap in compute_snaps(4, std::slice::from_ref(&existing), None) {
            assert!((0.0..360.0).contains(&snap.rot_deg), "{snap:?}");
            let signed = if snap.rot_deg > 180.0 {
                snap.rot_deg - 360.0
            } else {
                snap.rot_deg
            };
            assert!(signed.abs() <= 180.0);
        }
        // Next to a square turned by 40 degrees, the four edges ask for 40, 130, -140 and -50
        // degrees, and the least turn wins.
        let turned = tile(0, 4, Vec2::ZERO, 40.0);
        let signed: Vec<f32> = compute_snaps(4, std::slice::from_ref(&turned), None)
            .iter()
            .map(|snap| normalize_angle(snap.rot_deg.to_radians()).to_degrees())
            .collect();
        assert!(
            signed.iter().all(|degrees| (degrees - 40.0).abs() < 1e-2),
            "{signed:?}"
        );
    }

    #[test]
    fn the_dragged_tile_is_excluded_from_its_own_snaps() {
        let tiles = [
            tile(0, 4, Vec2::ZERO, 0.0),
            tile(1, 4, Vec2::new(5.0, 0.0), 0.0),
        ];
        let with_self = compute_snaps(4, &tiles, None).len();
        let without_self = compute_snaps(4, &tiles, Some(1)).len();
        assert!(without_self < with_self);
        assert_eq!(without_self, 4);
    }

    #[test]
    fn snaps_that_overlap_a_third_tile_are_dropped() {
        let tiles = [
            tile(0, 4, Vec2::ZERO, 0.0),
            tile(1, 4, Vec2::new(SIDE, 0.0), 0.0),
        ];
        let snaps = compute_snaps(4, &tiles, None);
        for snap in &snaps {
            assert!(!overlaps_any(4, snap.pos, snap.rot_deg, &tiles, None));
        }
        assert_eq!(snaps.len(), 6);
    }

    #[test]
    fn angles_normalize_into_a_half_open_interval() {
        assert!((normalize_angle(3.0 * PI / 2.0) + PI / 2.0).abs() < 1e-5);
        assert!((normalize_angle(-PI) + PI).abs() < 1e-5);
        assert!(normalize_angle(PI) < 0.0 || (normalize_angle(PI) + PI).abs() < 1e-5);
    }

    #[test]
    fn palette_items_rest_on_one_line() {
        for sides in KINDS {
            let bottom = world_vertices(sides, palette_position(sides), 0.0)
                .iter()
                .map(|v| v.y)
                .fold(f32::MAX, f32::min);
            assert!((bottom - 1.8).abs() < 1e-4);
        }
    }

    #[test]
    fn dropping_on_the_bin_deletes_the_tile() {
        let board = Board {
            tiles: vec![tile(0, 4, Vec2::new(5.3, 3.5), 0.0)],
            next_id: 1,
        };
        assert_eq!(board.with_tile(0, None).tiles.len(), 0);
        assert_eq!(board.with_new(3, Vec2::ZERO).1, 1);
    }

    #[test]
    fn the_other_tiles_stay_draggable_while_one_is_dragged() {
        let board = Board {
            tiles: vec![
                tile(0, 4, Vec2::ZERO, 0.0),
                tile(1, 3, Vec2::new(3.0, 0.0), 0.0),
            ],
            next_id: 2,
        };
        let frame = drag(&board, 0, Vec2::new(-4.0, -4.0));
        assert!(frame.view.get(&tile_id(0)).unwrap().drag.is_some());
        assert!(frame.view.get(&tile_id(1)).unwrap().drag.is_some());
    }

    #[test]
    fn dragging_a_tile_changes_only_its_own_pose() {
        let board = Board {
            tiles: vec![
                tile(0, 4, Vec2::ZERO, 0.0),
                tile(1, 3, Vec2::new(3.0, 0.0), 0.0),
            ],
            next_id: 2,
        };
        let far = Vec2::new(-4.0, -4.0);
        let frame = drag(&board, 1, far);
        assert_eq!(frame.drop_state.tiles.len(), 2);
        assert_eq!(frame.drop_state.tiles[0], board.tiles[0]);
        assert_eq!(frame.drop_state.tiles[1].id, board.tiles[1].id);
        assert_eq!(frame.drop_state.tiles[1].sides, board.tiles[1].sides);
        assert_ne!(frame.drop_state.tiles[1].pos, board.tiles[1].pos);
    }

    #[test]
    fn dropping_a_tile_on_the_bin_during_drag_deletes_it() {
        let board = Board {
            tiles: vec![
                tile(0, 4, Vec2::ZERO, 0.0),
                tile(1, 3, Vec2::new(3.0, 0.0), 0.0),
            ],
            next_id: 2,
        };
        let frame = drag(&board, 1, Vec2::new(5.3, 3.5));
        assert_eq!(frame.drop_state.tiles, vec![board.tiles[0].clone()]);
    }
}
