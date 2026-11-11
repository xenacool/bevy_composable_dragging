//! Stickiness: a dot snaps to the nearest slot, resisting small moves past the boundary.
//!
//! Three slots sit in a row. Without stickiness the dot switches slots at the midpoint.
//! With stickiness the dot stays in its current slot until the pointer is clearly closer
//! to another one, avoiding flicker at the boundary.

use bevy::prelude::*;
use bevy_composable_dragging::ops::closest_states_with_stickiness;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const POSITIONS: [f32; 3] = [-3.0, 0.0, 3.0];
const STICKINESS: f32 = 1.0;

#[derive(Clone)]
struct State {
    pos: usize,
}

fn slot_position(slot: usize) -> Vec2 {
    Vec2::new(POSITIONS[slot], 0.0)
}

fn view(state: &State) -> View<State> {
    View::new()
        .with(
            ViewNode::circle("dot", 0.5)
                .at_xy(slot_position(state.pos))
                .with_z(0.1)
                .with_color(Color::srgb(0.95, 0.6, 0.2))
                .draggable(move || {
                    closest_states_with_stickiness([0, 1, 2].map(|p| State { pos: p }), STICKINESS)
                }),
        )
        .with(
            ViewNode::circle("slot-0", 0.7)
                .at_xy(slot_position(0))
                .with_color(Color::srgb(0.3, 0.32, 0.38)),
        )
        .with(
            ViewNode::circle("slot-1", 0.7)
                .at_xy(slot_position(1))
                .with_color(Color::srgb(0.3, 0.32, 0.38)),
        )
        .with(
            ViewNode::circle("slot-2", 0.7)
                .at_xy(slot_position(2))
                .with_color(Color::srgb(0.3, 0.32, 0.38)),
        )
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app.add_plugins(common::default_plugins("stickiness"))
        .add_plugins(DraggablePlugin::new(State { pos: 0 }, view))
        .add_plugins(ScriptedDragPlugin::straight(
            slot_position(0),
            slot_position(2) + Vec2::new(0.0, 0.3),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_stickiness.gif")
                .with_title("stickiness")
                .with_description("closest with stickiness bias")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 8.0);
        });
}

fn main() {
    App::new()
        .add_plugins(common::default_plugins("stickiness"))
        .add_plugins(DraggablePlugin::new(State { pos: 0 }, view))
        .add_plugins(ScriptedDragPlugin::straight(
            slot_position(0),
            slot_position(2) + Vec2::new(0.0, 0.3),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_stickiness.gif")
                .with_title("stickiness")
                .with_description("closest with stickiness bias")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 8.0);
        })
        .run();
}
