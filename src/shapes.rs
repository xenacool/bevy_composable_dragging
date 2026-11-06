//! Meshes and materials for the shape keys.

use std::collections::HashMap;

use bevy::color::LinearRgba;
use bevy::prelude::*;

use crate::view::{ShapeKey, ViewNode};

#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
use bevy::color::Srgba;
#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
use bevy_gradmesh::{GradMeshMaterial, Shape, ShapeDetail, Style};

impl ShapeKey {
    /// Builds the unit-sized mesh of the shape.
    fn build_mesh(self) -> Mesh {
        match self {
            Self::Rectangle => Rectangle::new(1.0, 1.0).into(),
            Self::Circle => Circle::new(0.5).mesh().resolution(64).into(),
            #[cfg(feature = "shapes-3d")]
            Self::GradSphere => Shape::sphere(0.5, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-3d")]
            Self::GradCuboid => Shape::cuboid(Vec3::ONE, 0.08, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-3d")]
            Self::GradCylinder => Shape::cylinder(0.5, 1.0, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-3d")]
            Self::GradCone => Shape::cone(0.5, 1.0, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-2d")]
            Self::GradDisc => Shape::disc(0.5, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-2d")]
            Self::GradRectangle => Shape::rectangle(Vec2::ONE, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-2d")]
            Self::GradStar => Shape::star(5, 0.5, 0.22, ShapeDetail::Medium).build(),
            #[cfg(feature = "shapes-2d")]
            Self::GradRing => Shape::ring(0.5, 0.32, ShapeDetail::Medium).build(),
        }
    }

    /// Whether the shape is drawn with the gradient material of `bevy_gradmesh`.
    #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
    fn is_gradient(self) -> bool {
        !matches!(self, Self::Rectangle | Self::Circle)
    }
}

/// The meshes of the shapes in use, built on first use and shared by all nodes.
#[derive(Resource, Default)]
pub(crate) struct ShapeMeshes(HashMap<ShapeKey, Handle<Mesh>>);

impl ShapeMeshes {
    /// The shared mesh of `shape`.
    pub(crate) fn get(&mut self, shape: ShapeKey, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.0
            .entry(shape)
            .or_insert_with(|| meshes.add(shape.build_mesh()))
            .clone()
    }
}

fn flat_material(color: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: color.into(),
        unlit: true,
        alpha_mode: if color.alpha < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        ..default()
    }
}

/// A dual-directional gradient style from a darker shade of `color` at the bottom to `color` at
/// the top. Nodes without a style of their own are drawn with it, so blending their colors blends
/// their gradients too.
#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
pub(crate) fn gradient_style(color: LinearRgba) -> Style {
    let light = Srgba::from(color);
    let dark = bevy::color::Color::srgb(light.red * 0.35, light.green * 0.35, light.blue * 0.35);
    let bright = bevy::color::Color::srgb(light.red, light.green, light.blue);
    Style::dual_directional_up(dark, bright)
}

/// The resources needed to give nodes their mesh and material.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Looks<'w> {
    shapes: ResMut<'w, ShapeMeshes>,
    meshes: ResMut<'w, Assets<Mesh>>,
    flat: ResMut<'w, Assets<StandardMaterial>>,
    #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
    gradient: ResMut<'w, Assets<GradMeshMaterial>>,
}

impl Looks<'_> {
    /// The mesh component for `shape`.
    pub(crate) fn mesh(&mut self, shape: ShapeKey) -> Mesh3d {
        Mesh3d(self.shapes.get(shape, &mut self.meshes))
    }

    /// Whether the material of `node` depends on more than its shape and color, so it has to be
    /// rebuilt whenever the node is synced.
    pub(crate) fn has_own_style<S>(node: &ViewNode<S>) -> bool {
        #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
        return node.style.is_some() && node.shape.is_gradient();
        #[cfg(not(any(feature = "shapes-2d", feature = "shapes-3d")))]
        {
            let _ = node;
            false
        }
    }

    /// Gives the entity the material of `node`, replacing the other kind of material.
    ///
    /// Gradient shapes are drawn with the style of the node, or with the gradient of its color
    /// when it has none.
    pub(crate) fn apply_material<S>(&mut self, entity: &mut EntityCommands, node: &ViewNode<S>) {
        #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
        if node.shape.is_gradient() {
            let material = node
                .style
                .as_ref()
                .map_or_else(|| gradient_style(node.color).build(), Style::build);
            entity
                .insert(MeshMaterial3d(self.gradient.add(material)))
                .remove::<MeshMaterial3d<StandardMaterial>>();
            return;
        }
        entity.insert(MeshMaterial3d(self.flat.add(flat_material(node.color))));
        #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
        entity.remove::<MeshMaterial3d<GradMeshMaterial>>();
    }
}
