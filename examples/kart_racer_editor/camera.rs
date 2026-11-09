//! The camera of the editor: a map. It looks down at the ground at 45 degrees from a corner
//! through an orthographic projection, so the track keeps its proportions wherever it is on the
//! screen and a handle is as big on the screen on every part of it.
//!
//! The camera is framed once, on the square of the map and the track in it. Its scaling mode
//! keeps all of that on the screen whatever shape the window has, and the editor keeps every
//! control point inside the square, so nothing that is edited can leave the screen.

use std::f32::consts::FRAC_PI_4;

use bevy::camera::ScalingMode;
use bevy::prelude::*;

/// How far the camera looks down, in radians above the ground: a map seen at 45 degrees.
pub const MAP_PITCH: f32 = FRAC_PI_4;
/// How much room there is around what is framed, as a factor of its size.
const FRAMING_MARGIN: f32 = 1.1;

/// Marks the camera of the editor.
#[derive(Component)]
pub struct EditorCamera;

/// The camera that shows all of `points` from the corner, at 45 degrees, as a transform and an
/// orthographic projection that keeps them on the screen at any aspect ratio.
pub fn framing(points: &[Vec3]) -> (Transform, Projection) {
    let (low, high) = points.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), point| (low.min(*point), high.max(*point)),
    );
    let target = if points.is_empty() {
        Vec3::ZERO
    } else {
        (low + high) / 2.0
    };
    let rotation = Quat::from_euler(EulerRot::YXZ, FRAC_PI_4, -MAP_PITCH, 0.0);
    let (right, up) = (rotation * Vec3::X, rotation * Vec3::Y);
    let reach = |axis: Vec3| {
        points
            .iter()
            .map(|point| (*point - target).dot(axis).abs())
            .fold(0.0, f32::max)
            * 2.0
            * FRAMING_MARGIN
    };
    let (min_width, min_height) = (reach(right), reach(up));
    let distance = min_width.max(min_height);
    let transform =
        Transform::from_translation(target + rotation * Vec3::Z * distance).with_rotation(rotation);
    let projection = Projection::Orthographic(OrthographicProjection {
        near: -distance * 2.0,
        far: distance * 3.0,
        scaling_mode: ScalingMode::AutoMin {
            min_width,
            min_height,
        },
        ..OrthographicProjection::default_3d()
    });
    (transform, projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Vec<Vec3> {
        vec![
            Vec3::new(-300.0, 0.0, -100.0),
            Vec3::new(200.0, 8.0, -150.0),
            Vec3::new(250.0, 3.0, 120.0),
            Vec3::new(-250.0, 0.0, 160.0),
        ]
    }

    #[test]
    fn the_map_is_seen_at_forty_five_degrees_from_the_diagonal() {
        let (transform, projection) = framing(&track());
        let toward = *transform.forward();
        let level = Vec3::new(toward.x, 0.0, toward.z).normalize();
        assert!(
            (toward.dot(level).acos() - FRAC_PI_4).abs() < 1e-4,
            "looks down at 45 degrees"
        );
        assert!(
            (level.x.abs() - level.z.abs()).abs() < 1e-4,
            "from a diagonal: {level}"
        );
        assert!(matches!(projection, Projection::Orthographic(_)));
    }

    #[test]
    fn the_framing_holds_every_point_on_the_screen_at_any_aspect() {
        let (transform, projection) = framing(&track());
        let Projection::Orthographic(projection) = projection else {
            panic!("an orthographic projection");
        };
        let ScalingMode::AutoMin {
            min_width,
            min_height,
        } = projection.scaling_mode
        else {
            panic!("a scaling mode that keeps the minimum in view");
        };
        let inverse = transform.compute_affine().inverse();
        for aspect in [16.0 / 9.0, 1.0, 9.0 / 16.0] {
            let (width, height) = if aspect > min_width / min_height {
                (min_height * aspect, min_height)
            } else {
                (min_width, min_width / aspect)
            };
            for point in track() {
                let seen = inverse.transform_point3(point);
                assert!(seen.x.abs() <= width / 2.0, "{aspect}");
                assert!(seen.y.abs() <= height / 2.0, "{aspect}");
                assert!(
                    -seen.z > projection.near && -seen.z < projection.far,
                    "{aspect}"
                );
            }
        }
    }

    #[test]
    fn framing_nothing_looks_at_the_origin() {
        let (transform, _) = framing(&[]);
        assert!(transform.translation.is_finite());
        assert_eq!(transform.translation, Vec3::ZERO);
    }
}
