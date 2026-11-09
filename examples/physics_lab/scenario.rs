//! What a trial is made of: the shapes of the palette, the colliders built from their meshes and
//! the rig they are stepped in.

use avian3d::prelude::Collider;
use bevy::prelude::*;
use bevy_gradmesh::{Shape, ShapeDetail, ShapePart};

/// A shape of the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    Sphere,
    Cuboid,
    Cylinder,
    Cone,
    Capsule,
    Torus,
    Pyramid,
    Disc,
    Ring,
    Star,
    Compound,
}

impl Primitive {
    /// Every shape of the palette.
    pub const ALL: [Self; 11] = [
        Self::Sphere,
        Self::Cuboid,
        Self::Cylinder,
        Self::Cone,
        Self::Capsule,
        Self::Torus,
        Self::Pyramid,
        Self::Disc,
        Self::Ring,
        Self::Star,
        Self::Compound,
    ];

    /// The name shown in tables.
    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "sphere",
            Self::Cuboid => "cuboid",
            Self::Cylinder => "cylinder",
            Self::Cone => "cone",
            Self::Capsule => "capsule",
            Self::Torus => "torus",
            Self::Pyramid => "pyramid",
            Self::Disc => "disc",
            Self::Ring => "ring",
            Self::Star => "star",
            Self::Compound => "compound",
        }
    }

    /// Whether the shape is its own convex hull.
    #[cfg(test)]
    pub fn convex(self) -> bool {
        matches!(
            self,
            Self::Sphere
                | Self::Cuboid
                | Self::Cylinder
                | Self::Cone
                | Self::Capsule
                | Self::Pyramid
                | Self::Disc
        )
    }

    /// The shape, `size` across at its widest, built at `detail`.
    pub fn shape(self, size: f32, detail: ShapeDetail) -> Shape {
        let half = size / 2.0;
        match self {
            Self::Sphere => Shape::sphere(half, detail),
            Self::Cuboid => Shape::cuboid(Vec3::splat(size), 0.0, detail),
            Self::Cylinder => Shape::cylinder(half, size, detail),
            Self::Cone => Shape::cone(half, size, detail),
            Self::Capsule => Shape::capsule(size / 4.0, size, detail),
            Self::Torus => Shape::torus(size * 0.35, size * 0.15, 0.0, std::f32::consts::TAU),
            Self::Pyramid => Shape::pyramid(4, half, size, detail),
            Self::Disc => Shape::disc(half, detail),
            Self::Ring => Shape::ring(half, half / 2.0, detail),
            Self::Star => Shape::star(5, half, half / 2.0, detail),
            Self::Compound => Shape::compound(vec![
                ShapePart::new(Shape::cuboid(
                    Vec3::new(size, size / 2.0, size),
                    0.0,
                    detail,
                ))
                .with_transform(Transform::from_xyz(0.0, -size / 4.0, 0.0)),
                ShapePart::new(Shape::sphere(size / 4.0, detail))
                    .with_transform(Transform::from_xyz(0.0, size / 4.0, 0.0)),
            ]),
        }
    }
}

/// How a collider is built from a mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColliderKind {
    Trimesh,
    ConvexHull,
    Decomposition,
}

impl ColliderKind {
    /// Every way of building a collider.
    pub const ALL: [Self; 3] = [Self::Trimesh, Self::ConvexHull, Self::Decomposition];

    /// The name shown in tables.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trimesh => "trimesh",
            Self::ConvexHull => "hull",
            Self::Decomposition => "decomposition",
        }
    }

    /// The collider of `mesh`, or `None` when avian cannot build one from it.
    pub fn build(self, mesh: &Mesh) -> Option<Collider> {
        match self {
            Self::Trimesh => Collider::trimesh_from_mesh(mesh),
            Self::ConvexHull => Collider::convex_hull_from_mesh(mesh),
            Self::Decomposition => Collider::convex_decomposition_from_mesh(mesh),
        }
    }
}

/// The detail levels of gradmesh, from coarsest to finest.
pub const DETAILS: [ShapeDetail; 5] = [
    ShapeDetail::Minimal,
    ShapeDetail::Low,
    ShapeDetail::Medium,
    ShapeDetail::High,
    ShapeDetail::Extreme,
];

/// The name of a detail level.
pub fn detail_label(detail: ShapeDetail) -> &'static str {
    match detail {
        ShapeDetail::Minimal => "minimal",
        ShapeDetail::Low => "low",
        ShapeDetail::Medium => "medium",
        ShapeDetail::High => "high",
        ShapeDetail::Extreme => "extreme",
    }
}
