//! Transform gizmos on a rounded cuboid, with 3D drag spaces.
//!
//! Drag the arrows or the squares of the translate gizmo, the rings of the rotate gizmo, or the
//! cubes of the scale gizmo. Every handle drags in its own axis or plane space and settles onto
//! snap steps: the preview is pulled towards the nearest step as you get close, and releasing
//! commits the exact step. Drag the ring in the top left to switch tool; the handles of the other
//! tools fade out.
//!
//! The ball drags freely in a plane that faces the camera (`DragSpace::screen()`), and the puck
//! slides on a tilted plane (`DragSpace::plane`), both with `across_plane`.
//!
//! Run with `cargo run --example gizmo_3d --features shapes-3d`.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy_composable_dragging::gizmo::Gizmo;
use bevy_composable_dragging::ops::{Snap, across_plane, closest_states};
use bevy_composable_dragging::{DragSpace, DragSpec, DraggablePlugin, View, ViewNode};
use bevy_gradmesh::Style;

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

/// The pivot of the cuboid at the start.
const PIVOT: Vec3 = Vec3::new(0.0, 0.9, 0.0);
/// The size of the cuboid; its scale is a factor on top of it.
const SIZE: Vec3 = Vec3::new(1.0, 0.8, 0.6);
/// The tool selector sits at the top left.
const SELECTOR: Vec3 = Vec3::new(-3.2, 3.3, 0.0);
/// The tilted plane that the puck slides on.
const PLATE_ORIGIN: Vec3 = Vec3::new(3.2, 0.9, 0.0);
const PLATE_TILT: Vec3 = Vec3::new(-0.45, 1.0, 0.45);

/// Which gizmo is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Translate,
    Rotate,
    Scale,
}

impl Tool {
    const ALL: [Self; 3] = [Self::Translate, Self::Rotate, Self::Scale];

    fn slot(self) -> Vec3 {
        let index = match self {
            Self::Translate => 0.0,
            Self::Rotate => 1.0,
            Self::Scale => 2.0,
        };
        SELECTOR + Vec3::new(index * 0.7 - 0.7, 0.0, 0.0)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Translate => "translate",
            Self::Rotate => "rotate",
            Self::Scale => "scale",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Scene {
    tool: Tool,
    /// Position and rotation of the cuboid; the scale lives in `scale`.
    pivot: Transform,
    scale: Vec3,
    ball: Vec3,
    puck: Vec3,
}

impl Scene {
    fn initial() -> Self {
        Self {
            tool: Tool::Translate,
            pivot: Transform::from_translation(PIVOT),
            scale: Vec3::ONE,
            ball: Vec3::new(-2.6, 1.0, 0.3),
            puck: PLATE_ORIGIN,
        }
    }

    fn with_tool(&self, tool: Tool) -> Self {
        Self {
            tool,
            ..self.clone()
        }
    }
}

fn plate_normal() -> Dir3 {
    Dir3::new(PLATE_TILT).expect("the tilt is not zero")
}

fn flat(color: Color) -> Style {
    Style::shaded(color, 0.45)
}

fn selector(scene: &Scene, view: &mut View<Scene>) {
    view.push(
        ViewNode::rectangle("selector", Vec2::new(2.3, 0.9))
            .at(SELECTOR + Vec3::new(0.0, 0.0, -0.1))
            .with_color(Color::srgb(0.1, 0.12, 0.17)),
    );
    for tool in Tool::ALL {
        let icon = format!("icon-{}", tool.name());
        let at = tool.slot() + Vec3::new(0.0, 0.0, 0.1);
        view.push(
            match tool {
                Tool::Translate => ViewNode::cone(icon, 0.1, 0.3)
                    .with_rotation(Quat::from_rotation_z(-FRAC_PI_2))
                    .with_style(flat(Color::srgb(0.92, 0.3, 0.3))),
                Tool::Rotate => ViewNode::torus(icon, 0.14, 0.035)
                    .with_rotation(Quat::from_rotation_x(FRAC_PI_2))
                    .with_style(flat(Color::srgb(0.35, 0.8, 0.4))),
                Tool::Scale => ViewNode::cuboid(icon, Vec3::splat(0.2), 0.3)
                    .with_style(flat(Color::srgb(0.4, 0.55, 0.96))),
            }
            .at(at),
        );
    }
    let tools = Tool::ALL.map(|tool| scene.with_tool(tool));
    view.push(
        ViewNode::ring("tool-ring", 0.3, 0.24)
            .at(scene.tool.slot())
            .with_style(Style::shaded(Color::WHITE, 1.0))
            .draggable(move || {
                closest_states(tools.clone())
                    .with_floating()
                    .with_snap_radius(0.2)
            }),
    );
}

fn cuboid_with_gizmo(scene: &Scene, view: &mut View<Scene>) {
    view.push(
        ViewNode::sphere("cube", 0.05)
            .with_transform(scene.pivot)
            .with_color(Color::WHITE),
    );
    view.push(
        ViewNode::cuboid("cube/body", SIZE, 0.2)
            .with_parent("cube")
            .scaled(scene.scale)
            .with_style(Style::shaded(Color::srgb(0.9, 0.72, 0.5), 0.45)),
    );
    let gizmo = Gizmo::new("cube", scene.pivot);
    match scene.tool {
        Tool::Translate => gizmo.translate(Snap::step(0.5), |scene: &Scene, offset| Scene {
            pivot: Transform {
                translation: scene.pivot.translation + offset,
                ..scene.pivot
            },
            ..scene.clone()
        }),
        Tool::Rotate => gizmo.rotate(
            Snap::step(FRAC_PI_2 / 3.0).within(0.12),
            |scene: &Scene, rotation| Scene {
                pivot: Transform {
                    rotation: rotation * scene.pivot.rotation,
                    ..scene.pivot
                },
                ..scene.clone()
            },
        ),
        Tool::Scale => gizmo.scale(Snap::step(0.25), |scene: &Scene, factor| Scene {
            scale: (scene.scale * factor).max(Vec3::splat(0.25)),
            ..scene.clone()
        }),
    }
    .add_to(view);
}

fn ball_and_puck(scene: &Scene, view: &mut View<Scene>) {
    view.push(
        ViewNode::sphere("ball", 0.4)
            .at(scene.ball)
            .with_style(Style::shaded(Color::srgb(0.95, 0.45, 0.6), 0.4))
            .draggable_in(DragSpace::screen(), || {
                DragSpec::from(across_plane(|scene: &Scene, offset| Scene {
                    ball: scene.ball + offset,
                    ..scene.clone()
                }))
            }),
    );
    let normal = plate_normal();
    view.push(
        ViewNode::disc("plate", 1.5)
            .at(PLATE_ORIGIN)
            .with_rotation(Quat::from_rotation_arc(Vec3::Z, *normal))
            .with_style(Style::shaded(Color::srgb(0.25, 0.3, 0.4), 1.0))
            .casts_shadow(false),
    );
    let space = DragSpace::plane(PLATE_ORIGIN, normal);
    view.push(
        ViewNode::cylinder("puck", 0.25, 0.14)
            .at(scene.puck + *normal * 0.08)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, *normal))
            .with_style(Style::shaded(Color::srgb(0.98, 0.8, 0.3), 0.4))
            .draggable_in(space, || {
                DragSpec::from(
                    across_plane(|scene: &Scene, offset| Scene {
                        puck: scene.puck + offset,
                        ..scene.clone()
                    })
                    .snap(Snap::step(0.25)),
                )
            }),
    );
}

fn view(scene: &Scene) -> View<Scene> {
    let mut view = View::new();
    view.push(
        ViewNode::rectangle("floor", Vec2::new(16.0, 9.0))
            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
            .with_color(Color::srgb(0.17, 0.19, 0.24)),
    );
    selector(scene, &mut view);
    cuboid_with_gizmo(scene, &mut view);
    ball_and_puck(scene, &mut view);
    view
}

fn main() {
    let scene = Scene::initial();
    App::new()
        .add_plugins(common::default_plugins("gizmo_3d"))
        .add_plugins(DraggablePlugin::new(scene, view))
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(PIVOT.x + 0.85, PIVOT.y),
            Vec2::new(PIVOT.x + 2.3, PIVOT.y),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_gizmo_3d.webp")
                .with_title("gizmo_3d")
                .with_description("transform gizmos in 3D drag spaces")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_perspective_scene(
                &mut commands,
                Vec3::new(0.0, 3.4, 9.5),
                Vec3::new(0.0, 1.2, 0.0),
            );
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_slots_are_distinct_and_centered_on_the_selector() {
        let slots = Tool::ALL.map(Tool::slot);
        assert!((slots[1] - SELECTOR).length() < 1e-6);
        assert!(slots[0].x < slots[1].x && slots[1].x < slots[2].x);
    }

    #[test]
    fn each_tool_shows_only_its_own_handles() {
        for (tool, kind) in [
            (Tool::Translate, "translate-"),
            (Tool::Rotate, "rotate-"),
            (Tool::Scale, "scale-"),
        ] {
            let view = view(&Scene::initial().with_tool(tool));
            let handles: Vec<_> = view
                .nodes()
                .iter()
                .filter(|node| node.id.contains("/gizmo/"))
                .collect();
            assert!(!handles.is_empty());
            assert!(
                handles.iter().all(|node| node.id.contains(kind)),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn the_scene_has_a_screen_space_ball_and_a_puck_on_the_tilted_plate() {
        let view = view(&Scene::initial());
        assert_eq!(
            view.get("ball").unwrap().drag_space,
            Some(DragSpace::screen())
        );
        assert_eq!(
            view.get("puck").unwrap().drag_space,
            Some(DragSpace::plane(PLATE_ORIGIN, plate_normal()))
        );
    }
}
