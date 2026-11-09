//! A gridless cozy castle builder, drawn with gradient meshes from `bevy_gradmesh`.
//!
//! The ground is Bevy's Y-up XZ plane and there are no cells and no sockets. Every piece is an
//! engine node: a draggable root (its main body) with plain child nodes for roofs, posts and
//! doors, and its pose is a free `Transform`. A piece is dragged in the ground `DragSpace`, so
//! the engine's `closest`, `when_far`, `with_floating` and `with_snap_radius` work as they do
//! everywhere else:
//!
//! - Near the outline of a neighbour the piece settles into a pose computed from the two
//!   outlines (an edge laid flush against an edge, end to end, or centered), turning by the
//!   least angle. The poses come from geometry, never from authored sockets, so any footprint
//!   aligns.
//! - Far from every such pose it follows the pointer freely (`across_plane`), a little lifted.
//! - A piece rests on the highest piece under its footprint, so a wall can stand on a wall.
//!
//! Torches are real lights: a flickering point light follows the flame of every torch.
//!
//! Run with `cargo run --example cozy_castle --features shapes-3d`.

#![expect(
    clippy::disallowed_types,
    reason = "the views and the drags of the library are shared through `Arc`; the guidelines ban shared ownership, and this expectation warns once the last `Arc` of the example is gone"
)]

use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy_composable_dragging::gizmo::{Axis, GIZMO_LAYER};
use bevy_composable_dragging::hud::{HudPlugin, Layout, Screen, Theme};
use bevy_composable_dragging::ops::{across_plane, closest_states, custom};
use bevy_composable_dragging::radial::{RadialHost, RadialMenuPlugin, push_menu};
use bevy_composable_dragging::{DragModel, DragSpec, DraggablePlugin, Pointer, View, ViewNode};

#[path = "../common/mod.rs"]
mod common;
use common::screenshot::{AutoScreenshotPlugin, ScreenshotState};
mod curve;
#[cfg(test)]
mod hud_clicks;
mod kit;
mod menu;
mod select;
mod solid;
mod stack;
mod terrain;
mod wall;

use common::{HUD_LAYER, SceneCamera, spawn_hud_camera};
use kit::{Fabric, build_parts, door};
use menu::{Precision, Ring, Tool, WallStyle, scene_space, scene_spec};
use select::{piece_gizmo_size, selection_gizmo};
use terrain::{Terrain, push_terrain};
use wall::{Wall, push_wall};

/// How many world units the HUD camera shows vertically.
const HUD_HEIGHT: f32 = 720.0;
/// The aspect ratio of the window the castle starts in.
const ASPECT: f32 = 16.0 / 9.0;
/// The terrain is this many chunks across and deep, and its corner is here.
const TERRAIN_CHUNKS: (usize, usize) = (7, 6);
const TERRAIN_ORIGIN: Vec2 = Vec2::new(-5.6, -4.4);
/// The preview settles into an alignment once the piece is this close to it.
const SNAP: f32 = 0.35;
/// Beyond this distance from every alignment the piece follows the pointer freely.
const FAR: f32 = 0.7;
/// Pieces whose center is further than this from the pointer give no alignments.
const REACH: f32 = 2.2;
/// How many alignments, the nearest ones, a drag evaluates for each pointer position.
const MAX_ALIGNMENTS: usize = 8;
/// Footprints that overlap by less than this stand beside each other instead of stacking.
const OVERLAP_EPSILON: f32 = 0.02;
/// How far a piece that is carried freely is lifted above the ground.
const CARRY_LIFT: f32 = 0.35;
/// The height of the flame of a torch above its base.
const FLAME_HEIGHT: f32 = 0.7;

// ---------------------------------------------------------------------------------------------
// Geometry on the ground
// ---------------------------------------------------------------------------------------------

/// Wraps an angle to `-PI..PI`.
fn normalize_angle(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// A pose on the ground: a position in the `(x, z)` plane and the angle that turns an outline
/// in that plane (a piece with `Quat::from_rotation_y(yaw)` has the angle `-yaw`).
#[derive(Clone, Copy, Debug)]
struct Pose {
    position: Vec2,
    angle: f32,
}

impl Pose {
    /// Whether two poses are the same for all practical purposes.
    fn is_near(self, other: Self) -> bool {
        self.position.distance(other.position) < 0.02
            && normalize_angle(self.angle - other.angle).abs() < 0.02
    }
}

/// An outline carried to `pose`.
fn to_world(outline: &[Vec2], pose: Pose) -> Vec<Vec2> {
    let turn = Vec2::from_angle(pose.angle);
    outline
        .iter()
        .map(|point| pose.position + turn.rotate(*point))
        .collect()
}

/// A side of an outline, from `a` to `b`.
#[derive(Clone, Copy, Debug)]
struct Edge {
    a: Vec2,
    b: Vec2,
}

impl Edge {
    fn direction(self) -> Vec2 {
        self.b - self.a
    }

    fn mid(self) -> Vec2 {
        self.a.midpoint(self.b)
    }
}

/// The sides of a closed outline.
fn edges(outline: &[Vec2]) -> Vec<Edge> {
    outline
        .iter()
        .zip(outline.iter().cycle().skip(1))
        .map(|(a, b)| Edge { a: *a, b: *b })
        .collect()
}

/// Whether two convex outlines overlap by more than `epsilon`, by the separating axis test.
fn polygons_overlap(a: &[Vec2], b: &[Vec2], epsilon: f32) -> bool {
    let axes = edges(a)
        .into_iter()
        .chain(edges(b))
        .map(|edge| edge.direction().perp().normalize_or_zero())
        .filter(|axis| *axis != Vec2::ZERO);
    let project = |outline: &[Vec2], axis: Vec2| {
        outline
            .iter()
            .map(|point| point.dot(axis))
            .fold((f32::MAX, f32::MIN), |(low, high), value| {
                (low.min(value), high.max(value))
            })
    };
    let mut axes = axes;
    !axes.any(|axis| {
        let (a_low, a_high) = project(a, axis);
        let (b_low, b_high) = project(b, axis);
        a_high.min(b_high) - a_low.max(b_low) <= epsilon
    })
}

/// The outline with a counterclockwise winding.
fn counterclockwise(mut outline: Vec<Vec2>) -> Vec<Vec2> {
    let area: f32 = edges(&outline).iter().map(|e| e.a.perp_dot(e.b)).sum();
    if area < 0.0 {
        outline.reverse();
    }
    outline
}

fn rectangle_outline(width: f32, depth: f32) -> Vec<Vec2> {
    let (w, d) = (width / 2.0, depth / 2.0);
    vec![
        Vec2::new(-w, -d),
        Vec2::new(w, -d),
        Vec2::new(w, d),
        Vec2::new(-w, d),
    ]
}

fn round_outline(radius: f32, sides: u32) -> Vec<Vec2> {
    (0..sides)
        .map(|side| Vec2::from_angle(TAU * side as f32 / sides as f32) * radius)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum PieceKind {
    Tower,
    Keep,
    ArchGate,
    SquareGate,
    WoodGate,
    Cobble,
    Outpost,
    Torch,
    Banner,
    Cottage,
}

impl PieceKind {
    /// The outline of the piece on the ground, counterclockwise around its center.
    fn footprint(self) -> Vec<Vec2> {
        match self {
            Self::ArchGate | Self::SquareGate => rectangle_outline(1.0, 0.3),
            Self::WoodGate => rectangle_outline(1.0, 0.2),
            Self::Tower => round_outline(0.4, 16),
            Self::Keep => rectangle_outline(0.9, 0.9),
            Self::Cobble => rectangle_outline(1.0, 1.0),
            Self::Outpost => rectangle_outline(0.6, 0.6),
            Self::Torch => rectangle_outline(0.14, 0.14),
            Self::Banner => rectangle_outline(0.3, 0.3),
            Self::Cottage => rectangle_outline(0.72, 0.56),
        }
    }

    /// How far the piece reaches above its base, which is what other pieces stack on. Paths lie
    /// on the ground and things stand beside them.
    fn top(self) -> f32 {
        match self {
            Self::ArchGate | Self::SquareGate | Self::Torch => 0.82,
            Self::Tower => 1.76,
            Self::Keep | Self::Outpost => 1.5,
            Self::WoodGate => 1.04,
            Self::Cobble => 0.0,
            Self::Banner => 1.1,
            Self::Cottage => 0.86,
        }
    }
}

#[derive(Clone, Debug)]
struct Placed {
    id: u32,
    kind: PieceKind,
    transform: Transform,
    /// How far the piece floats above what it rests on.
    lift: f32,
    /// The side of a tower or a keep that has a door, as an angle from its front.
    door: Option<f32>,
}

impl Placed {
    fn pose(&self) -> Pose {
        let yaw = self.transform.rotation.to_euler(EulerRot::YXZ).0;
        Pose {
            position: Vec2::new(self.transform.translation.x, self.transform.translation.z),
            angle: -yaw,
        }
    }

    /// The outline of the piece on the ground.
    fn outline(&self) -> Vec<Vec2> {
        to_world(&self.kind.footprint(), self.pose())
    }
}

/// A wall drawn along a curve.
#[derive(Clone, Debug)]
struct WallPiece {
    id: u32,
    wall: Wall,
}

#[derive(Clone)]
struct Castle {
    placed: Vec<Placed>,
    walls: Vec<WallPiece>,
    /// The piece or wall whose gizmo is shown.
    selected: Option<u32>,
    terrain: Terrain,
    /// The ring of the radial menu that is open.
    menu: Option<Ring>,
    /// The piece the next drag across the ground places.
    armed: Option<PieceKind>,
    tool: Tool,
    precision: Precision,
    /// What the next wall that is drawn is made of.
    wall_style: WallStyle,
    next_id: u32,
    /// The size of the HUD and the projection of the scene camera, which the HUD lays itself
    /// out from.
    screen: Screen,
}

impl Castle {
    fn empty() -> Self {
        Self {
            placed: Vec::new(),
            walls: Vec::new(),
            selected: None,
            terrain: Terrain::flat(TERRAIN_CHUNKS.0, TERRAIN_CHUNKS.1, TERRAIN_ORIGIN),
            menu: None,
            armed: None,
            tool: Tool::Select,
            precision: Precision::Fine,
            wall_style: WallStyle::Stone,
            next_id: 0,
            screen: Screen::for_camera(HUD_HEIGHT, scene_camera(), ASPECT),
        }
    }

    fn piece(&self, id: u32) -> Option<&Placed> {
        self.placed.iter().find(|piece| piece.id == id)
    }

    /// The castle with `wall` added.
    fn with_wall(&self, wall: Wall) -> Self {
        let mut castle = self.clone();
        castle.walls.push(WallPiece {
            id: castle.next_id,
            wall,
        });
        castle.next_id += 1;
        castle
    }

    /// The castle after a freehand `stroke`: the wall it draws is added and selected, and if the
    /// stroke is too short to be a wall nothing is selected.
    fn with_stroke(&self, stroke: &[Vec3]) -> Self {
        Wall::from_stroke(stroke).map_or_else(
            || self.with_selected(None),
            |wall| {
                let castle = self.with_wall(wall.with_profile(self.wall_style.profile()));
                let id = castle.next_id - 1;
                castle.with_selected(Some(id))
            },
        )
    }

    /// The castle with wall `id` replaced by `edit` of it.
    fn with_wall_edited(&self, id: u32, edit: impl FnOnce(&Wall) -> Wall) -> Self {
        let mut castle = self.clone();
        if let Some(piece) = castle.walls.iter_mut().find(|piece| piece.id == id) {
            piece.wall = edit(&piece.wall);
        }
        castle
    }

    /// The top of the highest piece under the footprint of piece `id` at `pose`, or the ground.
    fn rest_height(&self, id: u32, pose: Pose) -> f32 {
        let Some(piece) = self.piece(id) else {
            return 0.0;
        };
        let outline = to_world(&piece.kind.footprint(), pose);
        let ground = self
            .terrain
            .highest_under(&[outline.as_slice(), &[pose.position]].concat())
            .max(
                self.wall_top_under(&outline, pose.position)
                    .unwrap_or(f32::MIN),
            );
        self.placed
            .iter()
            .filter(|other| other.id != id)
            .filter(|other| polygons_overlap(&outline, &other.outline(), OVERLAP_EPSILON))
            .map(|other| other.transform.translation.y + other.kind.top())
            .fold(ground, f32::max)
    }

    /// The castle with piece `id` standing at `pose`, resting on whatever is under it.
    fn posed(&self, id: u32, pose: Pose) -> Self {
        let rest = self.rest_height(id, pose);
        let mut castle = self.clone();
        if let Some(piece) = castle.placed.iter_mut().find(|piece| piece.id == id) {
            piece.transform =
                Transform::from_xyz(pose.position.x, rest + piece.lift, pose.position.y)
                    .with_rotation(Quat::from_rotation_y(-pose.angle));
        }
        castle
    }

    fn add(&mut self, kind: PieceKind, x: f32, z: f32, angle: f32) {
        let id = self.next_id;
        self.next_id += 1;
        self.placed.push(Placed {
            id,
            kind,
            transform: Transform::IDENTITY,
            lift: 0.0,
            door: None,
        });
        *self = self.posed(
            id,
            Pose {
                position: Vec2::new(x, z),
                angle,
            },
        );
    }

    /// The castle the example starts with: bare ground, to build on from nothing.
    /// When `DSD_SCREENSHOT` is set, loads the courtyard instead so the screenshot shows
    /// a built castle.
    fn initial() -> Self {
        if std::env::var("DSD_SCREENSHOT").is_ok() {
            Self::courtyard()
        } else {
            Self::empty()
        }
    }

    /// A courtyard with four towers, walls and gates, a keep, a cottage and a cobbled path with
    /// torches, for the tests that need a castle with many kinds of pieces in it.
    fn courtyard() -> Self {
        use PieceKind::{
            ArchGate, Banner, Cobble, Cottage, Keep, Outpost, SquareGate, Torch, Tower, WoodGate,
        };
        use std::f32::consts::FRAC_PI_2;

        let mut castle = Self::empty();
        for z in [0.0, 1.0, 2.0, 3.0] {
            castle.add(Cobble, 0.0, z, 0.0);
        }
        for (x, z) in [(-2.9, -2.0), (2.9, -2.0), (-2.9, 2.0), (2.9, 2.0)] {
            castle.add(Tower, x, z, 0.0);
        }
        castle.add(ArchGate, 0.0, 2.0, 0.0);
        for (from, to) in [
            ((-2.5, 2.0), (-0.5, 2.0)),
            ((0.5, 2.0), (2.5, 2.0)),
            ((-2.9, -1.6), (-2.9, -0.5)),
            ((-2.9, 0.5), (-2.9, 1.6)),
            ((2.9, -1.6), (2.9, -0.5)),
            ((2.9, 0.5), (2.9, 1.6)),
        ] {
            castle = castle.with_wall(Wall::straight(from, to));
        }
        castle = castle.with_wall(Wall::through(&[
            Vec3::new(-2.5, 0.0, -2.0),
            Vec3::new(-1.0, 0.0, -2.3),
            Vec3::new(0.9, 0.35, -1.8),
            Vec3::new(2.5, 0.0, -2.0),
        ]));
        castle.add(SquareGate, -2.9, 0.0, FRAC_PI_2);
        castle.add(WoodGate, 2.9, 0.0, FRAC_PI_2);
        castle.add(Keep, 0.9, -1.0, 0.0);
        castle.add(Cottage, -1.6, -0.9, 0.0);
        castle.add(Banner, -0.9, 0.9, 0.0);
        castle.add(Outpost, 3.9, 3.0, 0.0);
        for (x, z) in [(-0.75, 2.7), (0.75, 2.7), (-0.7, 0.4), (0.7, 0.4)] {
            castle.add(Torch, x, z, 0.0);
        }
        castle
    }
}

// ---------------------------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------------------------

/// The outlines that piece `id` can be aligned against near `point`: the other pieces, and the
/// end faces of the walls.
fn neighbour_outlines(castle: &Castle, id: u32, point: Vec2) -> Vec<Vec<Vec2>> {
    let near = |outline: &Vec<Vec2>| {
        let center = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
        center.distance(point) <= REACH
    };
    castle
        .placed
        .iter()
        .filter(|other| other.id != id)
        .map(Placed::outline)
        .chain(castle.walls.iter().flat_map(|piece| piece.wall.end_caps()))
        .filter(near)
        .collect()
}

/// The poses at which piece `id` lies against the outlines of the pieces near `point`, the
/// nearest to `point` first.
///
/// For every side of a neighbour, the side of the piece that needs the least turn is laid flush
/// against it, facing the other way, in three places: centered, and with either end meeting the
/// end of the neighbour's side. Outlines of any shape give alignments, with no sockets.
fn alignments(castle: &Castle, id: u32, point: Vec2) -> Vec<Pose> {
    let Some(piece) = castle.piece(id) else {
        return Vec::new();
    };
    let own = edges(&piece.kind.footprint());
    let current = piece.pose().angle;
    let mut poses = Vec::new();
    for outline in neighbour_outlines(castle, id, point) {
        for edge in edges(&outline) {
            let facing = edge.direction().to_angle() + PI;
            let turn =
                |side: &Edge| normalize_angle(facing - side.direction().to_angle() - current);
            let Some(best) = own
                .iter()
                .min_by(|a, b| turn(a).abs().total_cmp(&turn(b).abs()))
            else {
                continue;
            };
            let angle = current + turn(best);
            let rotation = Vec2::from_angle(angle);
            for (anchor, own_anchor) in
                [(edge.mid(), best.mid()), (edge.b, best.a), (edge.a, best.b)]
            {
                poses.push(Pose {
                    position: anchor - rotation.rotate(own_anchor),
                    angle,
                });
            }
        }
    }
    poses.sort_by(|a, b| {
        a.position
            .distance(point)
            .total_cmp(&b.position.distance(point))
    });
    let mut distinct: Vec<Pose> = Vec::new();
    for pose in poses {
        if distinct.len() == MAX_ALIGNMENTS {
            break;
        }
        if !distinct.iter().any(|kept| kept.is_near(pose)) {
            distinct.push(pose);
        }
    }
    distinct
}

/// The castles piece `id` can settle into near `point`: where it stands, and every alignment.
fn candidates(castle: &Castle, id: u32, point: Vec2) -> Vec<Castle> {
    std::iter::once(castle.clone())
        .chain(
            alignments(castle, id, point)
                .into_iter()
                .map(|pose| castle.posed(id, pose)),
        )
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Dragging
// ---------------------------------------------------------------------------------------------

fn piece_id(id: u32) -> String {
    format!("piece-{id}")
}

/// Picks among the alignments near the pointer. They depend on where the pointer is, so they
/// are worked out for every pointer position and only the nearest few are evaluated.
fn aligned(castle: &Castle, id: u32) -> DragSpec<Castle> {
    let castle = castle.clone();
    custom(move |context| {
        let context = context.clone();
        let castle = castle.clone();
        Box::new(move |pointer: Pointer| {
            let point = Vec2::new(pointer.point.x, pointer.point.z);
            closest_states(candidates(&castle, id, point)).compile(&context)(pointer)
        })
    })
}

/// Carries the piece across the ground, a little lifted so it is seen to float.
fn carried(id: u32) -> DragSpec<Castle> {
    let node = piece_id(id);
    DragSpec::from(across_plane(move |castle: &Castle, offset: Vec3| {
        castle.moved(id, offset)
    }))
    .change_result(move |mut frame| {
        if let Some(dragged) = frame.view.get_mut(&node) {
            dragged.transform.translation.y += CARRY_LIFT;
        }
        frame
    })
}

/// The drag of a placed piece: settle against a neighbour when close, carry it freely when far.
/// Releasing selects the piece, so a press without any movement selects it too.
fn move_spec(castle: &Castle, id: u32) -> DragSpec<Castle> {
    aligned(castle, id)
        .with_floating()
        .with_snap_radius(SNAP)
        .when_far(carried(id), FAR)
        .change_result(move |mut frame| {
            frame.drop_state = frame.drop_state.with_selected(Some(id)).gate_dropped(id);
            frame
        })
}

// ---------------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------------

/// The id of the node of the flame of the torch piece `root`.
fn flame_id(root: &str) -> String {
    format!("{root}/flame")
}

/// Adds the nodes of a piece to `view`: the draggable root, and its parts as children. A
/// `selected` piece also gets its gizmo once a tool that transforms it is chosen.
fn push_piece(view: &mut View<Castle>, placed: &Placed, snapshot: &Arc<Castle>, selected: bool) {
    let variant = snapshot.variant(placed.id);
    let mut parts = build_parts(placed.kind, variant).into_iter();
    let body = parts.next().expect("every piece has a body");
    let lift = body.transform.translation.y;
    let root = piece_id(placed.id);
    let id = placed.id;
    let precision = snapshot.precision;
    let revealing = Arc::clone(snapshot);
    let snapshot = Arc::clone(snapshot);
    let mut transform = placed.transform;
    transform.translation.y += lift;
    let mut nodes = vec![
        ViewNode::grad(root.clone(), body.shape)
            .with_style(body.fabric.style())
            .with_transform(transform)
            .draggable_in(
                scene_space(snapshot.tool, transform.translation),
                move || scene_spec(&snapshot, id),
            ),
    ];
    for (index, part) in parts.enumerate() {
        let mut local = part.transform;
        local.translation.y -= lift;
        let name = if part.fabric == Fabric::Flame {
            flame_id(&root)
        } else {
            format!("{root}/{index}")
        };
        nodes.push(
            ViewNode::grad(name, part.shape)
                .with_style(part.fabric.style())
                .with_transform(local)
                .with_parent(root.clone())
                .casts_shadow(part.fabric != Fabric::Flame),
        );
    }
    if let Some(door) = placed
        .door
        .and_then(|angle| door(placed.kind, variant, angle))
    {
        nodes.push(
            ViewNode::grad(format!("{root}/door"), door.shape)
                .with_style(door.fabric.style())
                .with_transform(door.transform)
                .with_parent(root.clone())
                .casts_shadow(false),
        );
    }
    for node in nodes {
        view.push(node);
    }
    if selected {
        selection_gizmo(
            id,
            transform,
            precision,
            piece_gizmo_size(placed.kind),
            placed.kind.top(),
            revealing.tool,
        )
        .rotate_axes(&[Axis::Y])
        .add_if_revealed(view, &revealing);
    }
}

fn view(castle: &Castle) -> View<Castle> {
    let snapshot = Arc::new(castle.clone());
    let mut view = View::new();
    push_terrain(&mut view, &snapshot);
    for piece in &castle.walls {
        push_wall(
            &mut view,
            piece,
            castle.selected == Some(piece.id),
            &snapshot,
        );
    }
    for placed in &castle.placed {
        push_piece(
            &mut view,
            placed,
            &snapshot,
            castle.selected == Some(placed.id),
        );
    }
    push_menu(&mut view, &snapshot, &Layout::new(&castle.screen), &theme());
    view
}

// ---------------------------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------------------------

/// Marks the point light of a torch.
#[derive(Component)]
struct TorchLight;

/// The light entity of every torch flame node.
#[derive(Resource, Default)]
struct TorchLights(HashMap<String, Entity>);

/// Keeps a flickering point light at every flame of the current view, so torches light the
/// scene and their light follows them while they are dragged.
fn sync_torch_lights(
    mut commands: Commands,
    model: Res<DragModel<Castle>>,
    time: Res<Time>,
    mut lights: ResMut<TorchLights>,
    mut torches: Query<(&mut Transform, &mut PointLight), With<TorchLight>>,
) {
    let view = model.view();
    let mut seen = HashSet::new();
    for node in view.nodes() {
        let name = node.id.to_string();
        if !name.ends_with("/flame") {
            continue;
        }
        let Some(world) = view.world_transform(&name) else {
            continue;
        };
        let position = world.translation + Vec3::Y * 0.08;
        let flicker = 1.0 + 0.1 * (time.elapsed_secs() * 9.0 + position.x * 3.0).sin();
        let intensity = 70_000.0 * flicker;
        seen.insert(name.clone());
        if let Some(entity) = lights.0.get(&name)
            && let Ok((mut transform, mut light)) = torches.get_mut(*entity)
        {
            transform.translation = position;
            light.intensity = intensity;
            continue;
        }
        let entity = commands
            .spawn((
                TorchLight,
                PointLight {
                    color: Color::srgb(1.0, 0.62, 0.3),
                    intensity,
                    range: 5.0,
                    radius: 0.06,
                    ..default()
                },
                Transform::from_translation(position),
            ))
            .id();
        lights.0.insert(name, entity);
    }
    lights.0.retain(|name, entity| {
        let keep = seen.contains(name);
        if !keep {
            commands.entity(*entity).try_despawn();
        }
        keep
    });
}

/// The look of the HUD: the library theme, drawn on the layer of the HUD camera.
fn theme() -> Theme {
    Theme::new(HUD_LAYER)
}

/// Where the scene camera stands and what it looks at.
fn scene_camera() -> Transform {
    Transform::from_xyz(0.0, 9.5, 8.5).looking_at(Vec3::new(0.0, 0.0, 0.2), Vec3::Y)
}

fn setup(mut commands: Commands) {
    spawn_hud_camera(&mut commands, HUD_HEIGHT);
    let layers = RenderLayers::from_layers(&[0, GIZMO_LAYER]);
    commands.spawn((SceneCamera, Camera3d::default(), layers, scene_camera()));
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            color: Color::srgb(1.0, 0.93, 0.8),
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            minimum_distance: 4.0,
            maximum_distance: 26.0,
            first_cascade_far_bound: 12.0,
            overlap_proportion: 0.2,
        }
        .build(),
        Transform::from_xyz(-5.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.8, 1.0),
        brightness: 280.0,
        ..default()
    });
}

/// Adds the castle to `app`: everything but the windowing and rendering plugins.
fn castle(app: &mut App) -> &mut App {
    app.add_plugins(
        DraggablePlugin::new(Castle::initial(), view)
            .with_transition_duration(theme().tween_seconds),
    )
    .add_plugins(HudPlugin::<Castle>::new(HUD_HEIGHT).camera::<SceneCamera>())
    .add_plugins(RadialMenuPlugin::<Castle>::new(theme()))
    .init_resource::<TorchLights>()
    .add_systems(Startup, setup)
    .add_systems(Update, sync_torch_lights)
    .add_systems(Update, open_menu_for_screenshot)
}

/// When screenshotting, opens the radial menu after the first shot is saved so the
/// second screenshot shows the menu open.
fn open_menu_for_screenshot(
    mut model: ResMut<DragModel<Castle>>,
    state: Option<ResMut<ScreenshotState>>,
    frame_count: Res<bevy::diagnostic::FrameCount>,
) {
    let Some(mut state) = state else {
        return;
    };
    if state.saved && !state.ready_for_second_shot {
        let castle = model.state();
        if castle.menu.is_none() {
            let castle = RadialHost::with_menu(castle, Some(menu::Ring::Root));
            model.set_state(castle);
        }
        // Signal that the menu is open and record the frame so the second shot waits
        // until the menu is actually rendered. Only set once.
        state.ready_for_second_shot = true;
        state.ready_frame = frame_count.0;
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(common::default_plugins("cozy_castle"));
    castle(&mut app)
        .add_plugins(
            AutoScreenshotPlugin::new("screenshots/screenshot_cozy_castle.png")
                .with_title("cozy_castle")
                .with_description("A gridless castle builder with gradient meshes")
                .with_second_shot("screenshots/screenshot_cozy_castle_menu.png")
                .wait_frames(3),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, DragSpace};

    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    use super::kit::cobble_tile;
    use super::stack::Variant;
    use super::*;

    fn pose(x: f32, z: f32, angle: f32) -> Pose {
        Pose {
            position: Vec2::new(x, z),
            angle,
        }
    }

    fn castle_of(pieces: &[(PieceKind, Pose)]) -> Castle {
        let mut castle = Castle::empty();
        for (kind, pose) in pieces {
            castle.add(*kind, pose.position.x, pose.position.y, pose.angle);
        }
        castle
    }

    fn height(castle: &Castle, id: u32) -> f32 {
        castle.piece(id).unwrap().transform.translation.y
    }

    #[test]
    fn every_footprint_is_counterclockwise() {
        use PieceKind::*;
        for kind in [
            Tower, Keep, ArchGate, SquareGate, WoodGate, Cobble, Outpost, Torch, Banner, Cottage,
        ] {
            let outline = kind.footprint();
            let area: f32 = edges(&outline).iter().map(|e| e.a.perp_dot(e.b)).sum();
            assert!(area > 0.0, "{kind:?}");
        }
    }

    #[test]
    fn touching_outlines_do_not_overlap_and_penetrating_ones_do() {
        let square = rectangle_outline(1.0, 1.0);
        let beside = to_world(&square, pose(1.0, 0.0, 0.0));
        let inside = to_world(&square, pose(0.5, 0.0, 0.0));
        assert!(!polygons_overlap(&square, &beside, OVERLAP_EPSILON));
        assert!(polygons_overlap(&square, &inside, OVERLAP_EPSILON));
        assert!(polygons_overlap(&square, &square, OVERLAP_EPSILON));
    }

    #[test]
    fn a_piece_rests_on_the_ground_or_on_the_top_of_the_piece_under_it() {
        let apart = castle_of(&[
            (PieceKind::ArchGate, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(1.0, 0.0, 0.0)),
        ]);
        assert!(height(&apart, 1).abs() < 1e-6, "beside is on the ground");
        let stacked = castle_of(&[
            (PieceKind::ArchGate, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(0.2, 0.0, 0.0)),
        ]);
        assert!((height(&stacked, 1) - PieceKind::ArchGate.top()).abs() < 1e-6);
        let roof = stacked.posed(0, pose(0.1, 0.0, 0.0));
        assert!(
            height(&roof, 0) > 0.0,
            "the other wall is now the upper one"
        );
    }

    #[test]
    fn a_tower_stands_on_a_path_without_rising() {
        let castle = castle_of(&[
            (PieceKind::Cobble, pose(0.0, 0.0, 0.0)),
            (PieceKind::Tower, pose(0.0, 0.0, 0.0)),
        ]);
        assert!(height(&castle, 1).abs() < 1e-6);
    }

    #[test]
    fn gates_align_end_to_end_and_side_by_side() {
        let castle = castle_of(&[
            (PieceKind::ArchGate, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(3.0, 1.0, 0.0)),
        ]);
        let poses = alignments(&castle, 1, Vec2::new(1.0, 0.05));
        let end_to_end = poses
            .iter()
            .any(|p| p.position.distance(Vec2::new(1.0, 0.0)) < 1e-3 && p.angle.abs() < 1e-3);
        assert!(end_to_end, "{poses:?}");
        let beside = alignments(&castle, 1, Vec2::new(0.1, 0.3));
        let flush = beside
            .iter()
            .any(|p| p.position.distance(Vec2::new(0.0, 0.3)) < 1e-3 && p.angle.abs() < 1e-3);
        assert!(flush, "{beside:?}");
    }

    #[test]
    fn alignment_turns_by_the_least_angle_and_works_for_any_outline() {
        let castle = castle_of(&[
            (PieceKind::Keep, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(3.0, 0.0, 0.3)),
        ]);
        let poses = alignments(&castle, 1, Vec2::new(0.0, 0.8));
        assert!(!poses.is_empty());
        for pose in &poses {
            let turn = normalize_angle(pose.angle - 0.3).abs();
            assert!(turn <= FRAC_PI_4 + 1e-3, "{pose:?}");
        }
        // A flush wall does not overlap the keep.
        let flush = castle.posed(1, poses[0]);
        assert!(!polygons_overlap(
            &flush.piece(1).unwrap().outline(),
            &flush.piece(0).unwrap().outline(),
            OVERLAP_EPSILON,
        ));
    }

    #[test]
    fn alignments_are_few_distinct_and_nearest_first() {
        let castle = Castle::courtyard();
        let id = castle
            .placed
            .iter()
            .find(|p| p.kind == PieceKind::Banner)
            .unwrap()
            .id;
        let point = Vec2::new(0.2, 1.4);
        let poses = alignments(&castle, id, point);
        assert!(!poses.is_empty() && poses.len() <= MAX_ALIGNMENTS);
        for pair in poses.windows(2) {
            assert!(pair[0].position.distance(point) <= pair[1].position.distance(point));
            assert!(!pair[0].is_near(pair[1]));
        }
        assert!(alignments(&castle, id, Vec2::new(40.0, 40.0)).is_empty());
    }

    #[test]
    fn candidates_start_with_the_castle_as_it_is() {
        let castle = castle_of(&[
            (PieceKind::ArchGate, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(3.0, 0.0, 0.0)),
        ]);
        let states = candidates(&castle, 1, Vec2::new(1.0, 0.0));
        assert!(states.len() > 1);
        let start = states[0].piece(1).unwrap().transform.translation;
        assert!(start.distance(Vec3::new(3.0, 0.0, 0.0)) < 1e-6);
    }

    #[test]
    fn the_initial_castle_has_towers_and_torches_on_the_ground() {
        let castle = Castle::courtyard();
        let towers: Vec<_> = castle
            .placed
            .iter()
            .filter(|p| p.kind == PieceKind::Tower)
            .collect();
        assert_eq!(towers.len(), 4);
        assert!(
            towers
                .iter()
                .all(|p| p.transform.translation.y.abs() < 1e-6)
        );
        let torches = castle
            .placed
            .iter()
            .filter(|p| p.kind == PieceKind::Torch)
            .count();
        assert_eq!(torches, 4);
        let ids: HashSet<_> = castle.placed.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), castle.placed.len());
    }

    #[test]
    fn pieces_are_draggable_roots_with_plain_children() {
        let castle = Castle::courtyard();
        let view = view(&castle);
        for placed in &castle.placed {
            let root = view.get(&piece_id(placed.id)).unwrap();
            assert!(root.drag.is_some() && root.parent.is_none());
            assert!(root.drag_space.is_some());
        }
        let roots: HashSet<String> = castle.placed.iter().map(|p| piece_id(p.id)).collect();
        let children = view
            .nodes()
            .iter()
            .filter(|node| {
                node.parent
                    .as_deref()
                    .is_some_and(|parent| roots.contains(parent))
            })
            .collect::<Vec<_>>();
        assert!(!children.is_empty());
        assert!(children.iter().all(|node| node.drag.is_none()));
        let flames = view
            .nodes()
            .iter()
            .filter(|node| node.id.ends_with("/flame"))
            .count();
        assert_eq!(flames, 4);
    }

    #[test]
    fn a_piece_sits_where_its_pose_says() {
        let castle = castle_of(&[(PieceKind::ArchGate, pose(1.5, -2.0, FRAC_PI_2))]);
        let view = view(&castle);
        let root = view.world_transform(&piece_id(0)).unwrap();
        assert!((root.translation.x - 1.5).abs() < 1e-5);
        assert!((root.translation.z + 2.0).abs() < 1e-5);
        let pose = castle.piece(0).unwrap().pose();
        assert!((normalize_angle(pose.angle - FRAC_PI_2)).abs() < 1e-5);
    }

    #[test]
    fn a_cobble_tile_lies_flat_on_the_ground_inside_its_footprint() {
        let bounds = cobble_tile().bounds();
        let (center, half) = (bounds.center(), bounds.half_extents());
        assert!(center.y - half.y > -1e-3, "{bounds:?}");
        assert!(center.y + half.y < 0.12, "{bounds:?}");
        assert!(center.x.abs() + half.x <= 0.5 + 1e-3, "{bounds:?}");
        assert!(center.z.abs() + half.z <= 0.5 + 1e-3, "{bounds:?}");
    }

    #[test]
    fn gates_and_banners_are_centered_in_depth_and_stand_on_the_ground() {
        for kind in [PieceKind::ArchGate, PieceKind::Banner] {
            for part in build_parts(kind, Variant::STANDING).iter().skip(1) {
                let bounds = part.shape.bounds();
                let low =
                    bounds.center().y - bounds.half_extents().y + part.transform.translation.y;
                assert!(low > -1e-3, "{kind:?}: {bounds:?}");
                let depth_center = bounds.center().z + part.transform.translation.z;
                assert!(depth_center.abs() < 0.05, "{kind:?}: {depth_center}");
            }
        }
    }

    #[test]
    fn every_piece_stays_inside_its_top_and_footprint() {
        use PieceKind::*;
        for kind in [
            Tower, Keep, ArchGate, SquareGate, WoodGate, Cobble, Outpost, Torch, Banner, Cottage,
        ] {
            let covered = Variant {
                open_top: true,
                thickness: 1.1,
                roof: false,
            };
            for variant in [Variant::STANDING, covered] {
                let parts = build_parts(kind, variant);
                let body = &parts[0].transform;
                assert!(body.translation.x.abs() < 1e-6 && body.translation.z.abs() < 1e-6);
                assert_eq!(body.rotation, Quat::IDENTITY);
                assert_eq!(body.scale, Vec3::ONE, "the root is never scaled");
                let highest = parts
                    .iter()
                    .map(|part| {
                        let bounds = part.shape.bounds();
                        let scale = part.transform.scale.y;
                        part.transform.translation.y
                            + (bounds.center().y + bounds.half_extents().y) * scale
                    })
                    .fold(0.0, f32::max);
                assert!(
                    (highest - kind.top()).abs() < 0.12 || kind == Cobble,
                    "{kind:?} {variant:?}: highest {highest}, top {}",
                    kind.top()
                );
            }
        }
    }

    #[test]
    fn a_covered_piece_has_a_platform_instead_of_a_roof_and_a_thicker_base() {
        let covered = Variant {
            open_top: true,
            thickness: 1.15,
            roof: false,
        };
        for kind in [PieceKind::Tower, PieceKind::Keep, PieceKind::Cottage] {
            let standing = build_parts(kind, Variant::STANDING);
            let platform = build_parts(kind, covered);
            let width = |parts: &[kit::Part]| parts[0].shape.bounds().half_extents().x;
            assert!(width(&platform) > width(&standing), "{kind:?} thickens");
            let tallest = |parts: &[kit::Part]| {
                parts
                    .iter()
                    .map(|part| part.shape.bounds().half_extents().y * 2.0)
                    .fold(0.0, f32::max)
            };
            assert!(
                tallest(&platform) < tallest(&standing) + 0.5,
                "{kind:?}: the roof has gone"
            );
        }
        let tower = |variant| build_parts(PieceKind::Tower, variant).len();
        assert_eq!(tower(Variant::STANDING), 3);
        assert_eq!(
            tower(covered),
            4,
            "a drum and a platform in place of the cone"
        );
    }

    fn drag(castle: &Castle, id: u32, to: Vec3) -> Frame {
        let space = DragSpace::plane(Vec3::ZERO, Dir3::Y);
        let context =
            DragContext::new(Arc::new(view), piece_id(id).into(), castle.clone()).with_space(space);
        let mut behavior = move_spec(castle, id).compile(&context);
        let frame = behavior(Pointer::at(to));
        Frame {
            drop: frame.drop_state,
            shown: frame.view.world_position(&piece_id(id)).unwrap(),
        }
    }

    struct Frame {
        drop: Castle,
        shown: Vec3,
    }

    #[test]
    fn dragging_near_a_neighbour_settles_flush_against_it() {
        let castle = castle_of(&[
            (PieceKind::ArchGate, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(4.0, 0.0, 0.0)),
        ]);
        let frame = drag(&castle, 1, Vec3::new(1.1, 0.0, 0.05));
        let dropped = frame.drop.piece(1).unwrap().pose();
        assert!(
            dropped.position.distance(Vec2::new(1.0, 0.0)) < 1e-3,
            "{dropped:?}"
        );
        assert!((frame.shown.x - 1.0).abs() < 1e-3, "the preview settled");
    }

    #[test]
    fn a_gate_moved_away_from_its_wall_and_back_stands_on_the_ground_again() {
        let castle = Castle::courtyard();
        let gate = castle
            .placed
            .iter()
            .find(|piece| piece.kind == PieceKind::ArchGate)
            .unwrap();
        let (id, home) = (gate.id, gate.transform.translation);
        assert!(home.y.abs() < 1e-6, "it starts on the ground");
        let away = drag(&castle, id, Vec3::new(0.0, 0.0, 4.5));
        let back = drag(&away.drop, id, Vec3::new(home.x, 0.0, home.z + 0.05));
        let rest = view(&castle).world_position(&piece_id(id)).unwrap();
        assert!(
            back.shown.distance(rest) < 1e-3,
            "the preview {} against {rest}",
            back.shown
        );
    }

    #[test]
    fn dragging_far_from_every_alignment_carries_the_piece_with_the_pointer() {
        let castle = castle_of(&[
            (PieceKind::ArchGate, pose(0.0, 0.0, 0.0)),
            (PieceKind::ArchGate, pose(4.0, 0.0, 0.0)),
        ]);
        let frame = drag(&castle, 1, Vec3::new(2.0, 0.0, 4.0));
        let dropped = frame.drop.piece(1).unwrap().pose();
        assert!(
            dropped.position.distance(Vec2::new(2.0, 4.0)) < 1e-3,
            "{dropped:?}"
        );
        assert!(frame.shown.y > 0.0, "a carried piece is lifted");
    }
}
