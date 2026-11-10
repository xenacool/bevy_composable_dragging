//! A clock: dragging either hand moves time, and the other hand follows.
//!
//! The state is a single number of hours. Both hand tips are positioned from it and both vary
//! it, so grabbing the minute hand winds the hour hand along without any extra code.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy_composable_dragging::ops::vary;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const FACE_RADIUS: f32 = 2.4;
const HOUR_LENGTH: f32 = 1.3;
const MINUTE_LENGTH: f32 = 2.0;

#[derive(Clone)]
struct Clock {
    /// Hours since twelve o'clock; it may grow past twelve.
    hours: f32,
}

/// The math angle (counterclockwise from +X) of the hour hand.
fn hour_angle(hours: f32) -> f32 {
    FRAC_PI_2 - hours / 12.0 * TAU
}

/// The math angle of the minute hand.
fn minute_angle(hours: f32) -> f32 {
    FRAC_PI_2 - hours * TAU
}

fn tip(angle: f32, length: f32) -> Vec2 {
    Vec2::from_angle(angle) * length
}

fn hand(id: &'static str, angle: f32, length: f32, width: f32, color: Color) -> ViewNode<Clock> {
    ViewNode::rectangle(id, Vec2::new(length, width))
        .at_xy(tip(angle, length / 2.0))
        .with_rotation_xy(angle)
        .with_z(0.05)
        .with_color(color)
}

fn view(state: &Clock) -> View<Clock> {
    let hour = hour_angle(state.hours);
    let minute = minute_angle(state.hours);
    let hours = || param!(Clock, hours).with_scale(0.1);
    let (hour_start, minute_start) = (state.clone(), state.clone());
    View::new()
        .with(ViewNode::circle("face", FACE_RADIUS * 2.0).with_color(Color::srgb(0.2, 0.22, 0.27)))
        .with(hand(
            "hour-hand",
            hour,
            HOUR_LENGTH,
            0.14,
            Color::srgb(0.9, 0.9, 0.95),
        ))
        .with(hand(
            "minute-hand",
            minute,
            MINUTE_LENGTH,
            0.08,
            Color::srgb(0.6, 0.75, 0.95),
        ))
        .with(
            ViewNode::circle("hour-tip", 0.5)
                .at_xy(tip(hour, HOUR_LENGTH))
                .with_z(0.1)
                .with_color(Color::srgb(0.95, 0.6, 0.2))
                .draggable(move || vary(hour_start.clone(), [hours()])),
        )
        .with(
            ViewNode::circle("minute-tip", 0.4)
                .at_xy(tip(minute, MINUTE_LENGTH))
                .with_z(0.1)
                .with_color(Color::srgb(0.3, 0.7, 0.45))
                .draggable(move || vary(minute_start.clone(), [hours()])),
        )
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let start = Clock { hours: 10.0 };
    let from = hour_angle(start.hours);
    app.add_plugins(common::default_plugins("clock"))
        .add_plugins(DraggablePlugin::new(start, view))
        .add_plugins(ScriptedDragPlugin::arc(
            Vec2::ZERO,
            HOUR_LENGTH,
            from,
            from - TAU * 0.2,
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_clock.webp")
                .with_title("clock")
                .with_description("vary one number")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 6.0);
        });
}

fn main() {
    let start = Clock { hours: 10.0 };
    let from = hour_angle(start.hours);
    App::new()
        .add_plugins(common::default_plugins("clock"))
        .add_plugins(DraggablePlugin::new(start, view))
        .add_plugins(ScriptedDragPlugin::arc(
            Vec2::ZERO,
            HOUR_LENGTH,
            from,
            from - TAU * 0.2,
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_clock.webp")
                .with_title("clock")
                .with_description("vary one number")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 6.0);
        })
        .run();
}
