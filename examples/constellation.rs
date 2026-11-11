//! A constellation: the `substate` zipper focuses on one star while the sky stays whole.
//!
//! Dragging a star focuses on that star's position alone. A ring marks the focus, the star
//! cannot leave the sky, the lines to its neighbors stretch with it, and every other star stays
//! in view and draggable.

use bevy::prelude::*;
use bevy_composable_dragging::constraint::{and, in_range};
use bevy_composable_dragging::ops::{Lens, substate, vary_constrained};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const STAR_DIAMETER: f32 = 0.8;
/// The stars live in `-SKY..=SKY`.
const SKY: Vec2 = Vec2::new(6.0, 3.5);

/// A star in the constellation: its color and its position.
#[derive(Clone, Debug, PartialEq)]
struct Star {
    color: Color,
    position: Vec2,
}

/// The stars and the lines connecting pairs of them by index.
#[derive(Clone, Debug, PartialEq)]
struct Constellation {
    stars: Vec<Star>,
    lines: Vec<(usize, usize)>,
}

/// Moves one star: the spec only sees the star's position and keeps it in the sky.
fn star_spec(constellation: &Constellation, index: usize) -> DragSpec<Constellation> {
    let position = Lens::new(
        move |constellation: &Constellation| constellation.stars[index].position,
        move |constellation: &Constellation, position| {
            let mut constellation = constellation.clone();
            constellation.stars[index].position = position;
            constellation
        },
    );
    let in_sky = and([
        in_range(param!(Vec2, x), -SKY.x, SKY.x),
        in_range(param!(Vec2, y), -SKY.y, SKY.y),
    ]);
    let free = vary_constrained(
        constellation.stars[index].position,
        [param!(Vec2, x), param!(Vec2, y)],
        in_sky,
    );
    substate(position, free).with_overlay(move |frame| {
        frame
            .view
            .world_position(&format!("star-{index}"))
            .map(|at| {
                vec![
                    ViewNode::ring("focus", STAR_DIAMETER, STAR_DIAMETER * 0.85)
                        .at_xy(at.truncate())
                        .with_color(Color::srgb(1.0, 0.95, 0.6)),
                ]
            })
            .unwrap_or_default()
    })
}

fn view(constellation: &Constellation) -> View<Constellation> {
    let mut view = View::new();
    for (index, &(a, b)) in constellation.lines.iter().enumerate() {
        let from = constellation.stars[a].position;
        let to = constellation.stars[b].position;
        view.push(
            ViewNode::rectangle(
                format!("line-{index}"),
                Vec2::new(from.distance(to).max(0.01), 0.05),
            )
            .at_xy((from + to) / 2.0)
            .with_rotation_xy((to - from).to_angle())
            .with_color(Color::srgb(0.35, 0.4, 0.55)),
        );
    }
    for (index, star) in constellation.stars.iter().enumerate() {
        let snapshot = constellation.clone();
        view.push(
            ViewNode::circle(format!("star-{index}"), STAR_DIAMETER)
                .at_xy(star.position)
                .with_z(0.1)
                .with_color(star.color)
                .draggable(move || star_spec(&snapshot, index)),
        );
    }
    view
}

fn initial() -> Constellation {
    let star = |color, x, y| Star {
        color,
        position: Vec2::new(x, y),
    };
    Constellation {
        stars: vec![
            star(Color::srgb(1.0, 0.35, 0.2), -3.0, 2.0),
            star(Color::srgb(0.3, 0.7, 1.0), 0.0, 3.0),
            star(Color::srgb(0.3, 1.0, 0.5), 3.0, 1.5),
            star(Color::srgb(1.0, 0.85, 0.2), -1.5, -1.0),
            star(Color::srgb(0.9, 0.4, 1.0), 2.0, -2.0),
            star(Color::srgb(1.0, 0.6, 0.3), -3.5, -0.5),
        ],
        lines: vec![
            (0, 1),
            (1, 2),
            (0, 3),
            (3, 4),
            (4, 2),
            (3, 5),
            (5, 0),
            (1, 3),
        ],
    }
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let constellation = initial();
    let from = constellation.stars[3].position;
    app.add_plugins(common::default_plugins("constellation"))
        .add_plugins(DraggablePlugin::new(constellation, view))
        .add_plugins(ScriptedDragPlugin::straight(from, Vec2::new(0.5, 0.5)))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_constellation.gif")
                .with_title("constellation")
                .with_description("substate: a star focuses on its own position")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 8.0);
        });
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, Frame, Pointer};

    use super::*;

    fn drag(index: usize, pointer: Vec2) -> Frame<Constellation> {
        let constellation = initial();
        star_spec(&constellation, index).compile(&DragContext::new(
            view,
            format!("star-{index}").into(),
            constellation,
        ))(Pointer::from(pointer))
    }

    #[test]
    fn dragging_a_star_changes_only_its_position() {
        let before = initial();
        let target = Vec2::new(0.5, 0.5);
        let frame = drag(3, target);
        let after = frame.drop_state;
        assert!(after.stars[3].position.distance(target) < 0.05);
        assert_eq!(after.stars[3].color, before.stars[3].color);
        assert_eq!(after.lines, before.lines);
        for other in [0, 1, 2, 4, 5] {
            assert_eq!(after.stars[other], before.stars[other]);
        }
    }

    #[test]
    fn the_scripted_drag_presses_the_star_it_moves() {
        let at = initial().stars[3].position;
        let press = Ray3d::new(at.extend(10.0), Dir3::NEG_Z);
        assert_eq!(view(&initial()).hit_test(press).unwrap().id, "star-3");
    }

    #[test]
    fn the_lines_stretch_to_the_dragged_star() {
        let frame = drag(3, Vec2::new(0.5, 0.5));
        let star = frame.view.world_position("star-3").unwrap().truncate();
        // Line 2 joins star 0 and star 3; its middle sits halfway between them.
        let middle = frame.view.world_position("line-2").unwrap().truncate();
        let other = initial().stars[0].position;
        assert!(middle.distance((star + other) / 2.0) < 0.05);
        let length = frame.view.get("line-2").unwrap().transform.scale.x;
        assert!((length - star.distance(other)).abs() < 0.05);
    }

    #[test]
    fn a_line_leans_toward_the_star_it_joins() {
        let frame = drag(3, Vec2::new(0.5, 0.5));
        let line = frame.view.get("line-2").unwrap();
        let along = line.transform.rotation * Vec3::X;
        let toward = (frame.drop_state.stars[3].position - initial().stars[0].position)
            .normalize()
            .extend(0.0);
        assert!(along.dot(toward).abs() > 0.99);
    }

    #[test]
    fn the_focus_is_marked_and_every_star_stays_draggable() {
        let frame = drag(3, Vec2::new(0.5, 0.5));
        let ring = frame.view.world_position("overlay-focus").unwrap();
        assert!(ring.truncate().distance(frame.drop_state.stars[3].position) < 0.05);
        for index in 0..initial().stars.len() {
            let star = frame.view.get(&format!("star-{index}")).unwrap();
            assert!(star.drag.is_some(), "star-{index}");
        }
    }

    #[test]
    fn a_star_cannot_be_dragged_out_of_the_sky() {
        let frame = drag(3, Vec2::new(20.0, 20.0));
        let at = frame.drop_state.stars[3].position;
        assert!(at.x <= SKY.x + 0.05 && at.y <= SKY.y + 0.05, "{at}");
    }
}
