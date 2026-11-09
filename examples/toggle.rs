//! A toggle switch: drag the knob and the whole switch blends between off and on.
//!
//! The view function only describes the two states. `between` makes the knob slide and the track
//! color fade while dragging, and releasing snaps to the nearer state.

use bevy::prelude::*;
use bevy_composable_dragging::ops::between;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

#[derive(Clone)]
enum Toggle {
    Off,
    On,
}

fn view(state: &Toggle) -> View<Toggle> {
    let (knob_x, track_color) = match state {
        Toggle::Off => (-0.9, Color::srgb(0.35, 0.37, 0.42)),
        Toggle::On => (0.9, Color::srgb(0.25, 0.7, 0.4)),
    };
    View::new()
        .with(ViewNode::rectangle("track", Vec2::new(3.2, 1.5)).with_color(track_color))
        .with(
            ViewNode::circle("knob", 1.2)
                .at_xy(Vec2::new(knob_x, 0.0))
                .with_z(0.1)
                .with_color(Color::WHITE)
                .draggable(|| between([Toggle::Off, Toggle::On])),
        )
}


/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app
        .add_plugins(common::default_plugins("toggle"))
        .add_plugins(DraggablePlugin::new(Toggle::Off, view))
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(-0.9, 0.0),
            Vec2::new(0.1, 0.0),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_toggle.webp")
                .with_title("toggle")
                .with_description("between: knob half way")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 5.0);
        })
        ;
}

fn main() {
    App::new()
        .add_plugins(common::default_plugins("toggle"))
        .add_plugins(DraggablePlugin::new(Toggle::Off, view))
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(-0.9, 0.0),
            Vec2::new(0.1, 0.0),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_toggle.webp")
                .with_title("toggle")
                .with_description("between: knob half way")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 5.0);
        })
        .run();
}
