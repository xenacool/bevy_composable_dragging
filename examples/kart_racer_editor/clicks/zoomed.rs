//! The operations of the editor at every zoom of the map and every turn of it.
//!
//! The tests of `clicks` use the map as it starts, framing the whole track. The map also comes
//! in on the selected point and turns, and then the handles are where the camera puts them: each
//! tool has to work there too, on the first point, the last one and in between.

use super::{Harness, POINT, harness, run, use_tool};
use crate::camera::MapCamera;
use crate::state::Tool;

/// How the map is seen: how far it has come in, from `0.0` framing the whole track to `1.0` as
/// close as it goes, and how far it is turned about the vertical axis, in radians.
#[derive(Clone, Copy, Debug)]
struct Seen {
    closeness: f32,
    turn: f32,
}

const VIEWS: [Seen; 7] = [
    Seen {
        closeness: 0.0,
        turn: 0.0,
    },
    Seen {
        closeness: 0.0,
        turn: 0.9,
    },
    Seen {
        closeness: 0.5,
        turn: 0.0,
    },
    Seen {
        closeness: 0.5,
        turn: -0.7,
    },
    Seen {
        closeness: 1.0,
        turn: 0.0,
    },
    Seen {
        closeness: 1.0,
        turn: 2.5,
    },
    Seen {
        closeness: 0.85,
        turn: -2.0,
    },
];

/// Selects point `index` and puts the camera in the view `seen`, settled.
fn see(harness: &mut Harness, index: usize, seen: Seen) {
    use_tool(harness, Tool::Select, Some(index));
    run(harness, 5);
    let mut cameras = harness.app.world_mut().query::<&mut MapCamera>();
    let mut camera = cameras.single_mut(harness.app.world_mut()).unwrap();
    let closest = camera.zoomed(f32::MAX).goal_height;
    let height = camera.frame_height + (closest - camera.frame_height) * seen.closeness;
    camera.yaw += seen.turn;
    camera.view_height = height;
    camera.goal_height = height;
    camera.focus = camera.goal_focus;
    run(harness, 60);
}

/// Runs `check` on the first point, the one in between and the last but one, in every view,
/// each on a fresh editor.
fn in_every_view(check: impl Fn(&mut Harness, usize)) {
    let count = harness().state().design.control_points.len();
    for seen in VIEWS {
        for index in [0, POINT, count - 2] {
            eprintln!("point {index} seen {seen:?}");
            let mut harness = harness();
            see(&mut harness, index, seen);
            check(&mut harness, index);
        }
    }
}

#[test]
fn a_gem_is_moved_in_every_view() {
    in_every_view(super::move_gem);
}

#[test]
fn a_point_is_raised_and_lowered_in_every_view() {
    in_every_view(super::raise_with_cone);
}

#[test]
fn the_road_is_widened_and_narrowed_in_every_view() {
    in_every_view(super::widen_and_narrow);
}

#[test]
fn the_road_is_banked_both_ways_in_every_view() {
    in_every_view(super::bank_both_ways);
}

#[test]
fn a_point_moves_with_the_gizmo_in_every_view() {
    in_every_view(super::move_with_gizmo);
}
