//! How the entities of the nodes are drawn.

use bevy::prelude::Resource;

/// How the entities of the nodes are drawn.
///
/// The plugin default is set with `DraggablePlugin::with_rendering` (behind `pbr`), and a node
/// overrides it with [`ViewNode::spatial`](crate::ViewNode::spatial) or
/// `ViewNode::flat` (behind `gradmesh`). Every [`Geometry`](crate::Geometry) works in both
/// modes, and drags and picking behave the same. Mixing the modes is how a `Camera2d` HUD
/// overlays a `Camera3d` scene: the HUD nodes are flat on a render layer of their own (see
/// [`ViewNode::on_layer`](crate::ViewNode::on_layer)).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rendering {
    /// `Mesh3d` entities with PBR or gradient materials, seen by a `Camera3d`. Gradient shapes
    /// are lit, cast shadows and can be solid.
    #[default]
    Spatial,
    /// `Mesh2d` entities with unlit materials, seen by a `Camera2d`. Gradient shapes are built
    /// with `Shape::build_2d` and drawn with `GradMeshMaterial2d`, and a node's `z` orders it.
    /// Solid 3D shapes are drawn as flat silhouettes. Needs the sprite rendering of Bevy, which
    /// `DefaultPlugins` includes.
    #[cfg(feature = "gradmesh")]
    Flat,
}
