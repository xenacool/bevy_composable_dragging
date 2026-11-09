//! The land in the editor, end to end: the Hills and Holes tools with the virtual mouse, the
//! chunks the world draws from the land the editor holds, and a kart that falls through it.

use bevy::prelude::*;
use bevy_composable_dragging::DragModel;
use bevy_composable_dragging::radial::{RadialHost, petal_id};

use super::{
    Harness, drag_by, ground_points, harness, is_under_pointer, press_and_release, run, use_tool,
};
use crate::ground::{BASE, CHUNKS, Ground};
use crate::kart::{self, Kart, KartState, abyss};
use crate::menu::{Item, Ring};
use crate::race::{Checkpoints, Race};
use crate::state::{Editor, Tool};
use crate::track::BakedTrack;
use crate::view::{CURSOR, GROUND_HEIGHT};
use crate::world::GroundChunk;

/// How far, in pixels, a drag goes to change the land.
const DRAG: f32 = 60.0;

/// The pixels, over the part of the ground the map shows, where a press lands on the terrain.
fn ground_pixels(harness: &mut Harness) -> Vec<Vec2> {
    let mut pixels = Vec::new();
    for world in ground_points(harness) {
        let pixel = harness.scene_point(world);
        harness.move_to(pixel + Vec2::X);
        harness.move_to(pixel);
        if is_under_pointer(harness, "terrain") {
            pixels.push(pixel);
        }
    }
    assert!(pixels.len() > 10, "{} places on the terrain", pixels.len());
    pixels
}

/// Where the ground is under `pixel`, before it has been changed.
fn ground_at(harness: &mut Harness, pixel: Vec2) -> Vec2 {
    harness.ground_point(pixel, GROUND_HEIGHT).xz()
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
fn dragging_up_with_the_hills_tool_raises_the_land_under_the_pointer_and_down_lowers_it() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hills, None);
    let before = harness.state().clone();
    let pixels = ground_pixels(&mut harness);
    let (up, down) = (pixels[0], pixels[pixels.len() - 1]);
    let (raised_at, lowered_at) = (ground_at(&mut harness, up), ground_at(&mut harness, down));

    drag_by(&mut harness, up, Vec2::new(0.0, -DRAG));
    let after = harness.state();
    assert!(
        after.ground.surface(raised_at) > BASE + 1.0,
        "{}",
        after.ground.surface(raised_at)
    );
    assert!(after.ground.surface(raised_at) <= after.ground.highest());
    assert_eq!(after.design, before.design, "the road is not touched");
    assert_eq!(after.selected, before.selected);
    assert!(after.cursor.is_none(), "the brush is put away");
    let far = after.ground.middle() + Vec2::splat(after.ground.half() * 0.95);
    assert_eq!(after.ground.height_at(far), before.ground.height_at(far));

    drag_by(&mut harness, down, Vec2::new(0.0, DRAG));
    let after = harness.state();
    assert!(
        after.ground.surface(lowered_at) < BASE - 1.0,
        "{}",
        after.ground.surface(lowered_at)
    );
    assert!(after.ground.surface(lowered_at) >= after.ground.lowest());
    assert!(
        after.ground.surface(raised_at) > BASE + 1.0,
        "the hill stays"
    );
}

#[test]
fn the_hills_brush_is_a_column_while_the_press_is_held_and_the_land_changes_when_it_is_let_go() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hills, None);
    let before = harness.state().ground.clone();
    let pixel = ground_pixels(&mut harness)[0];
    assert!(!has_node(&harness, CURSOR));

    harness.move_to(pixel);
    harness.press();
    harness.move_to(pixel + Vec2::new(0.0, -DRAG / 2.0));
    harness.move_to(pixel + Vec2::new(0.0, -DRAG));
    run(&mut harness, 5);
    assert!(has_node(&harness, CURSOR), "the column shows the brush");
    assert_eq!(
        harness.state().ground,
        before,
        "the land waits for the release"
    );
    harness.release();
    run(&mut harness, super::SETTLE);
    assert!(!has_node(&harness, CURSOR));
    assert_ne!(harness.state().ground, before);
}

#[test]
fn the_hills_brush_is_as_coarse_as_the_land_and_the_precision_says() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hills, None);
    let pixel = ground_pixels(&mut harness)[0];
    let at = ground_at(&mut harness, pixel);
    let mut widths = Vec::new();
    for precision in [
        crate::state::Precision::Fine,
        crate::state::Precision::Coarse,
    ] {
        let editor = Editor::upstream()
            .with_tool(Tool::Hills)
            .with_precision(precision);
        harness.show(editor);
        run(&mut harness, super::SETTLE);
        drag_by(&mut harness, pixel, Vec2::new(0.0, -DRAG));
        let ground = &harness.state().ground;
        let step = ground.pitch();
        let reach = (1..200)
            .map(|cell| cell as f32 * step)
            .take_while(|distance| ground.surface(at + Vec2::X * *distance) > BASE + 1e-3)
            .count();
        widths.push(reach);
    }
    assert!(widths[0] > 0 && widths[1] > widths[0], "{widths:?}");
}

#[test]
fn a_press_with_the_holes_tool_cuts_a_hole_and_a_press_in_the_hole_fills_it_again() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hole, None);
    let before = harness.state().clone();
    let pixel = ground_pixels(&mut harness)[0];
    let at = ground_at(&mut harness, pixel);
    assert!(!before.ground.is_hole(at));

    press_and_release(&mut harness, pixel);
    assert!(
        harness.state().ground.is_hole(at),
        "a hole where it was pressed"
    );
    assert_eq!(harness.state().ground.height_at(at), None);
    assert_eq!(harness.state().design, before.design);
    let far = before.ground.middle() + Vec2::splat(before.ground.half() * 0.95);
    assert!(!harness.state().ground.is_hole(far));

    press_and_release(&mut harness, pixel);
    assert!(
        !harness.state().ground.is_hole(at),
        "pressing the hole fills it"
    );
    assert_eq!(harness.state().ground, before.ground);
}

#[test]
fn dragging_with_the_holes_tool_cuts_a_trench_in_one_stroke() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hole, None);
    let pixels = ground_pixels(&mut harness);
    let start = pixels[0];
    let end = start + Vec2::new(90.0, 40.0);
    let (from, to) = (ground_at(&mut harness, start), ground_at(&mut harness, end));
    drag_by(&mut harness, start, end - start);
    let ground = &harness.state().ground;
    for along in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert!(ground.is_hole(from.lerp(to, along)), "{along}");
    }
    let aside = from.lerp(to, 0.5) + (to - from).perp().normalize() * ground.pitch() * 20.0;
    assert!(!ground.is_hole(aside), "the trench is narrow");
}

#[test]
fn a_stroke_that_starts_in_a_hole_fills_the_land_it_crosses() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hole, None);
    let pixel = ground_pixels(&mut harness)[0];
    let end = pixel + Vec2::new(90.0, 40.0);
    let (from, to) = (ground_at(&mut harness, pixel), ground_at(&mut harness, end));
    drag_by(&mut harness, pixel, end - pixel);
    assert!(harness.state().ground.is_hole(from.lerp(to, 0.5)));
    drag_by(&mut harness, pixel, end - pixel);
    assert_eq!(harness.state().ground, Editor::upstream().ground);
}

#[test]
fn a_press_that_is_not_on_the_land_changes_nothing() {
    let mut harness = harness();
    for tool in [Tool::Hills, Tool::Hole] {
        let editor = Editor::upstream().with_tool(tool);
        let small = Ground::new(editor.ground.middle(), 50.0, CHUNKS, BASE);
        harness.show(editor.with_ground(small));
        run(&mut harness, super::SETTLE);
        let before = harness.state().clone();
        let pixel = ground_pixels(&mut harness)[0];
        assert!(
            !before.ground.contains(ground_at(&mut harness, pixel)),
            "{tool:?}: the press is beyond the land"
        );
        drag_by(&mut harness, pixel, Vec2::new(30.0, -DRAG));
        assert_eq!(harness.state().ground, before.ground, "{tool:?}");
        assert_eq!(harness.state().design, before.design, "{tool:?}");
    }
}

#[test]
fn the_world_draws_the_land_the_editor_holds_and_builds_again_only_the_chunks_that_changed() {
    let mut harness = harness();
    let chunks = |harness: &mut Harness| -> Vec<(usize, u64)> {
        let world = harness.app.world_mut();
        let mut chunks: Vec<(usize, u64)> = world
            .query::<&GroundChunk>()
            .iter(world)
            .map(|chunk| (chunk.index, chunk.key))
            .collect();
        chunks.sort_unstable();
        chunks
    };
    let drawn = chunks(&mut harness);
    let ground = harness.state().ground.clone();
    assert_eq!(drawn.len(), ground.chunk_count());
    assert_eq!(harness.app.world().resource::<Ground>(), &ground);
    for (index, key) in &drawn {
        assert_eq!(*key, ground.chunk_key(*index), "{index}");
    }
    let world = harness.app.world_mut();
    for (chunk, transform) in world.query::<(&GroundChunk, &Transform)>().iter(world) {
        assert_eq!(
            transform.translation,
            ground.chunk_center(chunk.index),
            "chunk {} is not where it belongs",
            chunk.index
        );
    }

    use_tool(&mut harness, Tool::Hole, None);
    let pixel = ground_pixels(&mut harness)[0];
    let at = ground_at(&mut harness, pixel);
    press_and_release(&mut harness, pixel);
    run(&mut harness, 5);
    let now = chunks(&mut harness);
    let held = harness.state().ground.clone();
    assert_eq!(harness.app.world().resource::<Ground>(), &held);
    let changed: Vec<usize> = drawn
        .iter()
        .zip(&now)
        .filter(|(before, after)| before.1 != after.1)
        .map(|(before, _)| before.0)
        .collect();
    assert!(
        !changed.is_empty(),
        "the chunk under the press was built again"
    );
    assert!(changed.len() < ground.chunk_count() / 4, "{changed:?}");
    assert!(changed.iter().all(|index| {
        let chunk = ground.chunk_key(*index);
        chunk != held.chunk_key(*index)
    }));
    assert!(held.is_hole(at));
    for (index, key) in &now {
        assert_eq!(*key, held.chunk_key(*index), "{index}");
    }
}

#[test]
fn the_land_is_the_land_of_the_drive() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hole, None);
    let pixel = ground_pixels(&mut harness)[0];
    press_and_release(&mut harness, pixel);
    let held = harness.state().ground.clone();
    harness.show(harness.state().with_menu(Some(Ring::Tools)));
    harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(Tool::Drive)));
    run(&mut harness, 10);
    assert!(harness.state().testing);
    assert_eq!(harness.app.world().resource::<Ground>(), &held);
}

/// Where a kart beside the road at the start would stand: to its right, `beside` metres from the
/// edge, on the ground.
fn beside_the_road(harness: &mut Harness, beside: f32) -> Vec2 {
    let track = harness.state().design.bake();
    let frame = track.sample(0.0);
    frame.surface_point(frame.width / 2.0 + beside).xz()
}

/// Drives `harness` until the kart, which fell, is back above the abyss, and returns how many
/// frames that took, or `None` when it never fell or never came back.
fn fall_and_return(harness: &mut Harness) -> Option<usize> {
    let mut fell = None;
    for frame in 0..4000 {
        harness.app.update();
        let world = harness.app.world();
        let (state, track, ground) = (
            world.resource::<KartState>(),
            world.resource::<BakedTrack>(),
            world.resource::<Ground>(),
        );
        let below = abyss(ground, track);
        if state.position.y < below + (BASE - below) / 2.0 {
            fell = Some(frame);
        }
        if fell.is_some() && state.position.y > BASE - 1.0 && state.speed.abs() < 0.1 {
            return Some(frame);
        }
    }
    None
}

/// Cuts a hole beside the road at the start, puts the kart in it and lets it fall, and returns
/// where the kart is once it is back.
fn fall_through_a_hole(race_next: usize) -> (KartState, f32) {
    let mut harness = harness();
    let spot = beside_the_road(&mut harness, 30.0);
    let editor = harness.state().clone();
    let ground = editor.ground.with_holes(spot, 60.0, true);
    harness.show(editor.with_ground(ground).with_menu(Some(Ring::Tools)));
    harness.click(&petal_id::<Editor>(&Ring::Tools, &Item::Tool(Tool::Drive)));
    run(&mut harness, 10);
    assert_eq!(
        harness
            .app
            .world_mut()
            .query::<&Kart>()
            .iter(harness.app.world())
            .count(),
        1
    );
    harness.app.world_mut().resource_mut::<Race>().next = race_next;
    let gate = harness
        .app
        .world()
        .resource::<Checkpoints>()
        .0
        .get(race_next.saturating_sub(1))
        .map_or(0.0, |gate| gate.distance);
    kart::put(
        &mut harness.app,
        KartState {
            position: Vec3::new(spot.x, BASE + 0.5, spot.y),
            ..KartState::default()
        },
    );
    assert!(
        fall_and_return(&mut harness).is_some(),
        "the kart falls and comes back"
    );
    (*harness.app.world().resource::<KartState>(), gate)
}

#[test]
fn a_kart_that_falls_through_a_hole_is_put_back_at_the_start_line_when_it_has_passed_nothing() {
    let (state, gate) = fall_through_a_hole(0);
    let track = Editor::upstream().design.bake();
    assert_eq!(gate, 0.0);
    let start = track.sample_at_distance(0.0);
    assert!(
        state.position.distance(start.position) < 5.0,
        "{}",
        state.position
    );
    assert!(state.speed.abs() < 0.1, "{}", state.speed);
    assert!(!state.falling());
}

#[test]
fn a_kart_that_falls_is_put_back_at_the_last_checkpoint_it_passed() {
    let (state, gate) = fall_through_a_hole(3);
    let track = Editor::upstream().design.bake();
    assert!(gate > 0.0, "the checkpoint after the start line");
    let frame = track.sample_at_distance(gate);
    assert!(
        state.position.distance(frame.position) < 5.0,
        "{}",
        state.position
    );
    assert!(state.speed.abs() < 0.1, "{}", state.speed);
}

#[test]
fn a_hill_dragged_up_over_the_road_leaves_the_road_clear_and_the_hill_beside_it() {
    let mut harness = harness();
    use_tool(&mut harness, Tool::Hills, None);
    let track = harness.state().design.bake();
    let frames = track.frames();
    let pixel = (0..frames.len())
        .step_by(97)
        .find_map(|index| {
            let at = frames[index].position;
            let pixel = harness.scene_point(Vec3::new(at.x, GROUND_HEIGHT, at.z));
            harness.move_to(pixel + Vec2::X);
            harness.move_to(pixel);
            is_under_pointer(&mut harness, "terrain").then_some((index, pixel))
        })
        .expect("a place on the road where the terrain is under the pointer");
    let (index, pixel) = pixel;
    drag_by(&mut harness, pixel, Vec2::new(0.0, -3.0 * DRAG));

    let ground = &harness.state().ground;
    for frame in frames.iter().step_by(7) {
        for side in [-1.0, 0.0, 1.0] {
            let at = frame.surface_point(side * frame.width / 2.0);
            assert!(
                ground.surface(at.xz()) < at.y,
                "the land is {} where the road is {}",
                ground.surface(at.xz()),
                at.y
            );
        }
    }
    let frame = frames[index];
    let around = (0..24)
        .map(|step| {
            let turn = Vec2::from_angle(step as f32 * std::f32::consts::TAU / 24.0);
            ground.surface(frame.position.xz() + turn * ground.pitch() * 2.2)
        })
        .fold(f32::MIN, f32::max);
    assert!(
        around > frame.position.y + 10.0,
        "the hill stands around the road: {around}"
    );
}
