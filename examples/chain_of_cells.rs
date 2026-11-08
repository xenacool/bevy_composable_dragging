//! A token that walks across a row of cells in a single drag.
//!
//! The drag only offers the neighboring cells. `with_chaining` commits the state of the nearest
//! cell as soon as the token leaves its own, and the drag carries on from there, so one gesture
//! can cross the whole row. `with_overlay` underlines the cell the token would land in,
//! `with_floating` keeps the token under the pointer, and `with_drop_transition` makes the final
//! settle quick.

use bevy::prelude::*;
use bevy_composable_dragging::ops::{closest, state};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, View, ViewNode};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const CELLS: usize = 7;
const SPACING: f32 = 1.5;
/// How long, in seconds, the token takes to settle after it is dropped.
const SETTLE: f32 = 0.15;

/// The cell the token is in.
#[derive(Clone, PartialEq)]
struct Token(usize);

fn cell_position(cell: usize) -> Vec2 {
    Vec2::new((cell as f32 - (CELLS - 1) as f32 / 2.0) * SPACING, 0.0)
}

/// The underline drawn below the cell that the token would land in.
fn target_marker(cell: usize) -> ViewNode<Token> {
    ViewNode::rectangle("target", Vec2::new(1.1, 0.12))
        .at_xy(cell_position(cell) - Vec2::new(0.0, 0.95))
        .with_color(Color::srgb(1.0, 0.85, 0.3))
}

/// The token may move to the cells next to its own; chaining extends that reach as it moves.
fn walk(token: &Token) -> DragSpec<Token> {
    let neighbors = [
        token.0.saturating_sub(1),
        token.0,
        (token.0 + 1).min(CELLS - 1),
    ];
    closest(
        neighbors.map(|cell| state(Token(cell)).with_overlay(move |_| vec![target_marker(cell)])),
    )
    .with_floating()
    .with_chaining()
    .with_drop_transition(SETTLE)
}

fn view(token: &Token) -> View<Token> {
    let mut view = View::new();
    for cell in 0..CELLS {
        view.push(
            ViewNode::rectangle(format!("cell-{cell}"), Vec2::splat(1.2))
                .at_xy(cell_position(cell))
                .with_color(Color::srgb(0.22, 0.24, 0.3)),
        );
    }
    let current = token.clone();
    view.with(
        ViewNode::circle("token", 0.9)
            .at_xy(cell_position(token.0))
            .with_z(0.1)
            .with_color(Color::srgb(0.95, 0.5, 0.35))
            .draggable(move || walk(&current)),
    )
}

fn main() {
    App::new()
        .add_plugins(common::default_plugins("chain_of_cells"))
        .add_plugins(DraggablePlugin::new(Token(0), view))
        .add_plugins(ScriptedDragPlugin::straight(
            cell_position(0),
            cell_position(5),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_chain_of_cells.webp")
                .with_title("chain_of_cells")
                .with_description("with_chaining + overlay")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 6.0);
        })
        .run();
}
