//! The plain-data description of a scene that a view function returns.
//!
//! A [`View`] is a flat list of [`ViewNode`]s identified by a [`NodeId`]. Nothing here touches the
//! ECS, so the engine can evaluate the view function for many speculative states cheaply and blend
//! the results by id (see [`lerp_views`](crate::lerp_views)).

use std::borrow::Cow;
use std::collections::HashMap;

use bevy::color::LinearRgba;
use bevy::ecs::error::BevyError;
use bevy::math::{Mat4, Quat, Vec2, Vec3};
use bevy::prelude::{Color, EntityWorldMut, Ray3d, Scene, Transform};
use bevy::scene::EntityWorldMutSceneExt;
#[cfg(feature = "gradmesh")]
use bevy_gradmesh::{Shape, ShapeDetail, Style};

use crate::blend::Presence;
#[cfg(feature = "pbr")]
use crate::geometry::MeshGeometry;
use crate::geometry::{Bounds, Geometry};
use crate::rendering::Rendering;
#[cfg(feature = "gradmesh")]
use crate::sdf::SdfShape;
use crate::space::DragSpace;
use crate::spec::DragSpec;

/// The render layer for gizmo handles. They sit on this layer so they are always picked first,
/// even when scene geometry covers them. A camera that shows the scene should also show this
/// layer, which the plugin does by default.
pub const GIZMO_LAYER: usize = 31;

/// The radius of the sphere that stands in for a group, which is never seen.
#[cfg(feature = "gradmesh")]
const GROUP_RADIUS: f32 = 1e-3;

/// The stable, globally unique identifier that matches a node across different views.
pub type NodeId = Cow<'static, str>;

/// Lazily builds the [`DragSpec`] of a node from the state its view was rendered from, so specs
/// are only constructed for the node being dragged and not for every node of every speculative
/// view.
pub type DragFactory<S> = Box<dyn Fn(&S) -> DragSpec<S> + Send + Sync>;

/// What puts the scene of a node on its entity, built from the scene function of
/// [`ViewNode::with_scene`].
pub(crate) type SceneApplier = Box<dyn FnOnce(&mut EntityWorldMut) -> Result<(), BevyError> + Send>;

/// Lazily builds the scene of a node, so the scene function only runs for a node that appears
/// in the shown world and never for a speculative view.
pub(crate) type SceneFactory = Box<dyn Fn() -> SceneApplier + Send + Sync>;

/// A single element of a [`View`].
///
/// A clone has everything but the drag factory, which has one owner: a drag starts from the view
/// a state renders, never from a copy of it.
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
    #[cfg(feature = "gradmesh")]
    pub style: Option<Style>,
    /// The geometry the node is drawn with.
    pub geometry: Geometry,
    /// How opaque the node is, from `0.0` to `1.0`. Like in SVG, the opacity of a node is
    /// multiplied into the opacity of its children.
    pub opacity: f32,
    /// What happens to the node when it exists in only one of two blended views.
    pub presence: Presence,
    /// How far past its target the position, rotation and scale of the node run when an
    /// animation brings it there, as a fraction of the distance travelled, before they settle
    /// back onto it. `0.0` is the plain ease-out of the other nodes.
    pub overshoot: f32,
    /// Whether the node casts shadows.
    pub casts_shadow: bool,
    /// Whether the pointer can hit the node. A node that it cannot hit never takes a press and
    /// never covers the nodes behind it, so decorations such as labels let presses through.
    pub pickable: bool,
    /// How the node is drawn, or `None` for the default of the plugin (see [`Rendering`]).
    pub rendering: Option<Rendering>,
    /// The render layer of the node. A camera only draws, and a press from it only picks, the
    /// nodes on its own layers. Layer `0` is the default layer of Bevy cameras.
    pub layer: usize,
    /// The drag behavior attached to this node, if it can be dragged.
    pub drag: Option<DragFactory<S>>,
    /// The space a drag of this node happens in. `None` uses the default space of the scene
    /// (see `DragPlane`).
    pub drag_space: Option<DragSpace>,
    /// The scene of the app that goes on the entity of the node when the node first appears (see
    /// [`with_scene`](Self::with_scene)).
    pub(crate) scene: Option<SceneFactory>,
}

impl<S> Clone for ViewNode<S> {
    fn clone(&self) -> Self {
        self.rebind()
    }
}

impl<S> ViewNode<S> {
    /// Creates a white node with an identity transform.
    pub fn new(id: impl Into<NodeId>, geometry: impl Into<Geometry>) -> Self {
        Self {
            id: id.into(),
            parent: None,
            transform: Transform::IDENTITY,
            color: LinearRgba::WHITE,
            #[cfg(feature = "gradmesh")]
            style: None,
            geometry: geometry.into(),
            opacity: 1.0,
            presence: Presence::default(),
            overshoot: 0.0,
            casts_shadow: true,
            pickable: true,
            rendering: None,
            layer: 0,
            drag: None,
            drag_space: None,
            scene: None,
        }
    }

    /// Creates a rectangle of the given size.
    pub fn rectangle(id: impl Into<NodeId>, size: Vec2) -> Self {
        let mut node = Self::new(id, Geometry::Rectangle);
        node.transform.scale = size.extend(1.0);
        node
    }

    /// Creates a circle of the given diameter.
    pub fn circle(id: impl Into<NodeId>, diameter: f32) -> Self {
        let mut node = Self::new(id, Geometry::Circle);
        node.transform.scale = Vec3::new(diameter, diameter, 1.0);
        node
    }

    /// Creates a node drawn with a mesh of your own, at the dimensions of the mesh.
    #[cfg(feature = "pbr")]
    pub fn mesh(id: impl Into<NodeId>, mesh: MeshGeometry) -> Self {
        Self::new(id, Geometry::Mesh(mesh))
    }

    /// Creates a node drawn with any `bevy_gradmesh` shape, at the dimensions of the shape.
    #[cfg(feature = "gradmesh")]
    pub fn grad(id: impl Into<NodeId>, shape: Shape) -> Self {
        Self::new(id, Geometry::Grad(shape))
    }

    /// Creates a flat node drawn from a signed distance field, at the dimensions of the shape.
    /// It is meant for a `Camera2d`, so it is flat by default.
    #[cfg(feature = "gradmesh")]
    pub fn sdf(id: impl Into<NodeId>, shape: SdfShape) -> Self {
        Self::new(id, Geometry::Sdf(shape)).flat()
    }

    /// Creates a node that draws nothing and takes no presses, to hang other nodes from and
    /// move them together. Unlike a node with zero [`opacity`](Self::opacity), which hides its
    /// children as well, a group leaves them as they are.
    #[cfg(feature = "gradmesh")]
    pub fn group(id: impl Into<NodeId>) -> Self {
        Self::sphere(id, GROUP_RADIUS)
            .with_style(Style::transparent(Color::NONE))
            .casts_shadow(false)
            .pickable(false)
    }

    /// Creates a gradient sphere of the given radius.
    #[cfg(feature = "gradmesh")]
    pub fn sphere(id: impl Into<NodeId>, radius: f32) -> Self {
        Self::grad(id, Shape::sphere(radius, ShapeDetail::Medium))
    }

    /// Creates a gradient cuboid of the given size, with `roundness` rounding its edges.
    #[cfg(feature = "gradmesh")]
    pub fn cuboid(id: impl Into<NodeId>, size: Vec3, roundness: f32) -> Self {
        Self::grad(id, Shape::cuboid(size, roundness, ShapeDetail::Medium))
    }

    /// Creates a gradient cylinder standing along the Y axis.
    #[cfg(feature = "gradmesh")]
    pub fn cylinder(id: impl Into<NodeId>, radius: f32, height: f32) -> Self {
        Self::grad(id, Shape::cylinder(radius, height, ShapeDetail::Medium))
    }

    /// Creates a gradient cone pointing along the Y axis.
    #[cfg(feature = "gradmesh")]
    pub fn cone(id: impl Into<NodeId>, radius: f32, height: f32) -> Self {
        Self::grad(id, Shape::cone(radius, height, ShapeDetail::Medium))
    }

    /// Creates a full gradient torus with the given ring radius and tube thickness.
    #[cfg(feature = "gradmesh")]
    pub fn torus(id: impl Into<NodeId>, radius: f32, thickness: f32) -> Self {
        Self::grad(
            id,
            Shape::torus(
                radius,
                thickness,
                0.0,
                std::f32::consts::TAU,
                ShapeDetail::Medium,
            ),
        )
    }

    /// Creates a flat gradient disc in the XY plane.
    #[cfg(feature = "gradmesh")]
    pub fn disc(id: impl Into<NodeId>, radius: f32) -> Self {
        Self::grad(id, Shape::disc(radius, ShapeDetail::Medium))
    }

    /// Creates a flat gradient ring in the XY plane.
    #[cfg(feature = "gradmesh")]
    pub fn ring(id: impl Into<NodeId>, outer_radius: f32, inner_radius: f32) -> Self {
        Self::grad(
            id,
            Shape::ring(outer_radius, inner_radius, ShapeDetail::Medium),
        )
    }

    /// Creates a flat gradient star in the XY plane.
    #[cfg(feature = "gradmesh")]
    pub fn star(id: impl Into<NodeId>, points: u32, outer_radius: f32, inner_radius: f32) -> Self {
        Self::grad(
            id,
            Shape::star(points, outer_radius, inner_radius, ShapeDetail::Medium),
        )
    }

    /// Sets the transform of the node, relative to its parent.
    #[must_use]
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Moves the node to `position`.
    #[must_use]
    pub fn at(mut self, position: Vec3) -> Self {
        self.transform.translation = position;
        self
    }

    /// Moves the node to `position` in the XY plane, keeping its depth.
    #[must_use]
    pub fn at_xy(mut self, position: Vec2) -> Self {
        self.transform.translation.x = position.x;
        self.transform.translation.y = position.y;
        self
    }

    /// Scales the node by `scale` along each axis.
    #[must_use]
    pub fn scaled(mut self, scale: Vec3) -> Self {
        self.transform.scale = scale;
        self
    }

    /// Scales the node to `size` in the XY plane, keeping its depth scale.
    #[must_use]
    pub fn scaled_xy(mut self, size: Vec2) -> Self {
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

    /// Sets the rotation of the node.
    #[must_use]
    pub fn with_rotation(mut self, rotation: Quat) -> Self {
        self.transform.rotation = rotation;
        self
    }

    /// Rotates the node around the Z axis by `radians`.
    #[must_use]
    pub fn with_rotation_xy(mut self, radians: f32) -> Self {
        self.transform.rotation = Quat::from_rotation_z(radians);
        self
    }

    /// Sets whether the node casts shadows. Nodes cast shadows by default.
    #[must_use]
    pub fn casts_shadow(mut self, casts: bool) -> Self {
        self.casts_shadow = casts;
        self
    }

    /// Sets whether the pointer can hit the node. Nodes can be hit by default.
    #[must_use]
    pub fn pickable(mut self, pickable: bool) -> Self {
        self.pickable = pickable;
        self
    }

    /// Gives the entity of the node the components and observers of `scene`, which is built
    /// with `scene` when the node first appears.
    ///
    /// The view function stays a pure function of the state: `scene` is only called for a node
    /// that is shown, never while the engine evaluates a state that is not, so nothing in it runs
    /// for a preview. The node keeps its entity for as long as it is in the view, so the
    /// components and observers survive every change of state.
    ///
    /// The engine owns what it writes itself (the name, the transform, the mesh, the material,
    /// the render layers, the picking and the visibility of the entity), and writes it after the
    /// scene, so the scene cannot change it. The rest belongs to the app. The scene is applied
    /// once and the engine never touches what it adds again. It must not describe children,
    /// because the engine owns the hierarchy of the nodes.
    ///
    /// A node that first appears while one view blends into another takes its scene from the
    /// view that the blend settles on, and a node that is only in the view the blend leaves has
    /// none.
    #[must_use]
    pub fn with_scene<B: Scene + Send + Sync + 'static>(
        mut self,
        scene: impl Fn() -> B + Send + Sync + 'static,
    ) -> Self {
        self.scene = Some(Box::new(move || {
            let scene = scene();
            Box::new(move |entity| entity.apply_scene(scene).map_err(BevyError::from))
        }));
        self
    }

    /// Draws the node as a flat `Mesh2d` for a `Camera2d`, whatever the default of the plugin is.
    ///
    /// Together with [`on_layer`](Self::on_layer) this builds a HUD over a 3D scene.
    #[cfg(feature = "gradmesh")]
    #[must_use]
    pub fn flat(mut self) -> Self {
        self.rendering = Some(Rendering::Flat);
        self
    }

    /// Draws the node as a `Mesh3d` for a `Camera3d`, whatever the default of the plugin is.
    #[must_use]
    pub fn spatial(mut self) -> Self {
        self.rendering = Some(Rendering::Spatial);
        self
    }

    /// Puts the node on render layer `layer`, so only cameras that show that layer draw and pick
    /// it. Nodes are on layer `0` by default.
    #[must_use]
    pub fn on_layer(mut self, layer: usize) -> Self {
        self.layer = layer;
        self
    }

    /// Sets the color of the node.
    #[must_use]
    pub fn with_color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into().into();
        self
    }

    /// Sets the gradient style of the node. It only affects the gradient shapes.
    #[cfg(feature = "gradmesh")]
    #[must_use]
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Sets the opacity of the node, clamped to `0.0..=1.0`.
    #[must_use]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Sets what the node does when it exists in only one of two blended views.
    #[must_use]
    pub fn presence(mut self, presence: Presence) -> Self {
        self.presence = presence;
        self
    }

    /// Makes the transform of the node run past its target by `overshoot` of the distance
    /// travelled, a tenth being `0.1`, and settle back onto it when an animation ends. Negative
    /// values count as none.
    #[must_use]
    pub fn with_overshoot(mut self, overshoot: f32) -> Self {
        self.overshoot = overshoot.max(0.0);
        self
    }

    /// Makes the node grow from (and shrink into) the node `from` when it enters or leaves.
    #[must_use]
    pub fn emerge_from(self, from: impl Into<NodeId>) -> Self {
        self.presence(Presence::Emerge { from: from.into() })
    }

    /// Positions the node relative to the node with id `parent`.
    #[must_use]
    pub fn with_parent(mut self, parent: impl Into<NodeId>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    /// Makes the node draggable. `spec` is only called when a drag on this node starts.
    #[must_use]
    pub fn draggable(self, spec: impl Fn() -> DragSpec<S> + Send + Sync + 'static) -> Self {
        self.draggable_by(move |_| spec())
    }

    /// Makes the node draggable in `space`. `spec` is only called when a drag on this node starts.
    #[must_use]
    pub fn draggable_in(
        self,
        space: DragSpace,
        spec: impl Fn() -> DragSpec<S> + Send + Sync + 'static,
    ) -> Self {
        self.draggable(spec).in_space(space)
    }

    /// Makes the node draggable with a spec that depends on the state. `spec` is only called
    /// when a drag on this node starts, with the state that the view was rendered from, so the
    /// view function does not have to copy the state into the node.
    #[must_use]
    pub fn draggable_by(
        mut self,
        spec: impl Fn(&S) -> DragSpec<S> + Send + Sync + 'static,
    ) -> Self {
        self.drag = Some(Box::new(spec));
        self
    }

    /// Makes the node draggable in `space`, like [`draggable_by`](Self::draggable_by).
    #[must_use]
    pub fn draggable_by_in(
        self,
        space: DragSpace,
        spec: impl Fn(&S) -> DragSpec<S> + Send + Sync + 'static,
    ) -> Self {
        self.draggable_by(spec).in_space(space)
    }

    /// The drag spec of the node, built from `state`, which is the state this node's view was
    /// rendered from. `None` for a node that cannot be dragged.
    #[must_use]
    pub fn drag_spec(&self, state: &S) -> Option<DragSpec<S>> {
        self.drag.as_ref().map(|factory| factory(state))
    }

    /// Sets the space a drag of this node happens in.
    #[must_use]
    pub fn in_space(mut self, space: DragSpace) -> Self {
        self.drag_space = Some(space);
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
            #[cfg(feature = "gradmesh")]
            style: self.style.clone(),
            geometry: self.geometry.clone(),
            opacity: self.opacity,
            presence: self.presence.clone(),
            overshoot: self.overshoot,
            casts_shadow: self.casts_shadow,
            pickable: self.pickable,
            rendering: self.rendering,
            layer: self.layer,
            drag: None,
            drag_space: self.drag_space,
            scene: None,
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

    /// The transform of the node with the given id, relative to its parent.
    #[must_use]
    pub fn transform(&self, id: &str) -> Option<Transform> {
        self.get(id).map(|node| node.transform)
    }

    /// The world position of the node with the given id.
    #[must_use]
    pub fn world_position(&self, id: &str) -> Option<Vec3> {
        self.world_transform(id)
            .map(|transform| transform.translation)
    }

    /// Moves the node with the given id so that its world transform is `world`.
    ///
    /// Returns `false` when the view has no such node.
    pub fn set_world_transform(&mut self, id: &str, world: Transform) -> bool {
        let worlds = self.world_transforms();
        if !worlds.contains_key(id) {
            return false;
        }
        let parent_world = self
            .get(id)
            .and_then(|node| node.parent.as_deref())
            .and_then(|parent| worlds.get(parent).copied());
        let local = to_local(world, parent_world);
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
            node.transform = local;
        }
        true
    }

    /// Whether the ray hits the oriented bounds of the node with the given id. This is `false`
    /// when the view has no such node or the node is flattened to nothing.
    #[must_use]
    pub fn is_hit(&self, id: &str, ray: Ray3d) -> bool {
        let (Some(node), Some(world)) = (self.get(id), self.world_transform(id)) else {
            return false;
        };
        intersect(ray, &world, node.geometry.bounds()).is_some()
    }

    /// The node the ray hits first, testing the oriented bounds of every node: an ellipsoid for
    /// round geometry and a box otherwise. Nodes flattened to nothing are never hit, and of two
    /// hits at the same distance the node later in the view wins, like the one drawn on top.
    #[must_use]
    pub fn hit_test(&self, ray: Ray3d) -> Option<Hit> {
        self.hit_test_in(ray, |_| true)
    }

    /// Like [`hit_test`](Self::hit_test), but only for the nodes whose render layer `visible`
    /// accepts. A camera passes the layers it shows, so a ray from a HUD camera never hits a
    /// node of the 3D scene, and the other way round.
    ///
    /// Gizmo handles (on [`GIZMO_LAYER`]) are always included, even when the camera
    /// does not show that layer, so they remain clickable when scene geometry covers them.
    #[must_use]
    pub fn hit_test_in(&self, ray: Ray3d, visible: impl Fn(usize) -> bool) -> Option<Hit> {
        let worlds = self.world_transforms();
        let mut best: Option<Hit> = None;
        for node in self
            .nodes
            .iter()
            .filter(|node| visible(node.layer) || node.layer == GIZMO_LAYER)
        {
            let Some(world) = worlds.get(node.id.as_ref()) else {
                continue;
            };
            let Some(distance) = intersect(ray, world, node.geometry.bounds()) else {
                continue;
            };
            // Gizmo layer always wins over a non-gizmo hit at the same distance.
            #[allow(
                clippy::float_cmp,
                reason = "hits at exactly the same distance are the tie that is being broken"
            )]
            let wins = if node.layer == GIZMO_LAYER {
                best.as_ref().is_none_or(|hit| {
                    hit.distance < distance
                        || (hit.distance == distance && hit.id != node.id.as_ref())
                })
            } else {
                best.as_ref().is_none_or(|hit| distance <= hit.distance)
            };
            if wins {
                best = Some(Hit {
                    id: node.id.clone(),
                    distance,
                    point: ray.get_point(distance),
                });
            }
        }
        best
    }

    /// The node with the given id if it is draggable, and otherwise its nearest draggable
    /// ancestor. This is the node a press on `id` grabs.
    #[must_use]
    pub fn draggable_ancestor(&self, id: &str) -> Option<&ViewNode<S>> {
        let mut current = self.get(id)?;
        for _ in 0..=self.nodes.len() {
            if current.drag.is_some() {
                return Some(current);
            }
            current = self.get(current.parent.as_deref()?)?;
        }
        None
    }

    /// The opacity of every node multiplied by the opacities of its ancestors, as in SVG.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn effective_opacities(&self) -> HashMap<&str, f32> {
        let index: HashMap<&str, &ViewNode<S>> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_ref(), node))
            .collect();
        self.nodes
            .iter()
            .map(|node| {
                let mut opacity = node.opacity;
                let mut current = node;
                for _ in 0..self.nodes.len() {
                    let Some(parent) = current
                        .parent
                        .as_deref()
                        .and_then(|parent| index.get(parent))
                    else {
                        break;
                    };
                    opacity *= parent.opacity;
                    current = parent;
                }
                (node.id.as_ref(), opacity)
            })
            .collect()
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

/// The node that a ray hit first, see [`View::hit_test`].
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The node that was hit.
    pub id: NodeId,
    /// The distance from the origin of the ray to the hit.
    pub distance: f32,
    /// The world position of the hit.
    pub point: Vec3,
}

/// The distance along `ray` at which it first enters the oriented `bounds` of a node with the
/// given world transform (zero when it starts inside).
fn intersect(ray: Ray3d, world: &Transform, bounds: Bounds) -> Option<f32> {
    let affine = world.compute_affine();
    if affine.matrix3.determinant().abs() < f32::EPSILON {
        return None;
    }
    // The ray in the local space of the node. The direction is not renormalized, so distances
    // along the local ray are distances along the world ray.
    let inverse = affine.inverse();
    let origin = inverse.transform_point3(ray.origin) - bounds.center;
    let direction = inverse.transform_vector3(*ray.direction);
    let half = bounds.half_extents;
    if bounds.round {
        let (o, d) = (origin / half, direction / half);
        let a = d.length_squared();
        let b = o.dot(d);
        let discriminant = b * b - a * (o.length_squared() - 1.0);
        if discriminant < 0.0 || a <= f32::EPSILON {
            return None;
        }
        let root = discriminant.sqrt();
        let (near, far) = ((-b - root) / a, (-b + root) / a);
        return if near >= 0.0 {
            Some(near)
        } else if far >= 0.0 {
            Some(0.0)
        } else {
            None
        };
    }
    let mut enter = f32::NEG_INFINITY;
    let mut leave = f32::INFINITY;
    for axis in 0..3 {
        let (o, d, h) = (origin[axis], direction[axis], half[axis]);
        if d.abs() < f32::EPSILON {
            if o.abs() > h {
                return None;
            }
            continue;
        }
        let (t1, t2) = ((-h - o) / d, (h - o) / d);
        enter = enter.max(t1.min(t2));
        leave = leave.min(t1.max(t2));
    }
    (enter <= leave && leave >= 0.0).then_some(enter.max(0.0))
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
    use bevy::math::Dir3;

    use super::*;

    type TestView = View<()>;

    fn down_at(x: f32, y: f32) -> Ray3d {
        Ray3d::new(Vec3::new(x, y, 10.0), Dir3::NEG_Z)
    }

    #[test]
    fn world_transform_composes_through_parents() {
        let view: TestView = View::new()
            .with(ViewNode::rectangle("group", Vec2::ONE).at_xy(Vec2::new(10.0, 0.0)))
            .with(
                ViewNode::circle("item", 1.0)
                    .with_parent("group")
                    .at_xy(Vec2::new(2.0, 3.0)),
            );
        let position = view.world_position("item").unwrap();
        assert!(position.distance(Vec3::new(12.0, 3.0, 0.0)) < 1e-5);
    }

    #[test]
    fn unknown_parent_is_treated_as_the_world() {
        let view: TestView = View::new().with(
            ViewNode::circle("item", 1.0)
                .with_parent("gone")
                .at_xy(Vec2::X),
        );
        assert!(view.world_position("item").unwrap().distance(Vec3::X) < 1e-5);
    }

    #[test]
    fn parent_cycles_terminate() {
        let view: TestView = View::new()
            .with(ViewNode::circle("a", 1.0).with_parent("b"))
            .with(ViewNode::circle("b", 1.0).with_parent("a"));
        assert!(view.world_transform("a").is_some());
    }

    #[test]
    fn transform_builders_take_transforms_vectors_and_quaternions() {
        let node: ViewNode<()> = ViewNode::circle("n", 1.0)
            .with_transform(Transform::from_xyz(1.0, 2.0, 3.0))
            .scaled(Vec3::new(2.0, 3.0, 4.0))
            .with_rotation(Quat::from_rotation_y(1.0))
            .at(Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(node.transform.translation, Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(node.transform.scale, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(node.transform.rotation, Quat::from_rotation_y(1.0));
        let flat: ViewNode<()> = ViewNode::circle("n", 1.0)
            .at_xy(Vec2::new(1.0, 2.0))
            .with_z(3.0)
            .scaled_xy(Vec2::new(4.0, 5.0))
            .with_rotation_xy(0.5);
        assert_eq!(flat.transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(flat.transform.scale, Vec3::new(4.0, 5.0, 1.0));
        assert_eq!(flat.transform.rotation, Quat::from_rotation_z(0.5));
        assert!(flat.casts_shadow && !flat.casts_shadow(false).casts_shadow);
    }

    #[test]
    fn set_world_transform_accounts_for_the_parent() {
        let mut view: TestView = View::new()
            .with(ViewNode::rectangle("group", Vec2::ONE).at_xy(Vec2::new(10.0, 0.0)))
            .with(ViewNode::circle("item", 1.0).with_parent("group"));
        assert!(view.set_world_transform("item", Transform::from_xyz(15.0, 4.0, 2.0)));
        let local = view.transform("item").unwrap().translation;
        assert!(local.distance(Vec3::new(5.0, 4.0, 2.0)) < 1e-4);
        assert!(!view.set_world_transform("missing", Transform::IDENTITY));
        assert!(view.transform("missing").is_none());
    }

    #[test]
    fn a_ray_hits_a_flat_rectangle_from_above() {
        let view: TestView = View::new()
            .with(ViewNode::rectangle("bar", Vec2::new(4.0, 1.0)).at_xy(Vec2::new(10.0, 0.0)))
            .with(
                ViewNode::rectangle("tilted", Vec2::new(4.0, 1.0))
                    .with_rotation_xy(std::f32::consts::FRAC_PI_2),
            );
        assert_eq!(view.hit_test(down_at(11.5, 0.4)).unwrap().id, "bar");
        assert!(view.hit_test(down_at(11.5, 0.6)).is_none());
        assert_eq!(view.hit_test(down_at(0.4, 1.5)).unwrap().id, "tilted");
        assert!(view.hit_test(down_at(1.5, 0.4)).is_none());
        let hit = view.hit_test(down_at(11.5, 0.4)).unwrap();
        assert!((hit.distance - 10.0).abs() < 0.05);
        assert!(hit.point.distance(Vec3::new(11.5, 0.4, 0.0)) < 0.05);
    }

    #[test]
    fn round_nodes_are_hit_as_ellipses_and_spheres() {
        let view: TestView =
            View::new().with(ViewNode::circle("disc", 2.0).at_xy(Vec2::new(0.0, 10.0)));
        assert!(view.hit_test(down_at(0.0, 10.9)).is_some());
        assert!(view.hit_test(down_at(0.8, 10.8)).is_none());
    }

    #[cfg(feature = "pbr")]
    #[test]
    fn a_mesh_of_your_own_is_hit_inside_its_measured_bounds() {
        let mesh = MeshGeometry::new(
            bevy::prelude::Handle::default(),
            &bevy::prelude::Mesh::from(bevy::prelude::Rectangle::new(4.0, 2.0)),
        );
        let view: TestView = View::new().with(ViewNode::mesh("terrain", mesh).at(Vec3::X * 10.0));
        assert_eq!(view.hit_test(down_at(11.5, 0.8)).unwrap().id, "terrain");
        assert!(view.hit_test(down_at(11.5, 1.2)).is_none());
        assert!(view.hit_test(down_at(12.5, 0.0)).is_none());
    }

    #[test]
    fn a_node_flattened_to_nothing_is_never_hit() {
        let view: TestView = View::new().with(ViewNode::rectangle("gone", Vec2::new(0.0, 0.0)));
        assert!(view.hit_test(down_at(0.0, 0.0)).is_none());
        let flat: TestView = View::new()
            .with(ViewNode::rectangle("flat", Vec2::ONE).scaled(Vec3::new(1.0, 1.0, 0.0)));
        assert!(flat.hit_test(down_at(0.0, 0.0)).is_none());
    }

    #[test]
    fn the_nearest_of_two_overlapping_nodes_wins() {
        let view: TestView = View::new()
            .with(ViewNode::rectangle("far", Vec2::splat(2.0)).with_z(-1.0))
            .with(ViewNode::rectangle("near", Vec2::splat(2.0)).with_z(1.0));
        let hit = view.hit_test(down_at(0.0, 0.0)).unwrap();
        assert_eq!(hit.id, "near");
        assert!((hit.distance - 9.0).abs() < 0.05);
        let tie: TestView = View::new()
            .with(ViewNode::rectangle("below", Vec2::splat(2.0)))
            .with(ViewNode::rectangle("above", Vec2::splat(2.0)));
        assert_eq!(tie.hit_test(down_at(0.0, 0.0)).unwrap().id, "above");
    }

    #[test]
    fn a_layer_filter_hides_the_nodes_of_other_layers_from_a_ray() {
        let view: TestView = View::new()
            .with(ViewNode::rectangle("scene", Vec2::splat(2.0)))
            .with(
                ViewNode::rectangle("hud", Vec2::splat(2.0))
                    .on_layer(2)
                    .with_z(1.0),
            );
        let ray = down_at(0.0, 0.0);
        assert_eq!(view.hit_test(ray).unwrap().id, "hud");
        assert_eq!(view.hit_test_in(ray, |layer| layer == 2).unwrap().id, "hud");
        assert_eq!(
            view.hit_test_in(ray, |layer| layer == 0).unwrap().id,
            "scene"
        );
        assert!(view.hit_test_in(ray, |layer| layer == 1).is_none());
    }

    #[test]
    fn rendering_mode_and_layer_survive_a_rebind() {
        let node: ViewNode<()> = ViewNode::circle("n", 1.0).spatial().on_layer(3);
        assert_eq!(node.rendering, Some(Rendering::Spatial));
        let rebound = node.rebind::<u8>();
        assert_eq!(rebound.rendering, Some(Rendering::Spatial));
        assert_eq!(rebound.layer, 3);
        assert_eq!(ViewNode::<()>::circle("n", 1.0).layer, 0);
        assert_eq!(ViewNode::<()>::circle("n", 1.0).rendering, None);
    }

    #[test]
    fn a_ray_that_starts_inside_hits_at_distance_zero() {
        let view: TestView = View::new().with(ViewNode::circle("big", 100.0));
        let inside = Ray3d::new(Vec3::ZERO, Dir3::NEG_Z);
        assert_eq!(view.hit_test(inside).map(|hit| hit.distance), Some(0.0));
    }

    #[test]
    fn a_child_resolves_to_its_draggable_ancestor() {
        use crate::ops::state;
        let view: TestView = View::new()
            .with(ViewNode::circle("root", 1.0).draggable(|| state(())))
            .with(ViewNode::circle("middle", 1.0).with_parent("root"))
            .with(ViewNode::circle("leaf", 1.0).with_parent("middle"))
            .with(ViewNode::circle("orphan", 1.0));
        assert_eq!(view.draggable_ancestor("leaf").unwrap().id, "root");
        assert_eq!(view.draggable_ancestor("root").unwrap().id, "root");
        assert!(view.draggable_ancestor("orphan").is_none());
        assert!(view.draggable_ancestor("missing").is_none());
    }

    #[test]
    fn nodes_carry_their_drag_space() {
        use crate::ops::state;
        let space = DragSpace::axis(Vec3::ZERO, Dir3::X);
        let node: ViewNode<()> = ViewNode::circle("n", 1.0).draggable_in(space, || state(()));
        assert!(node.drag.is_some());
        assert_eq!(node.drag_space, Some(space));
        assert_eq!(
            ViewNode::<()>::circle("n", 1.0).in_space(space).drag_space,
            Some(space)
        );
        assert_eq!(node.rebind::<u8>().drag_space, Some(space));
    }

    #[test]
    fn opacity_multiplies_through_ancestors() {
        let view: TestView = View::new()
            .with(ViewNode::circle("group", 1.0).with_opacity(0.5))
            .with(
                ViewNode::circle("inner", 1.0)
                    .with_parent("group")
                    .with_opacity(0.5),
            )
            .with(ViewNode::circle("leaf", 1.0).with_parent("inner"))
            .with(ViewNode::circle("loose", 1.0).with_parent("gone"));
        let opacities = view.effective_opacities();
        assert!((opacities["group"] - 0.5).abs() < 1e-6);
        assert!((opacities["inner"] - 0.25).abs() < 1e-6);
        assert!((opacities["leaf"] - 0.25).abs() < 1e-6);
        assert!((opacities["loose"] - 1.0).abs() < 1e-6);
    }

    #[cfg(feature = "gradmesh")]
    #[test]
    fn a_group_draws_nothing_and_hides_nothing_that_hangs_from_it() {
        let group: ViewNode<()> = ViewNode::group("group");
        assert!(!group.pickable);
        assert!(!group.casts_shadow);
        assert!((group.opacity - 1.0).abs() < f32::EPSILON);
        let view: TestView = View::new()
            .with(group)
            .with(ViewNode::circle("leaf", 1.0).with_parent("group"));
        assert!((view.effective_opacities()["leaf"] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn opacity_is_clamped() {
        let node: ViewNode<()> = ViewNode::circle("n", 1.0).with_opacity(3.0);
        assert!((node.opacity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parent_cycles_do_not_hang_the_opacity() {
        let view: TestView = View::new()
            .with(ViewNode::circle("a", 1.0).with_parent("b"))
            .with(ViewNode::circle("b", 1.0).with_parent("a"));
        assert_eq!(view.effective_opacities().len(), 2);
    }

    #[cfg(feature = "gradmesh")]
    #[test]
    fn grad_nodes_are_hit_by_their_real_size() {
        let view: TestView = View::new().with(ViewNode::sphere("ball", 2.0)).with(
            ViewNode::cuboid("block", Vec3::new(4.0, 2.0, 2.0), 0.0).at_xy(Vec2::new(10.0, 0.0)),
        );
        assert!(view.hit_test(down_at(1.4, 1.4)).is_some());
        assert!(view.hit_test(down_at(1.6, 1.6)).is_none());
        assert_eq!(view.hit_test(down_at(11.9, 0.9)).unwrap().id, "block");
        assert!(view.hit_test(down_at(12.1, 0.0)).is_none());
    }

    #[cfg(feature = "gradmesh")]
    #[test]
    fn scale_still_sizes_grad_nodes() {
        let view: TestView = View::new()
            .with(ViewNode::cuboid("block", Vec3::ONE, 0.0).scaled_xy(Vec2::new(4.0, 1.0)));
        assert!(view.hit_test(down_at(1.9, 0.4)).is_some());
        assert!(view.hit_test(down_at(0.0, 0.6)).is_none());
    }

    #[cfg(feature = "gradmesh")]
    #[test]
    fn a_rotated_cuboid_is_hit_by_its_oriented_bounds() {
        // A bar 4 long and 1 high, turned a quarter around Z, is 1 wide and 4 high.
        let view: TestView = View::new().with(
            ViewNode::cuboid("bar", Vec3::new(4.0, 1.0, 1.0), 0.0)
                .with_rotation_xy(std::f32::consts::FRAC_PI_2),
        );
        assert!(view.hit_test(down_at(0.3, 1.8)).is_some());
        assert!(view.hit_test(down_at(1.8, 0.3)).is_none());
        // Tilted around X and looked at from the side, it is hit by its depth.
        let tilted: TestView = View::new().with(
            ViewNode::cuboid("block", Vec3::new(2.0, 2.0, 2.0), 0.0)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_4)),
        );
        let side = Ray3d::new(Vec3::new(-10.0, 0.0, 0.0), Dir3::X);
        assert!((tilted.hit_test(side).unwrap().distance - 9.0).abs() < 0.05);
    }
}
