//! The materials and models of the castle pieces, built from `bevy_gradmesh` shapes.

use std::f32::consts::FRAC_PI_4;

use bevy::prelude::*;
use bevy_gradmesh::{Shape, ShapeDetail, Style};

use super::PieceKind;
use crate::stack::Variant;

/// A building material.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fabric {
    Stone,
    Timber,
    Plaster,
    Slate,
    Clay,
    Ground,
}

impl Fabric {
    /// A gradient from the shaded side to the lit side of the material, with the roughness that
    /// suits it.
    pub fn style(self) -> Style {
        let (color, shade, roughness) = match self {
            Self::Stone => (Color::srgb(0.7, 0.69, 0.66), 0.55, 0.9),
            Self::Timber => (Color::srgb(0.55, 0.36, 0.18), 0.55, 0.75),
            Self::Plaster => (Color::srgb(0.96, 0.9, 0.78), 0.65, 0.95),
            Self::Slate => (Color::srgb(0.42, 0.5, 0.66), 0.6, 0.6),
            Self::Clay => (Color::srgb(0.8, 0.34, 0.24), 0.6, 0.6),
            Self::Ground => (Color::srgb(0.38, 0.54, 0.28), 0.75, 0.95),
        };
        Style::shaded(color, shade).with_perceptual_roughness(roughness)
    }
}

/// One mesh of a piece. Pieces are modeled Y-up with their base at `y = 0`.
pub struct Part {
    pub shape: Shape,
    pub fabric: Fabric,
    pub transform: Transform,
}

impl Part {
    fn new(shape: Shape, fabric: Fabric, at: Vec3) -> Self {
        Self {
            shape,
            fabric,
            transform: Transform::from_translation(at),
        }
    }
}

fn block(size: Vec3, roundness: f32, fabric: Fabric, at: Vec3) -> Part {
    Part::new(
        Shape::cuboid(size, roundness, ShapeDetail::Medium),
        fabric,
        at,
    )
}

fn post(radius: f32, height: f32, fabric: Fabric, at: Vec3) -> Part {
    Part::new(
        Shape::cylinder(radius, height, ShapeDetail::Medium),
        fabric,
        at,
    )
}

/// A cone with a rounded tip, so roofs catch the light along their whole slope.
fn cap(radius: f32, height: f32, fabric: Fabric, at: Vec3) -> Part {
    Part::new(
        Shape::rounded_cone(radius, height, 0.35, ShapeDetail::Medium),
        fabric,
        at,
    )
}

/// A four-sided roof with rounded edges, turned so its sides face the walls.
fn pyramid_roof(radius: f32, height: f32, fabric: Fabric, at: Vec3) -> Part {
    let mut part = Part::new(
        Shape::rounded_pyramid(4, radius, height, 0.25, ShapeDetail::Medium),
        fabric,
        at,
    );
    part.transform.rotation = Quat::from_rotation_y(FRAC_PI_4);
    part
}

/// An arch standing on the ground, pushed out along Z and centered on `z = 0`.
fn arch(width: f32, height: f32, depth: f32, fabric: Fabric) -> Part {
    let outline = Shape::arch(width, height, 12)
        .into_iter()
        .map(|point| point + Vec2::Y * height / 2.0)
        .collect();
    let shape = Shape::extruded_polygon(outline, depth, 0.2, ShapeDetail::Medium);
    let shift = -shape.bounds().center().z;
    Part::new(shape, fabric, Vec3::new(0.0, 0.0, shift))
}

/// The parts of a piece in the shape `variant` gives it. The first part is the body, which
/// becomes the draggable root: its center must lie on the vertical axis of the piece and it must
/// not be turned or scaled.
///
/// A piece with something standing on it gives up its roof for a platform that reaches as high
/// as the piece does, and thickens at the base.
pub fn build_parts(kind: PieceKind, variant: Variant) -> Vec<Part> {
    use Fabric::{Clay, Plaster, Slate, Stone, Timber};
    let at = Vec3::new;
    let Variant {
        open_top,
        thickness: t,
        roof,
    } = variant;
    let mut parts = Vec::new();
    match kind {
        PieceKind::Tower => {
            parts.push(Part::new(
                Shape::tapered_cylinder(0.34 * t, 0.4 * t, 1.1, ShapeDetail::Medium),
                Stone,
                at(0.0, 0.55, 0.0),
            ));
            parts.push(post(0.4 * t, 0.07, Timber, at(0.0, 1.12, 0.0)));
            if open_top {
                parts.push(post(0.38 * t, 0.58, Stone, at(0.0, 1.45, 0.0)));
                parts.push(post(0.44 * t, 0.06, Timber, at(0.0, 1.73, 0.0)));
            } else if roof {
                parts.push(cap(0.5, 0.6, Slate, at(0.0, 1.455, 0.0)));
            }
        }
        PieceKind::Keep => {
            parts.push(block(
                at(0.86 * t, 0.9, 0.86 * t),
                0.05,
                Plaster,
                at(0.0, 0.45, 0.0),
            ));
            parts.push(block(
                at(0.9 * t, 0.08, 0.9 * t),
                0.02,
                Timber,
                at(0.0, 0.55, 0.0),
            ));
            if open_top {
                parts.push(block(
                    at(0.8 * t, 0.54, 0.8 * t),
                    0.04,
                    Plaster,
                    at(0.0, 1.17, 0.0),
                ));
                parts.push(block(
                    at(0.96 * t, 0.06, 0.96 * t),
                    0.02,
                    Timber,
                    at(0.0, 1.47, 0.0),
                ));
            } else if roof {
                parts.push(pyramid_roof(0.68, 0.6, Clay, at(0.0, 1.2, 0.0)));
            }
        }
        PieceKind::ArchGate => {
            parts.push(block(
                at(1.0, 0.7, 0.3 * t),
                0.08,
                Stone,
                at(0.0, 0.35, 0.0),
            ));
            if open_top {
                parts.push(block(
                    at(1.0, 0.12, 0.34 * t),
                    0.03,
                    Stone,
                    at(0.0, 0.76, 0.0),
                ));
            } else {
                for x in [-0.36, 0.36] {
                    parts.push(block(
                        at(0.22, 0.12, 0.3 * t),
                        0.03,
                        Stone,
                        at(x, 0.76, 0.0),
                    ));
                }
            }
            parts.push(arch(0.62, 0.56, 0.32 * t, Plaster));
            parts.push(arch(0.46, 0.5, 0.34 * t, Timber));
        }
    }
    parts
}
