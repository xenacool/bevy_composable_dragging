//! Handles that edit a 3D curve: gems to move its control points, cones to raise them, gems to
//! insert new points, and for the chosen point a handle for the width and a ring for the roll.
//!
//! [`CurveHandles`] adds them to a [`View`] as nodes parented to a target node, which is
//! whatever draws the curve, so they blend, fade and snap like everything else. Each handle is
//! draggable in its own plane, axis or ring and uses the value operators ([`across_plane`],
//! [`along_axis`], [`around_axis`]) with the [`Snap`] you give it, so the preview is pulled onto
//! the steps and a drop commits them.
//!
//! The handles know nothing about the curve. They are told where the control points are and what
//! a drag does: every tool is a closure from the state the drag started from, the index of the
//! point and the measured value to the new state. Pressing a handle is how the point is chosen,
//! so the closure of a gem can select its point as well as move it.
//!
//! ```
//! use bevy::prelude::*;
//! use bevy_composable_dragging::handles::{CurveHandles, Lateral};
//! use bevy_composable_dragging::ops::Snap;
//! use bevy_composable_dragging::{View, ViewNode};
//!
//! #[derive(Clone)]
//! struct Road {
//!     points: Vec<Vec3>,
//!     half_widths: Vec<f32>,
//!     selected: Option<usize>,
//! }
//!
//! fn view(road: &Road) -> View<Road> {
//!     let mut view = View::new();
//!     view.push(ViewNode::cuboid("road", Vec3::new(1.0, 0.1, 8.0), 0.0));
//!     let lateral: Vec<Lateral> = road
//!         .half_widths
//!         .iter()
//!         .map(|half_width| Lateral { right: Vec3::X, half_width: *half_width })
//!         .collect();
//!     CurveHandles::new("road", Transform::IDENTITY, &road.points)
//!         .selected(road.selected)
//!         .move_point(Snap::NONE, |road: &Road, index, offset| {
//!             let mut road = road.clone();
//!             road.points[index] += offset;
//!             road.selected = Some(index);
//!             road
//!         })
//!         .width(&lateral, Snap::step(0.1), |road: &Road, index, change| {
//!             let mut road = road.clone();
//!             road.half_widths[index] += change;
//!             road
//!         })
//!         .add_to(&mut view);
//!     view
//! }
//!
//! let road = Road {
//!     points: vec![Vec3::ZERO, Vec3::new(0.0, 0.0, 4.0)],
//!     half_widths: vec![1.0, 1.0],
//!     selected: Some(1),
//! };
//! assert!(view(&road).get("road/width-1").is_some());
//! ```

use std::f32::consts::TAU;

use bevy::math::{Dir3, Quat, Vec3};
use bevy::prelude::{Color, Transform};
use bevy_gradmesh::Style;

use crate::gizmo::{EDGE_ON, END_ON};
use crate::ops::{Snap, across_plane, along_axis, around_axis};
use crate::owned::owned_fn;
use crate::space::DragSpace;
use crate::spec::DragSpec;
use crate::view::{NodeId, View, ViewNode};

/// The radius of the gem of a control point, which the other handles are sized from.
const GEM_RADIUS: f32 = 0.1;
/// The gem of the chosen point is this many times as big.
const CHOSEN_GROWTH: f32 = 1.3;
/// The gem that inserts a point is this many times as big as the gem of a point.
const MIDDLE_SIZE: f32 = 0.6;
/// The radius and height of the cone that raises a point, and how far above the gem it floats,
/// in radii of the gem.
const CONE_RADIUS: f32 = 0.7;
const CONE_HEIGHT: f32 = 2.0;
const CONE_LIFT: f32 = 3.0;
/// The side of the cube that widens, and the radius and tube of the ring that rolls, in radii of
/// the gem.
const WIDTH_SIDE: f32 = 1.4;
const ROLL_RING: f32 = 5.0;
const ROLL_TUBE: f32 = 0.25;
/// How many points [`CurveHandles::footprint`] takes around the roll ring.
const RING_SAMPLES: usize = 12;
/// The size of the mark that shows the roll on its ring, in radii of the gem.
const ROLL_MARK: f32 = 0.7;
/// How much darker the underside of a handle is.
const SHADE: f32 = 0.6;

/// What a handle does with a measured value: the steps it snaps to and the edit it applies.
struct Tool<S, V> {
    snap: Snap,
    edit: Edit<S, V>,
}

owned_fn! {
    /// Turns the state a drag started from, the index of a point or segment and a measured value
    /// into a new state.
    Edit<S, V>, EditCall, (state: &S, index: usize, value: V) -> S
}

impl<S, V> Clone for Tool<S, V> {
    fn clone(&self) -> Self {
        Self {
            snap: self.snap,
            edit: self.edit.clone(),
        }
    }
}

impl<S, V> Tool<S, V> {
    fn new(snap: Snap, edit: impl Fn(&S, usize, V) -> S + Clone + Send + Sync + 'static) -> Self {
        Self {
            snap,
            edit: Edit::new(edit),
        }
    }
}

/// How wide the curve is at a control point, which places its width handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lateral {
    /// The direction across the curve, in the local frame of the target. It is normalized.
    pub right: Vec3,
    /// The distance from the point to the edge on the side of `right`, in the same units.
    pub half_width: f32,
}

/// How far the curve is rolled about its direction at a control point, which places the mark on
/// its roll ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Twist {
    /// The direction along the curve, in the local frame of the target. It is normalized.
    pub axis: Vec3,
    /// The roll, in radians about `axis`. Zero puts the mark on the side facing up.
    pub angle: f32,
}

/// The styles of the handles.
struct Styles {
    gem: Style,
    chosen: Style,
    width: Style,
    roll: Style,
}

impl Default for Styles {
    fn default() -> Self {
        Self {
            gem: Style::shaded(Color::srgb(0.95, 0.78, 0.22), SHADE),
            chosen: Style::shaded(Color::srgb(1.0, 0.45, 0.2), SHADE),
            width: Style::shaded(Color::srgb(0.3, 0.8, 0.32), SHADE),
            roll: Style::shaded(Color::srgb(0.3, 0.46, 0.96), SHADE),
        }
    }
}

/// A set of handles that edit a curve through control points.
///
/// Build it with [`new`](Self::new), choose the tools, and [`add_to`](Self::add_to) a view. A
/// handle is added only for a tool that was chosen. The points, and the axes and widths that go
/// with them, are in the local frame of the target, whose world transform is `transform`; the
/// drags happen in world space.
///
/// The handles have the ids `<target>/point-<i>` (a gem that drags across the ground plane),
/// `<target>/lift-<i>` (a cone that drags along the Y axis), `<target>/mid-<segment>` (a small
/// gem that inserts a point), and for the [chosen](Self::selected) point only
/// `<target>/width-<i>` and `<target>/roll-<i>`, the latter with its mark as the child
/// `<target>/roll-<i>/mark`. The ids stay the same from one view to the next, so the handles
/// blend as the curve is edited.
pub struct CurveHandles<S> {
    target: NodeId,
    transform: Transform,
    points: Vec<Vec3>,
    middles: Option<Vec<Vec3>>,
    closed: bool,
    selected: Option<usize>,
    radius: f32,
    styles: Styles,
    move_point: Option<Tool<S, Vec3>>,
    raise_point: Option<Tool<S, f32>>,
    insert_point: Option<Tool<S, Vec3>>,
    width: Option<(Vec<Lateral>, Tool<S, f32>)>,
    roll: Option<(Vec<Twist>, Tool<S, f32>)>,
    eye: Option<Vec3>,
}

impl<S: Clone + Send + Sync + 'static> CurveHandles<S> {
    /// Handles for the control `points` of the curve drawn by the node `target`, which is at the
    /// world `transform`. Put the gems where they should be seen, such as a little above the
    /// curve.
    pub fn new(target: impl Into<NodeId>, transform: Transform, points: &[Vec3]) -> Self {
        Self {
            target: target.into(),
            transform,
            points: points.to_vec(),
            middles: None,
            closed: false,
            selected: None,
            radius: GEM_RADIUS,
            styles: Styles::default(),
            move_point: None,
            raise_point: None,
            insert_point: None,
            width: None,
            roll: None,
            eye: None,
        }
    }

    /// Makes the curve a closed loop, so the last point and the first have a segment between
    /// them too.
    #[must_use]
    pub fn closed(mut self, closed: bool) -> Self {
        self.closed = closed;
        self
    }

    /// Chooses the point whose gem is highlighted and which gets the width and roll handles.
    #[must_use]
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Puts the insert gem of segment `i` at `middles[i]` instead of halfway between its points,
    /// which a curve that bends between its points needs.
    #[must_use]
    pub fn with_middles(mut self, middles: &[Vec3]) -> Self {
        self.middles = Some(middles.to_vec());
        self
    }

    /// Sets the radius of the gem of a point, which sizes the other handles too.
    #[must_use]
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Tells the handles where the camera is, in the world, so that the ones that cannot be
    /// dragged from there are left out: the roll ring when it is seen edge-on, and the width
    /// handle when its axis points straight at the camera. The pointer moves a point wildly
    /// along either. Without it every handle is added, whatever the view.
    #[must_use]
    pub fn seen_from(mut self, eye: Vec3) -> Self {
        self.eye = Some(eye);
        self
    }

    /// How much the local direction `axis` at the local `point` points at the eye: one when
    /// straight at it, zero when square with the view. `None` when the eye is not known.
    fn facing(&self, point: Vec3, axis: Vec3) -> Option<f32> {
        let view = (self.eye? - self.world(point)).try_normalize()?;
        Some(view.dot(self.transform.rotation * axis).abs())
    }

    /// Sets the style of the gems and cones.
    #[must_use]
    pub fn with_style(mut self, style: Style) -> Self {
        self.styles.gem = style;
        self
    }

    /// Sets the style of the gem of the chosen point.
    #[must_use]
    pub fn with_chosen_style(mut self, style: Style) -> Self {
        self.styles.chosen = style;
        self
    }

    /// Sets the style of the width handle.
    #[must_use]
    pub fn with_width_style(mut self, style: Style) -> Self {
        self.styles.width = style;
        self
    }

    /// Sets the style of the roll ring.
    #[must_use]
    pub fn with_roll_style(mut self, style: Style) -> Self {
        self.styles.roll = style;
        self
    }

    /// Adds a gem to every point that drags it across the ground plane. `apply` gets the state
    /// the drag started from, the index of the point and the world offset to move it by, snapped
    /// to steps of `snap` along each axis of the plane.
    #[must_use]
    pub fn move_point(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, usize, Vec3) -> S + Clone + Send + Sync + 'static,
    ) -> Self {
        self.move_point = Some(Tool::new(snap, apply));
        self
    }

    /// Adds a cone above every point that drags it along the Y axis. `apply` gets the state the
    /// drag started from, the index of the point and the signed distance to raise it by, snapped
    /// to steps of `snap`.
    #[must_use]
    pub fn raise_point(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, usize, f32) -> S + Clone + Send + Sync + 'static,
    ) -> Self {
        self.raise_point = Some(Tool::new(snap, apply));
        self
    }

    /// Adds a small gem to every segment that pulls a new point out of it across the ground
    /// plane. `apply` gets the state the drag started from, the index of the segment (the new
    /// point goes between the points `segment` and `segment + 1`) and the world offset from the
    /// middle of the segment, snapped to steps of `snap`.
    #[must_use]
    pub fn insert_point(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, usize, Vec3) -> S + Clone + Send + Sync + 'static,
    ) -> Self {
        self.insert_point = Some(Tool::new(snap, apply));
        self
    }

    /// Adds a handle to the chosen point that widens the curve: a cube on the edge on the side
    /// of `right`, dragged along that direction. `lateral[i]` describes the point `i`. `apply`
    /// gets the state the drag started from, the index of the point and the change of the half
    /// width, positive when the handle moves outwards, snapped to steps of `snap`. Clamp the
    /// result in `apply` if the width must stay positive.
    #[must_use]
    pub fn width(
        mut self,
        lateral: &[Lateral],
        snap: Snap,
        apply: impl Fn(&S, usize, f32) -> S + Clone + Send + Sync + 'static,
    ) -> Self {
        self.width = Some((lateral.to_vec(), Tool::new(snap, apply)));
        self
    }

    /// Adds a ring to the chosen point that rolls the curve about its direction, dragged around
    /// that axis. `twist[i]` describes the point `i`. `apply` gets the state the drag started
    /// from, the index of the point and the change of the roll in radians, snapped to steps of
    /// `snap`.
    #[must_use]
    pub fn roll(
        mut self,
        twist: &[Twist],
        snap: Snap,
        apply: impl Fn(&S, usize, f32) -> S + Clone + Send + Sync + 'static,
    ) -> Self {
        self.roll = Some((twist.to_vec(), Tool::new(snap, apply)));
        self
    }

    /// Adds the handles to `view` as children of the target.
    pub fn add_to(self, view: &mut View<S>) {
        for index in 0..self.points.len() {
            self.push_gem(view, index);
            self.push_cone(view, index);
        }
        for (segment, middle) in self.middles().into_iter().enumerate() {
            self.push_middle(view, segment, middle);
        }
        if let Some(index) = self.selected.filter(|index| *index < self.points.len()) {
            self.push_width(view, index);
            self.push_roll(view, index);
        }
    }

    /// The points of the world that the handles of control point `index` occupy: its gem, the
    /// cone above it, the segment gems on either side of it, and for the chosen point the
    /// width handle and points around the roll ring. A pop-up that has to leave the handles
    /// reachable keeps clear of these.
    #[must_use]
    pub fn footprint(&self, index: usize) -> Vec<Vec3> {
        let Some(point) = self.points.get(index).copied() else {
            return Vec::new();
        };
        let mut spots = vec![self.world(point)];
        if self.raise_point.is_some() {
            let lift = self.radius * (CONE_LIFT + CONE_HEIGHT / 2.0);
            spots.push(self.world(point + self.transform.rotation.inverse() * (Vec3::Y * lift)));
        }
        if self.insert_point.is_some() {
            let middles = self.middles();
            let before = if index == 0 {
                middles.len().checked_sub(1).filter(|_| self.closed)
            } else {
                Some(index - 1)
            };
            for segment in before.into_iter().chain(Some(index)) {
                spots.extend(middles.get(segment).map(|middle| self.world(*middle)));
            }
        }
        if self.selected != Some(index) {
            return spots;
        }
        if let Some(lateral) = self
            .width
            .as_ref()
            .and_then(|(lateral, _)| lateral.get(index))
        {
            let right = lateral.right.normalize_or_zero();
            spots.push(self.world(point + right * lateral.half_width));
        }
        if let Some(twist) = self.roll.as_ref().and_then(|(twist, _)| twist.get(index)) {
            let axis = twist.axis.normalize_or_zero();
            if axis != Vec3::ZERO {
                let zero = (Vec3::Y - axis * axis.y)
                    .try_normalize()
                    .unwrap_or_else(|| axis.any_orthonormal_vector());
                let ring = ROLL_RING * self.radius;
                for step in 0..RING_SAMPLES {
                    let angle = TAU * step as f32 / RING_SAMPLES as f32;
                    spots
                        .push(self.world(point + Quat::from_axis_angle(axis, angle) * zero * ring));
                }
            }
        }
        spots
    }

    fn id(&self, kind: &str, index: usize) -> String {
        format!("{}/{kind}-{index}", self.target)
    }

    /// Where the insert gem of every segment is.
    fn middles(&self) -> Vec<Vec3> {
        if let Some(middles) = &self.middles {
            return middles.clone();
        }
        let count = self.points.len();
        let segments = if self.closed && count > 1 {
            count
        } else {
            count.saturating_sub(1)
        };
        (0..segments)
            .map(|segment| (self.points[segment] + self.points[(segment + 1) % count]) / 2.0)
            .collect()
    }

    /// The world position of a point of the local frame of the target.
    fn world(&self, local: Vec3) -> Vec3 {
        self.transform.transform_point(local)
    }

    /// The world direction of a direction of the local frame of the target.
    fn world_direction(&self, local: Vec3) -> Option<Dir3> {
        Dir3::new(self.transform.rotation * local).ok()
    }

    fn push_gem(&self, view: &mut View<S>, index: usize) {
        let Some(tool) = &self.move_point else {
            return;
        };
        let (radius, style) = if self.selected == Some(index) {
            (self.radius * CHOSEN_GROWTH, &self.styles.chosen)
        } else {
            (self.radius, &self.styles.gem)
        };
        let space = DragSpace::plane(self.world(self.points[index]), Dir3::Y);
        let tool = tool.clone();
        view.push(
            ViewNode::sphere(self.id("point", index), radius)
                .with_parent(self.target.clone())
                .at(self.points[index])
                .with_style(style.clone())
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        across_plane(move |state: &S, offset| tool.edit.call(state, index, offset))
                            .snap(tool.snap),
                    )
                }),
        );
    }

    fn push_cone(&self, view: &mut View<S>, index: usize) {
        let Some(tool) = &self.raise_point else {
            return;
        };
        let lift = self.radius * CONE_LIFT;
        let local = self.points[index] + self.transform.rotation.inverse() * (Vec3::Y * lift);
        let space = DragSpace::axis(self.world(self.points[index]) + Vec3::Y * lift, Dir3::Y);
        let tool = tool.clone();
        view.push(
            ViewNode::cone(
                self.id("lift", index),
                self.radius * CONE_RADIUS,
                self.radius * CONE_HEIGHT,
            )
            .with_parent(self.target.clone())
            .at(local)
            .with_style(self.styles.gem.clone())
            .casts_shadow(false)
            .draggable_in(space, move || {
                let tool = tool.clone();
                DragSpec::from(
                    along_axis(move |state: &S, distance| tool.edit.call(state, index, distance))
                        .snap(tool.snap),
                )
            }),
        );
    }

    fn push_middle(&self, view: &mut View<S>, segment: usize, middle: Vec3) {
        let Some(tool) = &self.insert_point else {
            return;
        };
        let space = DragSpace::plane(self.world(middle), Dir3::Y);
        let tool = tool.clone();
        view.push(
            ViewNode::sphere(self.id("mid", segment), self.radius * MIDDLE_SIZE)
                .with_parent(self.target.clone())
                .at(middle)
                .with_style(self.styles.gem.clone())
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        across_plane(move |state: &S, offset| {
                            tool.edit.call(state, segment, offset)
                        })
                        .snap(tool.snap),
                    )
                }),
        );
    }

    fn push_width(&self, view: &mut View<S>, index: usize) {
        let Some((lateral, tool)) = &self.width else {
            return;
        };
        let Some(lateral) = lateral.get(index) else {
            return;
        };
        let right = lateral.right.normalize_or_zero();
        let Some(direction) = self.world_direction(right) else {
            return;
        };
        let local = self.points[index] + right * lateral.half_width;
        if self
            .facing(local, right)
            .is_some_and(|facing| facing > END_ON)
        {
            return;
        }
        let space = DragSpace::axis(self.world(local), direction);
        let tool = tool.clone();
        view.push(
            ViewNode::cuboid(
                self.id("width", index),
                Vec3::splat(self.radius * WIDTH_SIDE),
                0.3,
            )
            .with_parent(self.target.clone())
            .at(local)
            .with_style(self.styles.width.clone())
            .casts_shadow(false)
            .draggable_in(space, move || {
                let tool = tool.clone();
                DragSpec::from(
                    along_axis(move |state: &S, distance| tool.edit.call(state, index, distance))
                        .snap(tool.snap),
                )
            }),
        );
    }

    fn push_roll(&self, view: &mut View<S>, index: usize) {
        let Some((twist, tool)) = &self.roll else {
            return;
        };
        let Some(twist) = twist.get(index) else {
            return;
        };
        let axis = twist.axis.normalize_or_zero();
        let Some(direction) = self.world_direction(axis) else {
            return;
        };
        if self
            .facing(self.points[index], axis)
            .is_some_and(|facing| facing < EDGE_ON)
        {
            return;
        }
        let id = self.id("roll", index);
        let ring = ROLL_RING * self.radius;
        let facing = Quat::from_rotation_arc(Vec3::Y, axis);
        let space = DragSpace::plane(self.world(self.points[index]), direction);
        let tool = tool.clone();
        view.push(
            ViewNode::torus(id.clone(), ring, ROLL_TUBE * self.radius)
                .with_parent(self.target.clone())
                .with_transform(Transform {
                    translation: self.points[index],
                    rotation: facing,
                    scale: Vec3::ONE,
                })
                .with_style(self.styles.roll.clone())
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        around_axis(move |state: &S, angle| tool.edit.call(state, index, angle))
                            .snap(tool.snap),
                    )
                }),
        );
        let zero = (Vec3::Y - axis * axis.y)
            .try_normalize()
            .unwrap_or_else(|| axis.any_orthonormal_vector());
        let mark = Quat::from_axis_angle(axis, twist.angle) * zero * ring;
        view.push(
            ViewNode::sphere(format!("{id}/mark"), self.radius * ROLL_MARK)
                .with_parent(id)
                .at(facing.inverse() * mark)
                .with_style(self.styles.roll.clone())
                .casts_shadow(false),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::behavior::DragContext;
    use crate::space::Pointer;

    /// A road through control points, with a width and a roll at each of them.
    #[derive(Clone, Debug, PartialEq)]
    struct Road {
        points: Vec<Vec3>,
        half_widths: Vec<f32>,
        rolls: Vec<f32>,
        selected: Option<usize>,
        closed: bool,
    }

    fn road() -> Road {
        Road {
            points: vec![
                Vec3::new(0.0, 0.2, 0.0),
                Vec3::new(0.0, 0.2, 4.0),
                Vec3::new(2.0, 0.2, 8.0),
            ],
            half_widths: vec![1.0, 1.5, 2.0],
            rolls: vec![0.0, 0.0, 0.0],
            selected: None,
            closed: false,
        }
    }

    fn handles(road: &Road, transform: Transform) -> CurveHandles<Road> {
        let lateral: Vec<Lateral> = road
            .half_widths
            .iter()
            .map(|half_width| Lateral {
                right: Vec3::X,
                half_width: *half_width,
            })
            .collect();
        let twist: Vec<Twist> = road
            .rolls
            .iter()
            .map(|angle| Twist {
                axis: Vec3::Z,
                angle: *angle,
            })
            .collect();
        CurveHandles::new("road", transform, &road.points)
            .closed(road.closed)
            .selected(road.selected)
            .move_point(Snap::NONE, |road: &Road, index, offset| {
                let mut road = road.clone();
                road.points[index] += offset;
                road.selected = Some(index);
                road
            })
            .raise_point(Snap::step(0.5), |road: &Road, index, distance| {
                let mut road = road.clone();
                road.points[index].y += distance;
                road
            })
            .insert_point(Snap::NONE, |road: &Road, segment, offset| {
                let next = (segment + 1) % road.points.len();
                let middle = (road.points[segment] + road.points[next]) / 2.0;
                let mut road = road.clone();
                road.points.insert(segment + 1, middle + offset);
                road
            })
            .width(&lateral, Snap::step(0.1), |road: &Road, index, change| {
                let mut road = road.clone();
                road.half_widths[index] += change;
                road
            })
            .roll(
                &twist,
                Snap::step(FRAC_PI_2),
                |road: &Road, index, angle| {
                    let mut road = road.clone();
                    road.rolls[index] += angle;
                    road
                },
            )
    }

    fn view_at(road: &Road, transform: Transform) -> View<Road> {
        let mut view = View::new();
        view.push(
            ViewNode::cuboid("road", Vec3::new(1.0, 0.1, 8.0), 0.0).with_transform(transform),
        );
        handles(road, transform).add_to(&mut view);
        view
    }

    fn view(road: &Road) -> View<Road> {
        view_at(road, Transform::IDENTITY)
    }

    fn ids(view: &View<Road>) -> Vec<String> {
        view.nodes()
            .iter()
            .skip(1)
            .map(|node| node.id.to_string())
            .collect()
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    /// Drags `node` of the road with the spec of the node, from its anchor to `to(anchor)`.
    fn drag(road: &Road, node: &str, to: impl Fn(Vec3) -> Vec3) -> Road {
        let view = view(road);
        let node_ref = view.get(node).unwrap();
        let spec = node_ref.drag_spec(road).unwrap();
        let anchor = view.world_position(node).unwrap();
        let context = DragContext::new(self::view, node.to_owned().into(), road.clone())
            .with_space(node_ref.drag_space.unwrap());
        spec.compile(&context)(Pointer::at(to(anchor))).drop_state
    }

    #[test]
    fn every_point_has_a_gem_and_a_cone_and_every_segment_a_gem() {
        let ids = ids(&view(&road()));
        let expected = [
            "road/point-0",
            "road/lift-0",
            "road/point-1",
            "road/lift-1",
            "road/point-2",
            "road/lift-2",
            "road/mid-0",
            "road/mid-1",
        ];
        assert_eq!(ids, expected);
    }

    #[test]
    fn only_the_chosen_point_has_width_and_roll_handles() {
        let mut chosen = road();
        chosen.selected = Some(1);
        let view = view(&chosen);
        let ids = ids(&view);
        for id in ["road/width-1", "road/roll-1", "road/roll-1/mark"] {
            assert!(ids.iter().any(|found| found == id), "missing {id}");
        }
        assert!(
            ids.iter()
                .all(|id| !id.contains("width-0") && !id.contains("roll-2"))
        );
        assert_eq!(
            view.get("road/roll-1/mark").unwrap().parent.as_deref(),
            Some("road/roll-1")
        );
        for node in view.nodes().iter().skip(1) {
            if node.id.ends_with("/mark") {
                assert!(node.drag.is_none());
            } else {
                assert_eq!(node.parent.as_deref(), Some("road"), "{}", node.id);
                assert!(node.drag.is_some(), "{} is draggable", node.id);
                assert!(!node.casts_shadow);
            }
        }
        chosen.selected = Some(7);
        assert!(ids_without_extras(&chosen));
    }

    /// Whether a chosen point that does not exist adds no width or roll handle.
    fn ids_without_extras(road: &Road) -> bool {
        ids(&view(road))
            .iter()
            .all(|id| !id.contains("width") && !id.contains("roll"))
    }

    #[test]
    fn a_handle_is_added_only_for_a_tool_that_was_chosen() {
        let road = road();
        let mut view: View<Road> = View::new();
        CurveHandles::new("road", Transform::IDENTITY, &road.points)
            .raise_point(Snap::NONE, |road: &Road, _, _| road.clone())
            .add_to(&mut view);
        assert_eq!(view.len(), 3);
        assert!(view.nodes().iter().all(|node| node.id.contains("/lift-")));
        let mut none: View<Road> = View::new();
        CurveHandles::new("road", Transform::IDENTITY, &road.points).add_to(&mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn the_handles_sit_on_the_points_and_follow_the_target() {
        let mut chosen = road();
        chosen.selected = Some(1);
        chosen.rolls[1] = FRAC_PI_2;
        let transform =
            Transform::from_xyz(10.0, 0.0, -3.0).with_rotation(Quat::from_rotation_y(FRAC_PI_2));
        let view = view_at(&chosen, transform);
        let world = |id: &str| view.world_position(id).unwrap();
        let point = |index: usize| transform.transform_point(chosen.points[index]);
        assert!(near(world("road/point-1"), point(1)));
        assert!(near(
            world("road/lift-1"),
            point(1) + Vec3::Y * GEM_RADIUS * CONE_LIFT
        ));
        let middle = (point(0) + point(1)) / 2.0;
        assert!(near(world("road/mid-0"), middle));
        let right = transform.rotation * Vec3::X;
        assert!(near(world("road/width-1"), point(1) + right * 1.5));
        assert!(near(world("road/roll-1"), point(1)));
        let space = |id: &str| view.get(id).unwrap().drag_space.unwrap();
        assert_eq!(space("road/point-1"), DragSpace::plane(point(1), Dir3::Y));
        assert_eq!(
            space("road/lift-1"),
            DragSpace::axis(point(1) + Vec3::Y * GEM_RADIUS * CONE_LIFT, Dir3::Y)
        );
        assert_eq!(space("road/mid-0"), DragSpace::plane(middle, Dir3::Y));
        let along = Dir3::new(right).unwrap();
        assert_eq!(
            space("road/width-1"),
            DragSpace::axis(point(1) + right * 1.5, along)
        );
        let tangent = Dir3::new(transform.rotation * Vec3::Z).unwrap();
        assert_eq!(space("road/roll-1"), DragSpace::plane(point(1), tangent));
    }

    #[test]
    fn the_mark_of_the_roll_ring_shows_the_current_roll() {
        let mut chosen = road();
        chosen.selected = Some(1);
        let ring = ROLL_RING * GEM_RADIUS;
        let at = |angle: f32| {
            let mut rolled = chosen.clone();
            rolled.rolls[1] = angle;
            let view = view(&rolled);
            view.world_position("road/roll-1/mark").unwrap() - rolled.points[1]
        };
        assert!(
            near(at(0.0), Vec3::Y * ring),
            "rolled flat the mark faces up"
        );
        assert!(near(at(FRAC_PI_2), Vec3::new(-ring, 0.0, 0.0)));
    }

    #[test]
    fn dragging_a_gem_moves_and_chooses_its_point() {
        let dropped = drag(&road(), "road/point-1", |anchor| {
            anchor + Vec3::new(0.5, 3.0, 0.25)
        });
        assert!(near(
            dropped.points[1],
            road().points[1] + Vec3::new(0.5, 0.0, 0.25)
        ));
        assert_eq!(dropped.selected, Some(1));
        assert_eq!(dropped.points[0], road().points[0]);
    }

    #[test]
    fn dragging_a_cone_raises_its_point_in_steps() {
        let dropped = drag(&road(), "road/lift-2", |anchor| {
            anchor + Vec3::new(2.0, 0.52, 0.0)
        });
        assert!(
            (dropped.points[2].y - 0.7).abs() < 1e-4,
            "{}",
            dropped.points[2]
        );
        assert_eq!(dropped.points[2].x, 2.0, "off the axis is ignored");
    }

    #[test]
    fn dragging_a_middle_gem_inserts_a_point_between_its_neighbours() {
        let dropped = drag(&road(), "road/mid-0", |anchor| {
            anchor + Vec3::new(0.5, 0.0, 0.25)
        });
        assert_eq!(dropped.points.len(), 4);
        assert!(near(dropped.points[1], Vec3::new(0.5, 0.2, 2.25)));
        assert_eq!(dropped.points[2], road().points[1]);
        let view = self::view(&dropped);
        assert!(view.get("road/point-3").is_some() && view.get("road/mid-2").is_some());
    }

    #[test]
    fn dragging_the_width_handle_outwards_widens_by_snapped_steps() {
        let mut chosen = road();
        chosen.selected = Some(1);
        let wider = drag(&chosen, "road/width-1", |anchor| {
            anchor + Vec3::new(0.31, 4.0, 4.0)
        });
        assert!(
            (wider.half_widths[1] - 1.8).abs() < 1e-4,
            "{}",
            wider.half_widths[1]
        );
        let narrower = drag(&chosen, "road/width-1", |anchor| anchor - Vec3::X * 0.52);
        assert!(
            (narrower.half_widths[1] - 1.0).abs() < 1e-4,
            "{}",
            narrower.half_widths[1]
        );
        assert_eq!(wider.half_widths[0], 1.0);
    }

    #[test]
    fn dragging_the_roll_ring_rolls_in_quarter_turns() {
        let mut chosen = road();
        chosen.selected = Some(1);
        let node = "road/roll-1";
        let view = view(&chosen);
        let node_ref = view.get(node).unwrap();
        let spec = node_ref.drag_spec(&chosen).unwrap();
        let center = chosen.points[1];
        let ring = ROLL_RING * GEM_RADIUS;
        let grab = Vec3::new(ring, 0.0, 0.0);
        let context = DragContext::new(self::view, node.to_owned().into(), chosen.clone())
            .with_space(node_ref.drag_space.unwrap())
            .with_grab_offset(grab);
        let mut behavior = spec.compile(&context);
        let angle = 80_f32.to_radians();
        let pointer = center + Vec3::new(angle.cos(), angle.sin(), 0.0) * ring;
        let dropped = behavior(Pointer::at(pointer - grab)).drop_state;
        assert!(
            (dropped.rolls[1] - FRAC_PI_2).abs() < 1e-3,
            "{}",
            dropped.rolls[1]
        );
        assert_eq!(dropped.rolls[0], 0.0);
    }

    #[test]
    fn a_closed_curve_has_a_segment_back_to_its_first_point() {
        let mut ring = road();
        ring.closed = true;
        let ids = ids(&view(&ring));
        assert!(ids.iter().any(|id| id == "road/mid-2"));
        let view = view(&ring);
        let wrapped = view.world_position("road/mid-2").unwrap();
        assert!(near(wrapped, (ring.points[2] + ring.points[0]) / 2.0));
        let dropped = drag(&ring, "road/mid-2", |anchor| anchor + Vec3::Z);
        assert_eq!(dropped.points.len(), 4, "the point is added at the end");
    }

    #[test]
    fn middles_of_a_bent_curve_can_be_given() {
        let road = road();
        let bent = [Vec3::new(1.0, 0.2, 2.0), Vec3::new(2.0, 0.2, 6.0)];
        let mut view: View<Road> = View::new();
        CurveHandles::new("road", Transform::IDENTITY, &road.points)
            .with_middles(&bent)
            .insert_point(Snap::NONE, |road: &Road, _, _| road.clone())
            .add_to(&mut view);
        assert!(near(view.world_position("road/mid-0").unwrap(), bent[0]));
        assert!(near(view.world_position("road/mid-1").unwrap(), bent[1]));
    }

    #[test]
    fn the_radius_sizes_the_handles_and_the_chosen_gem_is_larger() {
        let mut chosen = road();
        chosen.selected = Some(0);
        let size =
            |view: &View<Road>, id: &str| view.get(id).unwrap().geometry.bounds().half_extents.x;
        let view = view(&chosen);
        assert!(size(&view, "road/point-0") > size(&view, "road/point-1") * 1.2);
        assert!(size(&view, "road/mid-0") < size(&view, "road/point-1"));
        let mut small: View<Road> = View::new();
        CurveHandles::new("road", Transform::IDENTITY, &chosen.points)
            .with_radius(0.05)
            .move_point(Snap::NONE, |road: &Road, _, _| road.clone())
            .add_to(&mut small);
        assert!(size(&small, "road/point-1") < size(&view, "road/point-1"));
    }

    #[test]
    fn handles_that_cannot_be_dragged_from_the_camera_are_left_out() {
        let mut road = road();
        road.selected = Some(1);
        let point = road.points[1];
        let seen_from = |eye: Vec3| {
            let mut view = View::new();
            handles(&road, Transform::IDENTITY)
                .seen_from(eye)
                .add_to(&mut view);
            view.nodes()
                .iter()
                .map(|node| node.id.to_string())
                .collect::<Vec<_>>()
        };
        let has = |ids: &[String], id: &str| ids.iter().any(|found| found == id);

        let along_z = seen_from(point + Vec3::Z * 10.0);
        assert!(has(&along_z, "road/roll-1") && has(&along_z, "road/width-1"));
        let along_x = seen_from(point + Vec3::X * 10.0);
        assert!(!has(&along_x, "road/roll-1"), "the ring is edge-on");
        assert!(!has(&along_x, "road/width-1"), "the width axis is end-on");
        let above = seen_from(point + Vec3::Y * 10.0);
        assert!(!has(&above, "road/roll-1"), "the ring is edge-on");
        assert!(has(&above, "road/width-1"));
        assert!(has(&above, "road/point-1") && has(&above, "road/lift-1"));
    }

    #[test]
    fn the_footprint_of_a_point_is_where_its_handles_are() {
        let mut road = road();
        let count = |road: &Road, index: usize| handles(road, Transform::IDENTITY).footprint(index);
        assert_eq!(count(&road, 0).len(), 3, "gem, cone and one segment gem");
        assert_eq!(count(&road, 1).len(), 4, "and the other segment gem");
        assert_eq!(count(&road, 9), Vec::<Vec3>::new());

        road.selected = Some(1);
        let spots = count(&road, 1);
        assert_eq!(
            spots.len(),
            4 + 1 + RING_SAMPLES,
            "the width handle and the ring"
        );
        assert!(near(spots[0], road.points[1]));
        let width = road.points[1] + Vec3::X * road.half_widths[1];
        assert!(spots.iter().any(|spot| near(*spot, width)));
        let ring = ROLL_RING * GEM_RADIUS;
        let around = spots
            .iter()
            .filter(|spot| (spot.distance(road.points[1]) - ring).abs() < 1e-3);
        assert_eq!(around.count(), RING_SAMPLES);
    }
}
