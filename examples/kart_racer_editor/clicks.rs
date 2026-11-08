//! End-to-end tests of the editor in its map view: a virtual mouse presses and drags the gems,
//! the handles, the gizmo and the petals of the real editor app, so everything between the
//! pointer and the track runs: picking through the orthographic camera, the drag engine, the
//! edits of the track and the view that follows.
//!
//! The app is the one of `main`, with the default plugins but no window system and no GPU.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_composable_dragging::DragModel;
use bevy_composable_dragging::gizmo::EDGE_ON;
use bevy_composable_dragging::radial::{RadialHost, Ring as _, petal_id};

use super::camera::{EditorCamera, MAP_PITCH, MapCamera};
use super::common::mouse::Mouse;
use super::editor;
use super::kart::Kart;
use super::menu::{Item, Preset, Ring};
use super::state::{Editor, Precision, Tool};
use super::track::{BakedTrack, MAX_POINT_WIDTH, MAX_ROLL, MIN_POINT_WIDTH, MIN_POINTS};
use super::view::GROUND_HEIGHT;

mod land;
mod zoomed;

/// The editor app and the virtual mouse that drives it.
type Harness = Mouse<Editor, EditorCamera>;

/// How many frames the view needs to settle after it changes, which is longer than its tween.
const SETTLE: usize = 30;

/// The control point that most tests edit.
const POINT: usize = 5;

/// The offsets from the middle of a handle, in pixels, at which the pointer is tried: the ray
/// through the exact middle of a mesh can slip between two of its triangles, and a handle on
/// the same spot, like the arrow of the gizmo over the gem, covers part of it.
const TRIES: [Vec2; 17] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(3.0, 0.0),
    Vec2::new(-3.0, 0.0),
    Vec2::new(0.0, 3.0),
    Vec2::new(0.0, -3.0),
    Vec2::new(3.0, 3.0),
    Vec2::new(-3.0, 3.0),
    Vec2::new(3.0, -3.0),
    Vec2::new(-3.0, -3.0),
    Vec2::new(6.0, 0.0),
    Vec2::new(-6.0, 0.0),
    Vec2::new(0.0, 6.0),
    Vec2::new(0.0, -6.0),
    Vec2::new(5.0, 5.0),
    Vec2::new(-5.0, 5.0),
    Vec2::new(5.0, -5.0),
    Vec2::new(-5.0, -5.0),
];

/// An editor app that starts as the real one does, empty, settled and ready to be used.
fn empty_harness() -> Harness {
    Mouse::new(|app| {
        editor(app);
    })
}

/// An editor app with the upstream track laid down, settled and ready to be used.
fn harness() -> Harness {
    let mut harness = empty_harness();
    harness.show(Editor::upstream());
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
/// returns where it is, or `None` when something else is in the way.
fn try_grab(harness: &mut Harness, id: &str) -> Option<Vec2> {
    let at = harness.scene_position_of(id);
    for offset in TRIES {
        let pixel = at + offset;
        harness.move_to(pixel + Vec2::X);
        harness.move_to(pixel);
        if is_under_pointer(harness, id) {
            return Some(pixel);
        }
    }
    None
}

/// Puts the pointer on the node `id` of the scene, near where the map shows its middle, and
/// returns where it is.
fn grab(harness: &mut Harness, id: &str) -> Vec2 {
    try_grab(harness, id).unwrap_or_else(|| {
        panic!(
            "{id} is not under the pointer near {}: {:?}",
            harness.scene_position_of(id),
            harness.hovered()
        )
    })
}

/// Puts the pointer on the first of the world points `candidates` where the node `id` is, and
/// returns where it is.
fn grab_at(harness: &mut Harness, id: &str, candidates: &[Vec3]) -> Vec2 {
    for world in candidates {
        let pixel = harness.scene_point(*world);
        harness.move_to(pixel + Vec2::X);
        harness.move_to(pixel);
        if is_under_pointer(harness, id) {
            return pixel;
        }
    }
    panic!("{id} is not under the pointer on any of {candidates:?}");
}

/// The control point `index` of the track being edited.
fn point(harness: &Harness, index: usize) -> super::track::ControlPoint {
    harness.state().design.control_points[index]
}

/// Presses at `from` and releases without moving, as a click does.
fn press_and_release(harness: &mut Harness, from: Vec2) {
    harness.drag(from, &[]);
}

/// Drags from `from` by `by` in three steps.
fn drag_by(harness: &mut Harness, from: Vec2, by: Vec2) {
    harness.drag(from, &[from + by / 3.0, from + by * 2.0 / 3.0, from + by]);
}

/// The control points of `editor` but one, which is how edits of a point leave the others.
fn others(editor: &Editor, index: usize) -> Vec<super::track::ControlPoint> {
    let mut points = editor.design.control_points.clone();
    points.remove(index);
    points
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
    use_tool(&mut harness, Tool::Select, None);
    for index in 0..harness.state().design.control_points.len() {
        grab(&mut harness, &format!("track/point-{index}"));
    }
}

#[test]
fn pressing_a_gem_with_the_select_tool_selects_its_point_and_changes_nothing_else() {
    let mut harness = harness();
    let count = harness.state().design.control_points.len();
    for index in [0, POINT, count - 1] {
        use_tool(&mut harness, Tool::Select, None);
        let before = harness.state().design.clone();
        let pixel = grab(&mut harness, &format!("track/point-{index}"));
        press_and_release(&mut harness, pixel);
        assert_eq!(harness.state().selected, Some(index));
        assert_eq!(harness.state().design, before);
    }
}

pub(super) fn move_gem(harness: &mut Harness, index: usize) {
    use_tool(harness, Tool::Move, Some(index));
    let before = harness.state().clone();
    let height = before.handle_position(index).y;
    let start = grab(harness, &format!("track/point-{index}"));
    let end = start + Vec2::new(40.0, 25.0);
    let expected = harness.ground_point(end, height) - harness.ground_point(start, height);
    drag_by(harness, start, end - start);
    let moved = point(&harness, index).position - before.design.control_points[index].position;
    assert_eq!(moved.y, 0.0, "across the ground");
    assert!(
        (moved - expected).length() <= 1.0,
        "{moved} against {expected}"
    );
    assert!(moved.length() > 0.5, "it really moved: {moved}");
    assert_eq!(others(harness.state(), index), others(&before, index));
}

#[test]
fn the_move_tool_drags_a_gem_across_the_ground_to_where_the_pointer_goes() {
    move_gem(&mut harness(), POINT);
}

pub(super) fn raise_with_cone(harness: &mut Harness, index: usize) {
    let id = format!("track/lift-{index}");
    for (direction, name) in [(-1.0, "up"), (1.0, "down")] {
        use_tool(harness, Tool::Raise, Some(index));
        let before = harness.state().clone();
        let start = grab(harness, &id);
        drag_by(harness, start, Vec2::new(0.0, 40.0 * direction));
        let after = point(&harness, index).position;
        let was = before.design.control_points[index].position;
        assert_eq!(
            (after.x, after.z),
            (was.x, was.z),
            "{name}: stays above its place"
        );
        if direction < 0.0 {
            assert!(after.y > was.y + 1.0, "{name}: {was} to {after}");
        } else {
            assert!(after.y < was.y - 1.0, "{name}: {was} to {after}");
        }
        assert_eq!(others(harness.state(), index), others(&before, index));
    }
}

#[test]
fn the_raise_tool_lifts_a_point_with_its_cone_and_lowers_it_by_dragging_down() {
    raise_with_cone(&mut harness(), POINT);
}

pub(super) fn widen_and_narrow(harness: &mut Harness, index: usize) {
    let id = format!("track/width-{index}");
    for outward in [true, false] {
        use_tool(harness, Tool::Width, Some(index));
        let before = harness.state().clone();
        let gem = harness.scene_position_of(&format!("track/point-{index}"));
        let start = grab(harness, &id);
        let out = (start - gem).normalize();
        drag_by(harness, start, out * if outward { 40.0 } else { -40.0 });
        let was = before.design.control_points[index].width;
        let now = point(&harness, index).width;
        if outward {
            assert!(now > was, "wider: {was} to {now}");
            assert!(now <= MAX_POINT_WIDTH);
        } else {
            assert!(now < was, "narrower: {was} to {now}");
            assert!(now >= MIN_POINT_WIDTH);
        }
        assert_eq!(
            point(&harness, index).position,
            before.design.control_points[index].position
        );
        assert_eq!(others(harness.state(), index), others(&before, index));
    }
}

#[test]
fn the_width_tool_widens_the_road_by_dragging_out_and_narrows_it_by_dragging_in() {
    widen_and_narrow(&mut harness(), POINT);
}

pub(super) fn bank_both_ways(harness: &mut Harness, index: usize) {
    let id = format!("track/roll-{index}");
    use_tool(harness, Tool::Roll, Some(index));
    let forward = map(harness).rotation() * Vec3::NEG_Z;
    if harness.state().frames()[index].tangent.dot(forward).abs() < EDGE_ON {
        let model = harness.app.world().resource::<DragModel<Editor>>();
        assert!(
            model.view().get(&id).is_none(),
            "a ring seen edge-on is not offered, as it could not be dragged"
        );
        return;
    }
    let middle = harness.scene_position_of(&id);
    let mut found = None;
    'search: for radius in (4..160).step_by(3) {
        for step in 0..24 {
            let angle = step as f32 * std::f32::consts::TAU / 24.0;
            let pixel = middle + Vec2::from_angle(angle) * radius as f32;
            harness.move_to(pixel + Vec2::X);
            harness.move_to(pixel);
            if is_under_pointer(harness, &id) {
                found = Some((pixel, angle));
                break 'search;
            }
        }
    }
    let (start, angle) = found.expect("the ring is under the pointer somewhere around its middle");
    let tangent = Vec2::from_angle(angle + std::f32::consts::FRAC_PI_2);
    let mut rolls = Vec::new();
    for sign in [1.0, -1.0] {
        harness.show(
            Editor::upstream()
                .with_tool(Tool::Roll)
                .with_selected(Some(index)),
        );
        let before = harness.state().clone();
        harness.move_to(start + Vec2::X);
        harness.move_to(start);
        drag_by(harness, start, tangent * 30.0 * sign);
        let roll = point(&harness, index).roll;
        assert!(roll.abs() > 1e-3, "it banked: {roll}");
        assert!(roll.abs() <= MAX_ROLL + 1e-4, "within the limit: {roll}");
        assert_eq!(
            point(&harness, index).position,
            before.design.control_points[index].position
        );
        assert_eq!(others(harness.state(), index), others(&before, index));
        rolls.push(roll);
    }
    assert!(rolls[0] * rolls[1] < 0.0, "opposite ways: {rolls:?}");
}

#[test]
fn the_roll_tool_banks_the_road_both_ways_by_dragging_around_its_ring() {
    bank_both_ways(&mut harness(), POINT);
}

pub(super) fn insert_from_segment(harness: &mut Harness, index: usize) {
    use_tool(harness, Tool::Insert, None);
    let before = harness.state().design.clone();
    let count = before.control_points.len();
    let start = grab(harness, &format!("track/mid-{index}"));
    drag_by(harness, start, Vec2::new(25.0, 25.0));
    let after = &harness.state().design;
    assert_eq!(after.control_points.len(), count + 1);
    let new = after.control_points[index + 1];
    for neighbour in [
        before.control_points[index],
        before.control_points[index + 1],
    ] {
        assert_ne!(new.position, neighbour.position);
    }
    assert_eq!(
        after.control_points[..=index],
        before.control_points[..=index]
    );
    assert_eq!(
        after.control_points[index + 2..],
        before.control_points[index + 1..]
    );
}

#[test]
fn the_insert_tool_pulls_a_new_point_out_of_a_segment() {
    insert_from_segment(&mut harness(), POINT);
}

#[test]
fn the_delete_tool_removes_the_point_that_is_pressed_but_never_below_the_least_a_track_has() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Delete, None);
    let before = harness.state().design.clone();
    let pixel = grab(&mut harness, &format!("track/point-{POINT}"));
    press_and_release(&mut harness, pixel);
    let after = &harness.state().design;
    assert_eq!(after.control_points.len(), before.control_points.len() - 1);
    let mut expected = before.control_points.clone();
    expected.remove(POINT);
    assert_eq!(after.control_points, expected);

    let mut least = before;
    while least.control_points.len() > MIN_POINTS {
        least = least.with_point_removed(0);
    }
    let editor = harness.state().with_design(least).with_selected(None);
    harness.show(editor);
    run(&mut harness, SETTLE);
    let pixel = (0..MIN_POINTS)
        .find_map(|index| try_grab(&mut harness, &format!("track/point-{index}")))
        .expect("one of the gems of the shortest track can be pressed");
    press_and_release(&mut harness, pixel);
    assert_eq!(harness.state().design.control_points.len(), MIN_POINTS);
}

/// The ids of the handles of the gizmo in the view, without the tips of its arrows.
fn gizmo_handles(harness: &Harness) -> Vec<String> {
    harness
        .app
        .world()
        .resource::<DragModel<Editor>>()
        .view()
        .nodes()
        .iter()
        .filter(|node| node.id.starts_with("anchor/gizmo/translate-") && !node.id.ends_with("/tip"))
        .map(|node| node.id.to_string())
        .collect()
}

pub(super) fn move_with_gizmo(harness: &mut Harness, index: usize) {
    for tool in [Tool::Move] {
        use_tool(harness, tool, Some(index));
        let handles = gizmo_handles(&harness);
        assert!(
            handles.len() >= 3,
            "{tool:?}: the handles that are not seen edge-on stay: {handles:?}"
        );
        let size = harness.state().gizmo_size();
        for id in handles {
            use_tool(harness, tool, Some(index));
            let before = harness.state().clone();
            let letters = id.rsplit('-').next().unwrap().to_owned();
            let axes: Vec<Vec3> = letters
                .chars()
                .map(|letter| match letter {
                    'x' => Vec3::X,
                    'y' => Vec3::Y,
                    _ => Vec3::Z,
                })
                .collect();
            let middle = harness
                .app
                .world()
                .resource::<DragModel<Editor>>()
                .view()
                .world_position(&id)
                .unwrap();
            let spots: Vec<Vec3> = if axes.len() == 1 {
                [0.0, 0.15, -0.15, 0.3, -0.3, 0.45, -0.45]
                    .iter()
                    .map(|part| middle + axes[0] * size * part)
                    .collect()
            } else {
                let parts = [0.0, 0.06, -0.06, 0.12, -0.12];
                parts
                    .iter()
                    .flat_map(|first| parts.iter().map(move |second| (*first, *second)))
                    .map(|(first, second)| middle + (axes[0] * first + axes[1] * second) * size)
                    .collect()
            };
            let start = grab_at(harness, &id, &spots);
            let by = if axes.len() == 1 {
                let along =
                    harness.scene_point(middle + axes[0] * size) - harness.scene_point(middle);
                along.normalize() * 40.0
            } else {
                Vec2::new(30.0, 20.0)
            };
            drag_by(harness, start, by);
            let moved =
                point(&harness, index).position - before.design.control_points[index].position;
            assert!(moved.length() > 0.3, "{tool:?} {id}: it moved: {moved}");
            assert!(
                moved.length() < size * 3.0,
                "{tool:?} {id}: a small drag does not throw it: {moved}"
            );
            for (axis, name) in [(Vec3::X, 'x'), (Vec3::Y, 'y'), (Vec3::Z, 'z')] {
                if !letters.contains(name) {
                    assert!(
                        moved.dot(axis).abs() < 1e-3,
                        "{tool:?} {id}: not along {name}: {moved}"
                    );
                }
            }
            assert_eq!(
                others(harness.state(), index),
                others(&before, index),
                "{id}"
            );
        }
    }
}

#[test]
fn every_handle_of_the_gizmo_moves_the_point_along_its_axis_or_in_its_plane() {
    move_with_gizmo(&mut harness(), POINT);
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
fn the_hub_opens_the_root_ring_and_its_petals_open_the_other_rings() {
    let mut harness = harness();
    for selected in [None, Some(POINT)] {
        harness.show(Editor::upstream().with_selected(selected));
        let id = hub(&harness);
        harness.click(id);
        assert_eq!(harness.state().menu, Some(Ring::Root), "{selected:?}");
        let root = harness.state().clone();
        for item in Ring::Root.items(&root) {
            harness.show(root.clone());
            harness.click(&petal_id::<Editor>(&Ring::Root, &item));
            let Item::Open(ring) = item else {
                panic!("the root ring only opens rings: {item:?}");
            };
            assert_eq!(harness.state().menu, Some(ring), "{selected:?} {item:?}");
        }
        harness.show(root);
        harness.click(hub(&harness));
        assert_eq!(harness.state().menu, None, "the hub closes the menu again");
    }
}

#[test]
fn every_tool_petal_chooses_its_tool_and_closes_the_menu() {
    let mut harness = harness();
    for tool in Tool::ALL.into_iter().filter(|tool| *tool != Tool::Drive) {
        let open = Editor::upstream()
            .with_selected(Some(POINT))
            .with_menu(Some(Ring::Tools));
        harness.show(open);
        harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(tool)));
        assert_eq!(harness.state().tool, tool);
        assert_eq!(harness.state().menu, None, "{tool:?}");
        assert!(!harness.state().testing);
    }
}

#[test]
fn every_road_petal_changes_the_road_at_the_selected_point_as_it_says() {
    let mut harness = harness();
    let design = Editor::upstream().design;
    let start = design.control_points[POINT].position;
    let base = Editor::upstream()
        .with_design(
            design
                .with_point_roll(POINT, 0.3)
                .with_point_width(POINT, 1.2)
                .with_point_moved(POINT, start.with_y(2.0)),
        )
        .with_selected(Some(POINT))
        .with_menu(Some(Ring::Road));
    for preset in Preset::ALL {
        harness.show(base.clone());
        harness.click(&petal_id::<Editor>(&Ring::Road, &Item::Road(preset)));
        let was = base.design.control_points[POINT];
        let now = point(&harness, POINT);
        match preset {
            Preset::Level => assert_eq!(now.roll, 0.0),
            Preset::BankLeft => assert!(now.roll < was.roll, "{preset:?}"),
            Preset::BankRight => assert!(now.roll > was.roll, "{preset:?}"),
            Preset::Wider => assert!(now.width > was.width, "{preset:?}"),
            Preset::Narrower => assert!(now.width < was.width, "{preset:?}"),
            Preset::Ground => assert_eq!(now.position.y, 0.0),
        }
        if !matches!(preset, Preset::Ground) {
            assert_eq!(now.position, was.position, "{preset:?}");
        }
        assert_eq!(
            others(harness.state(), POINT),
            others(&base, POINT),
            "{preset:?}"
        );
        assert_eq!(harness.state().menu, None, "{preset:?}");
    }
}

#[test]
fn every_precision_petal_chooses_its_precision() {
    let mut harness = harness();
    for precision in Precision::ALL {
        harness.show(Editor::upstream().with_menu(Some(Ring::Precision)));
        harness.click(&petal_id::<Editor>(
            &Ring::Precision,
            &Item::Precision(precision),
        ));
        assert_eq!(harness.state().precision, precision);
        assert_eq!(harness.state().menu, None);
    }
}

/// How many karts there are in the app.
fn karts(harness: &mut Harness) -> usize {
    let world = harness.app.world_mut();
    world.query::<&Kart>().iter(world).count()
}

#[test]
fn the_drive_petal_puts_the_kart_on_the_track_and_escape_brings_the_editor_back() {
    let mut harness = harness();
    harness.show(Editor::upstream().with_menu(Some(Ring::Tools)));
    harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(Tool::Drive)));
    assert!(harness.state().testing);
    for _ in 0..5 {
        harness.app.update();
    }
    assert_eq!(karts(&mut harness), 1);

    let window = harness
        .app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(harness.app.world())
        .unwrap();
    harness.app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    for _ in 0..5 {
        harness.app.update();
    }
    assert!(!harness.state().testing);
    assert_eq!(karts(&mut harness), 0);
}

#[test]
fn handles_keep_their_size_on_the_screen_however_far_the_map_is_zoomed() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Select, Some(POINT));
    for _ in 0..5 {
        harness.app.update();
    }
    let (radius, size) = (
        harness.state().handle_radius(),
        harness.state().gizmo_size(),
    );
    let mut cameras = harness.app.world_mut().query::<&mut MapCamera>();
    let before = *cameras.single(harness.app.world()).unwrap();
    let zoomed = before.zoomed(4.0).eased(f32::INFINITY);
    *cameras.single_mut(harness.app.world_mut()).unwrap() = zoomed;
    for _ in 0..5 {
        harness.app.update();
    }
    let scale = zoomed.view_height / before.view_height;
    assert!(scale < 0.7, "it zoomed in: {scale}");
    let now = harness.state();
    assert!((now.handle_radius() / radius - scale).abs() < 1e-2);
    assert!((now.gizmo_size() / size - scale).abs() < 1e-2);
}

/// Points of the ground around the point the map looks at, spread over what it shows, where the
/// terrain may be under the pointer.
fn ground_points(harness: &mut Harness) -> Vec<Vec3> {
    let map = map(harness);
    let middle = map.target();
    let step = map.view_height / 12.0;
    let mut grid = Vec::new();
    for x in -5..=5 {
        for z in -5..=5 {
            grid.push(Vec3::new(
                middle.x + x as f32 * step,
                GROUND_HEIGHT,
                middle.z + z as f32 * step,
            ));
        }
    }
    grid
}

/// The camera of the editor.
fn map(harness: &mut Harness) -> MapCamera {
    let mut cameras = harness.app.world_mut().query::<&MapCamera>();
    *cameras.single(harness.app.world()).unwrap()
}

/// Runs `frames` frames.
fn run(harness: &mut Harness, frames: usize) {
    for _ in 0..frames {
        harness.app.update();
    }
}

#[test]
fn pressing_the_terrain_clears_the_selection_closes_the_menu_and_changes_nothing_else() {
    let mut harness = harness();
    let editor = Editor::upstream()
        .with_tool(Tool::Move)
        .with_selected(Some(POINT))
        .with_menu(Some(Ring::Tools));
    harness.show(editor);
    let before = harness.state().design.clone();
    let candidates = ground_points(&mut harness);
    let pixel = grab_at(&mut harness, "terrain", &candidates);
    press_and_release(&mut harness, pixel);
    assert_eq!(harness.state().selected, None);
    assert_eq!(harness.state().menu, None);
    assert_eq!(harness.state().design, before);
    assert_eq!(harness.state().tool, Tool::Move);
}

#[test]
fn the_wheel_and_the_arrow_keys_zoom_in_on_the_selected_point_and_the_terrain_zooms_out() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Select, Some(POINT));
    run(&mut harness, 5);
    let framed = map(&mut harness);
    let at = point(&harness, POINT).position;
    assert_eq!(framed.view_height, framed.frame_height);

    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);
    run(&mut harness, 90);
    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ArrowUp);
    run(&mut harness, 60);
    let close = map(&mut harness);
    assert!(
        close.view_height < framed.view_height / 2.0,
        "Up zooms in: {} against {}",
        close.view_height,
        framed.view_height
    );
    assert!(
        close.target().distance(at) < close.target().distance(close.center),
        "the view came in on the selected point"
    );

    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowDown);
    run(&mut harness, 400);
    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ArrowDown);
    run(&mut harness, 60);
    let out = map(&mut harness);
    assert!(
        out.view_height > framed.view_height,
        "Down zooms out past the track"
    );
    assert_eq!(
        out.view_height, out.space_height,
        "Down zooms all the way out to the whole space"
    );

    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);
    run(&mut harness, 120);
    harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ArrowUp);
    run(&mut harness, 60);
    let candidates = ground_points(&mut harness);
    let pixel = grab_at(&mut harness, "terrain", &candidates);
    press_and_release(&mut harness, pixel);
    assert_eq!(harness.state().selected, None);
    run(&mut harness, 240);
    let terrain = map(&mut harness);
    assert_eq!(
        terrain.view_height, terrain.frame_height,
        "choosing the terrain zooms all the way out"
    );
    assert!(terrain.target().distance(terrain.center) < 1e-2);
}

#[test]
fn an_empty_editor_has_its_menu_and_every_tool_but_the_drive_waits_for_a_track() {
    let mut harness = empty_harness();
    run(&mut harness, SETTLE);
    assert!(harness.state().design.control_points.is_empty());
    harness.click("menu/hub");
    assert_eq!(harness.state().menu, Some(Ring::Root));
    for tool in Tool::ALL.into_iter().filter(|tool| *tool != Tool::Drive) {
        harness.show(Editor::initial().with_menu(Some(Ring::Tools)));
        harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(tool)));
        assert_eq!(harness.state().tool, tool);
        assert_eq!(harness.state().menu, None, "{tool:?}");
    }
    harness.show(Editor::initial().with_menu(Some(Ring::Tools)));
    run(&mut harness, SETTLE);
    let drive = petal_id::<Editor>(&Ring::Tools, &Item::Tool(Tool::Drive));
    let model = harness.app.world().resource::<DragModel<Editor>>();
    assert!(
        model.view().get(&drive).is_none(),
        "there is nothing to drive on, so the petal is not offered"
    );
    assert_eq!(karts(&mut harness), 0);
}

#[test]
fn a_track_is_laid_down_by_pressing_the_ground_with_insert_and_then_driven() {
    let mut harness = empty_harness();
    use_tool(&mut harness, Tool::Insert, None);
    let corners = [(-40.0, -40.0), (40.0, -40.0), (40.0, 40.0), (-40.0, 40.0)];
    for (index, (x, z)) in corners.into_iter().enumerate() {
        let pixel = harness.scene_point(Vec3::new(x, GROUND_HEIGHT, z));
        press_and_release(&mut harness, pixel);
        run(&mut harness, SETTLE);
        let editor = harness.state().clone();
        assert_eq!(editor.design.control_points.len(), index + 1);
        let placed = editor.design.control_points[index].position;
        assert!(
            placed.distance(Vec3::new(x, 0.0, z)) < 1.0,
            "{index}: {placed}"
        );
        assert_eq!(editor.selected, Some(index), "the new point is selected");
        let drivable = index + 1 >= MIN_POINTS;
        assert_eq!(editor.error().is_none(), drivable, "{index}");
        assert_eq!(
            harness.app.world().contains_resource::<BakedTrack>(),
            drivable,
            "the road is built once the track can be driven"
        );
    }
    let open = harness.state().with_menu(Some(Ring::Tools));
    harness.show(open);
    harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(Tool::Drive)));
    run(&mut harness, 5);
    assert!(harness.state().testing);
    assert_eq!(karts(&mut harness), 1);
}
