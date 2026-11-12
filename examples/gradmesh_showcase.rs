//! Showcase of `bevy_gradmesh` Style variants and shape features.
//!
//! Demonstrates gradient styles, blend modes, noise, stripes, rim lighting,
//! and shape morphing. Each row of shapes uses a different style variant.
//! Drag any shape to see it blend with its neighbors.
//!
//! Run with `cargo run --example gradmesh_showcase --features shapes-3d`.

use bevy::prelude::*;
use bevy_composable_dragging::ops::between;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};
use bevy_gradmesh::Style;

mod common;
use common::screenshot::AutoScreenshotPlugin;

/// Three positions for each shape to drag between.
const POSITIONS: [Vec2; 3] = [Vec2::new(-3.0, 0.0), Vec2::ZERO, Vec2::new(3.0, 0.0)];

/// State: which position each row's shape is at.
#[derive(Clone)]
struct Showcase {
    positions: [usize; 6],
}

fn view(state: &Showcase) -> View<Showcase> {
    let mut view = View::new();
    let styles: Vec<Style> = vec![
        Style::dual_directional_up(Color::hsl(200.0, 0.8, 0.22), Color::hsl(250.0, 0.85, 0.6)),
        Style::dual_radial(Color::hsl(30.0, 0.9, 0.7), Color::hsl(340.0, 0.8, 0.3)),
        Style::multiplicative(Color::srgb(0.8, 0.2, 0.1)),
        Style::dual_directional_up(Color::hsl(120.0, 0.7, 0.2), Color::hsl(160.0, 0.8, 0.5))
            .with_noise(3.0, 0.3),
        Style::dual_directional_up(Color::hsl(260.0, 0.7, 0.25), Color::hsl(300.0, 0.8, 0.55))
            .with_stripes(Vec3::Y, 4.0, 0.2),
        Style::dual_directional_up(Color::hsl(0.0, 0.0, 0.15), Color::hsl(0.0, 0.0, 0.35))
            .with_rim(Color::srgb(1.0, 0.6, 0.3), 2.0),
    ];
    for (row, style) in styles.iter().enumerate() {
        let idx = state.positions[row];
        let pos = POSITIONS[idx];
        let y = -(row as f32) * 1.8;
        let id = format!("shape-{row}");
        let style = style.clone();
        let node = ViewNode::sphere(id, 0.5)
            .at_xy(Vec2::new(pos.x, y + pos.y))
            .with_style(style)
            .draggable({
                let positions = state.positions;
                move || {
                    between([0, 1, 2].map(move |p| {
                        let mut pos = positions;
                        pos[row] = p;
                        Showcase { positions: pos }
                    }))
                    .with_floating()
                }
            });
        view.push(node);
    }
    view
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let state = Showcase { positions: [1; 6] };
    app.add_plugins(common::default_plugins("gradmesh_showcase"))
        .add_plugins(DraggablePlugin::new(state, view))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_gradmesh_showcase.gif")
                .with_title("gradmesh_showcase")
                .with_description("shapes-3d")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_perspective_scene(
                &mut commands,
                Vec3::new(0.0, -3.0, 10.0),
                Vec3::new(0.0, -3.0, 0.0),
            );
        });
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}
