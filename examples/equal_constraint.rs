//! Two linked knobs: dragging either keeps both at the same value.
//!
//! The state has two independent numbers, but the `equal` constraint forces them to match.
//! Dragging one knob moves both because the solver must satisfy `a == b`.

use bevy::prelude::*;
use bevy_composable_dragging::constraint::equal;
use bevy_composable_dragging::ops::vary_constrained;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const SPACING: f32 = 3.0;

#[derive(Clone)]
struct State {
    a: f32,
    b: f32,
}

fn knob_position(value: f32, offset: f32) -> Vec2 {
    Vec2::new(offset + value, 0.0)
}

fn view(state: &State) -> View<State> {
    let constraint = equal(param!(State, a), param!(State, b));
    View::new()
        .with(
            ViewNode::circle("knob-a", 0.5)
                .at_xy(knob_position(state.a, -SPACING))
                .with_z(0.1)
                .with_color(Color::srgb(0.95, 0.6, 0.2))
                .draggable({
                    let start = state.clone();
                    let constraint = constraint.clone();
                    move || {
                        vary_constrained(
                            start.clone(),
                            [param!(State, a), param!(State, b)],
                            constraint.clone(),
                        )
                    }
                }),
        )
        .with(
            ViewNode::circle("knob-b", 0.5)
                .at_xy(knob_position(state.b, SPACING))
                .with_z(0.1)
                .with_color(Color::srgb(0.3, 0.7, 0.45))
                .draggable({
                    let start = state.clone();
                    let constraint = constraint.clone();
                    move || {
                        vary_constrained(
                            start.clone(),
                            [param!(State, a), param!(State, b)],
                            constraint.clone(),
                        )
                    }
                }),
        )
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app.add_plugins(common::default_plugins("equal_constraint"))
        .add_plugins(DraggablePlugin::new(State { a: 0.0, b: 0.0 }, view))
        .add_plugins(ScriptedDragPlugin::straight(
            knob_position(0.0, -SPACING),
            knob_position(2.0, -SPACING) + Vec2::new(0.0, 0.5),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_equal_constraint.gif")
                .with_title("equal constraint")
                .with_description("two knobs locked together")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 6.0);
        });
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}
