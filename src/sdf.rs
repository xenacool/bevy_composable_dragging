//! Flat shapes that are evaluated per pixel from a signed distance field.

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
/// The color of the node fills it. The shape is drawn flat, for
/// [`Rendering::Flat`](crate::Rendering::Flat); a spatial node of this kind is a plain rectangle.
/// Two shapes of the same kind blend by their size, corner radius, border and shade, so a panel
/// can animate its size and its border.
#[derive(Clone, Copy, Debug)]
pub struct SdfShape {
    ellipse: bool,
    size: Vec2,
    corner_radius: f32,
    rim: f32,
    shade: f32,
}

impl SdfShape {
    /// A filled rectangle of `size`.
    #[must_use]
    pub fn rect(size: Vec2) -> Self {
        Self {
            ellipse: false,
            size,
            corner_radius: 0.0,
            rim: 0.0,
            shade: 0.0,
        }
    }

    /// A filled ellipse that is `size` wide and high, a circle when both are equal.
    #[must_use]
    pub fn ellipse(size: Vec2) -> Self {
        Self {
            ellipse: true,
            ..Self::rect(size)
        }
    }

    /// Rounds the corners of a rectangle with `radius`, which cannot exceed half its shortest
    /// side.
    #[must_use]
    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }

    /// Draws only a border of `width`, inside the outline of the shape. `0.0` fills it.
    #[must_use]
    pub fn with_rim(mut self, width: f32) -> Self {
        self.rim = width.max(0.0);
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
        self.size
    }

    /// Whether the shape is an ellipse instead of a rectangle.
    #[must_use]
    pub fn is_ellipse(&self) -> bool {
        self.ellipse
    }

    /// The size of the quad the shape is drawn on, which has room for its fringe.
    pub(crate) fn quad_size(&self) -> Vec2 {
        self.size + Vec2::splat(PADDING * 2.0)
    }

    /// Blends towards `other` by `t`, which must be a shape of the same kind.
    pub(crate) fn lerp(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            ellipse: self.ellipse,
            size: self.size.lerp(other.size, t),
            corner_radius: mix(self.corner_radius, other.corner_radius),
            rim: mix(self.rim, other.rim),
            shade: mix(self.shade, other.shade),
        }
    }

    /// The material that draws the shape in `color`.
    pub(crate) fn material(&self, color: LinearRgba) -> SdfMaterial2d {
        let base = if self.ellipse {
            SdfMaterial2d::ellipse(self.size / 2.0)
        } else {
            SdfMaterial2d::rect(self.size).with_corner_radius(self.corner_radius)
        }
        .with_rim(self.rim)
        .with_padding(PADDING);
        let top = Color::from(color);
        if self.shade <= 0.0 {
            return base.with_color(top);
        }
        let keep = 1.0 - self.shade;
        let bottom = Color::from(LinearRgba {
            red: color.red * keep,
            green: color.green * keep,
            blue: color.blue * keep,
            alpha: color.alpha,
        });
        base.with_colors(bottom, top)
    }

    fn key(&self) -> (bool, [u32; 5]) {
        (
            self.ellipse,
            [
                self.size.x.to_bits(),
                self.size.y.to_bits(),
                self.corner_radius.to_bits(),
                self.rim.to_bits(),
                self.shade.to_bits(),
            ],
        )
    }
}

impl PartialEq for SdfShape {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for SdfShape {}

impl Hash for SdfShape {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key().hash(state);
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
        assert_ne!(a, b.with_rim(2.0));
        assert_ne!(
            a,
            SdfShape::ellipse(Vec2::new(40.0, 20.0)).with_corner_radius(6.0)
        );
        let set: HashSet<SdfShape> = [a, b, a.with_shade(0.2)].into();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn builders_clamp_their_inputs() {
        let shape = SdfShape::rect(Vec2::ONE)
            .with_corner_radius(-3.0)
            .with_rim(-1.0)
            .with_shade(4.0);
        assert_eq!(shape.corner_radius, 0.0);
        assert_eq!(shape.rim, 0.0);
        assert_eq!(shape.shade, 1.0);
    }

    #[test]
    fn blending_mixes_every_number_and_keeps_the_kind() {
        let a = SdfShape::rect(Vec2::new(10.0, 10.0)).with_corner_radius(2.0);
        let b = SdfShape::rect(Vec2::new(30.0, 50.0)).with_corner_radius(6.0);
        let middle = a.lerp(&b, 0.5);
        assert_eq!(middle.size(), Vec2::new(20.0, 30.0));
        assert_eq!(middle.corner_radius, 4.0);
        assert!(!middle.is_ellipse());
        assert_eq!(a.lerp(&b, 0.0), a);
        assert_eq!(a.lerp(&b, 1.0), b);
    }

    #[test]
    fn the_quad_has_room_for_the_fringe() {
        let quad = SdfShape::ellipse(Vec2::new(10.0, 20.0)).quad_size();
        assert_eq!(quad, Vec2::new(10.0 + PADDING * 2.0, 20.0 + PADDING * 2.0));
    }

    #[test]
    fn a_shaded_shape_grades_between_two_colors_and_a_flat_one_does_not() {
        let white = LinearRgba::WHITE;
        let flat = SdfShape::rect(Vec2::ONE).material(white);
        assert_eq!(flat.uniform.color, flat.uniform.secondary_color);
        let shaded = SdfShape::rect(Vec2::ONE).with_shade(0.5).material(white);
        assert_ne!(shaded.uniform.color, shaded.uniform.secondary_color);
        assert!(shaded.uniform.color.x < shaded.uniform.secondary_color.x);
    }
}
