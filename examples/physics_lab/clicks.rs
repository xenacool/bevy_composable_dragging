//! End-to-end tests of the lab: a virtual mouse presses and drags the chips and the pieces of the
//! real app, so everything between the pointer and the state runs: picking, the cameras, the drag
//! engine and the view that follows.

use bevy::prelude::*;

use super::common::SceneCamera;
use super::common::mouse::Mouse;
use super::editor::{Cell, Lab};
use super::lab;
use super::scenario::Primitive;

type Harness = Mouse<Lab, SceneCamera>;

fn harness() -> Harness {
    Mouse::new(|app| {
        lab(app);
    })
}

/// How far from the middle of a node, in pixels, the pointer is tried: a ring has a hole in its
/// middle, and a ray through the hole hits nothing.
const TRIES: [Vec2; 5] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(15.0, 0.0),
    Vec2::new(-15.0, 0.0),
    Vec2::new(0.0, 15.0),
    Vec2::new(0.0, -15.0),
];

/// Puts the pointer on the node `id` of the scene and returns where it is.
fn grab(harness: &mut Harness, id: &str) -> Vec2 {
    let at = harness.scene_position_of(id);
    for offset in TRIES {
        harness.move_to(at + offset + Vec2::X);
        harness.move_to(at + offset);
        if harness
            .hovered()
            .first()
            .is_some_and(|hit| hit.starts_with(id))
        {
            return at + offset;
        }
    }
    panic!("{id} is not under the pointer near {at}");
}

/// Drags the node `id` of the scene onto `cell`.
fn drag_onto(harness: &mut Harness, id: &str, cell: Cell) {
    let from = grab(harness, id);
    let center = cell.center();
    let to = harness.scene_point(Vec3::new(center.x, center.y, 0.0));
    harness.drag(from, &[from.lerp(to, 0.5), to]);
}

#[test]
fn a_chip_dragged_onto_a_free_cell_adds_a_piece_there() {
    let mut harness = harness();
    let before = harness.state().pieces.len();
    let cell = harness.state().free_cells()[0];
    let chip = harness.state().chip_id(Primitive::Torus);
    drag_onto(&mut harness, &chip, cell);
    let lab = harness.state();
    assert_eq!(lab.pieces.len(), before + 1, "the drop added nothing");
    let placed = lab.pieces.last().expect("a piece was placed");
    assert_eq!(placed.primitive, Primitive::Torus);
    assert_eq!(placed.cell, cell);
}

#[test]
fn the_palette_keeps_giving_pieces_of_every_shape_one_after_the_other() {
    let mut harness = harness();
    let before = harness.state().pieces.len();
    for (count, primitive) in Primitive::ALL.into_iter().enumerate() {
        let cell = harness.state().free_cells()[count];
        let chip = harness.state().chip_id(primitive);
        drag_onto(&mut harness, &chip, cell);
        let lab = harness.state();
        assert_eq!(
            lab.pieces.len(),
            before + count + 1,
            "{primitive:?} was not added"
        );
        let placed = lab.pieces.last().expect("a piece was placed");
        assert_eq!((placed.primitive, placed.cell), (primitive, cell));
    }
}

#[test]
fn a_placed_piece_dragged_to_another_cell_moves_there() {
    let mut harness = harness();
    let piece = harness.state().pieces[0];
    let cell = harness.state().free_cells()[5];
    drag_onto(&mut harness, &format!("piece-{}", piece.id), cell);
    let lab = harness.state();
    assert_eq!(lab.pieces.len(), 4);
    let moved = lab
        .pieces
        .iter()
        .find(|p| p.id == piece.id)
        .expect("it is still there");
    assert_eq!(moved.cell, cell);
}

#[test]
fn a_chip_dropped_between_cells_far_from_any_free_cell_still_lands_in_the_nearest() {
    let mut harness = harness();
    let before = harness.state().pieces.len();
    let chip = harness.state().chip_id(Primitive::Sphere);
    let from = grab(&mut harness, &chip);
    let cell = harness.state().free_cells()[3];
    let center = cell.center();
    let to = harness.scene_point(Vec3::new(center.x + 0.2, center.y - 0.2, 0.0));
    harness.drag(from, &[from.lerp(to, 0.5), to]);
    assert_eq!(harness.state().pieces.len(), before + 1);
    assert_eq!(harness.state().pieces.last().map(|p| p.cell), Some(cell));
}
