//! A slider: `vary` keeps the knob under the pointer and a constraint keeps it on the track.
//!
//! The state is a single number. The view function places the knob and the fill bar from it, and
//! the drag spec only names the number to vary and the range it must stay in.

use bevy::prelude::*;
use bevy_composable_dragging::constraint::in_range;
use bevy_composable_dragging::ops::vary_constrained;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const TRACK_WIDTH: f32 = 6.0;

#[derive(Clone)]
struct Slider {
    value: f32,
}

fn knob_position(value: f32) -> Vec2 {
    Vec2::new((value - 0.5) * TRACK_WIDTH, 0.0)
}

fn view(state: &Slider) -> View<Slider> {
    let fill_width = state.value * TRACK_WIDTH;
    let start = state.clone();
    View::new()
        .with(
            ViewNode::rectangle("track", Vec2::new(TRACK_WIDTH, 0.3))
                .with_color(Color::srgb(0.3, 0.32, 0.38)),
        )
        .with(
            ViewNode::rectangle("fill", Vec2::new(fill_width, 0.3))
                .at_xy(Vec2::new((fill_width - TRACK_WIDTH) / 2.0, 0.0))
                .with_z(0.05)
                .with_color(Color::srgb(0.3, 0.7, 0.45)),
        )
        .with(
            ViewNode::circle("knob", 0.9)
                .at_xy(knob_position(state.value))
                .with_z(0.1)
                .with_color(Color::srgb(0.95, 0.6, 0.2))
                .draggable(move || {
                    let value = param!(Slider, value).with_scale(0.5);
                    vary_constrained(start.clone(), [value.clone()], in_range(value, 0.0, 1.0))
                }),
        )
}


/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app
        .add_plugins(common::default_plugins("slider"))
        .add_plugins(DraggablePlugin::new(Slider { value: 0.2 }, view))
        .add_plugins(ScriptedDragPlugin::straight(
            knob_position(0.2),
            knob_position(0.7) + Vec2::new(0.0, 0.6),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_slider.webp")
                .with_title("slider")
                .with_description("vary + in_range")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 5.0);
        })
        ;
}

fn main() {
    App::new()
        .add_plugins(common::default_plugins("slider"))
        .add_plugins(DraggablePlugin::new(Slider { value: 0.2 }, view))
        .add_plugins(ScriptedDragPlugin::straight(
            knob_position(0.2),
            knob_position(0.7) + Vec2::new(0.0, 0.6),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_slider.webp")
                .with_title("slider")
                .with_description("vary + in_range")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 5.0);
        })
        .run();
}
