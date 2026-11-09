//! Driving a physics body with the cursor without letting it break the world.
//!
//! A body that follows the cursor by velocity can be sent through a thin wall by a quick flick,
//! and can leave a drag with whatever speed the last flick gave it. Neither can happen here: the
//! speed is capped so that the body never travels more than half of its smallest dimension in a
//! step, whatever the cursor does, and the same cap bounds the speed it is released with.

use bevy::prelude::*;

/// The most a body may travel in one step, as a fraction of its smallest dimension.
const TRAVEL_FRACTION: f32 = 0.5;

/// The fastest a body whose smallest dimension is `smallest` may move when stepped by `step`
/// seconds.
pub fn speed_limit(smallest: f32, step: f32) -> f32 {
    TRAVEL_FRACTION * smallest / step
}

/// The velocity that takes a body from `position` to `target` within one step, held to `limit`.
pub fn drive(position: Vec3, target: Vec3, step: f32, limit: f32) -> Vec3 {
    ((target - position) / step).clamp_length_max(limit)
}

#[cfg(test)]
mod tests {
    use bevy_gradmesh::ShapeDetail;

    use super::*;
    use crate::harness::{Body, Lab, Rig};
    use crate::mesh_check::MeshReport;
    use crate::scenario::{ColliderKind, Primitive};

    const WALL: f32 = 5.0;
    const THICKNESS: f32 = 0.05;

    /// Drags a cuboid by a cursor that jumps to the far side of a thin wall, and returns where the
    /// cuboid is a second later together with the fastest it moved.
    fn flick(limit: Option<f32>) -> (Vec3, f32) {
        let rig = Rig {
            gravity: 0.0,
            ..Rig::default()
        };
        let mesh = Primitive::Cuboid.shape(0.5, ShapeDetail::Low).build();
        let smallest = MeshReport::of(&mesh).size.min_element();
        let limit = limit.unwrap_or_else(|| speed_limit(smallest, rig.step));
        let mut lab = Lab::new(rig);
        lab.wall(Vec3::new(THICKNESS, 4.0, 4.0), Vec3::new(WALL, 1.0, 0.0));
        let body = Body::from_mesh(&mesh, ColliderKind::ConvexHull, Vec3::new(0.0, 1.0, 0.0))
            .expect("a cuboid has a hull");
        let cuboid = lab.spawn(body);
        let cursor = Vec3::new(WALL + 20.0, 1.0, 0.0);
        let mut fastest = 0.0_f32;
        for _ in 0..60 {
            let now = lab.read(cuboid).expect("the cuboid exists");
            let velocity = drive(now.position, cursor, rig.step, limit);
            lab.set_velocity(cuboid, velocity);
            lab.step(1);
            fastest = fastest.max(lab.read(cuboid).map_or(0.0, |r| r.velocity.length()));
        }
        (
            lab.read(cuboid).expect("the cuboid exists").position,
            fastest,
        )
    }

    #[test]
    fn the_limit_scales_with_the_body_and_the_step() {
        assert!((speed_limit(0.5, 1.0 / 60.0) - 15.0).abs() < 1e-4);
        assert!((speed_limit(0.5, 1.0 / 120.0) - 30.0).abs() < 1e-4);
        assert!(speed_limit(0.01, 1.0 / 60.0) < speed_limit(1.0, 1.0 / 60.0));
    }

    #[test]
    fn the_drive_never_exceeds_the_limit() {
        let velocity = drive(Vec3::ZERO, Vec3::new(1000.0, 0.0, 0.0), 1.0 / 60.0, 12.0);
        assert!((velocity.length() - 12.0).abs() < 1e-4);
        let near = drive(Vec3::ZERO, Vec3::new(0.01, 0.0, 0.0), 1.0 / 60.0, 12.0);
        assert!(
            (near.x - 0.6).abs() < 1e-4,
            "a near target is reached in one step"
        );
    }

    #[test]
    fn a_flick_past_a_thin_wall_cannot_carry_the_body_through_it() {
        let (end, fastest) = flick(None);
        assert!(end.x < WALL, "the body got to {end:?}");
        assert!(fastest <= 15.0 + 1e-3, "it moved at {fastest} m/s");
    }

    #[test]
    fn without_the_limit_the_same_flick_breaks_the_wall() {
        let (end, _) = flick(Some(f32::MAX));
        assert!(
            end.x > WALL,
            "the body stayed at {end:?}, so the test proves nothing"
        );
    }
}
