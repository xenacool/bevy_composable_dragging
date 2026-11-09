//! The geometry a [`ViewNode`](crate::ViewNode) is drawn with.

#[cfg(feature = "pbr")]
use std::hash::{Hash, Hasher};

use bevy::math::Vec3;
#[cfg(feature = "pbr")]
use bevy::mesh::VertexAttributeValues;
#[cfg(feature = "pbr")]
use bevy::prelude::{Assets, Handle, Mesh};
#[cfg(feature = "gradmesh")]
use bevy_gradmesh::Shape;

#[cfg(feature = "gradmesh")]
use crate::sdf::SdfShape;

/// The thinnest half extent of [`Geometry::bounds`], so flat shapes can still be hit by a ray.
const MIN_HALF_THICKNESS: f32 = 0.02;

/// The number of steps a morph between two shapes is quantized to, which bounds the number of
/// distinct meshes a blend builds.
#[cfg(feature = "gradmesh")]
const MORPH_STEPS: f32 = 32.0;

/// A local-space box around a [`Geometry`], used to pick nodes with a ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// The center of the box in the node's local space.
    pub center: Vec3,
    /// Half the size of the box along each local axis, never thinner than a small minimum.
    pub half_extents: Vec3,
    /// Whether the outline is round, so pointer hits are tested against an ellipsoid instead of
    /// the box.
    pub round: bool,
}

impl Bounds {
    fn new(center: Vec3, half_extents: Vec3, round: bool) -> Self {
        Self {
            center,
            half_extents: half_extents.max(Vec3::splat(MIN_HALF_THICKNESS)),
            round,
        }
    }
}

/// A mesh made by the application, such as generated terrain, that nodes can be drawn with.
///
/// The mesh is an asset: the application adds it to `Assets<Mesh>` and the view function holds
/// the [`Handle`], which is all a clone copies. Two values are the same geometry when their
/// handles are the same asset, so give a changed mesh a new asset, for example by adding the
/// rebuilt mesh when what it is made from is edited, and the old mesh is freed once no node
/// draws it. This is a stopgap for the missing `Shape::from_mesh` of `bevy_gradmesh`.
///
/// A mesh that is drawn with a gradient material (see [`Geometry::Mesh`]) needs the attributes
/// `bevy_gradmesh` gives its own meshes: positions, normals and `UV_0`, and optionally vertex
/// colors.
///
/// This needs the `pbr` feature, which brings Bevy's meshes.
#[cfg(feature = "pbr")]
#[derive(Clone, Debug)]
pub struct MeshGeometry {
    handle: Handle<Mesh>,
    center: Vec3,
    half_extents: Vec3,
}

#[cfg(feature = "pbr")]
impl MeshGeometry {
    /// The mesh behind `handle`, whose data is `mesh`. Its bounds are measured from its
    /// positions once, here.
    #[must_use]
    pub fn new(handle: Handle<Mesh>, mesh: &Mesh) -> Self {
        let (center, half_extents) = measure_mesh(mesh);
        Self {
            handle,
            center,
            half_extents,
        }
    }

    /// Adds `mesh` to `meshes` and returns the geometry that draws it.
    #[must_use]
    pub fn add(meshes: &mut Assets<Mesh>, mesh: Mesh) -> Self {
        let (center, half_extents) = measure_mesh(&mesh);
        Self {
            handle: meshes.add(mesh),
            center,
            half_extents,
        }
    }

    /// The handle of the mesh.
    #[must_use]
    pub fn handle(&self) -> &Handle<Mesh> {
        &self.handle
    }
}

#[cfg(feature = "pbr")]
impl PartialEq for MeshGeometry {
    fn eq(&self, other: &Self) -> bool {
        self.handle.id() == other.handle.id()
    }
}

#[cfg(feature = "pbr")]
impl Eq for MeshGeometry {}

#[cfg(feature = "pbr")]
impl Hash for MeshGeometry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.handle.id().hash(state);
    }
}

/// The center and half extents of the box around the positions of `mesh`, which are zero for a
/// mesh without positions.
#[cfg(feature = "pbr")]
fn measure_mesh(mesh: &Mesh) -> (Vec3, Vec3) {
    let Some(VertexAttributeValues::Float32x3(points)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    if points.is_empty() {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let (low, high) = points
        .iter()
        .map(|point| Vec3::from(*point))
        .fold((Vec3::MAX, Vec3::MIN), |(low, high), point| {
            (low.min(point), high.max(point))
        });
    ((low + high) / 2.0, (high - low) / 2.0)
}

/// What a node looks like: a unit shape scaled by its transform, any `bevy_gradmesh` shape, or a
/// mesh of your own.
///
/// [`Rectangle`](Self::Rectangle) and [`Circle`](Self::Circle) are unit sized and flat, and are
/// sized by the node's scale. `Grad` shapes are drawn at their own dimensions in
/// world units; prefer animating the node's scale to animating the shape, since every distinct
/// morphed shape is a distinct mesh.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Geometry {
    /// A unit square in the XY plane.
    Rectangle,
    /// A circle with unit diameter in the XY plane.
    Circle,
    /// Any `bevy_gradmesh` shape, 2D or 3D, drawn with its gradient material.
    #[cfg(feature = "gradmesh")]
    Grad(Shape),
    /// A flat rectangle or ellipse with rounded corners and a border, drawn per pixel from a
    /// signed distance field. It is drawn at its own dimensions in world units, and two of the
    /// same kind blend by their numbers.
    #[cfg(feature = "gradmesh")]
    Sdf(SdfShape),
    /// A mesh of your own, drawn at its own dimensions. With the `gradmesh` feature it is drawn
    /// with the gradient material, so its style applies; it never morphs, and a blend between
    /// two meshes takes the nearer one. It needs the `pbr` feature.
    #[cfg(feature = "pbr")]
    Mesh(MeshGeometry),
}

#[cfg(feature = "gradmesh")]
impl From<Shape> for Geometry {
    fn from(shape: Shape) -> Self {
        Self::Grad(shape)
    }
}

#[cfg(feature = "gradmesh")]
impl From<SdfShape> for Geometry {
    fn from(shape: SdfShape) -> Self {
        Self::Sdf(shape)
    }
}

#[cfg(feature = "pbr")]
impl From<MeshGeometry> for Geometry {
    fn from(mesh: MeshGeometry) -> Self {
        Self::Mesh(mesh)
    }
}

impl Geometry {
    /// Blends this geometry towards `other` by `t`.
    ///
    /// Two `Grad` shapes morph with `Shape::lerp`, with `t` quantized to 1/32 steps, and polygons
    /// with different point counts are resampled by gradmesh. Everything else, including shapes
    /// of different kinds, comes from the nearer state.
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        match (self, other) {
            #[cfg(feature = "gradmesh")]
            (Self::Grad(a), Self::Grad(b)) => {
                let steps = (t.clamp(0.0, 1.0) * MORPH_STEPS).round() / MORPH_STEPS;
                Self::Grad(a.lerp(b, steps))
            }
            #[cfg(feature = "gradmesh")]
            (Self::Sdf(a), Self::Sdf(b)) if a.is_ellipse() == b.is_ellipse() => {
                let steps = (t.clamp(0.0, 1.0) * MORPH_STEPS).round() / MORPH_STEPS;
                Self::Sdf(a.lerp(b, steps))
            }
            _ if t < 0.5 => self.clone(),
            _ => other.clone(),
        }
    }

    /// The local-space bounds of the geometry.
    ///
    /// A `Grad` shape remembers its bounds, so asking again is free.
    #[must_use]
    pub fn bounds(&self) -> Bounds {
        match self {
            Self::Rectangle => Bounds::new(Vec3::ZERO, Vec3::new(0.5, 0.5, 0.0), false),
            Self::Circle => Bounds::new(Vec3::ZERO, Vec3::new(0.5, 0.5, 0.0), true),
            #[cfg(feature = "gradmesh")]
            Self::Grad(shape) => Bounds::new(
                shape.bounds().center(),
                shape.bounds().half_extents(),
                shape.is_round(),
            ),
            #[cfg(feature = "gradmesh")]
            Self::Sdf(shape) => Bounds::new(
                Vec3::ZERO,
                (shape.size() / 2.0).extend(0.0),
                shape.is_ellipse(),
            ),
            #[cfg(feature = "pbr")]
            Self::Mesh(mesh) => Bounds::new(mesh.center, mesh.half_extents, false),
        }
    }

    /// Whether the geometry is drawn with the gradient material of `bevy_gradmesh`.
    #[cfg(feature = "gradmesh")]
    pub(crate) fn is_gradient(&self) -> bool {
        matches!(self, Self::Grad(_) | Self::Mesh(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_shapes_have_unit_bounds() {
        let rectangle = Geometry::Rectangle.bounds();
        assert!(!rectangle.round);
        assert_eq!(
            rectangle.half_extents.truncate(),
            bevy::math::Vec2::splat(0.5)
        );
        assert!(
            rectangle.half_extents.z > 0.0,
            "flat shapes keep a thickness"
        );
        assert!(Geometry::Circle.bounds().round);
    }

    #[test]
    fn unmorphable_geometries_come_from_the_nearer_one() {
        assert_eq!(
            Geometry::Rectangle.lerp(&Geometry::Circle, 0.25),
            Geometry::Rectangle
        );
        assert_eq!(
            Geometry::Rectangle.lerp(&Geometry::Circle, 0.75),
            Geometry::Circle
        );
    }

    #[cfg(feature = "pbr")]
    fn mesh_of(meshes: &mut Assets<Mesh>, width: f32, height: f32) -> Geometry {
        Geometry::Mesh(MeshGeometry::add(
            meshes,
            Mesh::from(bevy::prelude::Rectangle::new(width, height)),
        ))
    }

    #[cfg(feature = "pbr")]
    #[test]
    #[allow(
        clippy::mutable_key_type,
        reason = "the hash of a mesh never reads the bounds it remembers"
    )]
    fn a_mesh_is_identified_by_its_asset_alone() {
        use std::collections::HashSet;
        let mut meshes = Assets::<Mesh>::default();
        let a = mesh_of(&mut meshes, 2.0, 4.0);
        assert_eq!(a, a.clone(), "a clone is the same asset");
        let b = mesh_of(&mut meshes, 2.0, 4.0);
        assert_ne!(a, b, "the same data in another asset is another geometry");
        let set: HashSet<Geometry> = [a.clone(), a, b].into();
        assert_eq!(set.len(), 2);
    }

    #[cfg(feature = "pbr")]
    #[test]
    fn the_bounds_of_a_mesh_are_measured_from_its_positions() {
        let mut meshes = Assets::<Mesh>::default();
        let bounds = mesh_of(&mut meshes, 2.0, 4.0).bounds();
        assert!(bounds.center.length() < 1e-6);
        assert!((bounds.half_extents.x - 1.0).abs() < 1e-6);
        assert!((bounds.half_extents.y - 2.0).abs() < 1e-6);
        assert!(bounds.half_extents.z > 0.0, "a flat mesh keeps a thickness");
        assert!(!bounds.round);
        let empty = Geometry::Mesh(MeshGeometry::add(
            &mut meshes,
            Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            ),
        ));
        assert_eq!(empty.bounds().center, Vec3::ZERO);
    }

    #[cfg(feature = "pbr")]
    #[test]
    fn a_mesh_never_morphs_and_a_blend_takes_the_nearer_one() {
        let mut meshes = Assets::<Mesh>::default();
        let (a, b) = (
            mesh_of(&mut meshes, 1.0, 1.0),
            mesh_of(&mut meshes, 3.0, 3.0),
        );
        assert_eq!(a.lerp(&b, 0.25), a);
        assert_eq!(a.lerp(&b, 0.75), b);
        assert_eq!(a.lerp(&Geometry::Circle, 0.9), Geometry::Circle);
    }

    #[cfg(feature = "pbr")]
    #[test]
    fn a_mesh_geometry_exposes_its_handle() {
        let mut meshes = Assets::<Mesh>::default();
        let mesh = MeshGeometry::add(
            &mut meshes,
            Mesh::from(bevy::prelude::Rectangle::new(1.0, 1.0)),
        );
        assert!(meshes.contains(mesh.handle()));
        assert_eq!(Geometry::from(mesh.clone()), Geometry::Mesh(mesh));
    }

    #[cfg(feature = "gradmesh")]
    mod grad {
        use bevy::math::{Vec2, Vec3};
        use bevy_gradmesh::{Shape, ShapeDetail};

        use super::*;

        const DETAIL: ShapeDetail = ShapeDetail::Low;

        fn grad(shape: Shape) -> Geometry {
            Geometry::Grad(shape)
        }

        #[test]
        #[allow(
            clippy::mutable_key_type,
            reason = "the hash of a shape never reads the bounds it remembers"
        )]
        fn equal_shapes_are_equal_and_hash_alike() {
            use std::collections::HashSet;
            let a = grad(Shape::sphere(1.0, DETAIL));
            let b = grad(Shape::sphere(1.0, DETAIL));
            assert_eq!(a, b);
            let set: HashSet<Geometry> = [a, b, grad(Shape::sphere(2.0, DETAIL))].into();
            assert_eq!(set.len(), 2);
        }

        #[test]
        fn matching_shapes_morph() {
            let a = grad(Shape::sphere(1.0, DETAIL));
            let b = grad(Shape::sphere(3.0, DETAIL));
            assert_eq!(a.lerp(&b, 0.0), a);
            assert_eq!(a.lerp(&b, 1.0), b);
            assert_eq!(a.lerp(&b, 0.5), grad(Shape::sphere(2.0, DETAIL)));
        }

        #[test]
        fn morphs_are_quantized() {
            let a = grad(Shape::sphere(1.0, DETAIL));
            let b = grad(Shape::sphere(3.0, DETAIL));
            assert_eq!(a.lerp(&b, 0.5), a.lerp(&b, 0.5 + 0.004));
        }

        #[test]
        fn shapes_of_different_kinds_come_from_the_nearer_one() {
            let sphere = grad(Shape::sphere(1.0, DETAIL));
            let cuboid = grad(Shape::cuboid(Vec3::ONE, 0.0, DETAIL));
            assert_eq!(sphere.lerp(&cuboid, 0.4), sphere);
            assert_eq!(sphere.lerp(&cuboid, 0.6), cuboid);
            assert_eq!(sphere.lerp(&Geometry::Circle, 0.9), Geometry::Circle);
        }

        #[test]
        fn polygons_with_different_point_counts_morph() {
            let polygon = |sides: usize| {
                let points = (0..sides)
                    .map(|i| {
                        let angle = std::f32::consts::TAU * i as f32 / sides as f32;
                        Vec2::from_angle(angle)
                    })
                    .collect();
                grad(Shape::arbitrary_polygon(points, DETAIL))
            };
            let triangle = polygon(3);
            let square = polygon(4);
            assert_eq!(triangle.lerp(&square, 0.0), triangle);
            assert_eq!(triangle.lerp(&square, 1.0), square);
            let middle = triangle.lerp(&square, 0.5);
            assert_ne!(middle, triangle);
            assert_ne!(middle, square);
        }

        #[test]
        fn bounds_come_from_gradmesh() {
            let sphere = grad(Shape::sphere(1.5, DETAIL)).bounds();
            assert!(sphere.round);
            assert!((sphere.half_extents - Vec3::splat(1.5)).abs().max_element() < 0.05);

            let cuboid = grad(Shape::cuboid(Vec3::new(2.0, 4.0, 6.0), 0.0, DETAIL)).bounds();
            assert!(!cuboid.round);
            assert!(
                (cuboid.half_extents - Vec3::new(1.0, 2.0, 3.0))
                    .abs()
                    .max_element()
                    < 1e-3
            );

            let cylinder = grad(Shape::cylinder(1.0, 3.0, DETAIL)).bounds();
            assert!((cylinder.half_extents.y - 1.5).abs() < 1e-3);
            assert!((cylinder.half_extents.x - 1.0).abs() < 0.05);
        }

        #[test]
        fn a_flat_polygon_keeps_a_thickness() {
            let square = vec![
                Vec2::new(-1.0, -1.0),
                Vec2::new(1.0, -1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(-1.0, 1.0),
            ];
            let bounds = grad(Shape::arbitrary_polygon(square, DETAIL)).bounds();
            assert!((bounds.half_extents.x - 1.0).abs() < 1e-3);
            assert!(bounds.half_extents.z >= MIN_HALF_THICKNESS);
        }
    }
}
