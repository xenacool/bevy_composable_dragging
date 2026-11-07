//! The space a drag happens in, and the pointer that operators see.
//!
//! A drag resolves the pointer ray to a world point inside a [`DragSpace`]: a plane with any
//! origin and normal, an axis line, or a plane that faces the camera. Operators then work on
//! world positions and node transforms, so a drag on the ground, along an axis and on a
//! transform gizmo share one code path.

use bevy::math::{Dir3, Vec2, Vec3};
use bevy::prelude::Ray3d;

/// A ray that is closer to parallel with an axis than this (the sine of the angle) cannot place
/// the pointer on it reliably.
const PARALLEL_LIMIT: f32 = 1e-4;

/// Where a drag moves the pointer to: a plane or an axis of the world.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum DragSpace {
    /// The pointer is where the ray hits the plane through `origin` with the given `normal`.
    Plane {
        /// A point on the plane.
        origin: Vec3,
        /// The direction the plane faces.
        normal: Dir3,
    },
    /// The pointer is the point of the line through `origin` along `direction` that is nearest
    /// to the ray.
    Axis {
        /// A point on the line.
        origin: Vec3,
        /// The direction of the line.
        direction: Dir3,
    },
    /// A plane through the dragged node that faces the camera, fixed when the drag is pressed.
    /// It is turned into a [`Plane`](Self::Plane) at press time and never resolves rays itself.
    Screen,
}

impl Default for DragSpace {
    fn default() -> Self {
        Self::world_plane(0.0)
    }
}

impl DragSpace {
    /// The XY plane at depth `z`, the space of drags that do not ask for another one.
    #[must_use]
    pub fn world_plane(z: f32) -> Self {
        Self::Plane {
            origin: Vec3::new(0.0, 0.0, z),
            normal: Dir3::Z,
        }
    }

    /// The plane through `origin` that faces `normal`.
    #[must_use]
    pub fn plane(origin: Vec3, normal: Dir3) -> Self {
        Self::Plane { origin, normal }
    }

    /// The line through `origin` along `direction`.
    #[must_use]
    pub fn axis(origin: Vec3, direction: Dir3) -> Self {
        Self::Axis { origin, direction }
    }

    /// A plane through the dragged node that faces the camera.
    #[must_use]
    pub fn screen() -> Self {
        Self::Screen
    }

    /// The space as it is during the drag. A [`Screen`](Self::Screen) space becomes the plane
    /// through `through` that faces against `camera_forward`; other spaces stay as they are.
    #[must_use]
    pub fn fixed(self, camera_forward: Dir3, through: Vec3) -> Self {
        match self {
            Self::Screen => Self::Plane {
                origin: through,
                normal: -camera_forward,
            },
            other => other,
        }
    }

    /// The point of the space the pointer ray lands on, or `None` when the ray does not reach it
    /// (a ray parallel to a plane, or pointing away from it, or an axis along the ray).
    #[must_use]
    pub fn resolve(&self, ray: Ray3d) -> Option<Vec3> {
        match *self {
            Self::Plane { origin, normal } => {
                let denominator = normal.dot(*ray.direction);
                if denominator.abs() < f32::EPSILON {
                    return None;
                }
                let distance = normal.dot(origin - ray.origin) / denominator;
                (distance >= 0.0).then(|| ray.get_point(distance))
            }
            Self::Axis { origin, direction } => {
                // The nearest points of two skew lines: solve for the parameter along the axis.
                let along = *direction;
                let toward = *ray.direction;
                let offset = origin - ray.origin;
                let cosine = along.dot(toward);
                let denominator = 1.0 - cosine * cosine;
                if denominator < PARALLEL_LIMIT * PARALLEL_LIMIT {
                    return None;
                }
                let distance = (cosine * toward.dot(offset) - along.dot(offset)) / denominator;
                Some(origin + along * distance)
            }
            Self::Screen => None,
        }
    }

    /// The point of the space nearest to `world`.
    #[must_use]
    pub fn project_point(&self, world: Vec3) -> Vec3 {
        match *self {
            Self::Plane { origin, normal } => world - *normal * normal.dot(world - origin),
            Self::Axis { origin, direction } => origin + *direction * direction.dot(world - origin),
            Self::Screen => world,
        }
    }

    /// The direction the space faces: the normal of a plane, the direction of an axis. A node
    /// that floats while dragged is lifted along it.
    #[must_use]
    pub fn normal(&self) -> Dir3 {
        match *self {
            Self::Plane { normal, .. } => normal,
            Self::Axis { direction, .. } => direction,
            Self::Screen => Dir3::Z,
        }
    }

    /// The origin of the space.
    #[must_use]
    pub fn origin(&self) -> Vec3 {
        match *self {
            Self::Plane { origin, .. } | Self::Axis { origin, .. } => origin,
            Self::Screen => Vec3::ZERO,
        }
    }

    /// The two unit axes of the 2D basis of the space, for hull, Delaunay and natural-neighbor
    /// weights. The basis of the XY plane is `(X, Y)`, and an axis has the direction as its first
    /// axis.
    fn basis(&self) -> (Vec3, Vec3) {
        match *self {
            Self::Plane { normal, .. } => {
                let reference = if normal.x.abs() < 0.9 {
                    Vec3::X
                } else {
                    Vec3::Y
                };
                let u = (reference - *normal * normal.dot(reference)).normalize();
                (u, normal.cross(u))
            }
            Self::Axis { direction, .. } => {
                let reference = if direction.x.abs() < 0.9 {
                    Vec3::X
                } else {
                    Vec3::Y
                };
                let v = (reference - *direction * direction.dot(reference)).normalize();
                (*direction, v)
            }
            Self::Screen => (Vec3::X, Vec3::Y),
        }
    }

    /// The coordinates of the point of the space nearest to `world`, in the 2D basis of the
    /// space: two perpendicular unit axes of the plane, or the direction of an axis and one
    /// perpendicular to it. Positions in the XY plane at depth `z` keep their `x` and `y`.
    #[must_use]
    pub fn local(&self, world: Vec3) -> Vec2 {
        let (u, v) = self.basis();
        let offset = world - self.origin();
        Vec2::new(offset.dot(u), offset.dot(v))
    }

    /// The world position of the local coordinates `local` (the inverse of [`local`](Self::local)
    /// for points of the space).
    #[must_use]
    pub fn from_local(&self, local: Vec2) -> Vec3 {
        let (u, v) = self.basis();
        self.origin() + u * local.x + v * local.y
    }
}

/// The pointer as operators see it: the ray through the cursor and the world point it resolves
/// to in the space of the drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// The ray from the camera through the cursor.
    pub ray: Ray3d,
    /// Where the ray lands in the [`DragSpace`] of the drag.
    pub point: Vec3,
}

impl Pointer {
    /// A pointer at `point`, with a ray that looks at it straight down the Z axis. This is how
    /// the pointer of a drag in the XY plane looks, and what tests use.
    #[must_use]
    pub fn at(point: impl Into<Vec3>) -> Self {
        let point = point.into();
        Self {
            ray: Ray3d::new(point + Vec3::Z * 10.0, Dir3::NEG_Z),
            point,
        }
    }

    /// The same pointer moved to `point`, keeping the ray.
    #[must_use]
    pub fn moved_to(self, point: Vec3) -> Self {
        Self { point, ..self }
    }
}

impl From<Vec2> for Pointer {
    fn from(point: Vec2) -> Self {
        Self::at(point.extend(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::Pointer;

    fn ray(origin: Vec3, toward: Vec3) -> Ray3d {
        Ray3d::new(origin, Dir3::new(toward - origin).unwrap())
    }

    #[test]
    fn the_world_plane_resolves_a_ray_to_its_hit() {
        let space = DragSpace::world_plane(2.0);
        let hit = space.resolve(ray(Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 0.0)));
        assert!(hit.unwrap().distance(Vec3::new(1.0, 2.0, 2.0)) < 1e-5);
        assert_eq!(DragSpace::default(), DragSpace::world_plane(0.0));
    }

    #[test]
    fn rays_that_cannot_reach_a_plane_resolve_to_nothing() {
        let space = DragSpace::world_plane(0.0);
        let parallel = Ray3d::new(Vec3::new(0.0, 0.0, 5.0), Dir3::X);
        assert!(space.resolve(parallel).is_none());
        let away = Ray3d::new(Vec3::new(0.0, 0.0, -5.0), Dir3::NEG_Z);
        assert!(space.resolve(away).is_none());
    }

    #[test]
    fn a_tilted_plane_resolves_to_points_on_it() {
        let normal = Dir3::new(Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let space = DragSpace::plane(Vec3::new(0.0, 1.0, 0.0), normal);
        let hit = space
            .resolve(ray(Vec3::new(3.0, 4.0, 5.0), Vec3::new(0.0, 0.5, 0.5)))
            .unwrap();
        assert!(normal.dot(hit - Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-4);
    }

    #[test]
    fn an_axis_resolves_to_the_point_nearest_the_ray() {
        let space = DragSpace::axis(Vec3::ZERO, Dir3::X);
        let hit = space
            .resolve(ray(Vec3::new(4.0, 3.0, 10.0), Vec3::new(4.0, 3.0, 0.0)))
            .unwrap();
        assert!(hit.distance(Vec3::new(4.0, 0.0, 0.0)) < 1e-4, "{hit}");
        // A slanted ray crossing above the axis resolves to the nearest point of the line.
        let slanted = space
            .resolve(ray(Vec3::new(0.0, 5.0, 5.0), Vec3::new(6.0, 0.0, 1.0)))
            .unwrap();
        assert!(slanted.y.abs() < 1e-5 && slanted.z.abs() < 1e-5);
    }

    #[test]
    fn an_axis_along_the_ray_does_not_resolve() {
        let space = DragSpace::axis(Vec3::ZERO, Dir3::Z);
        let along = Ray3d::new(Vec3::new(0.0, 0.0, 10.0), Dir3::NEG_Z);
        assert!(space.resolve(along).is_none());
        let nearly = Ray3d::new(
            Vec3::new(0.0, 0.0, 10.0),
            Dir3::new(Vec3::new(1e-6, 0.0, -1.0)).unwrap(),
        );
        assert!(space.resolve(nearly).is_none());
        let crossing = Ray3d::new(
            Vec3::new(1.0, 0.0, 10.0),
            Dir3::new(Vec3::new(-0.1, 0.0, -1.0)).unwrap(),
        );
        assert!(space.resolve(crossing).is_some());
    }

    #[test]
    fn a_screen_space_becomes_a_plane_facing_the_camera() {
        let fixed = DragSpace::screen().fixed(Dir3::NEG_Z, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(fixed, DragSpace::plane(Vec3::new(1.0, 2.0, 3.0), Dir3::Z));
        assert!(
            fixed
                .resolve(ray(Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 3.0)))
                .is_some()
        );
        assert!(
            DragSpace::screen()
                .resolve(Ray3d::new(Vec3::ZERO, Dir3::X))
                .is_none()
        );
        let other = DragSpace::world_plane(1.0);
        assert_eq!(other.fixed(Dir3::NEG_Z, Vec3::ZERO), other);
    }

    #[test]
    fn projection_finds_the_nearest_point_of_the_space() {
        let plane = DragSpace::world_plane(1.0);
        assert_eq!(
            plane.project_point(Vec3::new(2.0, 3.0, 9.0)),
            Vec3::new(2.0, 3.0, 1.0)
        );
        let axis = DragSpace::axis(Vec3::new(0.0, 1.0, 0.0), Dir3::X);
        assert_eq!(
            axis.project_point(Vec3::new(5.0, 7.0, 2.0)),
            Vec3::new(5.0, 1.0, 0.0)
        );
    }

    #[test]
    fn the_local_basis_of_the_xy_plane_is_x_and_y() {
        let space = DragSpace::world_plane(3.0);
        assert!(
            space
                .local(Vec3::new(2.0, -4.0, 3.0))
                .distance(Vec2::new(2.0, -4.0))
                < 1e-5
        );
        assert!(
            space
                .from_local(Vec2::new(2.0, -4.0))
                .distance(Vec3::new(2.0, -4.0, 3.0))
                < 1e-5
        );
    }

    #[test]
    fn local_coordinates_round_trip_on_tilted_planes_and_axes() {
        let tilted = DragSpace::plane(
            Vec3::new(1.0, 2.0, 3.0),
            Dir3::new(Vec3::new(0.3, -1.0, 0.6)).unwrap(),
        );
        for local in [Vec2::new(1.0, 2.0), Vec2::new(-3.0, 0.5)] {
            let world = tilted.from_local(local);
            assert!(tilted.project_point(world).distance(world) < 1e-4);
            assert!(tilted.local(world).distance(local) < 1e-4);
        }
        let ground = DragSpace::plane(Vec3::ZERO, Dir3::Y);
        let world = ground.from_local(Vec2::new(2.0, 3.0));
        assert!(world.y.abs() < 1e-5);
        assert!((world.x - 2.0).abs() < 1e-5 && (world.z + 3.0).abs() < 1e-5);
        let axis = DragSpace::axis(Vec3::new(1.0, 0.0, 0.0), Dir3::Y);
        assert!((axis.local(Vec3::new(1.0, 4.0, 0.0)).x - 4.0).abs() < 1e-5);
    }

    #[test]
    fn distances_in_the_local_basis_match_world_distances() {
        let space = DragSpace::plane(Vec3::ONE, Dir3::new(Vec3::new(1.0, 2.0, 3.0)).unwrap());
        let a = space.from_local(Vec2::new(0.5, 1.0));
        let b = space.from_local(Vec2::new(-2.0, 3.0));
        assert!((space.local(a).distance(space.local(b)) - a.distance(b)).abs() < 1e-4);
    }

    #[test]
    fn a_pointer_at_a_point_looks_down_the_z_axis() {
        let pointer = Pointer::at(Vec2::new(1.0, 2.0).extend(0.5));
        assert_eq!(pointer.point, Vec3::new(1.0, 2.0, 0.5));
        assert_eq!(*pointer.ray.direction, Vec3::NEG_Z);
        assert_eq!(
            Pointer::from(Vec2::new(3.0, 4.0)).point,
            Vec3::new(3.0, 4.0, 0.0)
        );
        assert_eq!(pointer.moved_to(Vec3::X).point, Vec3::X);
    }
}
