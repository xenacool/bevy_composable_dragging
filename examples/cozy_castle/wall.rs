//! Walls: a curve with a cross-section, placed by a pivot and a scale. The mesh is derived from
//! the state and never stored, so moving a control point, turning or scaling the wall re-derives
//! it and the engine blends the two states.
//!
//! The control points of the curve are in the local frame of the wall. The world curve is the
//! pivot applied to the scaled local curve, and the root node of the wall carries only the pivot
//! (never a scale), so the handles of the gizmo that are parented to it are not stretched.
//!
//! A wall is the cross-section of its [`Profile`] swept along the baked curve with
//! `Shape::sweep`, every sample of the curve lifted by the ground under it so the wall follows the
//! terrain. The openings are cut through the swept body, and the merlons of battlements are
//! cuboids that join it in a compound shape, each standing on the ground and turned to follow
//! the curve.

use std::sync::Arc;

use std::ops::Range;

use bevy::prelude::*;
use bevy_composable_dragging::handles::CurveHandles;
use bevy_composable_dragging::ops::{Snap, custom};
use bevy_composable_dragging::{DragSpec, Frame, Pointer, View, ViewNode};
use bevy_gradmesh::{Shape, ShapeDetail, ShapePart, SweepOpening, SweepOptions};

use crate::curve::{Baked, Curve};
use crate::kit::Fabric;
use crate::menu::{Tool, scene_space, scene_spec};
use crate::select::{selection_gizmo, wall_gizmo_size};
use crate::stack::Variant;
use crate::terrain::Terrain;
use crate::{Castle, Pose, WallPiece, piece_id, rectangle_outline, to_world};

/// The distance between the points of the path a wall is swept along.
const SWEEP_GAP: f32 = 0.1;
/// The pieces the round top of an arch and the round crown of a wall are made of.
const ROUND_SEGMENTS: u32 = 10;
/// How far a drawn stroke may stray from the curve it becomes.
const SIMPLIFY: f32 = 0.06;
/// Strokes shorter than this draw no wall.
const MIN_LENGTH: f32 = 0.5;
/// The pointer must travel this far for a stroke to get a new point.
const STROKE_GAP: f32 = 0.05;
/// The radius of the handle of a control point.
const HANDLE_RADIUS: f32 = 0.1;
/// How far the end face of a wall reaches along it. Neighbours align against this face.
const CAP_DEPTH: f32 = 0.1;
/// How far either side of a merlon the ground is probed for the lean of the merlon.
const SLOPE_PROBE: f32 = 0.05;
/// How far beyond the control points of a wall the curve may reach, which is how far from them a
/// piece is looked for.
const NEAR_CURVE: f32 = 1.0;
/// How far a point may be from the curve of a wall, beyond half its width, and still be on it.
const WALL_MARGIN: f32 = 0.05;
/// The distance between the samples of the curve that points are measured against.
const SAMPLE_GAP: f32 = 0.1;
/// A wall is never scaled below this on any axis, so it cannot collapse.
const MIN_SCALE: f32 = 0.2;

/// How many pieces of about `size` fit along `length`, at least one. `round` says whether the
/// last, partial piece counts (`f32::ceil`) or not (`f32::floor`).
pub fn pieces_along(length: f32, size: f32, round: fn(f32) -> f32) -> usize {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the count is a small, positive whole number"
    )]
    let count = round(length / size).max(1.0) as usize;
    count
}

/// The blocks along the top of a wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Battlements {
    /// The length of a merlon along the wall.
    pub block: f32,
    /// The length of the gap between two merlons.
    pub gap: f32,
    /// How far a merlon rises above the top of the wall.
    pub height: f32,
}

/// The top of the cross-section of a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crown {
    /// Level, with square corners.
    Flat,
    /// Rising to a ridge in the middle, like the stakes of a palisade.
    Pointed,
    /// Half round, like the coping of a garden wall.
    Rounded,
}

/// The cross-section of a wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// How thick the wall is.
    pub width: f32,
    /// How far the wall reaches above its base, to the top of its crown.
    pub height: f32,
    /// The merlons on top of the wall, if it has any.
    pub battlements: Option<Battlements>,
    /// The shape of the top of the wall.
    pub crown: Crown,
    /// What the wall is made of.
    pub fabric: Fabric,
    /// A tint on top of the fabric, which sets walls of one fabric apart.
    pub tint: Color,
}

impl Profile {
    /// A stone wall with battlements.
    pub fn stone() -> Self {
        Self {
            width: 0.3,
            height: 0.7,
            battlements: Some(Battlements {
                block: 0.22,
                gap: 0.14,
                height: 0.12,
            }),
            crown: Crown::Flat,
            fabric: Fabric::Stone,
            tint: Color::srgb(0.96, 0.96, 0.94),
        }
    }

    /// A thick, tall rampart of dark stone with broad battlements.
    pub fn rampart() -> Self {
        Self {
            width: 0.5,
            height: 1.0,
            battlements: Some(Battlements {
                block: 0.3,
                gap: 0.2,
                height: 0.18,
            }),
            crown: Crown::Flat,
            fabric: Fabric::Stone,
            tint: Color::srgb(0.78, 0.78, 0.82),
        }
    }

    /// A thin timber palisade with pointed stakes and no battlements.
    pub fn palisade() -> Self {
        Self {
            width: 0.12,
            height: 0.8,
            battlements: None,
            crown: Crown::Pointed,
            fabric: Fabric::Timber,
            tint: Color::WHITE,
        }
    }

    /// A low, thin garden wall of cobbles with a rounded top.
    pub fn garden() -> Self {
        Self {
            width: 0.14,
            height: 0.25,
            battlements: None,
            crown: Crown::Rounded,
            fabric: Fabric::Cobble,
            tint: Color::WHITE,
        }
    }

    /// The outline of the cross-section, with `x` across the wall and `y` up from its base.
    fn outline(&self) -> Vec<Vec2> {
        let half = self.width / 2.0;
        let mut outline = vec![Vec2::new(-half, 0.0), Vec2::new(half, 0.0)];
        match self.crown {
            Crown::Flat => {
                outline.extend([Vec2::new(half, self.height), Vec2::new(-half, self.height)]);
            }
            Crown::Pointed => {
                let shoulder = (self.height - self.width * 1.2).max(self.height / 2.0);
                outline.extend([
                    Vec2::new(half, shoulder),
                    Vec2::new(0.0, self.height),
                    Vec2::new(-half, shoulder),
                ]);
            }
            Crown::Rounded => {
                let rise = half.min(self.height / 2.0);
                outline.extend((0..=ROUND_SEGMENTS).map(|step| {
                    let angle = step as f32 / ROUND_SEGMENTS as f32 * std::f32::consts::PI;
                    Vec2::new(half * angle.cos(), self.height - rise + rise * angle.sin())
                }));
            }
        }
        outline
    }

    /// How far the wall reaches above its base.
    pub fn top(&self) -> f32 {
        self.height + self.battlements.as_ref().map_or(0.0, |b| b.height)
    }
}

/// What an opening in a wall is like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningShape {
    /// A tall passage under a stone lintel.
    Arch,
    /// A passage as tall as a doorway in a square frame.
    Square,
    /// A low timber gateway.
    Timber,
    /// The wall is broken down to a stub, with no lintel.
    Breach,
}

impl OpeningShape {
    /// How much of the height of the wall is cleared, from the ground up.
    fn clearance(self) -> f32 {
        match self {
            Self::Arch => 0.62,
            Self::Square => 0.72,
            Self::Timber => 0.5,
            Self::Breach => 0.85,
        }
    }

    /// The hole this opening cuts through a swept wall of `profile` over the stretch `t` of its
    /// length. Gateways reach down to the base of the wall, and a breach leaves a stub of it.
    fn cut(self, t: Range<f32>, profile: &Profile) -> SweepOpening {
        let clear = profile.height * self.clearance();
        match self {
            Self::Arch => SweepOpening::arch(t, 0.0, clear * 0.6, clear, ROUND_SEGMENTS),
            Self::Square | Self::Timber => SweepOpening::rect(t, 0.0, clear),
            Self::Breach => {
                SweepOpening::rect(t, profile.height - clear, profile.top() + profile.height)
            }
        }
    }
}

/// A stretch of a wall that is open. It is a range of the length of the curve, from zero at the
/// start to one at the end, so it stays where it is along the wall when the wall is scaled,
/// moved or turned.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    /// The stretch of the length of the curve that is open.
    pub t: Range<f32>,
    /// What the opening is like.
    pub shape: OpeningShape,
}

/// A wall along a curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    /// The base of the wall, in its local frame.
    pub curve: Curve,
    pub profile: Profile,
    /// Where the local frame is in the world. It is never scaled.
    pub pivot: Transform,
    /// How much the local curve is stretched along each of its axes.
    pub scale: Vec3,
    /// The stretches of the wall that are open, which do not overlap.
    pub openings: Vec<Opening>,
}

impl Wall {
    /// A stone wall through the world `points`, which are its base. The pivot is put at the
    /// middle of the points on the ground.
    pub fn through(points: &[Vec3]) -> Self {
        let middle = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let pivot = Vec3::new(middle.x, 0.0, middle.z);
        Self {
            curve: Curve {
                points: points.iter().map(|point| *point - pivot).collect(),
                closed: false,
            },
            profile: Profile::stone(),
            pivot: Transform::from_translation(pivot),
            scale: Vec3::ONE,
            openings: Vec::new(),
        }
    }

    /// A stone wall straight along the ground between two `(x, z)` points.
    pub fn straight(from: (f32, f32), to: (f32, f32)) -> Self {
        Self::through(&[Vec3::new(from.0, 0.0, from.1), Vec3::new(to.0, 0.0, to.1)])
    }

    /// The wall a freehand `stroke` on the ground draws, if it is long enough to be one.
    pub fn from_stroke(stroke: &[Vec3]) -> Option<Self> {
        let curve = Curve::simplified(stroke, SIMPLIFY);
        let length: f32 = curve
            .points
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .sum();
        (curve.points.len() >= 2 && length >= MIN_LENGTH).then(|| Self::through(&curve.points))
    }

    /// The world position of a point of the local frame.
    pub fn world_point(&self, local: Vec3) -> Vec3 {
        self.pivot.transform_point(local * self.scale)
    }

    /// The world position of control point `index`.
    pub fn point(&self, index: usize) -> Vec3 {
        self.world_point(self.curve.points[index])
    }

    /// The point of the local frame at the world position `world`.
    fn local_point(&self, world: Vec3) -> Vec3 {
        (self.pivot.rotation.inverse() * (world - self.pivot.translation)) / self.scale
    }

    /// The wall with control point `index` at the world position `world`.
    fn with_world_point(&self, index: usize, world: Vec3) -> Self {
        Self {
            curve: self.curve.with_point(index, self.local_point(world)),
            ..self.clone()
        }
    }

    /// The opening the wall has at `ratio` of its length, if any.
    pub fn opening_at(&self, ratio: f32) -> Option<&Opening> {
        self.openings
            .iter()
            .find(|opening| opening.t.contains(&ratio))
    }

    /// The wall with `opening` in it. An opening that overlaps others joins them, and the new
    /// shape wins.
    pub fn with_opening(&self, opening: Opening) -> Self {
        let mut wall = self.clone();
        let mut t = opening.t.clone();
        wall.openings.retain(|other| {
            let overlaps = other.t.start <= t.end && t.start <= other.t.end;
            if overlaps {
                t = t.start.min(other.t.start)..t.end.max(other.t.end);
            }
            !overlaps
        });
        wall.openings.push(Opening { t, ..opening });
        wall.openings
            .sort_by(|a, b| a.t.start.total_cmp(&b.t.start));
        wall
    }

    /// The wall with `profile` as its cross-section.
    pub fn with_profile(&self, profile: Profile) -> Self {
        Self {
            profile,
            ..self.clone()
        }
    }

    /// The wall as it looks with `variant`: with something standing on it its battlements give
    /// way to a walkway as high as they were, and it is thicker.
    pub fn stacked(&self, variant: Variant) -> Self {
        let mut wall = self.clone();
        wall.profile.width *= variant.thickness;
        if !variant.roof
            && let Some(battlements) = wall.profile.battlements.take()
        {
            wall.profile.height += battlements.height;
        }
        wall
    }

    /// The world height of the top of this wall under the highest of `points` that is on it,
    /// or `None` when none is. A point is on the wall when it is within half its width of the
    /// curve.
    pub fn top_under(&self, terrain: &Terrain, points: &[Vec2]) -> Option<f32> {
        let reach = self.scaled_profile().width / 2.0 + WALL_MARGIN;
        let controls: Vec<Vec2> = self
            .curve
            .points
            .iter()
            .map(|point| {
                let world = self.world_point(*point);
                Vec2::new(world.x, world.z)
            })
            .collect();
        let low = controls.iter().copied().reduce(Vec2::min)? - Vec2::splat(NEAR_CURVE);
        let high = controls.iter().copied().reduce(Vec2::max)? + Vec2::splat(NEAR_CURVE);
        if !points
            .iter()
            .any(|point| point.cmpge(low).all() && point.cmple(high).all())
        {
            return None;
        }
        let baked = self.curve.baked(self.scale);
        let root = self.root(terrain);
        let steps = pieces_along(baked.length(), SAMPLE_GAP, f32::ceil);
        let samples: Vec<(Vec2, f32)> = (0..=steps)
            .filter(|step| self.opening_at(*step as f32 / steps as f32).is_none())
            .map(|step| {
                let local = baked.position(baked.length() * step as f32 / steps as f32);
                let lifted = local + Vec3::Y * self.ground_lift(local, terrain);
                let world = root.transform_point(lifted);
                (Vec2::new(world.x, world.z), world.y)
            })
            .collect();
        points
            .iter()
            .filter_map(|point| {
                samples
                    .iter()
                    .filter(|(at, _)| at.distance(*point) <= reach)
                    .map(|(_, y)| *y)
                    .reduce(f32::max)
            })
            .reduce(f32::max)
            .map(|y| y + self.scaled_profile().top())
    }

    /// The wall with control point `index` moved by the ground part of the world `offset`.
    pub fn moved_point(&self, index: usize, offset: Vec3) -> Self {
        self.with_world_point(
            index,
            self.point(index) + Vec3::new(offset.x, 0.0, offset.z),
        )
    }

    /// The wall with control point `index` raised by `distance`, never below the ground.
    pub fn raised_point(&self, index: usize, distance: f32) -> Self {
        let mut world = self.point(index);
        world.y = (world.y + distance).max(0.0);
        self.with_world_point(index, world)
    }

    /// The wall with a new control point on the curve, in the middle of `segment`, moved by the
    /// ground part of the world `offset`.
    pub fn with_point_inserted(&self, segment: usize, offset: Vec3) -> Self {
        Self {
            curve: self.curve.with_midpoint_inserted(segment),
            ..self.clone()
        }
        .moved_point(segment + 1, offset)
    }

    /// The wall moved by the world `offset`, never below the ground.
    pub fn moved(&self, offset: Vec3) -> Self {
        let mut wall = self.clone();
        wall.pivot.translation += offset;
        wall.pivot.translation.y = wall.pivot.translation.y.max(0.0);
        wall
    }

    /// The wall turned by `rotation` about its pivot.
    pub fn turned(&self, rotation: Quat) -> Self {
        let mut wall = self.clone();
        wall.pivot.rotation = (rotation * wall.pivot.rotation).normalize();
        wall
    }

    /// The wall with its scale multiplied by `factor` on each local axis, never below
    /// [`MIN_SCALE`]. The footprint stretches about the pivot on X and Z and the elevation and
    /// the height on Y, while the thickness follows the mean of the footprint factors, so the
    /// cross-section keeps its proportions.
    pub fn scaled(&self, factor: Vec3) -> Self {
        let mut wall = self.clone();
        wall.scale = (wall.scale * factor).max(Vec3::splat(MIN_SCALE));
        wall
    }

    /// The cross-section as it is after scaling.
    pub fn scaled_profile(&self) -> Profile {
        let across = (self.scale.x * self.scale.z).sqrt();
        let up = self.scale.y;
        Profile {
            width: self.profile.width * across,
            height: self.profile.height * up,
            battlements: self.profile.battlements.as_ref().map(|b| Battlements {
                block: b.block * across,
                gap: b.gap * across,
                height: b.height * up,
            }),
            ..self.profile.clone()
        }
    }

    /// How a merlon is turned: its length along the curve, its height as close to up as the
    /// slope allows.
    fn orientation(forward: Vec3) -> Quat {
        let up = (Vec3::Y - forward * forward.y)
            .try_normalize()
            .unwrap_or(Vec3::Y);
        if forward == Vec3::ZERO {
            return Quat::IDENTITY;
        }
        Quat::from_mat3(&Mat3::from_cols(forward, up, forward.cross(up)))
    }

    /// How far the ground under the point `local` of the scaled curve is above the ground under
    /// the pivot.
    fn ground_lift(&self, local: Vec3, terrain: &Terrain) -> f32 {
        let at = |world: Vec3| terrain.surface(Vec2::new(world.x, world.z));
        at(self.pivot.transform_point(local)) - at(self.pivot.translation)
    }

    /// The transform of the root node of the wall: the pivot, on the ground under it.
    pub fn root(&self, terrain: &Terrain) -> Transform {
        let mut root = self.pivot;
        root.translation.y += terrain.surface(Vec2::new(root.translation.x, root.translation.z));
        root
    }

    /// The point of the curve `distance` along it, in the scaled local frame, on the ground.
    fn placed(&self, baked: &Baked, terrain: &Terrain, distance: f32) -> Vec3 {
        let local = baked.position(distance);
        local + Vec3::Y * self.ground_lift(local, terrain)
    }

    /// The mesh of the wall standing on `terrain`, in the frame of [`root`](Self::root) and
    /// already scaled. The profile is swept along the curve with every sample on the ground under
    /// it, so a wall follows the terrain it is raised or lowered with. The merlons of the
    /// battlements stand on top of the swept body.
    pub fn shape(&self, terrain: &Terrain) -> Shape {
        let baked = self.curve.baked(self.scale);
        let length = baked.length();
        let profile = self.scaled_profile();
        let looped = self.curve.closed && self.curve.points.len() >= 3;
        let steps = pieces_along(length, SWEEP_GAP, f32::ceil);
        let path = (0..if looped { steps } else { steps + 1 })
            .map(|step| self.placed(&baked, terrain, length * step as f32 / steps as f32))
            .collect();
        let options = SweepOptions::default()
            .with_closed(looped)
            .with_tint_by_length(vec![(0.0, profile.tint), (1.0, profile.tint)])
            .with_openings(
                self.openings
                    .iter()
                    .map(|opening| opening.shape.cut(opening.t.clone(), &profile))
                    .collect(),
            );
        let body = Shape::sweep(profile.outline(), path, options);
        let Some(battlements) = &profile.battlements else {
            return body;
        };
        let mut parts = vec![ShapePart::new(body)];
        parts.extend(self.merlons(&baked, terrain, &profile, battlements));
        Shape::compound(parts)
    }

    /// The merlons on top of the wall, each standing on the ground under it. There are none over
    /// an opening.
    fn merlons(
        &self,
        baked: &Baked,
        terrain: &Terrain,
        profile: &Profile,
        battlements: &Battlements,
    ) -> Vec<ShapePart> {
        let length = baked.length();
        let count = pieces_along(length, battlements.block + battlements.gap, f32::floor);
        let pitch = length / count as f32;
        (0..count)
            .map(|merlon| (merlon as f32 + 0.5) * pitch)
            .filter(|distance| self.opening_at(distance / length).is_none())
            .map(|distance| {
                let place = |at: f32| self.placed(baked, terrain, at);
                let rotation = Self::orientation(
                    (place(distance + SLOPE_PROBE) - place(distance - SLOPE_PROBE))
                        .normalize_or_zero(),
                );
                let center = place(distance)
                    + rotation * Vec3::Y * (profile.height + battlements.height / 2.0);
                let size = Vec3::new(battlements.block, battlements.height, profile.width + 0.02);
                ShapePart::new(Shape::cuboid(size, 0.04, ShapeDetail::Low))
                    .with_transform(Transform::from_translation(center).with_rotation(rotation))
                    .with_tint(profile.tint)
            })
            .collect()
    }

    /// The outlines on the ground, in world coordinates, of the two end faces of an open wall,
    /// which pieces such as towers align to. A face lies inside the wall, so a piece set flush
    /// against it touches the end of the wall. A closed wall has no ends.
    pub fn end_caps(&self) -> Vec<Vec<Vec2>> {
        if self.curve.closed && self.curve.points.len() >= 3 {
            return Vec::new();
        }
        let baked = self.curve.baked(self.scale);
        let width = self.scaled_profile().width;
        let cap = |distance: f32, outward: f32| {
            let point = self.pivot.transform_point(baked.position(distance));
            let direction = self.pivot.rotation * baked.tangent(distance) * outward;
            let direction = Vec2::new(direction.x, direction.z).normalize_or_zero();
            to_world(
                &rectangle_outline(CAP_DEPTH, width),
                Pose {
                    position: Vec2::new(point.x, point.z) - direction * CAP_DEPTH / 2.0,
                    angle: direction.to_angle(),
                },
            )
        };
        vec![cap(0.0, -1.0), cap(baked.length(), 1.0)]
    }
}

/// Draws a wall from the stroke of the pointer across the ground. The stroke is shown as a wall
/// while it grows, and on release the wall is added to the castle and selected. A press that
/// draws no wall selects nothing.
pub fn draw(castle: &Castle) -> DragSpec<Castle> {
    let castle = castle.clone();
    custom(move |context| {
        let context = context.clone();
        let castle = castle.clone();
        let mut stroke: Vec<Vec3> = Vec::new();
        Box::new(move |pointer: Pointer| {
            let point = pointer.point + context.grab_offset();
            let point = Vec3::new(point.x, 0.0, point.z);
            if stroke
                .last()
                .is_none_or(|last| last.distance(point) >= STROKE_GAP)
            {
                stroke.push(point);
            }
            let drawn = castle.with_stroke(&stroke);
            Frame::new(context.render(&drawn.with_selected(None)), drawn, 0.0)
        })
    })
}

/// Adds a wall to `view`: the wall itself, which selects the wall when it is pressed. While it is
/// `selected`, the tool decides what is revealed: a tool that transforms it shows its gizmo, and
/// reshaping shows the handles that edit its curve. Every control point has a gem to drag across
/// the ground and a cone to raise or lower it, and every segment has a small gem that pulls a
/// new control point out of it.
pub fn push_wall(
    view: &mut View<Castle>,
    piece: &WallPiece,
    selected: bool,
    snapshot: &Arc<Castle>,
) {
    let root = piece_id(piece.id);
    let id = piece.id;
    let wall = &piece.wall;
    let snapshot = Arc::clone(snapshot);
    let precision = snapshot.precision;
    let terrain = &snapshot.terrain;
    let base = wall.root(terrain);
    let shown = wall.stacked(snapshot.wall_variant(id));
    view.push(
        ViewNode::grad(root.clone(), shown.shape(terrain))
            .with_transform(base)
            .with_style(shown.profile.fabric.style())
            .draggable_in(scene_space(snapshot.tool, base.translation), {
                let snapshot = Arc::clone(&snapshot);
                move || scene_spec(&snapshot, id)
            }),
    );
    if !selected {
        return;
    }
    let top = wall.scaled_profile().top();
    selection_gizmo(
        id,
        base,
        precision,
        wall_gizmo_size(top),
        top,
        snapshot.tool,
    )
    .add_if_revealed(view, &snapshot);
    if snapshot.tool != Tool::Reshape {
        return;
    }
    let lift = Vec3::Y * (wall.scaled_profile().top() + 0.2);
    let gem = |on_ground: Vec3| on_ground + Vec3::Y * wall.ground_lift(on_ground, terrain) + lift;
    let points: Vec<Vec3> = wall
        .curve
        .points
        .iter()
        .map(|point| gem(*point * wall.scale))
        .collect();
    let middles: Vec<Vec3> = (0..wall.curve.segments())
        .map(|segment| gem(wall.curve.segment_middle(segment) * wall.scale))
        .collect();
    CurveHandles::new(root, base, &points)
        .with_radius(HANDLE_RADIUS)
        .with_style(Fabric::Handle.style())
        .with_middles(&middles)
        .move_point(Snap::NONE, move |castle: &Castle, index, offset| {
            castle.with_wall_edited(id, |wall| wall.moved_point(index, offset))
        })
        .raise_point(Snap::NONE, move |castle: &Castle, index, distance| {
            castle.with_wall_edited(id, |wall| wall.raised_point(index, distance))
        })
        .insert_point(Snap::NONE, move |castle: &Castle, segment, offset| {
            castle.with_wall_edited(id, |wall| wall.with_point_inserted(segment, offset))
        })
        .add_to(view);
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use bevy::mesh::VertexAttributeValues;
    use bevy_composable_dragging::{DragContext, DragSpace};

    use super::*;

    fn straight(length: f32) -> Wall {
        Wall::through(&[Vec3::ZERO, Vec3::new(length, 0.0, 0.0)])
    }

    fn bent() -> Wall {
        Wall::through(&[
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 2.0),
        ])
    }

    fn flat_ground() -> Terrain {
        Terrain::flat(7, 6, Vec2::new(-5.6, -4.4))
    }

    fn castle_of(wall: Wall) -> Castle {
        let mut castle = Castle::empty().with_wall(wall).with_selected(Some(0));
        castle.tool = Tool::Reshape;
        castle
    }

    fn wall_of(castle: &Castle, id: u32) -> &Wall {
        &castle
            .walls
            .iter()
            .find(|piece| piece.id == id)
            .unwrap()
            .wall
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    /// The vertices of the mesh of `shape` that a triangle uses.
    fn vertices(shape: &Shape) -> Vec<Vec3> {
        let mesh = shape.build();
        let Some(VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions expected");
        };
        let indices: Vec<usize> = mesh.indices().expect("indices expected").iter().collect();
        let mut used = vec![false; points.len()];
        for index in indices {
            used[index] = true;
        }
        points
            .iter()
            .zip(used)
            .filter_map(|(point, used)| used.then(|| Vec3::from(*point)))
            .collect()
    }

    fn opening(shape: OpeningShape) -> Opening {
        Opening { t: 0.4..0.6, shape }
    }

    #[test]
    fn a_wall_keeps_its_world_points_and_puts_the_pivot_in_the_middle() {
        let wall = bent();
        assert!(near(wall.point(0), Vec3::ZERO));
        assert!(near(wall.point(1), Vec3::new(2.0, 0.0, 0.0)));
        assert!(near(wall.point(2), Vec3::new(2.0, 0.0, 2.0)));
        assert!(near(
            wall.pivot.translation,
            Vec3::new(4.0 / 3.0, 0.0, 2.0 / 3.0)
        ));
        assert_eq!(wall.pivot.scale, Vec3::ONE);
    }

    #[test]
    fn a_straight_wall_is_as_long_wide_and_tall_as_its_profile() {
        let bounds = straight(3.0).shape(&flat_ground()).bounds();
        let (center, half) = (bounds.center(), bounds.half_extents());
        assert!((half.x * 2.0 - 3.05).abs() < 0.1, "{bounds:?}");
        assert!((half.z * 2.0 - 0.32).abs() < 0.05, "{bounds:?}");
        assert!((half.y * 2.0 - 0.82).abs() < 0.05, "{bounds:?}");
        assert!(
            center.y - half.y > -1e-3,
            "it stands on its base: {bounds:?}"
        );
        assert!(center.x.abs() < 0.1, "centered on its pivot: {bounds:?}");
    }

    #[test]
    fn a_bent_wall_follows_its_curve_in_both_directions() {
        let bounds = bent().shape(&flat_ground()).bounds();
        assert!(bounds.half_extents().x * 2.0 > 2.0, "{bounds:?}");
        assert!(bounds.half_extents().z * 2.0 > 2.0, "{bounds:?}");
    }

    #[test]
    fn a_raised_end_lifts_the_wall_and_the_mesh_is_derived_from_the_state() {
        let flat = straight(3.0);
        let raised = flat.raised_point(1, 1.5);
        assert!(
            raised.shape(&flat_ground()).bounds().half_extents().y
                > flat.shape(&flat_ground()).bounds().half_extents().y
        );
        assert_eq!(
            raised.shape(&flat_ground()),
            flat.raised_point(1, 1.5).shape(&flat_ground())
        );
        assert!(
            flat.raised_point(0, -5.0).point(0).y.abs() < 1e-6,
            "never below the ground"
        );
    }

    #[test]
    fn scaling_stretches_the_footprint_about_the_pivot_and_the_height_on_y() {
        let wall = bent().raised_point(2, 1.0);
        let scaled = wall.scaled(Vec3::new(2.0, 3.0, 0.5));
        let pivot = wall.pivot.translation;
        for index in 0..3 {
            let before = wall.point(index) - pivot;
            let after = scaled.point(index) - pivot;
            let expected = before * Vec3::new(2.0, 3.0, 0.5);
            assert!(near(after, expected), "{index}: {after} against {expected}");
        }
        assert_eq!(scaled.pivot, wall.pivot);
        let before = wall.scaled_profile();
        let after = scaled.scaled_profile();
        assert!((after.height - before.height * 3.0).abs() < 1e-5);
        assert!(
            (after.width - before.width).abs() < 1e-5,
            "sqrt(2 * 0.5) is one"
        );
        let wide = wall.scaled(Vec3::new(4.0, 1.0, 1.0));
        assert!((wide.scaled_profile().width - before.width * 2.0).abs() < 1e-5);
    }

    #[test]
    fn scaling_by_one_changes_nothing_and_the_inverse_restores_the_wall() {
        let wall = bent();
        assert_eq!(wall.scaled(Vec3::ONE), wall);
        let factor = Vec3::new(1.6, 0.8, 2.5);
        let restored = wall.scaled(factor).scaled(factor.recip());
        assert!(near(restored.scale, Vec3::ONE));
        assert_eq!(restored.curve, wall.curve);
        assert_eq!(restored.pivot, wall.pivot);
    }

    #[test]
    fn a_wall_is_never_scaled_below_the_minimum() {
        let tiny = bent().scaled(Vec3::splat(0.001));
        assert!(tiny.scale.min_element() >= MIN_SCALE - 1e-6);
        assert!(
            tiny.shape(&flat_ground())
                .bounds()
                .half_extents()
                .min_element()
                > 0.0
        );
    }

    #[test]
    fn moving_and_turning_leave_the_curve_in_its_local_frame_unchanged() {
        let wall = bent();
        let moved = wall.moved(Vec3::new(1.0, 5.0, -2.0));
        assert_eq!(moved.curve, wall.curve);
        assert!(near(
            moved.point(1),
            wall.point(1) + Vec3::new(1.0, 5.0, -2.0)
        ));
        let turned = wall.turned(Quat::from_rotation_y(FRAC_PI_2));
        assert_eq!(turned.curve, wall.curve);
        let pivot = wall.pivot.translation;
        let expected = pivot + Quat::from_rotation_y(FRAC_PI_2) * (wall.point(1) - pivot);
        assert!(near(turned.point(1), expected));
        assert!(wall.moved(Vec3::NEG_Y * 9.0).pivot.translation.y >= 0.0);
        assert_eq!(
            turned.shape(&flat_ground()),
            wall.shape(&flat_ground()),
            "the mesh is in the local frame"
        );
    }

    #[test]
    fn the_end_faces_end_exactly_where_the_wall_ends() {
        let caps = straight(3.0).end_caps();
        assert_eq!(caps.len(), 2);
        let center = |cap: &Vec<Vec2>| cap.iter().copied().sum::<Vec2>() / cap.len() as f32;
        assert!(center(&caps[0]).distance(Vec2::new(CAP_DEPTH / 2.0, 0.0)) < 0.01);
        assert!(center(&caps[1]).distance(Vec2::new(3.0 - CAP_DEPTH / 2.0, 0.0)) < 0.01);
        let looped = Wall {
            curve: Curve {
                closed: true,
                ..bent().curve
            },
            ..bent()
        };
        assert_eq!(looped.end_caps(), Vec::<Vec<Vec2>>::new());
    }

    #[test]
    fn the_end_faces_follow_a_scaled_and_turned_wall() {
        let wall = straight(3.0)
            .scaled(Vec3::new(2.0, 1.0, 1.0))
            .turned(Quat::from_rotation_y(FRAC_PI_2));
        let center = |cap: &Vec<Vec2>| cap.iter().copied().sum::<Vec2>() / cap.len() as f32;
        let caps = wall.end_caps();
        let end = wall.point(1);
        let start = wall.point(0);
        assert!(center(&caps[1]).distance(Vec2::new(end.x, end.z)) < CAP_DEPTH);
        assert!(center(&caps[0]).distance(Vec2::new(start.x, start.z)) < CAP_DEPTH);
        assert!(
            Vec2::new(end.x - start.x, end.z - start.z).length() > 5.9,
            "twice as long"
        );
    }

    #[test]
    fn a_stroke_becomes_a_wall_only_when_it_is_long_enough() {
        let stroke = |length: f32| -> Vec<Vec3> {
            (0..=20)
                .map(|step| Vec3::new(length * step as f32 / 20.0, 0.0, 1.0))
                .collect()
        };
        assert!(Wall::from_stroke(&stroke(0.2)).is_none());
        let wall = Wall::from_stroke(&stroke(3.0)).unwrap();
        assert_eq!(wall.curve.points.len(), 2);
        assert!(near(wall.point(0), Vec3::new(0.0, 0.0, 1.0)));
        assert!(near(wall.point(1), Vec3::new(3.0, 0.0, 1.0)));
        assert!(Wall::from_stroke(&[]).is_none());
    }

    #[test]
    fn dragging_a_control_point_moves_it_across_the_ground_only() {
        let wall = bent();
        let moved = wall.moved_point(1, Vec3::new(0.5, 9.0, -0.25));
        assert!(near(moved.point(1), Vec3::new(2.5, 0.0, -0.25)));
        assert!(near(moved.point(0), wall.point(0)));
        let turned = wall
            .turned(Quat::from_rotation_y(0.7))
            .scaled(Vec3::new(2.0, 1.0, 0.5));
        let moved = turned.moved_point(2, Vec3::new(0.5, 0.0, 0.25));
        assert!(near(
            moved.point(2),
            turned.point(2) + Vec3::new(0.5, 0.0, 0.25)
        ));
        assert!(near(moved.point(0), turned.point(0)));
    }

    #[test]
    fn inserting_a_point_adds_one_in_the_middle_of_the_segment() {
        let wall = bent();
        let inserted = wall.with_point_inserted(0, Vec3::ZERO);
        assert_eq!(inserted.curve.points.len(), 4);
        assert!(near(
            inserted.point(1),
            wall.world_point(wall.curve.segment_middle(0))
        ));
        let pulled = wall.with_point_inserted(0, Vec3::new(0.0, 0.0, 0.5));
        assert!(
            (pulled.point(1).z - wall.world_point(wall.curve.segment_middle(0)).z - 0.5).abs()
                < 1e-5
        );
    }

    #[test]
    fn an_unselected_wall_is_one_pressable_node_and_a_selected_one_has_handles() {
        let castle = castle_of(bent());
        let plain = crate::view(&castle.with_selected(None));
        let root = plain.get(&piece_id(0)).unwrap();
        assert!(root.parent.is_none() && root.drag.is_some());
        assert_eq!(root.transform.scale, Vec3::ONE);
        assert!(
            plain
                .nodes()
                .iter()
                .all(|node| !node.id.contains("/point-"))
        );
        let view = crate::view(&castle);
        for index in 0..3 {
            for kind in ["point", "lift"] {
                let handle = view
                    .get(&format!("{}/{kind}-{index}", piece_id(0)))
                    .unwrap();
                assert_eq!(handle.parent.as_deref(), Some(piece_id(0).as_str()));
                assert!(handle.drag.is_some() && handle.drag_space.is_some());
            }
        }
        for segment in 0..2 {
            assert!(
                view.get(&format!("{}/mid-{segment}", piece_id(0)))
                    .is_some()
            );
        }
        assert!(view.get(&format!("{}/mid-2", piece_id(0))).is_none());
    }

    #[test]
    fn handles_sit_above_the_scaled_and_turned_control_points() {
        let wall = bent()
            .turned(Quat::from_rotation_y(0.9))
            .scaled(Vec3::new(1.5, 2.0, 0.7));
        let castle = castle_of(wall.clone());
        let view = crate::view(&castle);
        for index in 0..3 {
            let gem = view
                .world_position(&format!("{}/point-{index}", piece_id(0)))
                .unwrap();
            let above = wall.point(index) + Vec3::Y * (wall.scaled_profile().top() + 0.2);
            assert!(near(gem, above), "{index}: {gem} against {above}");
        }
    }

    /// Drags `node` of `castle` with the spec its node has, to `to(anchor)`, and returns the
    /// state a drop there leaves.
    fn drop_at(castle: &Castle, node: &str, to: impl Fn(Vec3) -> Vec3) -> Castle {
        let view = crate::view(castle);
        let node_ref = view.get(node).unwrap();
        let spec = (node_ref.drag.as_ref().unwrap())();
        let anchor = view.world_position(node).unwrap();
        let context = DragContext::new(
            Arc::new(crate::view),
            node.to_owned().into(),
            castle.clone(),
        )
        .with_space(node_ref.drag_space.unwrap());
        let mut behavior = spec.compile(&context);
        behavior(Pointer::at(to(anchor))).drop_state
    }

    #[test]
    fn handles_edit_the_wall_through_the_drag_operators() {
        let castle = castle_of(bent().turned(Quat::from_rotation_y(0.4)));
        let wall = wall_of(&castle, 0).clone();
        let node = |name: &str| format!("{}/{name}", piece_id(0));
        let moved = drop_at(&castle, &node("point-1"), |gem| {
            gem + Vec3::new(0.5, 0.0, 0.25)
        });
        let point = wall_of(&moved, 0).point(1);
        assert!(
            near(point, wall.point(1) + Vec3::new(0.5, 0.0, 0.25)),
            "{point}"
        );

        let lifted = drop_at(&castle, &node("lift-1"), |stem| stem + Vec3::Y * 0.75);
        assert!((wall_of(&lifted, 0).point(1).y - 0.75).abs() < 1e-4);

        let split = drop_at(&castle, &node("mid-0"), |mid| {
            mid + Vec3::new(0.0, 0.0, 0.5)
        });
        assert_eq!(wall_of(&split, 0).curve.points.len(), 4);
    }

    #[test]
    fn drawing_a_stroke_adds_a_wall_on_release_and_selects_it() {
        let castle = Castle::empty();
        let ground = DragSpace::plane(Vec3::ZERO, Dir3::Y);
        let context = DragContext::new(Arc::new(crate::view), "ground".into(), castle.clone())
            .with_space(ground);
        let mut behavior = draw(&castle).compile(&context);
        let first = behavior(Pointer::at(Vec3::ZERO));
        assert!(first.drop_state.walls.is_empty(), "a point is no wall yet");
        assert_eq!(first.drop_state.selected, None);
        let mut last = first;
        for step in 1..=30 {
            last = behavior(Pointer::at(Vec3::new(step as f32 * 0.1, 0.0, 0.0)));
        }
        assert_eq!(last.drop_state.walls.len(), 1);
        assert_eq!(last.drop_state.selected, Some(0));
        assert!(
            last.view.get(&piece_id(0)).is_some(),
            "the preview shows the wall"
        );
        assert!(
            last.view
                .nodes()
                .iter()
                .all(|node| !node.id.contains("/point-")),
            "without handles while it is drawn"
        );
        assert!(near(
            wall_of(&last.drop_state, 0).point(1),
            Vec3::new(3.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn a_gateway_is_cut_through_the_wall_up_to_its_lintel() {
        let ground = flat_ground();
        let solid = straight(4.0);
        let height = solid.profile.height;
        let middle = |shape: &Shape| -> Vec<f32> {
            vertices(shape)
                .into_iter()
                .filter(|vertex| vertex.x.abs() < 0.02)
                .map(|vertex| vertex.y)
                .collect()
        };
        let jamb = |shape: &Shape| -> f32 {
            vertices(shape)
                .into_iter()
                .filter(|vertex| (vertex.x.abs() - 0.4).abs() < 0.002 && vertex.y < height - 0.01)
                .map(|vertex| vertex.y)
                .fold(f32::MIN, f32::max)
        };
        let closed = middle(&solid.shape(&ground));
        assert!(
            closed.iter().all(|y| *y < 0.01 || *y > height - 0.01),
            "the solid wall has nothing between its base and its top: {closed:?}"
        );
        for (shape, clearance) in [
            (OpeningShape::Arch, 0.62),
            (OpeningShape::Square, 0.72),
            (OpeningShape::Timber, 0.5),
        ] {
            let open = solid.with_opening(opening(shape)).shape(&ground);
            let clear = height * clearance;
            let heights = middle(&open);
            assert!(
                heights.iter().any(|y| (*y - clear).abs() < 0.02),
                "{shape:?} has its lintel at {clear}: {heights:?}"
            );
            assert!(
                heights.iter().all(|y| *y < 0.01 || *y > clear - 0.02),
                "{shape:?} is open up to its lintel: {heights:?}"
            );
            assert_ne!(open, solid.shape(&ground));
        }
        let arch = solid
            .with_opening(opening(OpeningShape::Arch))
            .shape(&ground);
        let square = solid
            .with_opening(opening(OpeningShape::Square))
            .shape(&ground);
        assert!(
            (jamb(&square) - height * 0.72).abs() < 0.02,
            "a square gate has jambs as high as it is"
        );
        assert!(
            jamb(&arch) < height * 0.62 * 0.6 + 0.02,
            "an arch springs at {}",
            jamb(&arch)
        );
        assert_ne!(
            solid
                .with_opening(opening(OpeningShape::Timber))
                .shape(&ground),
            square
        );
    }

    #[test]
    fn a_breach_leaves_a_stub_and_no_merlons_over_it() {
        let ground = flat_ground();
        let solid = straight(4.0);
        let stub = solid.profile.height * (1.0 - OpeningShape::Breach.clearance());
        let breached = solid.with_opening(opening(OpeningShape::Breach));
        let inside: Vec<f32> = vertices(&breached.shape(&ground))
            .into_iter()
            .filter(|vertex| vertex.x.abs() < 0.35)
            .map(|vertex| vertex.y)
            .collect();
        let top = inside.iter().copied().fold(f32::MIN, f32::max);
        assert!((top - stub).abs() < 0.02, "the stub is {top} high");
        assert!(inside.iter().any(|y| *y < 0.01), "it keeps its base");
        let outside = vertices(&breached.shape(&ground))
            .into_iter()
            .filter(|vertex| vertex.x.abs() > 0.5)
            .map(|vertex| vertex.y)
            .fold(f32::MIN, f32::max);
        assert!(outside > solid.profile.top() - 0.02, "the rest stands");
    }

    #[test]
    fn an_opening_stays_inside_its_stretch_of_a_bent_wall() {
        let ground = flat_ground();
        let wall = bent().with_opening(Opening {
            t: 0.2..0.3,
            shape: OpeningShape::Square,
        });
        let baked = wall.curve.baked(wall.scale);
        let gate = baked.position(baked.length() * 0.25);
        let far = baked.position(baked.length() * 0.75);
        let top_near = |shape: &Shape, at: Vec3| {
            vertices(shape)
                .into_iter()
                .filter(|vertex| {
                    Vec2::new(vertex.x, vertex.z).distance(Vec2::new(at.x, at.z)) < 0.2
                })
                .map(|vertex| vertex.y)
                .filter(|y| *y < wall.profile.height - 0.01)
                .fold(f32::MIN, f32::max)
        };
        let (open, closed) = (wall.shape(&ground), bent().shape(&ground));
        let clear = wall.profile.height * OpeningShape::Square.clearance();
        assert!((top_near(&open, gate) - clear).abs() < 0.02);
        assert!(top_near(&closed, gate) < 0.01, "a solid wall has no lintel");
        assert!(top_near(&open, far) < 0.01, "the other end is solid");
    }

    #[test]
    fn openings_merge_when_they_overlap_and_stay_in_order() {
        let wall = straight(4.0)
            .with_opening(Opening {
                t: 0.7..0.8,
                shape: OpeningShape::Square,
            })
            .with_opening(Opening {
                t: 0.1..0.2,
                shape: OpeningShape::Square,
            })
            .with_opening(Opening {
                t: 0.15..0.3,
                shape: OpeningShape::Arch,
            });
        assert_eq!(wall.openings.len(), 2);
        assert_eq!(wall.openings[0].t, 0.1..0.3);
        assert_eq!(wall.openings[0].shape, OpeningShape::Arch);
        assert_eq!(wall.openings[1].t, 0.7..0.8);
        assert!(wall.opening_at(0.25).is_some());
        assert!(wall.opening_at(0.5).is_none());
    }

    #[test]
    fn an_opening_stays_where_it_is_along_the_wall_when_the_wall_is_scaled_moved_or_turned() {
        let wall = straight(4.0).with_opening(Opening {
            t: 0.4..0.6,
            shape: OpeningShape::Arch,
        });
        let changed = wall
            .scaled(Vec3::new(2.0, 1.5, 1.0))
            .moved(Vec3::new(1.0, 0.0, 1.0))
            .turned(Quat::from_rotation_y(0.8));
        assert_eq!(changed.openings, wall.openings);
        let before = wall.curve.baked(wall.scale);
        let after = changed.curve.baked(changed.scale);
        assert!(
            (after.length() / before.length() - 2.0).abs() < 0.02,
            "twice as wide"
        );
        let middle = |wall: &Wall| {
            let baked = wall.curve.baked(wall.scale);
            wall.world_point(wall.curve.points[0])
                .lerp(wall.world_point(wall.curve.points[1]), 0.5)
                + Vec3::ZERO * baked.length()
        };
        let gate = changed
            .world_point(changed.curve.points[0])
            .lerp(changed.world_point(changed.curve.points[1]), 0.5);
        assert!(middle(&changed).distance(gate) < 1e-4);
    }

    #[test]
    fn a_wall_without_battlements_is_only_as_tall_as_its_profile() {
        let ground = flat_ground();
        let stone = straight(3.0).shape(&ground).bounds();
        for profile in [Profile::palisade(), Profile::garden()] {
            let wall = straight(3.0).with_profile(profile.clone());
            let bounds = wall.shape(&ground).bounds();
            assert!(
                (bounds.size().y - profile.height).abs() < 0.01,
                "{:?}",
                bounds
            );
            assert!((bounds.size().z - profile.width).abs() < 0.01, "{bounds:?}");
            assert!((bounds.size().x - 3.0).abs() < 0.1, "{bounds:?}");
            assert!(bounds.min.y > -1e-3, "it stands on its base: {bounds:?}");
            assert!(bounds.size().y < stone.size().y);
        }
    }

    #[test]
    fn the_profiles_are_set_apart() {
        let profiles = [
            Profile::stone(),
            Profile::rampart(),
            Profile::palisade(),
            Profile::garden(),
        ];
        for (index, profile) in profiles.iter().enumerate() {
            for other in &profiles[index + 1..] {
                assert_ne!(profile, other);
                assert!(
                    profile.fabric != other.fabric
                        || profile.tint != other.tint
                        || profile.crown != other.crown,
                    "{profile:?} looks like {other:?}"
                );
            }
        }
        let (stone, rampart) = (Profile::stone(), Profile::rampart());
        assert!(rampart.width > stone.width && rampart.height > stone.height);
        assert!(rampart.battlements.is_some() && stone.battlements.is_some());
        assert!(rampart.top() > stone.top());
        let (palisade, garden) = (Profile::palisade(), Profile::garden());
        assert!(palisade.battlements.is_none() && garden.battlements.is_none());
        assert!(palisade.width < stone.width && garden.width < stone.width);
        assert!(garden.height < palisade.height && palisade.height > stone.height);
        assert_eq!(palisade.crown, Crown::Pointed);
        assert_eq!(garden.crown, Crown::Rounded);
        assert_eq!(palisade.fabric, Fabric::Timber);
        assert_eq!(palisade.top(), palisade.height);
    }

    #[test]
    fn a_wall_takes_the_profile_it_is_given_and_keeps_the_rest() {
        let wall = bent()
            .turned(Quat::from_rotation_y(0.4))
            .scaled(Vec3::new(1.5, 1.2, 0.8))
            .with_opening(opening(OpeningShape::Arch));
        let rampart = wall.with_profile(Profile::rampart());
        assert_eq!(rampart.profile, Profile::rampart());
        assert_eq!(
            Wall {
                profile: wall.profile.clone(),
                ..rampart.clone()
            },
            wall
        );
        assert_ne!(rampart.shape(&flat_ground()), wall.shape(&flat_ground()));
        let scaled = rampart.scaled_profile();
        assert!((scaled.height - Profile::rampart().height * 1.2).abs() < 1e-5);
        assert_eq!(scaled.fabric, Fabric::Stone);
        assert_eq!(scaled.tint, Profile::rampart().tint);
        assert_eq!(scaled.crown, Crown::Flat);
    }

    #[test]
    fn a_rampart_is_thick_tall_and_battlemented() {
        let ground = flat_ground();
        let stone = straight(3.0).shape(&ground).bounds();
        let rampart = straight(3.0)
            .with_profile(Profile::rampart())
            .shape(&ground)
            .bounds();
        let profile = Profile::rampart();
        assert!(
            (rampart.size().y - profile.top()).abs() < 0.01,
            "{rampart:?}"
        );
        assert!(rampart.size().z > stone.size().z + 0.15, "{rampart:?}");
        assert!(rampart.size().y > stone.size().y + 0.25, "{rampart:?}");
    }

    #[test]
    fn a_palisade_is_pointed_and_a_garden_wall_is_rounded() {
        let ground = flat_ground();
        let ridge = |profile: Profile| -> Vec<Vec3> {
            let top = profile.height;
            vertices(&straight(3.0).with_profile(profile).shape(&ground))
                .into_iter()
                .filter(|vertex| vertex.y > top - 0.005)
                .collect()
        };
        let stakes = ridge(Profile::palisade());
        assert!(!stakes.is_empty());
        assert!(
            stakes.iter().all(|vertex| vertex.z.abs() < 0.01),
            "the stakes end in a ridge"
        );
        let coping = ridge(Profile::garden());
        assert!(!coping.is_empty());
        assert!(
            coping
                .iter()
                .all(|vertex| vertex.z.abs() < Profile::garden().width / 4.0),
            "the garden wall is round on top"
        );
        let flat = ridge(Profile {
            battlements: None,
            ..Profile::stone()
        });
        assert!(
            flat.iter()
                .any(|vertex| vertex.z.abs() > Profile::stone().width / 2.0 - 0.01),
            "a flat top reaches the sides"
        );
    }

    #[test]
    fn the_swept_body_follows_the_ground_under_it() {
        let wall = Wall::straight((0.0, 0.0), (2.0, 0.0)).with_profile(Profile::garden());
        let castle = Castle::empty().with_wall(wall.clone());
        let hill = castle.sculpted(&crate::terrain::Brush {
            center: Vec2::new(1.0, 0.0),
            radius: 0.6,
            amount: 0.5,
            falloff: 1.0,
        });
        let shape = wall.shape(&hill.terrain);
        let root = wall.root(&hill.terrain);
        for vertex in vertices(&shape) {
            let world = root.transform_point(vertex);
            let nearby: Vec<f32> = (-5..=5)
                .map(|step| {
                    hill.terrain
                        .surface(Vec2::new(world.x + step as f32 * 0.05, 0.0))
                })
                .collect();
            let low = nearby.iter().copied().fold(f32::MAX, f32::min);
            let high = nearby.iter().copied().fold(f32::MIN, f32::max);
            assert!(
                world.y > low - 0.02 && world.y < high + wall.profile.top() + 0.02,
                "{world} between the ground {low} and {high}"
            );
        }
        let top = vertices(&shape)
            .into_iter()
            .map(|vertex| root.transform_point(vertex))
            .filter(|world| (world.x - 1.0).abs() < 0.03)
            .map(|world| world.y)
            .fold(f32::MIN, f32::max);
        assert!((top - (0.5 + wall.profile.top())).abs() < 0.03, "{top}");
        let bounds = shape.bounds();
        assert!(bounds.min.y < -0.4, "the ends are low: {bounds:?}");
    }

    #[test]
    fn a_closed_wall_is_swept_all_the_way_round() {
        let looped = Wall {
            curve: Curve {
                closed: true,
                ..bent().curve
            },
            ..bent()
        };
        let bounds = looped.shape(&flat_ground()).bounds();
        assert!(bounds.size().x > 1.8 && bounds.size().z > 1.8, "{bounds:?}");
        let open = bent().shape(&flat_ground());
        assert_ne!(looped.shape(&flat_ground()), open);
        let ends = |wall: &Wall| {
            vertices(&wall.shape(&flat_ground()))
                .into_iter()
                .filter(|vertex| {
                    vertex.y < 0.01 && vertex.distance(Vec3::new(-4.0 / 3.0, 0.0, -2.0 / 3.0)) < 0.2
                })
                .count()
        };
        assert!(ends(&looped) > 0 && ends(&bent()) > 0);
    }
}
