//! End-to-end tests of the editor: a virtual mouse presses and drags the gems, the handles and
//! the petals of the real editor app, so everything between the pointer and the track runs:
//! picking through the orthographic camera, the drag engine, the edits of the track and the view
//! that follows.
//!
//! The app is the one of `main`, with the default plugins but no window system and no GPU.

use bevy::prelude::*;
use bevy_composable_dragging::DragModel;
use bevy_composable_dragging::radial::{RadialHost, petal_id};

use super::camera::{EditorCamera, MAP_PITCH};
use super::common::mouse::Mouse;
use super::editor;
use super::menu::{Item, Ring};
use super::state::{Editor, Tool};
use super::track::{ControlPoint, MAX_POINT_WIDTH, MIN_POINT_WIDTH};
use super::view::{CURSOR, GROUND_HEIGHT};
use crate::ground::BASE;

/// The editor app and the virtual mouse that drives it.
type Harness = Mouse<Editor, EditorCamera>;

/// How many frames the view needs to settle after it changes, which is longer than its tween.
const SETTLE: usize = 30;

/// The control point that most tests edit.
const POINT: usize = 5;

/// How far, in pixels, a drag goes to change the land.
const DRAG: f32 = 60.0;

/// The offsets from the middle of a handle, in pixels, at which the pointer is tried: the ray
/// through the exact middle of a mesh can slip between two of its triangles.
const TRIES: [Vec2; 9] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(3.0, 0.0),
    Vec2::new(-3.0, 0.0),
    Vec2::new(0.0, 3.0),
    Vec2::new(0.0, -3.0),
    Vec2::new(3.0, 3.0),
    Vec2::new(-3.0, 3.0),
    Vec2::new(3.0, -3.0),
    Vec2::new(-3.0, -3.0),
];

/// Runs `frames` frames of the app.
fn run(harness: &mut Harness, frames: usize) {
    for _ in 0..frames {
        harness.app.update();
    }
}

/// An editor app with the initial track laid down, settled and ready to be used.
fn harness() -> Harness {
    let mut harness = Mouse::new(|app| {
        editor(app);
    });
    run(&mut harness, SETTLE);
    harness
}

/// The editor of `harness` with `tool` chosen and control point `selected` selected.
fn use_tool(harness: &mut Harness, tool: Tool, selected: Option<usize>) {
    let editor = harness.state().with_tool(tool).with_selected(selected);
    harness.show(editor);
    run(harness, SETTLE);
}

/// Whether the entity under the pointer, nearest to the camera, is the node `id` or a part of it.
fn is_under_pointer(harness: &mut Harness, id: &str) -> bool {
    harness
        .hovered()
        .first()
        .is_some_and(|hit| hit.starts_with(id))
}

/// Puts the pointer on the node `id` of the scene, near where the map shows its middle, and
/// returns where it is.
fn grab(harness: &mut Harness, id: &str) -> Vec2 {
    let at = harness.scene_position_of(id);
    for offset in TRIES {
        let pixel = at + offset;
        harness.move_to(pixel + Vec2::X);
        harness.move_to(pixel);
        if is_under_pointer(harness, id) {
            return pixel;
        }
    }
    panic!(
        "{id} is not under the pointer near {at}: {:?}",
        harness.hovered()
    );
}

/// The control point `index` of the track being edited.
fn point(harness: &Harness, index: usize) -> ControlPoint {
    harness.state().design.control_points[index]
}

/// Drags from `from` by `by` in three steps.
fn drag_by(harness: &mut Harness, from: Vec2, by: Vec2) {
    harness.drag(from, &[from + by / 3.0, from + by * 2.0 / 3.0, from + by]);
}

/// The control points of `editor` but one, which is how edits of a point leave the others.
fn others(editor: &Editor, index: usize) -> Vec<ControlPoint> {
    let mut points = editor.design.control_points.clone();
    points.remove(index);
    points
}

/// The pixel where a press lands on the terrain, `offset` from the middle of the land.
fn terrain_pixel(harness: &mut Harness, offset: Vec2) -> Vec2 {
    let middle = harness.state().ground.middle() + offset;
    let pixel = harness.scene_point(Vec3::new(middle.x, GROUND_HEIGHT, middle.y));
    harness.move_to(pixel + Vec2::X);
    harness.move_to(pixel);
    assert!(
        is_under_pointer(harness, "terrain"),
        "{:?}",
        harness.hovered()
    );
    pixel
}

/// Whether the view holds the node `id`.
fn has_node(harness: &Harness, id: &str) -> bool {
    harness
        .app
        .world()
        .resource::<DragModel<Editor>>()
        .view()
        .get(id)
        .is_some()
}

#[test]
fn the_editor_looks_down_at_forty_five_degrees_through_an_orthographic_camera() {
    let mut harness = harness();
    let mut cameras = harness
        .app
        .world_mut()
        .query_filtered::<(&Transform, &Projection), With<EditorCamera>>();
    let (transform, projection) = cameras.single(harness.app.world()).unwrap();
    assert!(matches!(projection, Projection::Orthographic(_)));
    let pitch = (-transform.forward().y).asin();
    assert!((pitch - MAP_PITCH).abs() < 1e-3, "{pitch}");
}

#[test]
fn every_gem_is_under_the_pointer_where_the_map_shows_it() {
    let mut harness = harness();
    for index in 0..harness.state().design.control_points.len() {
        grab(&mut harness, &format!("track/point-{index}"));
    }
}

#[test]
fn the_move_tool_drags_a_gem_across_the_ground_to_where_the_pointer_goes() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Move, Some(POINT));
    let before = harness.state().clone();
    let height = before.handle_position(POINT).y;
    let start = grab(&mut harness, &format!("track/point-{POINT}"));
    let end = start + Vec2::new(40.0, 25.0);
    let expected = harness.ground_point(end, height) - harness.ground_point(start, height);
    drag_by(&mut harness, start, end - start);
    let moved = point(&harness, POINT).position - before.design.control_points[POINT].position;
    assert_eq!(moved.y, 0.0, "across the ground");
    assert!(
        (moved - expected).length() <= 1.0,
        "{moved} against {expected}"
    );
    assert!(moved.length() > 0.5, "it really moved: {moved}");
    assert_eq!(others(harness.state(), POINT), others(&before, POINT));
}

#[test]
fn the_width_tool_widens_the_road_by_dragging_out_and_narrows_it_by_dragging_in() {
    let mut harness = harness();
    let id = format!("track/width-{POINT}");
    for outward in [true, false] {
        use_tool(&mut harness, Tool::Width, Some(POINT));
        let before = harness.state().clone();
        let was = point(&harness, POINT).width;
        let start = grab(&mut harness, &id);
        let handle = harness.scene_position_of(&id);
        let centre = harness.scene_point(before.design.control_points[POINT].position);
        let along = (handle - centre).normalize_or_zero() * 30.0;
        drag_by(&mut harness, start, if outward { along } else { -along });
        let now = point(&harness, POINT).width;
        if outward {
            assert!(now > was, "wider: {was} to {now}");
            assert!(now <= MAX_POINT_WIDTH);
        } else {
            assert!(now < was, "narrower: {was} to {now}");
            assert!(now >= MIN_POINT_WIDTH);
        }
        assert_eq!(
            point(&harness, POINT).position,
            before.design.control_points[POINT].position
        );
        assert_eq!(others(harness.state(), POINT), others(&before, POINT));
    }
}

/// The id of the hub that is shown: it rests in its corner until a point is selected, and then
/// pops up over it.
fn hub(harness: &Harness) -> &'static str {
    if harness.state().selected.is_some() {
        "menu/pop"
    } else {
        "menu/hub"
    }
}

#[test]
fn the_hub_opens_the_root_ring_which_opens_the_tools_and_a_petal_chooses_a_tool() {
    let mut harness = harness();
    for selected in [None, Some(POINT)] {
        harness.show(Editor::initial().with_selected(selected));
        harness.click(hub(&harness));
        assert_eq!(harness.state().menu, Some(Ring::Root), "{selected:?}");
        harness.click(&petal_id::<Editor>(&Ring::Root, &Item::Open(Ring::Tools)));
        assert_eq!(harness.state().menu, Some(Ring::Tools), "{selected:?}");
        for tool in [Tool::Width, Tool::Hills, Tool::Move] {
            harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(tool)));
            assert_eq!(harness.state().tool, tool);
            assert_eq!(harness.state().menu, None, "{tool:?}");
            assert_eq!(harness.state().selected, selected);
            harness.click(hub(&harness));
            harness.click(&petal_id::<Editor>(&Ring::Root, &Item::Open(Ring::Tools)));
        }
        harness.click(hub(&harness));
        harness.click(hub(&harness));
        assert_eq!(harness.state().menu, None, "the hub closes the menu again");
    }
}

#[test]
fn dragging_up_with_the_hills_tool_raises_the_land_and_down_lowers_it() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hills, None);
    let before = harness.state().clone();
    let up = terrain_pixel(&mut harness, Vec2::new(-180.0, 180.0));
    let down = terrain_pixel(&mut harness, Vec2::new(180.0, -180.0));
    let at = |harness: &mut Harness, pixel: Vec2| harness.ground_point(pixel, GROUND_HEIGHT).xz();
    let (raised_at, lowered_at) = (at(&mut harness, up), at(&mut harness, down));

    harness.move_to(up);
    harness.press();
    harness.move_to(up + Vec2::new(0.0, -DRAG / 2.0));
    harness.move_to(up + Vec2::new(0.0, -DRAG));
    run(&mut harness, 5);
    assert!(has_node(&harness, CURSOR), "the column shows the brush");
    assert_eq!(
        harness.state().ground,
        before.ground,
        "the land waits for the release"
    );
    harness.release();
    run(&mut harness, SETTLE);
    let after = harness.state();
    assert!(after.ground.surface(raised_at) > BASE + 1.0);
    assert_eq!(after.design, before.design, "the road is not touched");
    assert!(after.cursor.is_none(), "the brush is put away");
    assert!(!has_node(&harness, CURSOR));

    drag_by(&mut harness, down, Vec2::new(0.0, DRAG));
    let after = harness.state();
    assert!(after.ground.surface(lowered_at) < BASE - 1.0);
    assert!(
        after.ground.surface(raised_at) > BASE + 1.0,
        "the hill stays"
    );
}

#[test]
fn pressing_the_terrain_clears_the_selection_and_closes_the_menu() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Move, Some(POINT));
    let before = harness.state().design.clone();
    harness.show(
        harness
            .state()
            .with_menu(Some(Ring::Tools))
            .with_selected(Some(POINT)),
    );
    run(&mut harness, SETTLE);
    let pixel = terrain_pixel(&mut harness, Vec2::new(-200.0, 150.0));
    harness.drag(pixel, &[]);
    assert_eq!(harness.state().selected, None);
    assert_eq!(harness.state().menu, None);
    assert_eq!(harness.state().design, before);
}
