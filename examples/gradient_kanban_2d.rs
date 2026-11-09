//! A kanban board with gradient 2D shapes from `bevy_gradmesh`.
//!
//! A card floats under the pointer while the other cards preview the nearest insertion point
//! across all columns, and it settles into place once it gets close.
//!
//! Every card carries its own gradient `Style`, which changes with the column it sits in. While a
//! card is dragged, the engine blends the styles of the previewed columns with `Style::lerp`
//! from `bevy_gradmesh`, so the gradient shifts as the card travels.
//!
//! The scene is a plain `Camera2d`: `Rendering::Flat` draws every node as a `Mesh2d` with
//! `GradMeshMaterial2d`, and the `z` of a node orders it.
//!
//! Run with `cargo run --example gradient_kanban_2d --features shapes-2d`.

use bevy::prelude::*;
use bevy_composable_dragging::ops::closest_states;
use bevy_composable_dragging::{DraggablePlugin, Rendering, View, ViewNode};
use bevy_gradmesh::{Shape, ShapeDetail, Style};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const COLUMNS: usize = 3;
const COLUMN_X: [f32; COLUMNS] = [-4.0, 0.0, 4.0];
const COLUMN_SIZE: Vec2 = Vec2::new(3.4, 6.0);
const CARD_SIZE: Vec2 = Vec2::new(2.8, 0.95);
const SLOT_HEIGHT: f32 = 1.15;
/// The preview settles into a slot once the card is this close to it.
const SNAP: f32 = 0.4;
const TITLES: [&str; COLUMNS] = ["todo", "doing", "done"];

#[derive(Clone)]
struct Board {
    columns: [Vec<usize>; COLUMNS],
}

fn slot_position(column: usize, slot: usize) -> Vec2 {
    let top = COLUMN_SIZE.y / 2.0 - 0.9;
    Vec2::new(COLUMN_X[column], top - slot as f32 * SLOT_HEIGHT)
}

fn position_of(board: &Board, card: usize) -> Option<(usize, usize)> {
    board
        .columns
        .iter()
        .enumerate()
        .find_map(|(column, cards)| {
            cards
                .iter()
                .position(|&candidate| candidate == card)
                .map(|slot| (column, slot))
        })
}

/// Every board where `card` sits in some column at some position.
fn placements(board: &Board, card: usize) -> Vec<Board> {
    let mut base = board.clone();
    for cards in &mut base.columns {
        cards.retain(|&candidate| candidate != card);
    }
    let mut placements = Vec::new();
    for column in 0..COLUMNS {
        for slot in 0..=base.columns[column].len() {
            let mut placed = base.clone();
            placed.columns[column].insert(slot, card);
            placements.push(placed);
        }
    }
    placements
}

/// The gradient of a card: a dark shade of its hue at the bottom and a bright color at the top
/// whose hue depends on the column, so the two ends of the gradient change independently.
fn card_style(card: usize, column: usize) -> Style {
    let hue = card as f32 * 47.0;
    Style::dual_directional_up(
        Color::hsl(hue, 0.85, 0.2),
        Color::hsl(hue + column as f32 * 40.0, 0.85, 0.55),
    )
}

/// A unit gradient square, scaled by the caller.
fn rectangle(id: String) -> ViewNode<Board> {
    ViewNode::grad(id, Shape::rectangle(Vec2::ONE, ShapeDetail::Medium))
}

fn view(board: &Board) -> View<Board> {
    let mut view = View::new();
    for (column, x) in COLUMN_X.iter().enumerate() {
        view.push(
            rectangle(format!("column-{column}"))
                .at_xy(Vec2::new(*x, 0.0))
                .with_color(Color::srgb(0.08, 0.1, 0.2))
                .scaled_xy(COLUMN_SIZE),
        );
        view.push(
            ViewNode::ring(format!("marker-{column}"), 0.5, 0.32)
                .at_xy(Vec2::new(*x, COLUMN_SIZE.y / 2.0 - 0.4))
                .with_z(0.1)
                .with_color(Color::hsl(column as f32 * 120.0 + 20.0, 0.9, 0.45))
                .scaled_xy(Vec2::splat(0.5)),
        );
    }
    for (column, cards) in board.columns.iter().enumerate() {
        for (slot, &card) in cards.iter().enumerate() {
            let snapshot = board.clone();
            let hue = card as f32 * 47.0 + column as f32 * 20.0;
            view.push(
                rectangle(format!("card-{card}"))
                    .at_xy(slot_position(column, slot))
                    .with_z(0.2)
                    .with_color(Color::hsl(hue, 0.85, 0.4))
                    .with_style(card_style(card, column))
                    .scaled_xy(CARD_SIZE)
                    .draggable(move || {
                        closest_states(placements(&snapshot, card))
                            .with_floating()
                            .with_snap_radius(SNAP)
                    }),
            );
        }
    }
    view
}


/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let board = Board {
        columns: [vec![0, 1, 2], vec![3, 4], vec![5]],
    };
    let from =
        position_of(&board, 1).map_or(Vec2::ZERO, |(column, slot)| slot_position(column, slot));
    app
        .add_plugins(common::default_plugins("gradient_kanban_2d"))
        .add_plugins(DraggablePlugin::new(board, view).with_rendering(Rendering::Flat))
        .add_plugins(ScriptedDragPlugin::straight(
            from,
            slot_position(1, 1) + Vec2::new(0.3, 0.45),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_gradient_kanban_2d.webp")
                .with_title("gradient_kanban_2d")
                .with_description(TITLES.join(" | "))
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras_2d(&mut commands, 8.0);
        })
        ;
}

fn main() {
    let board = Board {
        columns: [vec![0, 1, 2], vec![3, 4], vec![5]],
    };
    let from =
        position_of(&board, 1).map_or(Vec2::ZERO, |(column, slot)| slot_position(column, slot));
    App::new()
        .add_plugins(common::default_plugins("gradient_kanban_2d"))
        .add_plugins(DraggablePlugin::new(board, view).with_rendering(Rendering::Flat))
        .add_plugins(ScriptedDragPlugin::straight(
            from,
            slot_position(1, 1) + Vec2::new(0.3, 0.45),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_gradient_kanban_2d.webp")
                .with_title("gradient_kanban_2d")
                .with_description(TITLES.join(" | "))
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras_2d(&mut commands, 8.0);
        })
        .run();
}
