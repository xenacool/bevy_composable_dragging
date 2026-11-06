//! Lists and a free canvas: items snap into a list when dragged near one and float when far away.
//!
//! `closest` previews the nearest insertion point of every list while the dragged item floats.
//! `when_far` switches to `vary` once the item is far from every list, so it follows the pointer
//! freely, and `on_drop_with` rounds its final position to a grid.

use bevy::prelude::*;
use bevy_composable_dragging::ops::{closest_states, vary};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const LIST_X: [f32; 2] = [-4.0, -1.2];
const LIST_SIZE: Vec2 = Vec2::new(2.4, 5.6);
const SLOT_HEIGHT: f32 = 0.9;
const ITEMS: usize = 6;
/// Beyond this distance from every insertion point an item is free.
const FAR: f32 = 1.4;
/// Free positions are rounded to this grid when dropped.
const GRID: f32 = 0.25;

#[derive(Clone)]
struct Free {
    item: usize,
    x: f32,
    y: f32,
}

#[derive(Clone)]
struct Board {
    lists: Vec<Vec<usize>>,
    free: Vec<Free>,
}

fn slot_position(list: usize, slot: usize) -> Vec2 {
    let top = LIST_SIZE.y / 2.0 - 0.55;
    Vec2::new(LIST_X[list], top - slot as f32 * SLOT_HEIGHT)
}

fn item_position(board: &Board, item: usize) -> Vec2 {
    for (list, items) in board.lists.iter().enumerate() {
        if let Some(slot) = items.iter().position(|&candidate| candidate == item) {
            return slot_position(list, slot);
        }
    }
    board
        .free
        .iter()
        .find(|free| free.item == item)
        .map_or(Vec2::ZERO, |free| Vec2::new(free.x, free.y))
}

fn without(board: &Board, item: usize) -> Board {
    let mut board = board.clone();
    for list in &mut board.lists {
        list.retain(|&candidate| candidate != item);
    }
    board.free.retain(|free| free.item != item);
    board
}

/// Every board where `item` sits in some list at some position.
fn placements(board: &Board, item: usize) -> Vec<Board> {
    let base = without(board, item);
    let mut placements = Vec::new();
    for list in 0..base.lists.len() {
        for slot in 0..=base.lists[list].len() {
            let mut placed = base.clone();
            placed.lists[list].insert(slot, item);
            placements.push(placed);
        }
    }
    placements
}

fn item_spec(board: &Board, item: usize) -> DragSpec<Board> {
    let position = item_position(board, item);
    let mut free = without(board, item);
    free.free.push(Free {
        item,
        x: position.x,
        y: position.y,
    });
    let index = free.free.len() - 1;
    let free_spec = vary(
        free,
        [
            param!(Board, free[index].x).with_scale(1.0),
            param!(Board, free[index].y).with_scale(1.0),
        ],
    );
    closest_states(placements(board, item))
        .with_floating()
        .when_far(free_spec, FAR)
        .on_drop_with(|board| {
            let mut board = board.clone();
            for free in &mut board.free {
                free.x = (free.x / GRID).round() * GRID;
                free.y = (free.y / GRID).round() * GRID;
            }
            board
        })
}

fn view(board: &Board) -> View<Board> {
    let mut view = View::new();
    for (list, x) in LIST_X.iter().enumerate() {
        view.push(
            ViewNode::rectangle(format!("list-{list}"), LIST_SIZE)
                .at(Vec2::new(*x, 0.0))
                .with_color(Color::srgb(0.2, 0.22, 0.27)),
        );
    }
    for item in 0..ITEMS {
        let on_list = board.lists.iter().any(|list| list.contains(&item));
        let hue = item as f32 * 360.0 / ITEMS as f32;
        let snapshot = board.clone();
        view.push(
            ViewNode::rectangle(format!("item-{item}"), Vec2::new(2.0, 0.7))
                .at(item_position(board, item))
                .with_z(if on_list { 0.1 } else { 0.2 })
                .with_color(Color::hsl(hue, 0.65, 0.55))
                .draggable(move || item_spec(&snapshot, item)),
        );
    }
    view
}

fn main() {
    let board = Board {
        lists: vec![vec![0, 1, 2], vec![3, 4]],
        free: vec![Free {
            item: 5,
            x: 3.0,
            y: 1.8,
        }],
    };
    App::new()
        .add_plugins(common::default_plugins("lists_in_lists"))
        .add_plugins(DraggablePlugin::new(board, view))
        .add_plugins(ScriptedDragPlugin::straight(
            slot_position(0, 0),
            Vec2::new(2.6, -0.6),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_lists_in_lists.webp")
                .with_title("lists_in_lists")
                .with_description("closest + when_far")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 7.0);
        })
        .run();
}
