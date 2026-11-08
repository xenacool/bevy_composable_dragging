//! Transform gizmos: translate, rotate and scale handles built from ordinary nodes.
//!
//! A [`Gizmo`] adds its handles to a [`View`] as nodes parented to a target node, so they blend,
//! fade and snap like everything else. Each handle is draggable in its own axis or plane space
//! and uses the value operators ([`along_axis`], [`around_axis`], [`across_plane`]) with the
//! [`Snap`] you configure, so the preview is pulled onto the steps and a drop commits them.
//!
//! The handles are children of the target, so the target should not be scaled non-uniformly
//! through its transform (give the shape its size instead), or the handles would stretch with it.
//!
//! An editor usually shows the gizmo of the selection only while the chosen tool transforms it,
//! so the others leave the view uncluttered. A state says so by implementing [`GizmoReveal`],
//! and [`Gizmo::add_if_revealed`] adds the handles only then.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

use bevy::math::{Dir3, Quat, Vec3};
use bevy::prelude::{Color, Transform};
use bevy_gradmesh::Style;

use crate::behavior::{DragContext, Frame};
use crate::ops::{Snap, across_plane, along_axis, around_axis};
use crate::space::DragSpace;
use crate::spec::DragSpec;
use crate::view::{NodeId, View, ViewNode};

/// The length of the shaft of a translate arrow, and the distance a scale handle travels to
/// double the scale.
const ARM: f32 = 1.0;
/// The radius of the ring of a rotate handle.
const RING: f32 = 0.9;
/// How many points [`Gizmo::footprint`] takes around a ring.
const RING_SAMPLES: usize = 12;
/// The sides of the square of a plane handle, and how far from the axes it sits.
const PLANE_SIDE: f32 = 0.3;
const PLANE_OFFSET: f32 = 0.35;
/// The radius of the widest part of a handle, the cone of an arrow, which is how far the
/// handles have to be above the ground not to sink into it.
const HANDLE_RADIUS: f32 = 0.11;
/// How much darker the underside of a handle is.
const SHADE: f32 = 0.6;
/// How far the uniform scale handle sits from the pivot along each axis.
const UNIFORM_OFFSET: f32 = 0.25;
/// A plane or a ring whose normal is this close to square with the view is seen edge-on, which
/// makes a drag across it jump, so [`Gizmo::seen_from`] leaves it out.
pub const EDGE_ON: f32 = 0.2;
/// An arrow that points this much at the eye is seen end-on and cannot be dragged along its
/// length, so [`Gizmo::seen_from`] leaves it out.
pub const END_ON: f32 = 0.95;

/// Re-exported from [`crate::view::GIZMO_LAYER`] so that code importing from this module
/// continues to work.
pub use crate::view::GIZMO_LAYER;

/// One of the three axes of the target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// The local X axis, shown in red.
    X,
    /// The local Y axis, shown in green.
    Y,
    /// The local Z axis, shown in blue.
    Z,
}

impl Axis {
    const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }

    fn unit(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
        }
    }

    /// The color of handles of this axis: red, green and blue.
    fn color(self) -> Color {
        match self {
            Self::X => Color::srgb(0.92, 0.26, 0.26),
            Self::Y => Color::srgb(0.3, 0.8, 0.32),
            Self::Z => Color::srgb(0.3, 0.46, 0.96),
        }
    }

    /// Turns the Y axis of a shape (the way cylinders and cones point) onto this axis.
    fn y_onto(self) -> Quat {
        match self {
            Self::X => Quat::from_rotation_z(-FRAC_PI_2),
            Self::Y => Quat::IDENTITY,
            Self::Z => Quat::from_rotation_x(FRAC_PI_2),
        }
    }

    /// Turns the Y axis, the axis of a torus, onto this axis.
    fn ring(self) -> Quat {
        match self {
            Self::X => Quat::from_rotation_z(FRAC_PI_2),
            Self::Y => Quat::IDENTITY,
            Self::Z => Quat::from_rotation_x(FRAC_PI_2),
        }
    }

    /// Turns the Z axis, the way a flat shape faces, onto this axis.
    fn z_onto(self) -> Quat {
        match self {
            Self::X => Quat::from_rotation_y(FRAC_PI_2),
            Self::Y => Quat::from_rotation_x(-FRAC_PI_2),
            Self::Z => Quat::IDENTITY,
        }
    }

    /// The other two axes.
    fn others(self) -> [Self; 2] {
        match self {
            Self::X => [Self::Y, Self::Z],
            Self::Y => [Self::X, Self::Z],
            Self::Z => [Self::X, Self::Y],
        }
    }
}

/// The state of an interface that shows the gizmo of its selection only for some tools.
///
/// Implement it for the state, answering from the chosen tool, and add the gizmo with
/// [`Gizmo::add_if_revealed`], so that every element of the editor follows the same rule.
pub trait GizmoReveal {
    /// Whether the chosen tool transforms the selection, which shows its gizmo.
    fn reveals_gizmo(&self) -> bool;
}

/// Turns the state a drag started from and a measured value into a new state.
type Apply<S, V> = Arc<dyn Fn(&S, V) -> S + Send + Sync>;

/// What a handle does with a measured value.
struct Tool<S, V> {
    snap: Snap,
    apply: Apply<S, V>,
}

impl<S, V> Clone for Tool<S, V> {
    fn clone(&self) -> Self {
        Self {
            snap: self.snap,
            apply: Arc::clone(&self.apply),
        }
    }
}

/// A set of handles around a target node.
///
/// Build it with [`new`](Self::new), choose the tools, and [`add_to`](Self::add_to) a view. The
/// handles have the ids `<target>/gizmo/<kind>-<axis>` (`translate-x`, `translate-xy`,
/// `rotate-z`, `scale-y`, `scale-uniform`, ...), and the arrow heads of the translate handles are
/// the children `<id>/tip`.
pub struct Gizmo<S> {
    target: NodeId,
    transform: Transform,
    translate: Option<Tool<S, Vec3>>,
    lift: Option<Tool<S, Vec3>>,
    rotate: Option<Tool<S, Quat>>,
    scale: Option<Tool<S, Vec3>>,
    scale_uniform: Option<Tool<S, f32>>,
    size: f32,
    rings: Vec<Axis>,
    eye: Option<Vec3>,
    raise: Option<f32>,
}

impl<S: Clone + Send + Sync + 'static> Gizmo<S> {
    /// A gizmo for the node `target`, which is at the world `transform`. The axes of the handles
    /// are the local axes of the target.
    pub fn new(target: impl Into<NodeId>, transform: Transform) -> Self {
        Self {
            target: target.into(),
            transform,
            translate: None,
            lift: None,
            rotate: None,
            scale: None,
            scale_uniform: None,
            size: 1.0,
            rings: Axis::ALL.to_vec(),
            eye: None,
            raise: None,
        }
    }

    /// Lifts every handle, and the point its drag is measured from, by `height` along the local
    /// Y axis of the target, for a target whose pivot is at its foot: its body hides handles at
    /// the pivot. Pass half the height of the target to center the gizmo on it. However little
    /// it is, a raised gizmo is lifted by the width of its handles, so that those of a target that
    /// lies flat on the ground do not sink into it. A gizmo that is not raised stays at the pivot.
    #[must_use]
    pub fn raised(mut self, height: f32) -> Self {
        self.raise = Some(height);
        self
    }

    /// Limits the rings of [`rotate`](Self::rotate) to the given axes, for a target that only
    /// turns about some of them, such as a piece that stands on the ground and turns about the
    /// vertical axis alone. The default is a ring for every axis.
    #[must_use]
    pub fn rotate_axes(mut self, axes: &[Axis]) -> Self {
        self.rings = axes.to_vec();
        self
    }

    /// Tells the gizmo where the camera is, in the world, so that it leaves out the handles that
    /// cannot be dragged from there: squares and rings seen edge-on, where the pointer moves
    /// the target wildly or not at all, and arrows and scale cubes pointing straight at the
    /// camera. Without it every handle is added, whatever the view.
    #[must_use]
    pub fn seen_from(mut self, eye: Vec3) -> Self {
        self.eye = Some(eye);
        self
    }

    /// Sets how big the handles are: every length of the gizmo is multiplied by `size`, so an
    /// editor can make the handles reach clear of what they edit and keep them in reach of the
    /// camera. A scale handle still grows the scale by one for every unit of its length. The
    /// default is one world unit for an arrow.
    #[must_use]
    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size.max(f32::EPSILON);
        self
    }

    /// Adds translate handles: an arrow for each axis and a square for each plane. `apply` gets
    /// the state the drag started from and the world offset to move the target by, snapped to
    /// steps of `snap` (in world units) along each axis.
    #[must_use]
    pub fn translate(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, Vec3) -> S + Send + Sync + 'static,
    ) -> Self {
        self.translate = Some(Tool {
            snap,
            apply: Arc::new(apply),
        });
        self
    }

    /// Adds a single translate arrow, the vertical one, for editors that only raise and lower.
    /// `apply` gets the state the drag started from and the height to move the target by,
    /// snapped to steps of `snap`. It replaces [`translate`](Self::translate) rather than
    /// adding to it, as both would make the same handle.
    #[must_use]
    pub fn lift(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, f32) -> S + Send + Sync + 'static,
    ) -> Self {
        self.lift = Some(Tool {
            snap,
            apply: Arc::new(move |state: &S, offset: Vec3| apply(state, offset.y)),
        });
        self
    }

    /// Adds rotate handles: a ring around each axis. `apply` gets the state the drag started
    /// from and the rotation to compose with the rotation of the target, with the angle snapped
    /// to steps of `snap` in radians.
    #[must_use]
    pub fn rotate(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, Quat) -> S + Send + Sync + 'static,
    ) -> Self {
        self.rotate = Some(Tool {
            snap,
            apply: Arc::new(apply),
        });
        self
    }

    /// Adds scale handles: a cube at the end of each axis. `apply` gets the state the drag
    /// started from and the factor to multiply the scale of the target by, which is one on the
    /// other axes and grows by one for every unit the handle is dragged along its axis. `snap`
    /// steps are in that unit.
    #[must_use]
    pub fn scale(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, Vec3) -> S + Send + Sync + 'static,
    ) -> Self {
        self.scale = Some(Tool {
            snap,
            apply: Arc::new(apply),
        });
        self
    }

    /// Adds one scale handle, a cube beside the pivot, that scales all three axes together.
    /// `apply` gets the state the drag started from and the factor to multiply the scale of the
    /// target by. The handle is dragged in the plane that faces the camera, and the factor grows
    /// by one for every unit the pointer moves away from the pivot and shrinks as it moves
    /// towards it, so it can reach zero and below: clamp it in `apply` if the scale must stay
    /// positive. `snap` steps are in that unit.
    #[must_use]
    pub fn scale_uniform(
        mut self,
        snap: Snap,
        apply: impl Fn(&S, f32) -> S + Send + Sync + 'static,
    ) -> Self {
        self.scale_uniform = Some(Tool {
            snap,
            apply: Arc::new(apply),
        });
        self
    }

    /// Adds the handles to `view` as children of the target.
    pub fn add_to(self, view: &mut View<S>) {
        for axis in Axis::ALL {
            if let Some(tool) = &self.translate {
                if !self.end_on(axis) {
                    self.push_arrow(view, axis, tool);
                }
                if !self.edge_on(axis) {
                    self.push_plane(view, axis, tool);
                }
            }
            if let Some(tool) = &self.rotate
                && self.rings.contains(&axis)
                && !self.edge_on(axis)
            {
                self.push_ring(view, axis, tool);
            }
            if let Some(tool) = &self.scale
                && !self.end_on(axis)
            {
                self.push_cube(view, axis, tool);
            }
        }
        if let Some(tool) = &self.scale_uniform {
            self.push_uniform(view, tool);
        }
        if self.translate.is_none()
            && let Some(tool) = &self.lift
            && !self.end_on(Axis::Y)
        {
            self.push_arrow(view, Axis::Y, tool);
        }
    }

    /// Adds the handles to `view` if `state` [reveals](GizmoReveal::reveals_gizmo) the gizmo,
    /// and nothing otherwise.
    pub fn add_if_revealed(self, view: &mut View<S>, state: &S)
    where
        S: GizmoReveal,
    {
        if state.reveals_gizmo() {
            self.add_to(view);
        }
    }

    /// How much the local `axis` of the target points at the eye: one when straight at it, zero
    /// when square with the view. `None` when the eye is not known or is at the pivot.
    fn facing(&self, axis: Axis) -> Option<f32> {
        let view = (self.eye? - self.transform.translation).try_normalize()?;
        Some(view.dot(*self.world_axis(axis)).abs())
    }

    /// Whether the plane or ring around `axis` is seen edge-on.
    fn edge_on(&self, axis: Axis) -> bool {
        self.facing(axis).is_some_and(|facing| facing < EDGE_ON)
    }

    /// Whether the arrow along `axis` is seen end-on.
    fn end_on(&self, axis: Axis) -> bool {
        self.facing(axis).is_some_and(|facing| facing > END_ON)
    }

    /// Where the handles are centered, in the local frame of the target.
    fn raise_offset(&self) -> Vec3 {
        self.raise.map_or(Vec3::ZERO, |height| {
            Vec3::Y * height.max(HANDLE_RADIUS * self.size)
        })
    }

    /// Where the handles are centered, in the world.
    fn origin(&self) -> Vec3 {
        self.transform.translation + self.transform.rotation * self.raise_offset()
    }

    /// The points of the world that the handles occupy: the pivot, along each arrow, in each
    /// plane handle and around each ring. A pop-up that has to leave the handles reachable keeps
    /// clear of these.
    #[must_use]
    pub fn footprint(&self) -> Vec<Vec3> {
        let size = self.size;
        let world = |point: Vec3| {
            self.transform.translation
                + self.transform.rotation * (self.raise_offset() + point * size)
        };
        let mut spots = vec![world(Vec3::ZERO)];
        for axis in Axis::ALL {
            spots.push(world(axis.unit() * ARM / 2.0));
            spots.push(world(axis.unit() * ARM));
        }
        for (first, second) in [(Axis::X, Axis::Y), (Axis::X, Axis::Z), (Axis::Y, Axis::Z)] {
            spots.push(world((first.unit() + second.unit()) * PLANE_OFFSET));
        }
        for axis in &self.rings {
            let normal = axis.unit();
            let start = normal.any_orthonormal_vector() * RING;
            for step in 0..RING_SAMPLES {
                let angle = TAU * step as f32 / RING_SAMPLES as f32;
                spots.push(world(Quat::from_axis_angle(normal, angle) * start));
            }
        }
        spots
    }

    fn id(&self, kind: &str) -> String {
        format!("{}/gizmo/{kind}", self.target)
    }

    /// The world direction of a local axis of the target.
    fn world_axis(&self, axis: Axis) -> Dir3 {
        Dir3::new(self.transform.rotation * axis.unit()).unwrap_or(Dir3::X)
    }

    fn style(axis: Axis) -> Style {
        Style::shaded(axis.color(), SHADE)
    }

    fn push_arrow(&self, view: &mut View<S>, axis: Axis, tool: &Tool<S, Vec3>) {
        let id = self.id(&format!("translate-{}", axis.name()));
        let direction = self.world_axis(axis);
        let space = DragSpace::axis(self.origin(), direction);
        let tool = tool.clone();
        let size = self.size;
        view.push(
            ViewNode::cylinder(id.clone(), 0.045 * size, ARM * size)
                .with_parent(self.target.clone())
                .on_layer(GIZMO_LAYER)
                .with_transform(Transform {
                    translation: self.raise_offset() + axis.unit() * ARM * size / 2.0,
                    rotation: axis.y_onto(),
                    scale: Vec3::ONE,
                })
                .with_style(Self::style(axis))
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        along_axis(move |state: &S, distance| {
                            (tool.apply)(state, *direction * distance)
                        })
                        .snap(tool.snap),
                    )
                }),
        );
        view.push(
            ViewNode::cone(format!("{id}/tip"), HANDLE_RADIUS * size, 0.28 * size)
                .with_parent(id)
                .on_layer(GIZMO_LAYER)
                .at(Vec3::new(0.0, (ARM / 2.0 + 0.14) * size, 0.0))
                .with_style(Self::style(axis))
                .casts_shadow(false),
        );
    }

    /// The square that drags in the plane spanned by the two axes other than `normal`.
    fn push_plane(&self, view: &mut View<S>, normal: Axis, tool: &Tool<S, Vec3>) {
        let [a, b] = normal.others();
        let id = self.id(&format!("translate-{}{}", a.name(), b.name()));
        let space = DragSpace::plane(self.origin(), self.world_axis(normal));
        let tool = tool.clone();
        let size = self.size;
        view.push(
            ViewNode::cuboid(id, Vec3::new(PLANE_SIDE, PLANE_SIDE, 0.02) * size, 0.0)
                .with_parent(self.target.clone())
                .on_layer(GIZMO_LAYER)
                .with_transform(Transform {
                    translation: self.raise_offset() + (a.unit() + b.unit()) * PLANE_OFFSET * size,
                    rotation: normal.z_onto(),
                    scale: Vec3::ONE,
                })
                .with_style(Self::style(normal))
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        across_plane(move |state: &S, offset| (tool.apply)(state, offset))
                            .snap(tool.snap),
                    )
                }),
        );
    }

    fn push_ring(&self, view: &mut View<S>, axis: Axis, tool: &Tool<S, Quat>) {
        let id = self.id(&format!("rotate-{}", axis.name()));
        let direction = self.world_axis(axis);
        let space = DragSpace::plane(self.origin(), direction);
        let tool = tool.clone();
        view.push(
            ViewNode::torus(id, RING * self.size, 0.025 * self.size)
                .with_parent(self.target.clone())
                .on_layer(GIZMO_LAYER)
                .with_transform(Transform {
                    translation: self.raise_offset(),
                    rotation: axis.ring(),
                    scale: Vec3::ONE,
                })
                .with_style(Self::style(axis))
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        around_axis(move |state: &S, angle| {
                            (tool.apply)(state, Quat::from_axis_angle(*direction, angle))
                        })
                        .snap(tool.snap),
                    )
                }),
        );
    }

    fn push_cube(&self, view: &mut View<S>, axis: Axis, tool: &Tool<S, Vec3>) {
        let id = self.id(&format!("scale-{}", axis.name()));
        let space = DragSpace::axis(self.origin(), self.world_axis(axis));
        let tool = tool.clone();
        let unit = axis.unit();
        let size = self.size;
        view.push(
            ViewNode::cuboid(id, Vec3::splat(0.14 * size), 0.25 * size)
                .with_parent(self.target.clone())
                .on_layer(GIZMO_LAYER)
                .at(self.raise_offset() + unit * (ARM + 0.2) * size)
                .with_style(Self::style(axis))
                .casts_shadow(false)
                .draggable_in(space, move || {
                    let tool = tool.clone();
                    DragSpec::from(
                        along_axis(move |state: &S, distance| {
                            (tool.apply)(state, Vec3::ONE + unit * distance / size)
                        })
                        .snap(Snap {
                            step: tool.snap.step * size,
                            radius: tool.snap.radius * size,
                        }),
                    )
                }),
        );
    }

    fn push_uniform(&self, view: &mut View<S>, tool: &Tool<S, f32>) {
        let pivot = self.origin();
        let tool = tool.clone();
        let size = self.size;
        view.push(
            ViewNode::cuboid(
                self.id("scale-uniform"),
                Vec3::splat(0.2 * size),
                0.25 * size,
            )
            .with_parent(self.target.clone())
            .on_layer(GIZMO_LAYER)
            .at(self.raise_offset() + Vec3::splat(UNIFORM_OFFSET * size))
            .with_style(Style::shaded(Color::srgb(0.9, 0.9, 0.9), SHADE))
            .casts_shadow(false)
            .draggable_in(DragSpace::screen(), move || {
                uniform_spec(pivot, size, tool.clone())
            }),
        );
    }
}

/// The drag of the uniform scale handle: the distance of the pointer from `pivot`, measured in
/// the plane of the drag, relative to where the drag started.
fn uniform_spec<S: Clone + Send + Sync + 'static>(
    pivot: Vec3,
    size: f32,
    tool: Tool<S, f32>,
) -> DragSpec<S> {
    DragSpec::new(move |context: &DragContext<S>| {
        let state = context.start_state().clone();
        let space = *context.space();
        let center = space.project_point(pivot);
        let grab = context.grab_offset();
        let anchor = context
            .anchor_in(&context.render(&state))
            .map_or(center, |anchor| space.project_point(anchor));
        let start = (anchor + grab - center).length();
        let tool = tool.clone();
        let context = context.clone();
        Box::new(move |pointer| {
            let raw = ((pointer.point + grab - center).length() - start) / size;
            let (shown, committed) = tool.snap.apply(raw);
            Frame::new(
                context.render(&(tool.apply)(&state, 1.0 + shown)),
                (tool.apply)(&state, 1.0 + committed),
                0.0,
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::space::Pointer;

    /// A cube with a position, a rotation and a scale.
    #[derive(Clone, Debug, PartialEq)]
    struct Cube {
        transform: Transform,
    }

    fn cube_view(cube: &Cube) -> View<Cube> {
        let mut view = View::new();
        view.push(ViewNode::cuboid("cube", Vec3::ONE, 0.1).with_transform(cube.transform));
        Gizmo::new("cube", cube.transform)
            .translate(Snap::step(0.5), |cube: &Cube, offset| Cube {
                transform: Transform {
                    translation: cube.transform.translation + offset,
                    ..cube.transform
                },
            })
            .rotate(Snap::step(FRAC_PI_2), |cube: &Cube, rotation| Cube {
                transform: Transform {
                    rotation: rotation * cube.transform.rotation,
                    ..cube.transform
                },
            })
            .scale(Snap::step(0.25), |cube: &Cube, factor| Cube {
                transform: Transform {
                    scale: cube.transform.scale * factor,
                    ..cube.transform
                },
            })
            .scale_uniform(Snap::step(0.25), |cube: &Cube, factor| Cube {
                transform: Transform {
                    scale: cube.transform.scale * factor,
                    ..cube.transform
                },
            })
            .add_to(&mut view);
        view
    }

    fn start() -> Cube {
        Cube {
            transform: Transform::from_xyz(1.0, 2.0, 3.0),
        }
    }

    fn ids(view: &View<Cube>) -> Vec<String> {
        view.nodes()
            .iter()
            .map(|node| node.id.to_string())
            .collect()
    }

    #[test]
    fn handles_have_ids_parents_and_spaces() {
        let view = cube_view(&start());
        let ids = ids(&view);
        for expected in [
            "cube/gizmo/translate-x",
            "cube/gizmo/translate-y",
            "cube/gizmo/translate-z",
            "cube/gizmo/translate-yz",
            "cube/gizmo/translate-xz",
            "cube/gizmo/translate-xy",
            "cube/gizmo/rotate-x",
            "cube/gizmo/rotate-y",
            "cube/gizmo/rotate-z",
            "cube/gizmo/scale-x",
            "cube/gizmo/scale-y",
            "cube/gizmo/scale-z",
            "cube/gizmo/scale-uniform",
            "cube/gizmo/translate-x/tip",
        ] {
            assert!(ids.iter().any(|id| id == expected), "missing {expected}");
        }
        for node in view
            .nodes()
            .iter()
            .filter(|node| node.id.contains("/gizmo/"))
        {
            let is_tip = node.id.ends_with("/tip");
            let parent = node.parent.as_deref().unwrap();
            if is_tip {
                assert!(parent.starts_with("cube/gizmo/translate-"), "{parent}");
                assert!(node.drag.is_none());
            } else {
                assert_eq!(parent, "cube");
                assert!(node.drag.is_some(), "{} is draggable", node.id);
                assert!(!node.casts_shadow);
            }
        }
        let origin = Vec3::new(1.0, 2.0, 3.0);
        let space = |id: &str| view.get(id).unwrap().drag_space.unwrap();
        assert_eq!(
            space("cube/gizmo/translate-x"),
            DragSpace::axis(origin, Dir3::X)
        );
        assert_eq!(
            space("cube/gizmo/scale-z"),
            DragSpace::axis(origin, Dir3::Z)
        );
        assert_eq!(space("cube/gizmo/scale-uniform"), DragSpace::screen());
        assert_eq!(
            space("cube/gizmo/rotate-y"),
            DragSpace::plane(origin, Dir3::Y)
        );
        assert_eq!(
            space("cube/gizmo/translate-xy"),
            DragSpace::plane(origin, Dir3::Z)
        );
        assert!(view.get("cube").unwrap().drag.is_none());
    }

    #[test]
    fn only_the_chosen_tools_add_handles() {
        let mut view: View<Cube> = View::new();
        Gizmo::new("cube", Transform::IDENTITY)
            .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .add_to(&mut view);
        assert_eq!(view.len(), 3);
        assert!(ids(&view).iter().all(|id| id.contains("rotate-")));
    }

    #[test]
    fn the_axes_follow_the_rotation_of_the_target() {
        let turned = Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2));
        let mut view: View<Cube> = View::new();
        Gizmo::new("cube", turned)
            .translate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .add_to(&mut view);
        let DragSpace::Axis { direction, .. } = view
            .get("cube/gizmo/translate-x")
            .unwrap()
            .drag_space
            .unwrap()
        else {
            panic!("an axis space");
        };
        assert!((*direction - Vec3::Y).length() < 1e-5, "{direction:?}");
    }

    fn context(cube: Cube, handle: &str, space: DragSpace) -> DragContext<Cube> {
        DragContext::new(Arc::new(cube_view), handle.to_owned().into(), cube).with_space(space)
    }

    fn drag(handle: &str, pointer: Pointer) -> Cube {
        let cube = start();
        let view = cube_view(&cube);
        let node = view.get(handle).unwrap();
        let spec = (node.drag.as_ref().unwrap())();
        let context = context(cube, handle, node.drag_space.unwrap());
        spec.compile(&context)(pointer).drop_state
    }

    #[test]
    fn dragging_a_translate_arrow_commits_a_snapped_offset() {
        let handle = "cube/gizmo/translate-x";
        // The shaft sits at x = 1.5 (its center); the pointer moves 0.8 along the axis.
        let dropped = drag(handle, Pointer::at(Vec3::new(2.3, 2.0, 3.0)));
        assert!(
            dropped
                .transform
                .translation
                .distance(Vec3::new(2.0, 2.0, 3.0))
                < 1e-4,
            "{:?}",
            dropped.transform.translation
        );
    }

    #[test]
    fn dragging_a_plane_square_moves_in_both_axes_of_the_plane() {
        let handle = "cube/gizmo/translate-xy";
        let anchor = Vec3::new(1.0 + PLANE_OFFSET, 2.0 + PLANE_OFFSET, 3.0);
        let dropped = drag(handle, Pointer::at(anchor + Vec3::new(1.1, -0.6, 0.0)));
        let moved = dropped.transform.translation - Vec3::new(1.0, 2.0, 3.0);
        assert!(moved.distance(Vec3::new(1.0, -0.5, 0.0)) < 1e-4, "{moved}");
    }

    #[test]
    fn dragging_a_ring_rotates_in_quarter_turns() {
        let handle = "cube/gizmo/rotate-z";
        let center = Vec3::new(1.0, 2.0, 3.0);
        let grab = center + Vec3::new(RING, 0.0, 0.0);
        let cube = start();
        let view = cube_view(&cube);
        let node = view.get(handle).unwrap();
        let spec = (node.drag.as_ref().unwrap())();
        let context =
            context(cube, handle, node.drag_space.unwrap()).with_grab_offset(grab - center);
        let mut behavior = spec.compile(&context);
        // The pointer swings from the grab at angle 0 to 80 degrees around the center.
        let angle = 80_f32.to_radians();
        let pointer = center + Vec3::new(angle.cos(), angle.sin(), 0.0) * RING;
        let frame = behavior(Pointer::at(pointer - (grab - center)));
        let rotation = frame.drop_state.transform.rotation;
        assert!(rotation.angle_between(Quat::from_rotation_z(FRAC_PI_2)) < 1e-3);
    }

    #[test]
    fn dragging_a_scale_cube_scales_along_its_axis() {
        let handle = "cube/gizmo/scale-y";
        let anchor = Vec3::new(1.0, 2.0 + ARM + 0.2, 3.0);
        let dropped = drag(handle, Pointer::at(anchor + Vec3::new(0.0, 0.52, 0.0)));
        let scale = dropped.transform.scale;
        assert!(scale.distance(Vec3::new(1.0, 1.5, 1.0)) < 1e-4, "{scale}");
    }

    /// The view of the cube with the gizmo at `size`.
    fn sized(size: f32) -> View<Cube> {
        let mut view: View<Cube> = View::new();
        Gizmo::new("cube", Transform::IDENTITY)
            .with_size(size)
            .translate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .scale(Snap::NONE, |cube: &Cube, factor| Cube {
                transform: Transform {
                    scale: cube.transform.scale * factor,
                    ..cube.transform
                },
            })
            .scale_uniform(Snap::NONE, |cube: &Cube, factor| Cube {
                transform: Transform {
                    scale: cube.transform.scale * factor,
                    ..cube.transform
                },
            })
            .add_to(&mut view);
        view
    }

    fn lifted(cube: &Cube) -> View<Cube> {
        let mut view = View::new();
        view.push(ViewNode::cuboid("cube", Vec3::ONE, 0.1).with_transform(cube.transform));
        Gizmo::new("cube", cube.transform)
            .lift(Snap::NONE, |cube: &Cube, height| Cube {
                transform: Transform::from_translation(
                    cube.transform.translation + Vec3::Y * height,
                ),
            })
            .add_to(&mut view);
        view
    }

    #[test]
    fn a_lift_gizmo_has_one_arrow_that_raises_by_the_height_dragged() {
        let cube = Cube {
            transform: Transform::IDENTITY,
        };
        let view = lifted(&cube);
        assert_eq!(
            ids(&view),
            [
                "cube",
                "cube/gizmo/translate-y",
                "cube/gizmo/translate-y/tip"
            ]
        );
        let node = view.get("cube/gizmo/translate-y").unwrap();
        let spec = (node.drag.as_ref().unwrap())();
        let context = DragContext::new(Arc::new(lifted), node.id.clone(), cube)
            .with_space(node.drag_space.unwrap());
        let anchor = Vec3::new(0.0, ARM / 2.0, 0.0);
        let dropped = spec.compile(&context)(Pointer::at(anchor + Vec3::Y * 0.7)).drop_state;
        assert!(
            (dropped.transform.translation.y - 0.7).abs() < 1e-4,
            "{}",
            dropped.transform.translation
        );
    }

    #[test]
    fn handles_that_cannot_be_dragged_from_the_camera_are_left_out() {
        let seen = |eye: Vec3| {
            let mut view: View<Cube> = View::new();
            Gizmo::new("cube", Transform::IDENTITY)
                .seen_from(eye)
                .translate(Snap::NONE, |cube: &Cube, _| cube.clone())
                .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
                .scale(Snap::NONE, |cube: &Cube, _| cube.clone())
                .add_to(&mut view);
            ids(&view)
        };
        let has =
            |ids: &[String], name: &str| ids.iter().any(|id| id == &format!("cube/gizmo/{name}"));
        let side = seen(Vec3::new(0.0, 5.0, 5.0));
        assert!(
            !has(&side, "translate-yz") && !has(&side, "rotate-x"),
            "edge-on from the side"
        );
        for kept in [
            "translate-x",
            "translate-y",
            "translate-z",
            "translate-xy",
            "translate-xz",
            "rotate-y",
            "rotate-z",
            "scale-y",
        ] {
            assert!(has(&side, kept), "{kept}");
        }
        let above = seen(Vec3::new(0.0, 6.0, 0.0));
        for left_out in [
            "translate-y",
            "scale-y",
            "translate-xy",
            "translate-yz",
            "rotate-x",
            "rotate-z",
        ] {
            assert!(!has(&above, left_out), "{left_out}");
        }
        for kept in [
            "translate-x",
            "translate-z",
            "translate-xz",
            "rotate-y",
            "scale-x",
        ] {
            assert!(has(&above, kept), "{kept}");
        }
        assert!(
            seen(Vec3::ZERO).len() > 12,
            "an eye at the pivot says nothing"
        );
    }

    #[test]
    fn a_raised_gizmo_lifts_its_handles_and_the_origin_of_their_drags_together() {
        let mut view: View<Cube> = View::new();
        let turned = Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2));
        Gizmo::new("cube", turned)
            .raised(0.5)
            .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .translate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .add_to(&mut view);
        let ring = view.get("cube/gizmo/rotate-y").unwrap();
        assert_eq!(ring.transform.translation, Vec3::new(0.0, 0.5, 0.0));
        let arrow = view.get("cube/gizmo/translate-x").unwrap();
        assert!((arrow.transform.translation - Vec3::new(0.5, 0.5, 0.0)).length() < 1e-5);
        // The local Y axis points along the world -X once the target is turned a quarter.
        let DragSpace::Plane { origin, .. } = ring.drag_space.unwrap() else {
            panic!("a plane space");
        };
        assert!(
            (origin - Vec3::new(-0.5, 0.0, 0.0)).length() < 1e-5,
            "{origin}"
        );
    }

    #[test]
    fn a_raised_gizmo_clears_the_ground_by_the_width_of_its_handles_however_flat_the_target() {
        for size in [0.5, 1.0, 3.0] {
            let mut view: View<Cube> = View::new();
            Gizmo::new("cube", Transform::IDENTITY)
                .with_size(size)
                .raised(0.0)
                .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
                .add_to(&mut view);
            let ring = view.get("cube/gizmo/rotate-y").unwrap();
            assert!((ring.transform.translation.y - HANDLE_RADIUS * size).abs() < 1e-6);
        }
        let mut view: View<Cube> = View::new();
        Gizmo::new("cube", Transform::IDENTITY)
            .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .add_to(&mut view);
        let ring = view.get("cube/gizmo/rotate-y").unwrap();
        assert_eq!(
            ring.transform.translation,
            Vec3::ZERO,
            "not raised: at the pivot"
        );
    }

    #[test]
    fn the_rings_can_be_limited_to_some_axes() {
        let mut view: View<Cube> = View::new();
        Gizmo::new("cube", Transform::IDENTITY)
            .rotate_axes(&[Axis::Y])
            .rotate(Snap::NONE, |cube: &Cube, _| cube.clone())
            .add_to(&mut view);
        assert_eq!(ids(&view), ["cube/gizmo/rotate-y"]);
    }

    #[test]
    fn a_bigger_gizmo_puts_its_handles_proportionally_further_out() {
        let (small, big) = (sized(1.0), sized(2.5));
        for id in [
            "cube/gizmo/translate-x",
            "cube/gizmo/translate-xz",
            "cube/gizmo/scale-y",
            "cube/gizmo/scale-uniform",
            "cube/gizmo/translate-x/tip",
        ] {
            let at = |view: &View<Cube>| view.get(id).unwrap().transform.translation;
            assert!(
                (at(&big) - at(&small) * 2.5).length() < 1e-5,
                "{id}: {} vs {}",
                at(&big),
                at(&small)
            );
        }
    }

    #[test]
    fn a_scale_handle_scales_by_one_per_unit_of_its_length_at_any_size() {
        let size = 3.0;
        let view = sized(size);
        let node = view.get("cube/gizmo/scale-x").unwrap();
        let spec = (node.drag.as_ref().unwrap())();
        let reach = (ARM + 0.2) * size;
        let cube = Cube {
            transform: Transform::IDENTITY,
        };
        let context =
            DragContext::new(Arc::new(move |_: &Cube| sized(size)), node.id.clone(), cube)
                .with_space(node.drag_space.unwrap());
        let dropped =
            spec.compile(&context)(Pointer::at(Vec3::X * (reach + ARM * size))).drop_state;
        assert!(
            dropped.transform.scale.distance(Vec3::new(2.0, 1.0, 1.0)) < 1e-4,
            "{}",
            dropped.transform.scale
        );
    }

    /// Drags the uniform handle in the plane facing the camera, as the engine fixes it when the
    /// drag is pressed, with the pointer at `distance` from the pivot along `direction`.
    fn drag_uniform(direction: Vec2, distance: f32) -> Cube {
        let handle = "cube/gizmo/scale-uniform";
        let cube = start();
        let view = cube_view(&cube);
        let spec = (view.get(handle).unwrap().drag.as_ref().unwrap())();
        let pivot = Vec3::new(1.0, 2.0, 3.0);
        let context = context(cube, handle, DragSpace::plane(pivot, Dir3::Z));
        let pointer = pivot + direction.normalize().extend(0.0) * distance;
        spec.compile(&context)(Pointer::at(pointer)).drop_state
    }

    #[test]
    fn dragging_the_uniform_cube_away_from_the_pivot_scales_every_axis() {
        let handle_distance = Vec2::splat(UNIFORM_OFFSET).length();
        let grown = drag_uniform(Vec2::ONE, handle_distance + 0.52);
        assert!(grown.transform.scale.distance(Vec3::splat(1.5)) < 1e-4);
        let shrunk = drag_uniform(Vec2::ONE, handle_distance - 0.26);
        assert!(shrunk.transform.scale.distance(Vec3::splat(0.75)) < 1e-4);
        let untouched = drag_uniform(Vec2::ONE, handle_distance);
        assert!(untouched.transform.scale.distance(Vec3::ONE) < 1e-4);
    }

    #[test]
    fn the_uniform_factor_depends_on_the_distance_not_the_direction() {
        let handle_distance = Vec2::splat(UNIFORM_OFFSET).length();
        let to_the_left = drag_uniform(Vec2::new(-1.0, 0.3), handle_distance + 0.52);
        let above = drag_uniform(Vec2::new(0.0, 1.0), handle_distance + 0.52);
        assert!(to_the_left.transform.scale.distance(above.transform.scale) < 1e-4);
    }

    /// A state that shows the gizmo only while its tool transforms.
    #[derive(Clone)]
    struct Tooled {
        transforming: bool,
    }

    impl GizmoReveal for Tooled {
        fn reveals_gizmo(&self) -> bool {
            self.transforming
        }
    }

    #[test]
    fn the_gizmo_is_added_only_for_a_state_that_reveals_it() {
        let handles = |transforming: bool| {
            let state = Tooled { transforming };
            let mut view = View::new();
            view.push(ViewNode::cuboid("cube", Vec3::ONE, 0.1));
            Gizmo::new("cube", Transform::IDENTITY)
                .translate(Snap::NONE, |state: &Tooled, _| state.clone())
                .add_if_revealed(&mut view, &state);
            view.len() - 1
        };
        assert_eq!(handles(false), 0);
        assert!(handles(true) > 0);
    }

    #[test]
    fn a_handle_ignores_motion_off_its_axis() {
        let dropped = drag(
            "cube/gizmo/translate-z",
            Pointer::at(Vec3::new(5.0, 9.0, 3.5)),
        );
        let moved = dropped.transform.translation - Vec3::new(1.0, 2.0, 3.0);
        assert!(moved.x.abs() < 1e-5 && moved.y.abs() < 1e-5, "{moved}");
        assert!(Vec2::new(moved.x, moved.y).length() < 1e-5);
    }
}
