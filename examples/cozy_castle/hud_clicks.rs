//! End-to-end tests of the HUD: a virtual mouse moves over the buttons of the radial menu and
//! clicks them in the real castle app, so everything between the pointer and the castle state
//! runs: picking, the HUD camera, the drag engine, the tweens and the view that follows.
//!
//! The app is the one of `main`, with the default plugins but no window system and no GPU.

use bevy::prelude::*;

use bevy_composable_dragging::radial::{Petal, RadialHost, Ring as _, petal_id};

use super::common::SceneCamera;
use super::common::mouse::Mouse;
use super::menu::{Ring, Tool};
use super::select::{piece_gizmo_size, wall_gizmo_size};
use super::{Castle, DragModel, PieceKind, castle, view};

/// The castle app and the virtual mouse that drives it.
type Harness = Mouse<Castle, SceneCamera>;

/// A castle app, settled and ready to be clicked.
fn harness() -> Harness {
    Mouse::new(|app| {
        castle(app);
    })
}

/// How a button is clicked.
type Click = fn(&mut Harness, &str);

#[test]
fn clicking_the_resting_hub_opens_the_root_ring() {
    let mut harness = harness();
    assert_eq!(harness.state().menu, None);
    harness.click("menu/hub");
    assert_eq!(harness.state().menu, Some(Ring::Root));
}

#[test]
fn the_pointer_hovers_the_resting_hub_when_it_is_over_it() {
    let mut harness = harness();
    let at = harness.position_of("menu/hub");
    harness.move_to(at);
    harness.move_to(at + Vec2::splat(1.0));
    let hovered = harness.hovered();
    assert!(
        hovered
            .first()
            .is_some_and(|name| name.starts_with("menu/hub")),
        "{hovered:?}"
    );
}

#[test]
fn clicking_the_resting_hub_with_a_little_jitter_opens_the_root_ring() {
    let mut harness = harness();
    let at = harness.position_of("menu/hub");
    harness.move_to(at);
    harness.press();
    harness.move_to(at + Vec2::splat(1.0));
    harness.release();
    harness.settle();
    assert_eq!(harness.state().menu, Some(Ring::Root));
}

/// What the menu changes, to compare states that have no equality of their own.
fn choices(castle: &Castle) -> String {
    format!(
        "{:?} {:?} {:?} {:?} {:?} {:?}",
        castle.menu,
        castle.tool,
        castle.armed,
        castle.precision,
        castle.wall_style,
        castle.selected,
    )
}

/// The castles to open the menu in: nothing selected, a piece selected and a wall selected.
fn starts() -> Vec<(&'static str, Castle)> {
    let castle = Castle::courtyard();
    let piece = castle.placed[0].id;
    let wall = castle.walls[0].id;
    vec![
        ("ground", castle.clone()),
        ("piece", castle.with_selected(Some(piece))),
        ("wall", castle.with_selected(Some(wall))),
    ]
}

#[test]
fn every_petal_of_every_ring_is_under_the_pointer_at_its_center() {
    let mut harness = harness();
    let mut misses = Vec::new();
    for (name, start) in starts() {
        for ring in Ring::all(&start) {
            harness.show(start.with_menu(Some(ring)));
            for item in ring.items(&start) {
                let id = petal_id::<Castle>(&ring, &item);
                let at = harness.position_of(&id);
                harness.move_to(at + Vec2::splat(1.0));
                harness.move_to(at);
                let hovered = harness.hovered();
                if !hovered.first().is_some_and(|hit| hit.starts_with(&id)) {
                    misses.push(format!("{name} {id}: {hovered:?}"));
                }
            }
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}

fn every_petal_does_what_it_says(click: Click) {
    let mut harness = harness();
    let mut wrong = Vec::new();
    for (name, start) in starts() {
        for ring in Ring::all(&start) {
            let open = start.with_menu(Some(ring));
            for item in ring.items(&start) {
                harness.show(open.clone());
                let id = petal_id::<Castle>(&ring, &item);
                click(&mut harness, &id);
                let (got, expected) = (choices(harness.state()), choices(&item.press(&open)));
                if got != expected {
                    wrong.push(format!("{name} {id}: got {got}, expected {expected}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn clicking_a_petal_does_what_the_petal_says() {
    every_petal_does_what_it_says(Harness::click);
}

#[test]
fn clicking_a_petal_with_a_little_jitter_does_what_the_petal_says() {
    every_petal_does_what_it_says(Harness::click_jittering);
}

#[test]
fn the_menu_leads_from_the_hub_to_an_armed_piece() {
    let mut harness = harness();
    for id in [
        "menu/hub",
        "menu/root/pieces",
        "menu/pieces/Towers",
        "menu/Towers/Tower",
    ] {
        harness.click(id);
    }
    assert_eq!(harness.state().armed, Some(PieceKind::Tower));
    assert_eq!(harness.state().menu, None);
}

#[test]
fn the_menu_leads_from_the_hub_to_a_tool_of_the_selected_piece() {
    let mut harness = harness();
    let piece = Castle::courtyard().placed[0].id;
    harness.show(Castle::courtyard().with_selected(Some(piece)));
    for id in ["menu/pop", "menu/root/tools", "menu/tools/Rotate"] {
        harness.click(id);
    }
    assert_eq!(harness.state().tool, Tool::Rotate);
    assert_eq!(harness.state().selected, Some(piece));
}

fn the_hud_settles_where_a_fresh_view_puts_it(click: Click) {
    let mut harness = harness();
    let mut out_of_place = Vec::new();
    for (name, start) in starts() {
        for ring in Ring::all(&start) {
            let open = start.with_menu(Some(ring));
            for item in ring.items(&start) {
                harness.show(open.clone());
                let id = petal_id::<Castle>(&ring, &item);
                click(&mut harness, &id);
                let model = harness.app.world().resource::<DragModel<Castle>>();
                let fresh = view(model.state());
                for node in fresh
                    .nodes()
                    .iter()
                    .filter(|node| node.id.starts_with("menu/"))
                {
                    let shown = model.view().world_position(&node.id);
                    let wanted = fresh.world_position(&node.id);
                    if shown.zip(wanted).is_none_or(|(a, b)| a.distance(b) > 0.5) {
                        out_of_place.push(format!(
                            "{name} after {id}: {} is {shown:?}, not {wanted:?}",
                            node.id
                        ));
                    }
                }
            }
        }
    }
    assert!(out_of_place.is_empty(), "{out_of_place:#?}");
}

#[test]
fn a_click_leaves_the_hud_where_a_fresh_view_puts_it() {
    the_hud_settles_where_a_fresh_view_puts_it(Harness::click);
}

#[test]
fn a_click_with_a_little_jitter_leaves_the_hud_where_a_fresh_view_puts_it() {
    the_hud_settles_where_a_fresh_view_puts_it(Harness::click_jittering);
}

#[test]
fn clicking_the_ground_does_not_move_the_menu_or_the_pieces() {
    let mut harness = harness();
    harness.click("menu/hub");
    let before = choices(harness.state());
    let pieces = harness.state().placed.len();
    harness.move_to(Vec2::new(1000.0, 650.0));
    harness.press();
    harness.release();
    harness.settle();
    assert_eq!(harness.state().placed.len(), pieces);
    assert_eq!(harness.state().menu, Some(Ring::Root), "{before}");
}

/// The HUD nodes and where they are.
fn hud_positions(harness: &Harness) -> Vec<(String, Vec3)> {
    let model = harness.app.world().resource::<DragModel<Castle>>();
    model
        .view()
        .nodes()
        .iter()
        .filter(|node| node.id.starts_with("menu/"))
        .map(|node| {
            let at = model.view().world_position(&node.id);
            (node.id.to_string(), at.unwrap_or(Vec3::NAN))
        })
        .collect()
}

#[test]
fn holding_a_button_leaves_the_rest_of_the_hud_where_it_was() {
    let mut harness = harness();
    let mut moved = Vec::new();
    for (name, start) in starts() {
        let open = start.with_menu(Some(Ring::Root));
        for item in Ring::Root.items(&open) {
            harness.show(open.clone());
            let id = petal_id::<Castle>(&Ring::Root, &item);
            let before = hud_positions(&harness);
            let at = harness.position_of(&id);
            harness.move_to(at);
            harness.press();
            harness.move_to(at + Vec2::X);
            harness.settle();
            let held = hud_positions(&harness);
            harness.release();
            harness.settle();
            for (was, is) in before.iter().zip(&held) {
                if was.0 != is.0 || was.1.distance(is.1) > 0.5 {
                    moved.push(format!(
                        "{name} holding {id}: {} {:?} -> {:?}",
                        was.0, was.1, is.1
                    ));
                }
            }
        }
    }
    assert!(moved.is_empty(), "{moved:#?}");
}

#[test]
fn dragging_a_button_away_cancels_it() {
    let mut harness = harness();
    let open = Castle::courtyard().with_menu(Some(Ring::Root));
    harness.show(open.clone());
    let at = harness.position_of("menu/root/pieces");
    harness.move_to(at);
    harness.press();
    harness.move_to(at + Vec2::new(300.0, 0.0));
    harness.release();
    harness.settle();
    assert_eq!(choices(harness.state()), choices(&open));
}

#[test]
fn the_hub_is_under_the_pointer_at_its_center_even_with_a_ring_open() {
    let mut harness = harness();
    harness.show(Castle::courtyard().with_menu(Some(Ring::Root)));
    let at = harness.position_of("menu/hub");
    harness.move_to(at + Vec2::splat(1.0));
    harness.move_to(at);
    let hovered = harness.hovered();
    assert!(
        hovered
            .first()
            .is_some_and(|hit| hit.starts_with("menu/hub")),
        "{hovered:?}"
    );
}

#[test]
fn every_petal_is_on_the_window_and_under_the_pointer_in_every_window_size() {
    let mut harness = harness();
    let mut misses = Vec::new();
    for (width, height) in [
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (800.0, 600.0),
        (600.0, 900.0),
        (480.0, 320.0),
    ] {
        harness.resize(width, height);
        for (name, start) in starts() {
            let current = harness.state().clone();
            for ring in Ring::all(&start) {
                harness.show(current.with_selected(start.selected).with_menu(Some(ring)));
                for item in ring.items(harness.state()) {
                    let id = petal_id::<Castle>(&ring, &item);
                    let at = harness.position_of(&id);
                    let inside = at.x > 0.0 && at.y > 0.0 && at.x < width && at.y < height;
                    harness.move_to(at + Vec2::splat(1.0));
                    harness.move_to(at);
                    let hovered = harness.hovered();
                    let hit = hovered.first().is_some_and(|hit| hit.starts_with(&id));
                    if !inside || !hit {
                        misses.push(format!(
                            "{width}x{height} {name} {id} at {at}: inside {inside}, {hovered:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}

/// What the castle holds, to see whether something changed it.
fn contents(castle: &Castle) -> String {
    format!("{:?} {:?}", castle.placed, castle.walls)
}

/// What a gizmo is drawn around: a piece or wall of the castle, and how big its handles are.
struct Subject {
    name: String,
    castle: Castle,
    id: u32,
    size: f32,
    tools: Vec<Tool>,
}

/// `castle` with only the piece or wall `id` standing on its terrain, selected. A neighbour in
/// front of a handle takes the pointer first, which is not what the handles are tested for.
fn alone(castle: &Castle, id: u32) -> Castle {
    let mut castle = castle.clone();
    castle.placed.retain(|piece| piece.id == id);
    castle.walls.retain(|piece| piece.id == id);
    castle.with_selected(Some(id))
}

/// A piece of every kind the courtyard castle has, and its first wall.
fn subjects() -> Vec<Subject> {
    let courtyard = Castle::courtyard();
    let mut kinds = Vec::new();
    let mut subjects = Vec::new();
    for piece in &courtyard.placed {
        if kinds.contains(&piece.kind) {
            continue;
        }
        kinds.push(piece.kind);
        subjects.push(Subject {
            name: format!("{:?}", piece.kind),
            castle: alone(&courtyard, piece.id),
            id: piece.id,
            size: piece_gizmo_size(piece.kind),
            tools: vec![Tool::Move, Tool::Rotate, Tool::Raise],
        });
    }
    let wall = &courtyard.walls[0];
    subjects.push(Subject {
        name: "wall".into(),
        castle: alone(&courtyard, wall.id),
        id: wall.id,
        size: wall_gizmo_size(wall.wall.scaled_profile().top()),
        tools: vec![Tool::Move, Tool::Rotate, Tool::Raise, Tool::Scale],
    });
    subjects
}

/// The world points to try to grab a handle at, which is placed at `at`: the pointer has to be on
/// its mesh, which for an arrow is along its shaft, which is centered on `at`, and for a ring is on
/// the circle around `at` and not at its center.
fn grab_points(handle: &str, turn: Quat, size: f32, at: Vec3) -> Vec<Vec3> {
    let axis = |letter: char| {
        turn * match letter {
            'x' => Vec3::X,
            'y' => Vec3::Y,
            _ => Vec3::Z,
        }
    };
    let name = handle.rsplit('/').next().expect("a handle name");
    let (kind, letters) = name.split_once('-').expect("a handle name");
    match (kind, letters) {
        ("translate", letter) if letter.len() == 1 => {
            let along = axis(letter.chars().next().unwrap());
            [0.0, 0.2, -0.2, 0.4, -0.4, 0.6, 0.8]
                .iter()
                .map(|part| at + along * size * part)
                .collect()
        }
        ("rotate", letter) => {
            let [a, b] = match letter {
                "x" => ['y', 'z'],
                "y" => ['x', 'z'],
                _ => ['x', 'y'],
            };
            (0..8)
                .map(|step| {
                    let angle = step as f32 * std::f32::consts::FRAC_PI_4 + 0.3;
                    at + (axis(a) * angle.cos() + axis(b) * angle.sin()) * 0.9 * size
                })
                .collect()
        }
        ("translate", _) => {
            let [a, b] = [
                letters.chars().next().unwrap(),
                letters.chars().nth(1).unwrap(),
            ];
            let offset = (axis(a) + axis(b)) * 0.1 * size;
            vec![
                at,
                at + offset,
                at - offset,
                at + (axis(a) - axis(b)) * 0.1 * size,
                at - (axis(a) - axis(b)) * 0.1 * size,
            ]
        }
        _ => vec![
            at,
            at + Vec3::Y * 0.1 * size,
            at + turn * Vec3::X * 0.1 * size,
        ],
    }
}

/// The handles of the gizmo of `id` in the view, without the decorations on them.
fn handles(harness: &Harness, id: u32) -> Vec<String> {
    let prefix = format!("{}/gizmo/", super::piece_id(id));
    harness
        .app
        .world()
        .resource::<DragModel<Castle>>()
        .view()
        .nodes()
        .iter()
        .filter(|node| node.id.starts_with(&prefix) && !node.id.ends_with("/tip"))
        .map(|node| node.id.to_string())
        .collect()
}

#[test]
fn every_handle_of_every_gizmo_can_be_grabbed_and_dragged_in_the_castle() {
    let mut harness = harness();
    let mut broken = Vec::new();
    let mut tried = 0;
    for subject in subjects() {
        for tool in &subject.tools {
            let mut castle = subject.castle.clone();
            castle.tool = *tool;
            harness.show(castle.clone());
            let root = super::piece_id(subject.id);
            let view = harness
                .app
                .world()
                .resource::<DragModel<Castle>>()
                .view()
                .clone();
            let turn = view.get(&root).expect("the root").transform.rotation;
            let names = handles(&harness, subject.id);
            if names.is_empty() {
                broken.push(format!("{} {tool:?}: no handles", subject.name));
            }
            for handle in names {
                tried += 1;
                let at = view.world_position(&handle).expect("the handle");
                let mut grabbed = None;
                for world in grab_points(&handle, turn, subject.size, at) {
                    let pixel = harness.scene_point(world);
                    harness.move_to(pixel + Vec2::X);
                    harness.move_to(pixel);
                    if harness
                        .hovered()
                        .first()
                        .is_some_and(|hit| hit.starts_with(&handle))
                    {
                        grabbed = Some(pixel);
                        break;
                    }
                }
                let Some(pixel) = grabbed else {
                    let pixel = harness.scene_point(at);
                    harness.move_to(pixel);
                    broken.push(format!(
                        "{} {tool:?} {handle}: not under the pointer, which is over {:?}",
                        subject.name,
                        harness.hovered().first()
                    ));
                    continue;
                };
                let before = contents(harness.state());
                let mut moved = false;
                for direction in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                    harness.show(castle.clone());
                    harness.move_to(pixel);
                    harness.press();
                    for step in [10.0, 25.0, 45.0] {
                        harness.move_to(pixel + direction * step);
                    }
                    harness.release();
                    harness.settle();
                    if contents(harness.state()) != before {
                        moved = true;
                        break;
                    }
                }
                if !moved {
                    broken.push(format!(
                        "{} {tool:?} {handle}: dragging it changes nothing",
                        subject.name
                    ));
                }
                harness.show(castle.clone());
            }
        }
    }
    assert!(tried > 40, "only {tried} handles were tried");
    assert!(broken.is_empty(), "{broken:#?}");
}
