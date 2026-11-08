//! The materials and models of the castle pieces, built from `bevy_gradmesh` shapes.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

use bevy::prelude::*;
use bevy_gradmesh::{Shape, ShapeDetail, ShapePart, Style};

use super::{FLAME_HEIGHT, PieceKind, counterclockwise};
use crate::stack::Variant;

/// A building material.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fabric {
    Stone,
    Timber,
    Plaster,
    Slate,
    Clay,
    Thatch,
    Cloth,
    Flame,
    Cobble,
    Terrain,
    Handle,
    Door,
}

fn srgb(red: f32, green: f32, blue: f32) -> Color {
    Color::srgb(red, green, blue)
}

impl Fabric {
    /// A gradient from the shaded side to the lit side of the material, with the roughness that
    /// suits it. Torch flames glow.
    pub fn style(self) -> Style {
        let (color, shade, roughness) = match self {
            Self::Stone => (srgb(0.7, 0.69, 0.66), 0.55, 0.9),
            Self::Timber => (srgb(0.55, 0.36, 0.18), 0.55, 0.75),
            Self::Plaster => (srgb(0.96, 0.9, 0.78), 0.65, 0.95),
            Self::Slate => (srgb(0.42, 0.5, 0.66), 0.6, 0.6),
            Self::Clay => (srgb(0.8, 0.34, 0.24), 0.6, 0.6),
            Self::Thatch => (srgb(0.86, 0.7, 0.32), 0.6, 0.95),
            Self::Cloth => (srgb(0.85, 0.16, 0.2), 0.5, 0.9),
            Self::Cobble => (srgb(0.8, 0.77, 0.72), 0.6, 0.9),
            Self::Flame => (srgb(1.0, 0.62, 0.12), 0.8, 0.5),
            Self::Terrain => (srgb(1.0, 1.0, 1.0), 0.75, 0.95),
            Self::Handle => (srgb(0.98, 0.82, 0.3), 0.6, 0.4),
            Self::Door => (srgb(0.22, 0.14, 0.09), 0.7, 0.8),
        };
        let style = Style::shaded(color, shade).with_perceptual_roughness(roughness);
        if self == Self::Flame {
            style.with_emissive(LinearRgba::rgb(6.0, 2.6, 0.4))
        } else {
            style
        }
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

    fn turned(mut self, rotation: Quat) -> Self {
        self.transform.rotation = rotation;
        self
    }

    fn scaled(mut self, scale: Vec3) -> Self {
        self.transform.scale = scale;
        self
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
    Part::new(
        Shape::rounded_pyramid(4, radius, height, 0.25, ShapeDetail::Medium),
        fabric,
        at,
    )
    .turned(Quat::from_rotation_y(FRAC_PI_4))
}

/// An outline in the XY plane pushed out along Z, centered on `at.z`.
fn extruded(outline: Vec<Vec2>, depth: f32, roundness: f32, fabric: Fabric, at: Vec3) -> Part {
    let shape = Shape::extruded_polygon(
        counterclockwise(outline),
        depth,
        roundness,
        ShapeDetail::Medium,
    );
    let shift = at.z - shape.bounds().center().z;
    Part::new(shape, fabric, Vec3::new(at.x, at.y, shift))
}

/// An arch standing on the ground, `width` wide and `height` tall.
fn arch_outline(width: f32, height: f32) -> Vec<Vec2> {
    Shape::arch(width, height, 12)
        .into_iter()
        .map(|point| point + Vec2::Y * height / 2.0)
        .collect()
}

/// A path tile: nine rounded cobbles of their own tint, set in dirt, as one mesh.
pub fn cobble_tile() -> Shape {
    const TINTS: [(f32, f32, f32); 4] = [
        (0.84, 0.8, 0.72),
        (0.72, 0.75, 0.82),
        (0.88, 0.72, 0.64),
        (0.75, 0.82, 0.7),
    ];
    let dirt = ShapePart::new(Shape::cuboid(
        Vec3::new(0.98, 0.04, 0.98),
        0.01,
        ShapeDetail::Low,
    ))
    .with_transform(Transform::from_xyz(0.0, 0.02, 0.0))
    .with_tint(srgb(0.42, 0.32, 0.22));
    let mut parts = vec![dirt];
    for row in 0..3_u32 {
        for column in 0..3_u32 {
            let seed = row * 3 + column;
            let stone = Shape::extruded_polygon(
                Shape::blob(0.14, 0.15, u64::from(seed)),
                0.05,
                0.7,
                ShapeDetail::Low,
            );
            let bounds = stone.bounds();
            let bottom = bounds.center().z - bounds.size().z / 2.0;
            let (red, green, blue) = TINTS[seed as usize % TINTS.len()];
            // Extruded shapes stand along Z; the stone is laid down so it grows upward.
            parts.push(
                ShapePart::new(stone)
                    .with_transform(
                        Transform::from_xyz(
                            (column as f32 - 1.0) * 0.32,
                            0.035 - bottom,
                            (row as f32 - 1.0) * 0.32,
                        )
                        .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                    )
                    .with_tint(srgb(red, green, blue)),
            );
        }
    }
    Shape::compound(parts)
}

/// The door of a tower or a keep, on the side `angle` from its front, standing a little out of
/// the wall. Other pieces have no door.
pub fn door(kind: PieceKind, variant: Variant, angle: f32) -> Option<Part> {
    let radius = match kind {
        PieceKind::Tower => 0.39,
        PieceKind::Keep => 0.43,
        _ => return None,
    } * variant.thickness;
    let shape = Shape::extruded_polygon(
        counterclockwise(arch_outline(0.2, 0.34)),
        0.08,
        0.2,
        ShapeDetail::Medium,
    );
    let turn = Quat::from_rotation_y(angle);
    let depth = shape.bounds().center().z;
    Some(Part {
        shape,
        fabric: Fabric::Door,
        transform: Transform {
            translation: turn * Vec3::new(0.0, 0.01, radius - depth),
            rotation: turn,
            scale: Vec3::ONE,
        },
    })
}

/// The parts of a piece in the shape `variant` gives it. The first part is the body, which
/// becomes the draggable root: its center must lie on the vertical axis of the piece and it must
/// not be turned or scaled.
///
/// A piece with something standing on it gives up its roof, or its battlements, for a platform
/// that reaches as high as the piece does, and thickens at the base.
pub fn build_parts(kind: PieceKind, variant: Variant) -> Vec<Part> {
    use Fabric::{Clay, Cloth, Cobble, Flame, Plaster, Stone, Thatch, Timber};
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
                parts.push(cap(0.5, 0.6, Fabric::Slate, at(0.0, 1.455, 0.0)));
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
            parts.push(extruded(
                arch_outline(0.62, 0.56),
                0.32 * t,
                0.2,
                Plaster,
                at(0.0, 0.0, 0.0),
            ));
            parts.push(extruded(
                arch_outline(0.46, 0.5),
                0.34 * t,
                0.2,
                Timber,
                at(0.0, 0.0, 0.0),
            ));
        }
        PieceKind::SquareGate => {
            parts.push(block(
                at(1.0, 0.2, 0.32 * t),
                0.04,
                Stone,
                at(0.0, 0.65, 0.0),
            ));
            for x in [-0.38, 0.38] {
                parts.push(block(
                    at(0.24, 0.75, 0.32 * t),
                    0.04,
                    Stone,
                    at(x, 0.375, 0.0),
                ));
            }
            parts.push(block(
                at(0.52, 0.55, 0.06),
                0.02,
                Timber,
                at(0.0, 0.275, 0.0),
            ));
            if open_top {
                parts.push(block(
                    at(1.0, 0.07, 0.36 * t),
                    0.02,
                    Timber,
                    at(0.0, 0.785, 0.0),
                ));
            } else {
                parts.push(block(at(0.5, 0.05, 0.36), 0.02, Timber, at(0.0, 0.78, 0.0)));
            }
        }
        PieceKind::WoodGate => {
            parts.push(block(at(1.0, 0.1, 0.12), 0.02, Timber, at(0.0, 0.8, 0.0)));
            for x in [-0.4, 0.4] {
                parts.push(post(0.07 * t, 0.9, Timber, at(x, 0.45, 0.0)));
                if roof && !open_top {
                    parts.push(cap(0.11, 0.14, Thatch, at(x, 0.97, 0.0)));
                }
            }
            if open_top {
                parts.push(block(at(1.0, 0.06, 0.2), 0.02, Timber, at(0.0, 1.01, 0.0)));
            }
            parts.push(block(at(0.8, 0.07, 0.1), 0.02, Timber, at(0.0, 0.55, 0.0)));
            for x in [-0.18, 0.18] {
                parts.push(block(at(0.34, 0.5, 0.05), 0.02, Timber, at(x, 0.27, 0.0)));
            }
        }
        PieceKind::Cobble => {
            parts.push(Part::new(cobble_tile(), Cobble, at(0.0, 0.0, 0.0)));
        }
        PieceKind::Outpost => {
            parts.push(block(
                at(0.6 * t, 0.07, 0.6 * t),
                0.02,
                Timber,
                at(0.0, 0.72, 0.0),
            ));
            for (x, z) in [(-0.2, -0.2), (0.2, -0.2), (-0.2, 0.2), (0.2, 0.2)] {
                parts.push(post(0.045 * t, 0.7, Timber, at(x, 0.35, z)));
                if open_top {
                    parts.push(post(0.03, 0.74, Timber, at(x * 1.2, 1.1, z * 1.2)));
                } else {
                    parts.push(post(0.03, 0.45, Timber, at(x * 1.2, 0.975, z * 1.2)));
                }
            }
            if open_top {
                parts.push(block(
                    at(0.62 * t, 0.06, 0.62 * t),
                    0.02,
                    Timber,
                    at(0.0, 1.47, 0.0),
                ));
            } else if roof {
                parts.push(cap(0.45, 0.3, Thatch, at(0.0, 1.35, 0.0)));
            }
        }
        PieceKind::Torch => {
            parts.push(post(0.03, 0.55, Timber, at(0.0, 0.275, 0.0)));
            parts.push(Part::new(
                Shape::tapered_cylinder(0.07, 0.04, 0.1, ShapeDetail::Low),
                Stone,
                at(0.0, 0.58, 0.0),
            ));
            parts.push(Part::new(
                Shape::rounded_cone(0.06, 0.16, 0.5, ShapeDetail::Medium),
                Flame,
                at(0.0, FLAME_HEIGHT, 0.0),
            ));
        }
        PieceKind::Banner => {
            parts.push(block(at(0.3, 0.06, 0.3), 0.02, Stone, at(0.0, 0.03, 0.0)));
            parts.push(post(0.025, 1.1, Timber, at(0.0, 0.55, 0.0)));
            parts.push(block(at(0.4, 0.04, 0.04), 0.01, Timber, at(0.0, 1.05, 0.0)));
            let cloth = vec![
                Vec2::new(-0.15, 1.02),
                Vec2::new(-0.15, 0.5),
                Vec2::new(0.0, 0.62),
                Vec2::new(0.15, 0.5),
                Vec2::new(0.15, 1.02),
            ];
            parts.push(extruded(cloth, 0.02, 0.5, Cloth, at(0.0, 0.0, 0.03)));
        }
        PieceKind::Cottage => {
            parts.push(block(
                at(0.72 * t, 0.42, 0.56 * t),
                0.03,
                Plaster,
                at(0.0, 0.22, 0.0),
            ));
            if open_top {
                parts.push(block(
                    at(0.7 * t, 0.4, 0.54 * t),
                    0.02,
                    Plaster,
                    at(0.0, 0.64, 0.0),
                ));
                parts.push(block(
                    at(0.76 * t, 0.06, 0.6 * t),
                    0.02,
                    Timber,
                    at(0.0, 0.83, 0.0),
                ));
            } else if roof {
                parts.push(
                    pyramid_roof(0.62, 0.38, Thatch, at(0.0, 0.62, 0.0)).scaled(at(1.0, 1.0, 0.8)),
                );
                parts.push(block(at(0.1, 0.3, 0.1), 0.01, Stone, at(0.22, 0.7, 0.1)));
            }
            parts.push(block(
                at(0.14, 0.24, 0.04),
                0.01,
                Timber,
                at(0.0, 0.12, 0.29 * t),
            ));
            parts.push(block(
                at(0.12, 0.12, 0.02),
                0.01,
                Timber,
                at(0.22, 0.2, 0.285 * t),
            ));
        }
    }
    parts
}
