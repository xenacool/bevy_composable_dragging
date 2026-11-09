//! The scripted tour of the editor for the README: the pointer pulls a control point so the
//! curve bends, picks the width tool through the rings of the menu and widens and narrows the
//! road, then picks the hills tool the same way and raises a hill inside the loop.
//!
//! The tour is real pointer input: it finds where to press from the view of the editor and where
//! to go from the design it starts with, and everything else is the drag engine at work.

use bevy::prelude::*;
use bevy_composable_dragging::DragModel;
use bevy_composable_dragging::radial::petal_id;

use crate::camera::EditorCamera;
use crate::common::HudCamera;
use crate::common::tour::{Point, Tour, through_camera};
use crate::menu::{Item, Ring};
use crate::state::{Editor, Tool};
use crate::track::{TrackDesign, on_the_map};
use crate::view::GROUND_HEIGHT;

/// The control point that is pulled out and then widened.
const POINT: usize = 2;
/// How far the control point is pulled, in metres on the map: to the right in `x` and towards
/// the viewer in `z`.
const PULL: Vec3 = Vec3::new(-40.0, 0.0, 55.0);
/// The half-width the road is widened to, and the half-width it is narrowed to, in metres.
const WIDE: f32 = 24.0;
const NARROW: f32 = 5.0;
/// How high the hill is dragged up, in metres.
const RISE: f32 = 45.0;
/// How long the pointer holds still at the end of a drag, so a viewer can read the result.
const HOLD: f32 = 0.7;

fn editor(world: &World) -> &Editor {
    world.resource::<DragModel<Editor>>().state()
}

/// The pixel of the gem of control point `index` where it is now.
fn gem_now(index: usize) -> Point {
    through_camera::<EditorCamera>(move |world| Some(editor(world).handle_position(index)))
}

/// The pixel of the gem of control point `index` of the design the tour starts with, moved by
/// `offset`, which is to the right in `x` and towards the viewer in `z`, and which stays where it
/// is while the gem is being dragged.
fn gem_moved(index: usize, offset: Vec3) -> Point {
    through_camera::<EditorCamera>(move |world| {
        let start = TrackDesign::initial().control_points[index].position;
        let offset = on_the_map(offset.x, 0.0, offset.z);
        Some(start + offset + Vec3::Y * editor(world).handle_radius())
    })
}

/// The pixel of the width handle of control point `index` where it is now.
fn width_handle_now(index: usize) -> Point {
    through_camera::<EditorCamera>(move |world| {
        let editor = editor(world);
        let frame = editor.frames().get(index)?;
        Some(editor.handle_position(index) + frame.right * frame.width / 2.0)
    })
}

/// The pixel where the width handle of control point `index` is when the road is `half` wide on
/// each side of the point. The point and the direction across the road do not change while the
/// handle is dragged.
fn width_handle_at(index: usize, half: f32) -> Point {
    through_camera::<EditorCamera>(move |world| {
        let editor = editor(world);
        let frame = editor.frames().get(index)?;
        Some(editor.handle_position(index) + frame.right * half)
    })
}

/// The pixel where the HUD node `id` of the view is.
fn hud(id: String) -> Point {
    through_camera::<HudCamera>(move |world| {
        let at = world
            .resource::<DragModel<Editor>>()
            .view()
            .world_position(&id)?;
        Some(at.truncate().extend(0.0))
    })
}

/// The pixel of the land under the middle of the loop, `rise` metres above the ground.
fn land_above(rise: f32) -> Point {
    through_camera::<EditorCamera>(move |world| {
        let middle = editor(world).ground.middle();
        Some(Vec3::new(middle.x, GROUND_HEIGHT + rise, middle.y))
    })
}

/// Presses the hub, opens the ring of tools and presses the petal of `tool`, with a pause after
/// each press for the ring to bloom.
fn choose(tour: Tour, tool: Tool) -> Tour {
    tour.click(hud("menu/pop".into()))
        .wait(0.7)
        .click(hud(petal_id::<Editor>(
            &Ring::Root,
            &Item::Open(Ring::Tools),
        )))
        .wait(0.7)
        .click(hud(petal_id::<Editor>(&Ring::Tools, &Item::Tool(tool))))
        .wait(0.6)
}

/// The tour of the editor, a little under twenty seconds long.
pub fn tour() -> Tour {
    let tour = Tour::new()
        .wait(1.0)
        .glide(gem_now(POINT))
        .press()
        .glide(gem_moved(POINT, PULL * 0.5))
        .glide(gem_moved(POINT, PULL))
        .wait(HOLD)
        .release()
        .wait(0.6);
    let tour = choose(tour, Tool::Width)
        .glide(width_handle_now(POINT))
        .press()
        .glide(width_handle_at(POINT, WIDE))
        .wait(HOLD)
        .glide(width_handle_at(POINT, NARROW))
        .wait(HOLD)
        .release()
        .wait(0.6);
    choose(tour, Tool::Hills)
        .glide(land_above(0.0))
        .press()
        .glide(land_above(RISE / 2.0))
        .glide(land_above(RISE))
        .wait(HOLD)
        .release()
        .wait(1.2)
}
