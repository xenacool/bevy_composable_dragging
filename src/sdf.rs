//! Flat shapes that are evaluated per pixel from a signed distance field.
//!
//! [`SdfShape`] stores the shape parameters in an [`SdfMaterial2d`](bevy_gradmesh::SdfMaterial2d)
//! from gradmesh, with the `shade` value kept separately because it is applied from the node
//! color at render time.  Two shapes blend with
//! [`SdfMaterial2d::lerp`](bevy_gradmesh::SdfMaterial2d::lerp), which handles circle-to-rectangle
//! morphs through corner radius.

use std::hash::{Hash, Hasher};

use bevy::color::LinearRgba;
use bevy::math::Vec2;
use bevy::prelude::Color;
use bevy_gradmesh::SdfMaterial2d;

/// How far the quad of a shape extends past it on each side, in world units, so the anti-aliased
/// fringe of its edge is not clipped.
pub(crate) const PADDING: f32 = 2.0;

/// A flat rectangle or ellipse drawn from a signed distance field, so its rounded corners, its
/// border and its edge stay crisp at any size.
///
/// The shape parameters (size, corner radius, rim) are stored in an
/// [`SdfMaterial2d`](bevy_gradmesh::SdfMaterial2d) from gradmesh.  The `shade` value is kept
/// separately because it is applied from the node color at render time.  Two shapes blend with
/// [`SdfMaterial2d::lerp`](bevy_gradmesh::SdfMaterial2d::lerp), which handles circle-to-rectangle
/// morphs through corner radius, gradient direction interpolation, and dash pattern handling.
#[derive(Clone, Debug)]
pub struct SdfShape {
    material: SdfMaterial2d,
    /// How much the fill darkens toward the bottom, `0.0` (flat) to `1.0`.
    shade: f32,
}

impl SdfShape {
    /// A filled rectangle of `size`.
    #[must_use]
    pub fn rect(size: Vec2) -> Self {
        Self {
            material: SdfMaterial2d::rect(size).with_padding(PADDING),
            shade: 0.0,
        }
    }

    /// A filled ellipse that is `size` wide and high, a circle when both are equal.
    #[must_use]
    pub fn ellipse(size: Vec2) -> Self {
        Self {
            material: SdfMaterial2d::ellipse(size / 2.0).with_padding(PADDING),
            shade: 0.0,
        }
    }

    /// Rounds the corners of a rectangle with `radius`, which cannot exceed half its shortest
    /// side.
    #[must_use]
    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.material = self.material.with_corner_radius(radius);
        self
    }

    /// Draws only a border of `width`, inside the outline of the shape. `0.0` fills it.
    #[must_use]
    pub fn with_rim(mut self, width: f32) -> Self {
        self.material = self.material.with_rim(width);
        self
    }

    /// Darkens the bottom of the shape by `shade` of its color, from `0.0` (flat) to `1.0`, so
    /// the fill grades from the color at the top to a darker one at the bottom.
    #[must_use]
    pub fn with_shade(mut self, shade: f32) -> Self {
        self.shade = shade.clamp(0.0, 1.0);
        self
    }

    /// The width and height of the shape, without its padding.
    #[must_use]
    pub fn size(&self) -> Vec2 {
        self.material.size()
    }

    /// Whether the shape is an ellipse instead of a rectangle.
    #[must_use]
    pub fn is_ellipse(&self) -> bool {
        self.material.uniform.shape.x >= 0.5
    }

    /// The size of the quad the shape is drawn on, which has room for its fringe.
    #[must_use]
    pub fn quad_size(&self) -> Vec2 {
        self.material.quad_scale().truncate()
    }

    /// Blends towards `other` by `t`.
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            material: self.material.lerp(&other.material, t),
            shade: mix(self.shade, other.shade),
        }
    }

    /// Converts the shape into the underlying material, with `shade` applied as a vertical
    /// gradient from `color` at the top to a darker shade at the bottom.
    #[must_use]
    pub fn with_color(&self, color: Color) -> SdfMaterial2d {
        let linear: LinearRgba = color.into();
        if self.shade <= 0.0 {
            return self.material.clone().with_color(color);
        }
        let keep = 1.0 - self.shade;
        let bottom = Color::from(LinearRgba {
            red: linear.red * keep,
            green: linear.green * keep,
            blue: linear.blue * keep,
            alpha: linear.alpha,
        });
        self.material.clone().with_colors(bottom, color)
    }

    /// Returns the underlying material without color information, suitable for
    /// [`ViewNode::sdf`](crate::ViewNode::sdf).
    #[must_use]
    pub fn into_material(self) -> SdfMaterial2d {
        self.material
    }
}

impl PartialEq for SdfShape {
    fn eq(&self, other: &Self) -> bool {
        self.material.uniform == other.material.uniform
            && self.shade.to_bits() == other.shade.to_bits()
    }
}

impl Eq for SdfShape {}

impl Hash for SdfShape {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let u = &self.material.uniform;
        for v in &[
            u.color,
            u.secondary_color,
            u.shape,
            u.size,
            u.gradient,
            u.dash,
        ] {
            v.x.to_bits().hash(state);
            v.y.to_bits().hash(state);
            v.z.to_bits().hash(state);
            v.w.to_bits().hash(state);
        }
        self.shade.to_bits().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn shapes_are_equal_and_hash_alike_by_their_numbers() {
        let a = SdfShape::rect(Vec2::new(40.0, 20.0)).with_corner_radius(6.0);
        let b = SdfShape::rect(Vec2::new(40.0, 20.0)).with_corner_radius(6.0);
        assert_eq!(a, b);
        assert_ne!(a.clone(), b.clone().with_rim(2.0));
        assert_ne!(
            a.clone(),
            SdfShape::ellipse(Vec2::new(40.0, 20.0)).with_corner_radius(6.0)
        );
        let set: HashSet<SdfShape> = [a.clone(), b, a.with_shade(0.2)].into();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn builders_clamp_their_inputs() {
        let shape = SdfShape::rect(Vec2::ONE)
            .with_corner_radius(-3.0)
            .with_rim(-1.0)
            .with_shade(4.0);
        assert_eq!(shape.material.uniform.size.z, 0.0);
        assert_eq!(shape.material.uniform.shape.y, 0.0);
        assert_eq!(shape.shade, 1.0);
    }

    #[test]
    fn a_shaded_material_grades_between_two_colors() {
        let white = Color::WHITE;
        let flat = SdfShape::rect(Vec2::ONE).with_color(white);
        assert_eq!(flat.uniform.color, flat.uniform.secondary_color);
        let shaded = SdfShape::rect(Vec2::ONE).with_shade(0.5).with_color(white);
        assert_ne!(shaded.uniform.color, shaded.uniform.secondary_color);
        assert!(shaded.uniform.color.x < shaded.uniform.secondary_color.x);
    }

    #[test]
    fn blending_delegates_to_material() {
        let a = SdfShape::rect(Vec2::new(10.0, 10.0)).with_corner_radius(2.0);
        let b = SdfShape::rect(Vec2::new(30.0, 50.0)).with_corner_radius(6.0);
        let middle = a.lerp(&b, 0.5);
        assert_eq!(middle.size(), Vec2::new(20.0, 30.0));
        assert!(!middle.is_ellipse());
    }

    #[test]
    fn the_quad_has_room_for_the_fringe() {
        let quad = SdfShape::ellipse(Vec2::new(10.0, 20.0)).quad_size();
        assert!((quad.x - (10.0 + PADDING * 2.0)).abs() < 1e-4);
        assert!((quad.y - (20.0 + PADDING * 2.0)).abs() < 1e-4);
    }
}
