//! The plain-data description of a scene that a view function returns.
//!
//! A [`View`] is a flat list of [`ViewNode`]s identified by a [`NodeId`]. Nothing here touches the
//! ECS, so the engine can evaluate the view function for many speculative states cheaply and blend
//! the results by id (see [`lerp_views`](crate::lerp_views)).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use bevy::color::LinearRgba;
use bevy::math::{Mat4, Quat, Vec2, Vec3};
use bevy::prelude::{Color, Transform};
#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
use bevy_gradmesh::Style;

use crate::spec::DragSpec;

/// The stable, globally unique identifier that matches a node across different views.
pub type NodeId = Cow<'static, str>;

/// Lazily builds the [`DragSpec`] of a node, so specs are only constructed for the node being
/// dragged and not for every node of every speculative view.
pub type DragFactory<S> = Arc<dyn Fn() -> DragSpec<S> + Send + Sync>;

/// The geometry a [`ViewNode`] is drawn with.
///
/// Shapes are unit sized and scaled by the node's transform, so blending two views also blends
/// the node sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShapeKey {
    /// A unit square in the XY plane.
    Rectangle,
    /// A circle with unit diameter in the XY plane.
    Circle,
    /// A gradient sphere with unit diameter.
    #[cfg(feature = "shapes-3d")]
    GradSphere,
    /// A gradient cube with unit edges.
    #[cfg(feature = "shapes-3d")]
    GradCuboid,
    /// A gradient cylinder with unit diameter and height, standing along the Y axis.
    #[cfg(feature = "shapes-3d")]
    GradCylinder,
    /// A gradient cone with unit base diameter and height, pointing along the Y axis.
    #[cfg(feature = "shapes-3d")]
    GradCone,
    /// A gradient disc with unit diameter in the XY plane.
    #[cfg(feature = "shapes-2d")]
    GradDisc,
    /// A gradient square with unit edges in the XY plane.
    #[cfg(feature = "shapes-2d")]
    GradRectangle,
    /// A gradient five-pointed star within a unit circle in the XY plane.
    #[cfg(feature = "shapes-2d")]
    GradStar,
    /// A gradient ring with unit outer diameter in the XY plane.
    #[cfg(feature = "shapes-2d")]
    GradRing,
}

impl ShapeKey {
    /// Whether the outline is round, which decides how pointer hits are tested against it.
    fn is_round(self) -> bool {
        match self {
            Self::Rectangle => false,
            Self::Circle => true,
            #[cfg(feature = "shapes-3d")]
            Self::GradSphere => true,
            #[cfg(feature = "shapes-3d")]
            Self::GradCuboid | Self::GradCylinder | Self::GradCone => false,
            #[cfg(feature = "shapes-2d")]
            Self::GradDisc | Self::GradStar | Self::GradRing => true,
            #[cfg(feature = "shapes-2d")]
            Self::GradRectangle => false,
        }
    }
}

/// A single element of a [`View`].
#[derive(Clone)]
pub struct ViewNode<S> {
    /// Identifies this node across views.
    pub id: NodeId,
    /// The node this one is positioned relative to, if any.
    pub parent: Option<NodeId>,
    /// Position, rotation and scale relative to the parent (or the world without a parent).
    pub transform: Transform,
    /// The color the node is drawn with.
    pub color: LinearRgba,
    /// The gradient style of the node, used by the gradient shapes instead of a gradient derived
    /// from [`color`](Self::color).
    ///
    /// When two views are blended, nodes that both have a style blend them with `Style::lerp`
    /// from `bevy_gradmesh`. A node without a style takes part with the gradient derived from
    /// its color.
    #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
    pub style: Option<Style>,
    /// The geometry the node is drawn with.
    pub shape: ShapeKey,
    /// The drag behavior attached to this node, if it can be dragged.
    pub drag: Option<DragFactory<S>>,
}

impl<S> ViewNode<S> {
    /// Creates a white node with an identity transform.
    pub fn new(id: impl Into<NodeId>, shape: ShapeKey) -> Self {
        Self {
            id: id.into(),
            parent: None,
            transform: Transform::IDENTITY,
            color: LinearRgba::WHITE,
            #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
            style: None,
            shape,
            drag: None,
        }
    }

    /// Creates a rectangle of the given size.
    pub fn rectangle(id: impl Into<NodeId>, size: Vec2) -> Self {
        let mut node = Self::new(id, ShapeKey::Rectangle);
        node.transform.scale = size.extend(1.0);
        node
    }

    /// Creates a circle of the given diameter.
    pub fn circle(id: impl Into<NodeId>, diameter: f32) -> Self {
        let mut node = Self::new(id, ShapeKey::Circle);
        node.transform.scale = Vec3::new(diameter, diameter, 1.0);
        node
    }

    /// Moves the node to `position` in the XY plane, keeping its depth.
    #[must_use]
    pub fn at(mut self, position: Vec2) -> Self {
        self.transform.translation.x = position.x;
        self.transform.translation.y = position.y;
        self
    }

    /// Scales the node to `size` in the XY plane, keeping its depth scale.
    #[must_use]
    pub fn scaled(mut self, size: Vec2) -> Self {
        self.transform.scale.x = size.x;
        self.transform.scale.y = size.y;
        self
    }

    /// Sets the depth of the node; nodes with a larger `z` are drawn in front.
    #[must_use]
    pub fn with_z(mut self, z: f32) -> Self {
        self.transform.translation.z = z;
        self
    }

    /// Rotates the node around the Z axis by `radians`.
    #[must_use]
    pub fn with_rotation(mut self, radians: f32) -> Self {
        self.transform.rotation = Quat::from_rotation_z(radians);
        self
    }

    /// Sets the color of the node.
    #[must_use]
    pub fn with_color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into().into();
        self
    }

    /// Sets the gradient style of the node. It only affects the gradient shapes.
    #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
    #[must_use]
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Positions the node relative to the node with id `parent`.
    #[must_use]
    pub fn with_parent(mut self, parent: impl Into<NodeId>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    /// Makes the node draggable. `spec` is only called when a drag on this node starts.
    #[must_use]
    pub fn draggable(mut self, spec: impl Fn() -> DragSpec<S> + Send + Sync + 'static) -> Self {
        self.drag = Some(Arc::new(spec));
        self
    }

    /// The same node for another state type. A drag spec of one state type cannot drive another,
    /// so the new node is not draggable.
    fn rebind<U>(&self) -> ViewNode<U> {
        ViewNode {
            id: self.id.clone(),
            parent: self.parent.clone(),
            transform: self.transform,
            color: self.color,
            #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
            style: self.style.clone(),
            shape: self.shape,
            drag: None,
        }
    }
}

/// A flat collection of [`ViewNode`]s describing one state of the interface.
#[derive(Clone)]
pub struct View<S> {
    nodes: Vec<ViewNode<S>>,
}

impl<S> Default for View<S> {
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

impl<S> View<S> {
    /// Creates an empty view.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `node` and returns the view, for chaining.
    #[must_use]
    pub fn with(mut self, node: ViewNode<S>) -> Self {
        self.nodes.push(node);
        self
    }

    /// Adds `node` to the view.
    pub fn push(&mut self, node: ViewNode<S>) {
        self.nodes.push(node);
    }

    /// The same scene for another state type, as when a spec for part of a state previews the
    /// whole (see [`substate`](crate::ops::substate)). Drag specs cannot be carried over, so the
    /// nodes of the new view are not draggable.
    #[must_use]
    pub fn rebind<U>(&self) -> View<U> {
        View {
            nodes: self.nodes.iter().map(ViewNode::rebind).collect(),
        }
    }

    /// The nodes of the view, in insertion order.
    #[must_use]
    pub fn nodes(&self) -> &[ViewNode<S>] {
        &self.nodes
    }

    /// Returns the node with the given id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ViewNode<S>> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Returns the node with the given id for modification.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ViewNode<S>> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    /// The number of nodes in the view.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the view has no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The world transform of the node with the given id, composed through its parents.
    ///
    /// A parent id that is not part of the view is treated as the world.
    #[must_use]
    pub fn world_transform(&self, id: &str) -> Option<Transform> {
        self.world_transforms().get(id).copied()
    }

    /// The world position of the node with the given id in the XY plane.
    #[must_use]
    pub fn position(&self, id: &str) -> Option<Vec2> {
        self.world_transform(id)
            .map(|transform| transform.translation.truncate())
    }

    /// Whether `point` (in the XY plane) lies inside the node's shape.
    ///
    /// Returns `false` when the view has no such node or the node is flattened to nothing.
    #[must_use]
    pub fn contains_point(&self, id: &str, point: Vec2) -> bool {
        let (Some(node), Some(world)) = (self.get(id), self.world_transform(id)) else {
            return false;
        };
        let affine = world.compute_affine();
        if affine.matrix3.determinant().abs() < f32::EPSILON {
            return false;
        }
        let local = affine
            .inverse()
            .transform_point3(point.extend(world.translation.z))
            .truncate();
        if node.shape.is_round() {
            local.length() <= 0.5
        } else {
            local.x.abs() <= 0.5 && local.y.abs() <= 0.5
        }
    }

    /// Moves the node with the given id so that its world position in the XY plane is `position`.
    ///
    /// Returns `false` when the view has no such node.
    pub fn set_position(&mut self, id: &str, position: Vec2) -> bool {
        let worlds = self.world_transforms();
        let Some(mut world) = worlds.get(id).copied() else {
            return false;
        };
        let parent_world = self
            .get(id)
            .and_then(|node| node.parent.as_deref())
            .and_then(|parent| worlds.get(parent).copied());
        world.translation.x = position.x;
        world.translation.y = position.y;
        let local = to_local(world, parent_world);
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
            node.transform = local;
        }
        true
    }

    /// World transforms of all nodes, resolved once per node.
    pub(crate) fn world_transforms(&self) -> HashMap<&str, Transform> {
        let index: HashMap<&str, &ViewNode<S>> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_ref(), node))
            .collect();
        let mut worlds = HashMap::with_capacity(self.nodes.len());
        for node in &self.nodes {
            resolve_world(node.id.as_ref(), &index, &mut worlds, self.nodes.len());
        }
        worlds
    }
}

fn resolve_world<'a, S>(
    id: &'a str,
    index: &HashMap<&'a str, &'a ViewNode<S>>,
    worlds: &mut HashMap<&'a str, Transform>,
    depth_left: usize,
) -> Transform {
    if let Some(world) = worlds.get(id) {
        return *world;
    }
    let node = index[id];
    let parent = node
        .parent
        .as_deref()
        .filter(|parent| depth_left > 0 && index.contains_key(parent));
    let world = match parent {
        Some(parent) => {
            resolve_world(parent, index, worlds, depth_left - 1).mul_transform(node.transform)
        }
        None => node.transform,
    };
    worlds.insert(id, world);
    world
}

/// Expresses `world` relative to `parent_world`.
pub(crate) fn to_local(world: Transform, parent_world: Option<Transform>) -> Transform {
    let Some(parent_world) = parent_world else {
        return world;
    };
    let parent = parent_world.compute_affine();
    if parent.matrix3.determinant().abs() < f32::EPSILON {
        return world;
    }
    Transform::from_matrix(Mat4::from(parent.inverse()) * world.to_matrix())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestView = View<()>;

    #[test]
    fn world_transform_composes_through_parents() {
        let view: TestView = View::new()
            .with(ViewNode::rectangle("group", Vec2::ONE).at(Vec2::new(10.0, 0.0)))
            .with(
                ViewNode::circle("item", 1.0)
                    .with_parent("group")
                    .at(Vec2::new(2.0, 3.0)),
            );
        let position = view.position("item").unwrap();
        assert!(position.distance(Vec2::new(12.0, 3.0)) < 1e-5);
    }

    #[test]
    fn unknown_parent_is_treated_as_the_world() {
        let view: TestView = View::new().with(
            ViewNode::circle("item", 1.0)
                .with_parent("gone")
                .at(Vec2::X),
        );
        assert!(view.position("item").unwrap().distance(Vec2::X) < 1e-5);
    }

    #[test]
    fn parent_cycles_terminate() {
        let view: TestView = View::new()
            .with(ViewNode::circle("a", 1.0).with_parent("b"))
            .with(ViewNode::circle("b", 1.0).with_parent("a"));
        assert!(view.world_transform("a").is_some());
    }

    #[test]
    fn contains_point_follows_shape_scale_and_rotation() {
        let view: TestView = View::new()
            .with(ViewNode::rectangle("bar", Vec2::new(4.0, 1.0)).at(Vec2::new(10.0, 0.0)))
            .with(
                ViewNode::rectangle("tilted", Vec2::new(4.0, 1.0))
                    .with_rotation(std::f32::consts::FRAC_PI_2),
            )
            .with(ViewNode::circle("disc", 2.0).at(Vec2::new(0.0, 10.0)));
        assert!(view.contains_point("bar", Vec2::new(11.5, 0.4)));
        assert!(!view.contains_point("bar", Vec2::new(11.5, 0.6)));
        assert!(view.contains_point("tilted", Vec2::new(0.4, 1.5)));
        assert!(!view.contains_point("tilted", Vec2::new(1.5, 0.4)));
        assert!(view.contains_point("disc", Vec2::new(0.0, 10.9)));
        assert!(!view.contains_point("disc", Vec2::new(0.8, 10.8)));
        assert!(!view.contains_point("missing", Vec2::ZERO));
    }

    #[test]
    fn set_position_accounts_for_the_parent() {
        let mut view: TestView = View::new()
            .with(ViewNode::rectangle("group", Vec2::ONE).at(Vec2::new(10.0, 0.0)))
            .with(ViewNode::circle("item", 1.0).with_parent("group"));
        assert!(view.set_position("item", Vec2::new(15.0, 4.0)));
        let local = view.get("item").unwrap().transform.translation.truncate();
        assert!(local.distance(Vec2::new(5.0, 4.0)) < 1e-4);
        assert!(!view.set_position("missing", Vec2::ZERO));
    }
}
