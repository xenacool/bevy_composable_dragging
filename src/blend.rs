//! Blending of node properties, and what happens to nodes that only exist in one of two views.
//!
//! Every animatable property of a [`ViewNode`](crate::ViewNode) implements [`Blend`], and
//! [`lerp_views`](crate::lerp_views) is the only place that knows the layout of a node, like
//! `lerpSvgx` is for Dragology's elements. To animate a property of your own, implement the trait
//! for its type.

use bevy::color::{LinearRgba, Mix, Oklaba};
use bevy::math::{Quat, Vec2, Vec3};
use bevy::prelude::{Color, Transform};

use crate::geometry::Geometry;
use crate::view::NodeId;

/// A value that can be blended with another value of the same type.
pub trait Blend: Sized {
    /// The value `t` of the way from `self` (at `0.0`) to `other` (at `1.0`).
    #[must_use]
    fn blend(&self, other: &Self, t: f32) -> Self;
}

impl Blend for f32 {
    fn blend(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Blend for Vec2 {
    fn blend(&self, other: &Self, t: f32) -> Self {
        self.lerp(*other, t)
    }
}

impl Blend for Vec3 {
    fn blend(&self, other: &Self, t: f32) -> Self {
        self.lerp(*other, t)
    }
}

/// Spherical interpolation along the shortest arc.
impl Blend for Quat {
    fn blend(&self, other: &Self, t: f32) -> Self {
        self.slerp(*other, t)
    }
}

/// Translation and scale blend linearly, rotation along the shortest arc.
impl Blend for Transform {
    fn blend(&self, other: &Self, t: f32) -> Self {
        Self {
            translation: self.translation.blend(&other.translation, t),
            rotation: self.rotation.blend(&other.rotation, t),
            scale: self.scale.blend(&other.scale, t),
        }
    }
}

/// Colors blend in Oklab, the closest Bevy color space to the Lab of Dragology. A fully
/// transparent color takes the hue of the other one and only fades, like SVG `none`.
impl Blend for LinearRgba {
    fn blend(&self, other: &Self, t: f32) -> Self {
        if t <= 0.0 {
            return *self;
        }
        if t >= 1.0 {
            return *other;
        }
        if self.alpha <= 0.0 && other.alpha > 0.0 {
            return Self {
                alpha: other.alpha * t,
                ..*other
            };
        }
        if other.alpha <= 0.0 && self.alpha > 0.0 {
            return Self {
                alpha: self.alpha * (1.0 - t),
                ..*self
            };
        }
        Self::from(Oklaba::from(*self).mix(&Oklaba::from(*other), t))
    }
}

impl Blend for Color {
    fn blend(&self, other: &Self, t: f32) -> Self {
        Self::from(self.to_linear().blend(&other.to_linear(), t))
    }
}

impl Blend for Geometry {
    fn blend(&self, other: &Self, t: f32) -> Self {
        self.lerp(other, t)
    }
}

#[cfg(feature = "gradmesh")]
impl Blend for bevy_gradmesh::Style {
    fn blend(&self, other: &Self, t: f32) -> Self {
        self.lerp(other, t)
    }
}

/// A missing value takes the other side, as an undefined property does in Dragology.
impl<T: Blend + Clone> Blend for Option<T> {
    fn blend(&self, other: &Self, t: f32) -> Self {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.blend(b, t)),
            (Some(value), None) | (None, Some(value)) => Some(value.clone()),
            (None, None) => None,
        }
    }
}

/// What a node does when it exists in only one of the two views that are blended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
    /// Fades in or out by scaling its opacity, and is dropped once it is invisible. This is what
    /// Dragology does with elements that exist in one state only.
    #[default]
    Fade,
    /// Switches at the midpoint of the blend.
    Pop,
    /// Grows from (or shrinks into) the node with the given id: scale zero and opacity zero at
    /// that node's transform, as `dragologyEmergeFrom` does.
    Emerge {
        /// The node the node emerges from.
        from: NodeId,
    },
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use super::*;

    #[test]
    fn numbers_and_vectors_hit_endpoints_and_midpoint() {
        assert_eq!(2.0_f32.blend(&6.0, 0.0), 2.0);
        assert_eq!(2.0_f32.blend(&6.0, 1.0), 6.0);
        assert_eq!(2.0_f32.blend(&6.0, 0.5), 4.0);
        assert_eq!(
            Vec2::ZERO.blend(&Vec2::new(2.0, 4.0), 0.5),
            Vec2::new(1.0, 2.0)
        );
        assert_eq!(Vec3::ZERO.blend(&Vec3::splat(2.0), 0.25), Vec3::splat(0.5));
    }

    #[test]
    fn rotations_take_the_shortest_arc() {
        let a = Quat::from_rotation_z(350_f32.to_radians());
        let b = Quat::from_rotation_z(10_f32.to_radians());
        let mid = a.blend(&b, 0.5);
        // Halfway between 350 and 10 degrees is 0 degrees, not 180.
        assert!(mid.angle_between(Quat::IDENTITY) < 1e-3, "{mid:?}");
        let back = b.blend(&a, 0.5);
        assert!(back.angle_between(Quat::IDENTITY) < 1e-3);
        let quarter = Quat::IDENTITY.blend(&Quat::from_rotation_z(PI), 0.5);
        assert!((quarter.angle_between(Quat::IDENTITY) - FRAC_PI_2).abs() < 1e-3);
    }

    #[test]
    fn transforms_blend_each_part() {
        let a = Transform::from_xyz(0.0, 0.0, 0.0);
        let b = Transform::from_xyz(4.0, 0.0, 0.0).with_scale(Vec3::splat(3.0));
        let mid = a.blend(&b, 0.5);
        assert_eq!(mid.translation, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(mid.scale, Vec3::splat(2.0));
    }

    #[test]
    fn colors_blend_in_oklab() {
        let red = LinearRgba::new(1.0, 0.0, 0.0, 1.0);
        let blue = LinearRgba::new(0.0, 0.0, 1.0, 1.0);
        assert_eq!(red.blend(&blue, 0.0), red);
        let mid = red.blend(&blue, 0.5);
        let linear_mid = LinearRgba::new(0.5, 0.0, 0.5, 1.0);
        assert!(mid != linear_mid, "Oklab differs from a linear mix");
        assert!(mid.red > 0.0 && mid.blue > 0.0);
        assert_eq!(red.blend(&blue, 1.0), blue);
    }

    #[test]
    fn a_transparent_color_only_fades() {
        let green = LinearRgba::new(0.0, 1.0, 0.0, 1.0);
        let none = LinearRgba::new(1.0, 0.0, 0.0, 0.0);
        let mid = none.blend(&green, 0.5);
        assert_eq!((mid.red, mid.green, mid.blue), (0.0, 1.0, 0.0));
        assert!((mid.alpha - 0.5).abs() < 1e-6);
        let faded = green.blend(&none, 0.25);
        assert_eq!(faded.green, 1.0);
        assert!((faded.alpha - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_missing_option_takes_the_other_side() {
        assert_eq!(Some(2.0_f32).blend(&Some(4.0), 0.5), Some(3.0));
        assert_eq!(Some(2.0_f32).blend(&None, 0.9), Some(2.0));
        assert_eq!(None.blend(&Some(4.0_f32), 0.1), Some(4.0));
        assert_eq!(None::<f32>.blend(&None, 0.5), None);
    }

    #[test]
    fn geometry_blends_through_the_morph() {
        assert_eq!(
            Geometry::Rectangle.blend(&Geometry::Circle, 0.25),
            Geometry::Rectangle
        );
    }

    #[test]
    fn the_default_presence_fades() {
        assert_eq!(Presence::default(), Presence::Fade);
    }
}
