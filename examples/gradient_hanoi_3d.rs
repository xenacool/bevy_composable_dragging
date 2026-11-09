//! The Towers of Hanoi with gradient 3D shapes from `bevy_gradmesh`.
//!
//! Only the top disk of a peg can be dragged. It floats under the pointer while the preview
//! shows the nearest legal peg, and settles into place when it gets close.
//!
//! Every disk carries its own gradient `Style`, which changes with the peg it rests on. While a
//! disk is dragged, the engine blends the styles of the previewed pegs with `Style::lerp` from
//! `bevy_gradmesh` 0.2.0, so the gradient shifts as the disk travels.
//!
//! Run with `cargo run --example gradient_hanoi_3d --features shapes-3d`.

use bevy::prelude::*;
use bevy_composable_dragging::ops::closest_states;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};
use bevy_gradmesh::Style;

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const DISKS: usize = 4;
const PEG_SPACING: f32 = 3.0;
const DISK_HEIGHT: f32 = 0.4;
/// The preview settles into a peg once the disk is this close to it.
const SNAP: f32 = 0.35;

/// The disks on each peg from bottom to top; a disk is identified by its size.
#[derive(Clone)]
struct Hanoi {
    pegs: [Vec<usize>; 3],
}

fn peg_x(peg: usize) -> f32 {
    (peg as f32 - 1.0) * PEG_SPACING
}

fn disk_position(peg: usize, level: usize) -> Vec2 {
    Vec2::new(
        peg_x(peg),
        0.2 + (level as f32 + 0.5) * (DISK_HEIGHT + 0.05),
    )
}

fn disk_diameter(disk: usize) -> f32 {
    1.0 + 0.55 * disk as f32
}

/// The states reachable by moving the top disk of `peg` to any peg it may rest on, including
/// leaving it where it is.
fn moves(hanoi: &Hanoi, peg: usize) -> Vec<Hanoi> {
    let disk = *hanoi.pegs[peg].last().expect("a peg with a draggable disk");
    let mut options = vec![hanoi.clone()];
    for target in (0..3).filter(|&target| target != peg) {
        if hanoi.pegs[target].last().is_none_or(|&top| top > disk) {
            let mut moved = hanoi.clone();
            moved.pegs[peg].pop();
            moved.pegs[target].push(disk);
            options.push(moved);
        }
    }
    options
}

/// A unit cuboid, scaled by the caller.
fn cuboid(id: String) -> ViewNode<Hanoi> {
    ViewNode::cuboid(id, Vec3::ONE, 0.08)
}

/// A unit cylinder, scaled by the caller.
fn cylinder(id: String) -> ViewNode<Hanoi> {
    ViewNode::cylinder(id, 0.5, 1.0)
}

/// The gradient of a disk: a dark shade of its hue at the bottom and a bright color at the top
/// whose hue depends on the peg, so the two ends of the gradient change independently.
fn disk_style(disk: usize, peg: usize) -> Style {
    let hue = disk as f32 * 360.0 / DISKS as f32;
    Style::dual_directional_up(
        Color::hsl(hue, 0.8, 0.22),
        Color::hsl(hue + peg as f32 * 50.0, 0.85, 0.6),
    )
}

fn view(hanoi: &Hanoi) -> View<Hanoi> {
    let mut view = View::new();
    let mut base = cuboid("base".into())
        .at_xy(Vec2::new(0.0, 0.05))
        .with_color(Color::srgb(0.45, 0.4, 0.38));
    base.transform.scale = Vec3::new(10.0, 0.3, 2.2);
    view.push(base);
    for peg in 0..3 {
        let mut post = cylinder(format!("peg-{peg}"))
            .at_xy(Vec2::new(peg_x(peg), 1.35))
            .with_color(Color::srgb(0.7, 0.68, 0.65));
        post.transform.scale = Vec3::new(0.18, 2.6, 0.18);
        view.push(post);
    }
    for (peg, disks) in hanoi.pegs.iter().enumerate() {
        for (level, &disk) in disks.iter().enumerate() {
            let hue = disk as f32 * 360.0 / DISKS as f32;
            let mut shape = cylinder(format!("disk-{disk}"))
                .at_xy(disk_position(peg, level))
                .with_color(Color::hsl(hue, 0.7, 0.55))
                .with_style(disk_style(disk, peg));
            let diameter = disk_diameter(disk);
            shape.transform.scale = Vec3::new(diameter, DISK_HEIGHT, diameter);
            if level + 1 == disks.len() {
                let snapshot = hanoi.clone();
                shape = shape.draggable(move || {
                    closest_states(moves(&snapshot, peg))
                        .with_floating()
                        .with_snap_radius(SNAP)
                });
            }
            view.push(shape);
        }
    }
    view
}


/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app
let hanoi = Hanoi {
        pegs: [(0..DISKS).rev().collect(), Vec::new(), Vec::new()],
    };
    App::new()
        .add_plugins(common::default_plugins("gradient_hanoi_3d"))
        .add_plugins(DraggablePlugin::new(hanoi, view))
        .add_plugins(ScriptedDragPlugin::straight(
            disk_position(0, DISKS - 1),
            disk_position(1, 0) + Vec2::new(0.7, 0.8),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_gradient_hanoi_3d.webp")
                .with_title("gradient_hanoi_3d")
                .with_description("shapes-3d")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_perspective_scene(
                &mut commands,
                Vec3::new(0.0, 3.2, 9.5),
                Vec3::new(0.0, 1.3, 0.0),
            );
        })
        ;
}

fn main() {
    let hanoi = Hanoi {
        pegs: [(0..DISKS).rev().collect(), Vec::new(), Vec::new()],
    };
    App::new()
        .add_plugins(common::default_plugins("gradient_hanoi_3d"))
        .add_plugins(DraggablePlugin::new(hanoi, view))
        .add_plugins(ScriptedDragPlugin::straight(
            disk_position(0, DISKS - 1),
            disk_position(1, 0) + Vec2::new(0.7, 0.8),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_gradient_hanoi_3d.webp")
                .with_title("gradient_hanoi_3d")
                .with_description("shapes-3d")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_perspective_scene(
                &mut commands,
                Vec3::new(0.0, 3.2, 9.5),
                Vec3::new(0.0, 1.3, 0.0),
            );
        })
        .run();
}
