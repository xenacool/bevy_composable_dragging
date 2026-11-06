//! Tiles on a grid with a toolbar and a trash bin.
//!
//! Dragging a tile floats it over the grid and settles it into the nearest free cell, or into the
//! bin to delete it. Dragging a tool creates a new tile and carries on dragging that tile: the
//! drag starts with `switch_to_state_and_follow` and continues with the tile's own spec.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy_composable_dragging::ops::{closest, drop_target, state, switch_to_state_and_follow};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, View, ViewNode};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const COLUMNS: usize = 5;
const ROWS: usize = 3;
const CELL: f32 = 1.2;
const KINDS: usize = 3;
/// The preview settles into a cell once the dragged tile is this close to it.
const SNAP: f32 = 0.4;

#[derive(Clone)]
struct Tile {
    id: usize,
    kind: usize,
    /// The cell the tile sits in; `None` while a new tile is being dragged out of the toolbar.
    cell: Option<usize>,
    /// Where a tile without a cell is.
    position: Vec2,
}

#[derive(Clone)]
struct Board {
    tiles: Vec<Tile>,
    next_id: usize,
}

fn cell_position(cell: usize) -> Vec2 {
    let (column, row) = (cell % COLUMNS, cell / COLUMNS);
    Vec2::new(
        (column as f32 - 2.0) * CELL - 1.5,
        (1.0 - row as f32) * CELL + 0.4,
    )
}

fn tool_position(kind: usize) -> Vec2 {
    Vec2::new(3.6, (1.0 - kind as f32) * CELL + 0.4)
}

fn tile_position(tile: &Tile) -> Vec2 {
    tile.cell.map_or(tile.position, cell_position)
}

fn tile_color(kind: usize) -> Color {
    Color::hsl([20.0, 200.0, 120.0][kind], 0.65, 0.55)
}

/// A node shaped and colored for a kind of tile; kind 1 is round, the others are square.
fn kind_node(id: String, kind: usize) -> ViewNode<Board> {
    let node = if kind == 1 {
        ViewNode::circle(id, 0.95)
    } else {
        ViewNode::rectangle(id, Vec2::splat(0.95))
    };
    node.with_color(tile_color(kind))
}

fn without(board: &Board, id: usize) -> Board {
    let mut board = board.clone();
    board.tiles.retain(|tile| tile.id != id);
    board
}

fn in_cell(board: &Board, id: usize, cell: usize) -> Board {
    let mut board = board.clone();
    if let Some(tile) = board.tiles.iter_mut().find(|tile| tile.id == id) {
        tile.cell = Some(cell);
    }
    board
}

/// Drag a tile into any free cell, or into the bin to delete it.
fn tile_spec(board: &Board, id: usize) -> DragSpec<Board> {
    let occupied: HashSet<usize> = board
        .tiles
        .iter()
        .filter(|tile| tile.id != id)
        .filter_map(|tile| tile.cell)
        .collect();
    let mut branches = vec![drop_target("bin", without(board, id))];
    branches.extend(
        (0..COLUMNS * ROWS)
            .filter(|cell| !occupied.contains(cell))
            .map(|cell| state(in_cell(board, id, cell))),
    );
    closest(branches).with_floating().with_snap_radius(SNAP)
}

fn view(board: &Board) -> View<Board> {
    let mut view = View::new();
    for cell in 0..COLUMNS * ROWS {
        view.push(
            ViewNode::rectangle(format!("cell-{cell}"), Vec2::splat(CELL - 0.1))
                .at(cell_position(cell))
                .with_color(Color::srgb(0.2, 0.22, 0.27)),
        );
    }
    view.push(
        ViewNode::rectangle("bin", Vec2::new(1.8, 1.0))
            .at(Vec2::new(3.6, -2.4))
            .with_color(Color::srgb(0.55, 0.22, 0.25)),
    );
    for kind in 0..KINDS {
        let created = Board {
            tiles: board
                .tiles
                .iter()
                .cloned()
                .chain([Tile {
                    id: board.next_id,
                    kind,
                    cell: None,
                    position: tool_position(kind),
                }])
                .collect(),
            next_id: board.next_id + 1,
        };
        let new_id = board.next_id;
        view.push(
            kind_node(format!("tool-{kind}"), kind)
                .at(tool_position(kind))
                .with_z(0.05)
                .draggable(move || {
                    switch_to_state_and_follow(created.clone(), format!("tile-{new_id}"))
                }),
        );
    }
    for tile in &board.tiles {
        let id = tile.id;
        let snapshot = board.clone();
        view.push(
            kind_node(format!("tile-{id}"), tile.kind)
                .at(tile_position(tile))
                .with_z(0.1)
                .draggable(move || tile_spec(&snapshot, id)),
        );
    }
    view
}

fn main() {
    let tiles = [(0, 0), (1, 1), (5, 2), (7, 0), (11, 1), (12, 2)]
        .iter()
        .enumerate()
        .map(|(id, &(cell, kind))| Tile {
            id,
            kind,
            cell: Some(cell),
            position: Vec2::ZERO,
        })
        .collect();
    let board = Board { tiles, next_id: 6 };
    App::new()
        .add_plugins(common::default_plugins("tessellations"))
        .add_plugins(DraggablePlugin::new(board, view))
        .add_plugins(ScriptedDragPlugin::straight(
            tool_position(0),
            cell_position(13) + Vec2::new(0.4, 0.3),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_tessellations.webp")
                .with_title("tessellations")
                .with_description("toolbar + bin")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 7.0);
        })
        .run();
}
