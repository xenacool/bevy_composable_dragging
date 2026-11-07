//! A three-way selector: `between` blends inside the hull of three targets.
//!
//! The knob blends its color between the neighboring choices while it is dragged along the rail.

use bevy::prelude::*;
use bevy_composable_dragging::ops::between;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

#[derive(Clone)]
enum Choice {
    Red,
    Green,
    Blue,
}

const CHOICES: [Choice; 3] = [Choice::Red, Choice::Green, Choice::Blue];

impl Choice {
    fn x(&self) -> f32 {
        match self {
            Choice::Red => -2.5,
            Choice::Green => 0.0,
            Choice::Blue => 2.5,
        }
    }

    fn color(&self) -> Color {
        match self {
            Choice::Red => Color::srgb(0.9, 0.3, 0.3),
            Choice::Green => Color::srgb(0.3, 0.8, 0.4),
            Choice::Blue => Color::srgb(0.3, 0.5, 0.95),
        }
    }

    fn id(&self) -> &'static str {
        match self {
            Choice::Red => "red",
            Choice::Green => "green",
            Choice::Blue => "blue",
        }
    }
}

fn view(state: &Choice) -> View<Choice> {
    let mut view = View::new().with(
        ViewNode::rectangle("rail", Vec2::new(5.6, 0.25)).with_color(Color::srgb(0.3, 0.32, 0.38)),
    );
    for choice in CHOICES {
        view.push(
            ViewNode::circle(format!("dot-{}", choice.id()), 0.5)
                .at_xy(Vec2::new(choice.x(), 0.0))
                .with_z(0.05)
                .with_color(Color::srgb(0.55, 0.57, 0.62)),
        );
    }
    view.with(
        ViewNode::circle("knob", 1.1)
            .at_xy(Vec2::new(state.x(), 0.0))
            .with_z(0.1)
            .with_color(state.color())
            .draggable(|| between(CHOICES)),
    )
}

fn main() {
    App::new()
        .add_plugins(common::default_plugins("three_way"))
        .add_plugins(DraggablePlugin::new(Choice::Red, view))
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(-2.5, 0.0),
            Vec2::new(-0.9, 0.0),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_three_way.webp")
                .with_title("three_way")
                .with_description("between: three targets")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 5.0);
        })
        .run();
}
