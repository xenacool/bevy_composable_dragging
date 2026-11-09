//! Meshes and materials for the shape keys.

use std::collections::HashMap;

use bevy::color::LinearRgba;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::geometry::Geometry;
use crate::rendering::Rendering;
use crate::view::ViewNode;

#[cfg(feature = "gradmesh")]
use bevy::sprite_render::{AlphaMode2d, ColorMaterial, MeshMaterial2d};
#[cfg(feature = "gradmesh")]
use bevy_gradmesh::{
    GradMeshMaterial, GradMeshMaterial2d, GradMeshMaterialPool, SdfMaterial2d, Style,
};

impl Geometry {
    /// Builds the mesh of the geometry, flat for `Mesh2d` or in 3D otherwise.
    #[cfg_attr(not(feature = "gradmesh"), allow(unused_variables))]
    fn build_mesh(&self, flat: bool) -> Mesh {
        match self {
            Self::Rectangle => Rectangle::new(1.0, 1.0).into(),
            Self::Circle => Circle::new(0.5).mesh().resolution(64).into(),
            #[cfg(feature = "gradmesh")]
            Self::Grad(shape) if flat => shape.build_2d(),
            #[cfg(feature = "gradmesh")]
            Self::Grad(shape) => shape.build(),
            #[cfg(feature = "gradmesh")]
            Self::Sdf(shape) => Rectangle::from_size(shape.quad_size()).into(),
            Self::Mesh(mesh) => mesh.mesh().clone(),
        }
    }
}

/// The meshes of the geometries in use, built on first use and shared by all nodes.
///
/// The cache only remembers asset ids, keyed by geometry and by whether the mesh is flat. The
/// entities hold the strong handles, so a mesh that no node uses any more is freed, and rebuilt
/// if a node needs it again.
#[derive(Resource, Default)]
pub(crate) struct ShapeMeshes(HashMap<(Geometry, bool), AssetId<Mesh>>);

impl ShapeMeshes {
    /// The shared mesh of `geometry`.
    pub(crate) fn get(
        &mut self,
        geometry: &Geometry,
        flat: bool,
        meshes: &mut Assets<Mesh>,
    ) -> Handle<Mesh> {
        let key = (geometry.clone(), flat);
        if let Some(handle) = self
            .0
            .get(&key)
            .and_then(|id| meshes.get_strong_handle(*id))
        {
            return handle;
        }
        self.0.retain(|_, id| meshes.contains(*id));
        let handle = meshes.add(geometry.build_mesh(flat));
        self.0.insert(key, handle.id());
        handle
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

/// The number of steps the alpha of a fading gradient is quantized to.
#[cfg(feature = "gradmesh")]
const ALPHA_STEPS: f32 = 16.0;

/// The gradient of nodes without a style of their own: their color on top and a darker shade
/// below. Blending their colors therefore blends their gradients too.
#[cfg(feature = "gradmesh")]
pub(crate) fn gradient_style(color: LinearRgba) -> Style {
    Style::shaded(Color::from(color), 0.35)
}

/// Whether the rendering mode draws `Mesh2d` entities.
#[cfg(feature = "gradmesh")]
fn is_flat(mode: Rendering) -> bool {
    mode == Rendering::Flat
}

/// Whether the rendering mode draws `Mesh2d` entities, which needs the `gradmesh` feature.
#[cfg(not(feature = "gradmesh"))]
fn is_flat(_mode: Rendering) -> bool {
    false
}

/// Makes the entity cast shadows or not.
fn set_shadow_casting(entity: &mut EntityCommands, casts: bool) {
    if casts {
        entity.remove::<NotShadowCaster>();
    } else {
        entity.insert(NotShadowCaster);
    }
}

/// The resources needed to give nodes their mesh and material.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Looks<'w> {
    shapes: ResMut<'w, ShapeMeshes>,
    meshes: ResMut<'w, Assets<Mesh>>,
    flat: ResMut<'w, Assets<StandardMaterial>>,
    #[cfg(feature = "gradmesh")]
    gradient: ResMut<'w, Assets<GradMeshMaterial>>,
    #[cfg(feature = "gradmesh")]
    pool: ResMut<'w, GradMeshMaterialPool>,
    #[cfg(feature = "gradmesh")]
    rendering: Res<'w, Rendering>,
    #[cfg(feature = "gradmesh")]
    flat_2d: Option<ResMut<'w, Assets<ColorMaterial>>>,
    #[cfg(feature = "gradmesh")]
    gradient_2d: Option<ResMut<'w, Assets<GradMeshMaterial2d>>>,
    #[cfg(feature = "gradmesh")]
    sdf_2d: Option<ResMut<'w, Assets<SdfMaterial2d>>>,
}

impl Looks<'_> {
    /// The rendering mode of a node: its own, or the default of the plugin.
    #[cfg(feature = "gradmesh")]
    pub(crate) fn mode_of(&self, own: Option<Rendering>) -> Rendering {
        own.unwrap_or(*self.rendering)
    }

    /// The rendering mode of a node, which is always spatial without the `gradmesh` feature.
    #[cfg(not(feature = "gradmesh"))]
    #[allow(clippy::unused_self)]
    pub(crate) fn mode_of(&self, own: Option<Rendering>) -> Rendering {
        own.unwrap_or_default()
    }

    /// Gives the entity the mesh of `geometry`, as `Mesh2d` or `Mesh3d` depending on the
    /// rendering `mode`, replacing the other kind.
    pub(crate) fn apply_mesh(
        &mut self,
        entity: &mut EntityCommands,
        geometry: &Geometry,
        mode: Rendering,
    ) {
        let handle = self.shapes.get(geometry, is_flat(mode), &mut self.meshes);
        #[cfg(feature = "gradmesh")]
        if is_flat(mode) {
            entity.insert(Mesh2d(handle)).remove::<Mesh3d>();
            return;
        }
        entity.insert(Mesh3d(handle));
        #[cfg(feature = "gradmesh")]
        entity.remove::<Mesh2d>();
    }

    /// Gives the entity the material of `node`, drawn with `opacity` (the opacity of the node
    /// multiplied by those of its ancestors), replacing the other kind of material.
    ///
    /// Gradient shapes are drawn with the style of the node, or with the gradient of its color
    /// when it has none. Translucent ones fade through the alpha of the style, in steps so a fade
    /// does not need a new material every frame, and stop casting shadows.
    pub(crate) fn apply_material<S>(
        &mut self,
        entity: &mut EntityCommands,
        node: &ViewNode<S>,
        opacity: f32,
        mode: Rendering,
    ) {
        #[cfg(feature = "gradmesh")]
        if is_flat(mode) {
            entity
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .remove::<MeshMaterial3d<GradMeshMaterial>>();
            self.apply_flat_material(entity, node, opacity);
            return;
        }
        #[cfg(feature = "gradmesh")]
        entity
            .remove::<MeshMaterial2d<ColorMaterial>>()
            .remove::<MeshMaterial2d<GradMeshMaterial2d>>()
            .remove::<MeshMaterial2d<SdfMaterial2d>>();
        #[cfg(not(feature = "gradmesh"))]
        let _ = mode;
        #[cfg(feature = "gradmesh")]
        if node.geometry.is_gradient() {
            let mut style = node
                .style
                .clone()
                .unwrap_or_else(|| gradient_style(node.color));
            if opacity < 1.0 {
                style = style.with_alpha((opacity * ALPHA_STEPS).round() / ALPHA_STEPS);
            }
            set_shadow_casting(entity, node.casts_shadow && style.casts_shadows());
            let material = self.pool.get_or_add(&style, &mut self.gradient);
            entity
                .insert(MeshMaterial3d(material))
                .remove::<MeshMaterial3d<StandardMaterial>>();
            return;
        }
        set_shadow_casting(entity, node.casts_shadow);
        let mut color = node.color;
        color.alpha *= opacity;
        entity.insert(MeshMaterial3d(self.flat.add(flat_material(color))));
        #[cfg(feature = "gradmesh")]
        entity.remove::<MeshMaterial3d<GradMeshMaterial>>();
    }

    /// The `Mesh2d` counterpart of [`apply_material`](Self::apply_material): unlit colors and
    /// 2D gradients, which fade through their alpha.
    #[cfg(feature = "gradmesh")]
    fn apply_flat_material<S>(
        &mut self,
        entity: &mut EntityCommands,
        node: &ViewNode<S>,
        opacity: f32,
    ) {
        if let Geometry::Sdf(shape) = &node.geometry {
            let Some(sdf) = self.sdf_2d.as_mut() else {
                return;
            };
            let mut color = node.color;
            color.alpha *= opacity;
            entity
                .insert(MeshMaterial2d(sdf.add(shape.material(color))))
                .remove::<MeshMaterial2d<ColorMaterial>>()
                .remove::<MeshMaterial2d<GradMeshMaterial2d>>();
            return;
        }
        let (Some(flat), Some(gradient)) = (self.flat_2d.as_mut(), self.gradient_2d.as_mut())
        else {
            return;
        };
        if node.geometry.is_gradient() {
            let mut style = node
                .style
                .clone()
                .unwrap_or_else(|| gradient_style(node.color));
            if opacity < 1.0 {
                style = style.with_alpha((opacity * ALPHA_STEPS).round() / ALPHA_STEPS);
            }
            entity
                .insert(MeshMaterial2d(gradient.add(style.build_2d())))
                .remove::<MeshMaterial2d<ColorMaterial>>()
                .remove::<MeshMaterial2d<SdfMaterial2d>>();
        } else {
            let mut color = node.color;
            color.alpha *= opacity;
            entity
                .insert(MeshMaterial2d(flat.add(ColorMaterial {
                    color: color.into(),
                    alpha_mode: if color.alpha < 1.0 {
                        AlphaMode2d::Blend
                    } else {
                        AlphaMode2d::Opaque
                    },
                    ..default()
                })))
                .remove::<MeshMaterial2d<GradMeshMaterial2d>>()
                .remove::<MeshMaterial2d<SdfMaterial2d>>();
        }
    }
}
