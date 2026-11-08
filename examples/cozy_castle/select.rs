//! Selection, and the gizmo that moves, turns and scales the selected piece.
//!
//! Selection is state, `Castle::selected`. Pressing a wall and releasing it selects it through a
//! fixed drop state, so it needs no click event and works for a drag of any length, including
//! none. The gizmo is the library [`Gizmo`], added to the view for the selected piece only. Its
//! handles edit the state through the methods here: they never scale a node, because the handles
//! are children of the node and a scaled node would stretch them.

use bevy::prelude::*;
use bevy_composable_dragging::DragSpec;
use bevy_composable_dragging::gizmo::{Gizmo, GizmoReveal};
use bevy_composable_dragging::ops::state;

use crate::menu::{Precision, Tool};
use crate::{Castle, PieceKind, Pose, piece_id};

/// How much bigger than the reach of a piece its gizmo is.
const GIZMO_PER_REACH: f32 = 3.0;
/// How much bigger than the height of a wall its gizmo is.
const GIZMO_PER_HEIGHT: f32 = 2.5;
/// The smallest gizmo of a piece, and of a wall.
const MIN_GIZMO: f32 = 0.8;
const MIN_WALL_GIZMO: f32 = 1.0;
/// The biggest gizmo.
const MAX_GIZMO: f32 = 2.2;

impl Castle {
    /// The castle with `selected` selected.
    pub fn with_selected(&self, selected: Option<u32>) -> Self {
        let mut castle = self.clone();
        castle.selected = selected;
        castle
    }

    /// The castle with piece or wall `id` moved by the world `offset`. A piece stays on what it
    /// rests on, and the vertical part of the offset lifts it above that, never below.
    pub fn moved(&self, id: u32, offset: Vec3) -> Self {
        let Some(piece) = self.piece(id) else {
            return self.with_wall_edited(id, |wall| wall.moved(offset));
        };
        let pose = piece.pose();
        let lift = (piece.lift + offset.y).max(0.0);
        let mut castle = self.clone();
        if let Some(piece) = castle.placed.iter_mut().find(|piece| piece.id == id) {
            piece.lift = lift;
        }
        castle.posed(
            id,
            Pose {
                position: pose.position + Vec2::new(offset.x, offset.z),
                ..pose
            },
        )
    }

    /// The castle with piece or wall `id` turned by `rotation`. A piece only turns about the
    /// vertical axis, so only that part of the rotation counts; a wall turns freely.
    pub fn turned(&self, id: u32, rotation: Quat) -> Self {
        let Some(piece) = self.piece(id) else {
            return self.with_wall_edited(id, |wall| wall.turned(rotation));
        };
        let pose = piece.pose();
        let twist = 2.0 * rotation.y.atan2(rotation.w);
        self.posed(
            id,
            Pose {
                angle: pose.angle - twist,
                ..pose
            },
        )
    }

    /// The castle with wall `id` scaled by `factor` on each of its axes.
    pub fn scaled(&self, id: u32, factor: Vec3) -> Self {
        self.with_wall_edited(id, |wall| wall.scaled(factor))
    }
}

impl GizmoReveal for Castle {
    /// Only the tools that transform the selection show its gizmo.
    fn reveals_gizmo(&self) -> bool {
        self.tool.reveals_gizmo()
    }
}

/// The spec of a press on a wall: whatever the pointer does, releasing selects the wall.
pub fn select_spec(castle: &Castle, id: u32) -> DragSpec<Castle> {
    state(castle.with_selected(Some(id)))
}

/// How big the gizmo of a piece of `kind` is: its arrows and squares have to reach out of the
/// body, or the pointer would grab the piece instead of the handle.
pub fn piece_gizmo_size(kind: PieceKind) -> f32 {
    let reach = kind
        .footprint()
        .iter()
        .map(|corner| corner.length())
        .fold(0.0, f32::max);
    (reach * GIZMO_PER_REACH).clamp(MIN_GIZMO, MAX_GIZMO)
}

/// How big the gizmo of a wall whose cross-section is `height` tall is.
pub fn wall_gizmo_size(height: f32) -> f32 {
    (height * GIZMO_PER_HEIGHT).clamp(MIN_WALL_GIZMO, MAX_GIZMO)
}

/// The gizmo of piece or wall `id`, whose root node is at `transform`, for `tool`. It has the
/// handles of that tool alone, in handles of the given `size` and in the steps of `precision`:
/// handles of other tools would only be in the way of the pointer, and a ring seen edge-on
/// covers the arrows. Handles the scene camera sees edge-on or end-on are left out as well. The
/// gizmo is centered on the body, which is `body` tall above `transform`. Tools that do not
/// transform the selection have no gizmo.
pub fn selection_gizmo(
    id: u32,
    transform: Transform,
    precision: Precision,
    size: f32,
    body: f32,
    tool: Tool,
) -> Gizmo<Castle> {
    let gizmo = Gizmo::new(piece_id(id), transform)
        .with_size(size)
        .raised(body / 2.0)
        .seen_from(crate::scene_camera().translation);
    match tool {
        Tool::Move => gizmo.translate(precision.snap(), move |castle: &Castle, offset| {
            castle.moved(id, offset)
        }),
        Tool::Rotate => gizmo.rotate(precision.turn(), move |castle: &Castle, rotation| {
            castle.turned(id, rotation)
        }),
        Tool::Raise => gizmo.lift(precision.snap(), move |castle: &Castle, height| {
            castle.moved(id, Vec3::Y * height)
        }),
        Tool::Scale => gizmo
            .scale(precision.snap(), move |castle: &Castle, factor| {
                castle.scaled(id, factor)
            })
            .scale_uniform(precision.snap(), move |castle: &Castle, factor| {
                castle.scaled(id, Vec3::splat(factor))
            }),
        _ => gizmo,
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;
    use std::sync::Arc;

    use bevy_composable_dragging::{DragContext, DragSpace, Pointer, View};

    use super::*;
    use crate::wall::{Wall, draw};
    use crate::{PieceKind, alignments, move_spec};

    fn tower_castle() -> Castle {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        castle.tool = crate::menu::Tool::Move;
        castle
    }

    fn wall_castle() -> Castle {
        let mut castle = Castle::empty()
            .with_wall(Wall::straight((-1.5, 1.0), (1.5, 1.0)))
            .with_selected(Some(0));
        castle.tool = crate::menu::Tool::Move;
        castle
    }

    fn scaling_castle() -> Castle {
        let mut castle = wall_castle();
        castle.tool = Tool::Scale;
        castle
    }

    #[test]
    fn the_gizmo_stays_hidden_until_a_tool_that_transforms_is_chosen() {
        use crate::menu::Tool;
        let gizmos = |castle: &Castle| {
            crate::view(castle)
                .nodes()
                .iter()
                .filter(|node| node.id.contains("/gizmo/"))
                .count()
        };
        for mut castle in [tower_castle().with_selected(Some(0)), wall_castle()] {
            for tool in [Tool::Move, Tool::Rotate, Tool::Raise] {
                castle.tool = tool;
                assert!(gizmos(&castle) > 0, "{tool:?}");
            }
            for tool in [Tool::Select, Tool::Duplicate, Tool::Delete, Tool::Reshape] {
                castle.tool = tool;
                assert_eq!(gizmos(&castle), 0, "{tool:?}");
            }
        }
        let mut wall = wall_castle();
        wall.tool = Tool::Scale;
        assert!(gizmos(&wall) > 0);
    }

    fn handle(id: u32, name: &str) -> String {
        format!("{}/gizmo/{name}", piece_id(id))
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    /// Drags `node` of `castle` with the spec its node has. The pointer goes from the node to
    /// `to(anchor)`; a camera-facing handle drags in the plane through its pivot facing +Z.
    fn drag(castle: &Castle, node: &str, to: impl Fn(Vec3) -> Vec3) -> (Castle, View<Castle>) {
        let view = crate::view(castle);
        let node_ref = view.get(node).unwrap();
        let spec = (node_ref.drag.as_ref().unwrap())();
        let anchor = view.world_position(node).unwrap();
        let space = match node_ref.drag_space.unwrap() {
            DragSpace::Screen => DragSpace::plane(anchor, Dir3::Z),
            other => other,
        };
        let context = DragContext::new(
            Arc::new(crate::view),
            node.to_owned().into(),
            castle.clone(),
        )
        .with_space(space);
        let frame = spec.compile(&context)(Pointer::at(to(anchor)));
        (frame.drop_state, frame.view)
    }

    #[test]
    fn releasing_a_press_on_a_piece_selects_it_even_without_moving() {
        let castle = tower_castle();
        assert_eq!(castle.selected, None);
        let context = DragContext::new(Arc::new(crate::view), piece_id(0).into(), castle.clone())
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let still = move_spec(&castle, 0).compile(&context)(Pointer::at(Vec3::ZERO));
        assert_eq!(still.drop_state.selected, Some(0));
        let moved = castle.piece(0).unwrap().transform.translation;
        let kept = still.drop_state.piece(0).unwrap().transform.translation;
        assert!(
            near(moved, kept),
            "a press without movement leaves it where it is"
        );
    }

    #[test]
    fn pressing_a_wall_selects_it_and_pressing_the_ground_deselects() {
        let castle = wall_castle().with_selected(None);
        let (selected, _) = drag(&castle, &piece_id(0), |anchor| anchor);
        assert_eq!(selected.selected, Some(0));

        let context = DragContext::new(Arc::new(crate::view), "ground".into(), selected.clone())
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let pressed = draw(&selected).compile(&context)(Pointer::at(Vec3::new(3.0, 0.0, 3.0)));
        assert_eq!(pressed.drop_state.selected, None);
        assert_eq!(pressed.drop_state.walls.len(), 1, "no wall was drawn");
    }

    #[test]
    fn only_the_selected_piece_shows_a_gizmo_with_the_handles_its_kind_needs() {
        let mut castle = tower_castle();
        castle.add(PieceKind::Keep, 3.0, 0.0, 0.0);
        let gizmos = |castle: &Castle| -> Vec<String> {
            crate::view(castle)
                .nodes()
                .iter()
                .filter(|node| node.id.contains("/gizmo/"))
                .map(|node| node.id.to_string())
                .collect()
        };
        assert_eq!(gizmos(&castle), Vec::<String>::new());

        let moving = castle.with_selected(Some(1));
        let view = crate::view(&moving);
        assert!(view.get(&handle(1, "translate-x")).is_some());
        assert!(
            view.get(&handle(1, "rotate-y")).is_none(),
            "only arrows to move"
        );
        assert!(
            view.get(&handle(1, "scale-x")).is_none(),
            "pieces are not scaled"
        );
        assert!(gizmos(&moving).iter().all(|id| id.starts_with("piece-1/")));
        let handle_node = view.get(&handle(1, "translate-x")).unwrap();
        assert_eq!(handle_node.parent.as_deref(), Some("piece-1"));

        let mut turning = moving.clone();
        turning.tool = Tool::Rotate;
        let view = crate::view(&turning);
        assert!(view.get(&handle(1, "rotate-y")).is_some());
        assert!(
            view.get(&handle(1, "translate-x")).is_none(),
            "only rings to turn"
        );

        let mut raising = moving;
        raising.tool = Tool::Raise;
        assert_eq!(
            gizmos(&raising),
            [
                handle(1, "translate-y"),
                format!("{}/tip", handle(1, "translate-y"))
            ]
        );

        for (tool, kinds) in [
            (Tool::Move, &["translate-x", "translate-xz"][..]),
            (Tool::Rotate, &["rotate-y", "rotate-z"]),
            (Tool::Raise, &["translate-y"]),
            (
                Tool::Scale,
                &["scale-x", "scale-y", "scale-z", "scale-uniform"],
            ),
        ] {
            let mut wall = wall_castle();
            wall.tool = tool;
            let walls = crate::view(&wall);
            for kind in kinds {
                let node = walls.get(&handle(0, kind)).unwrap();
                assert_eq!(node.parent.as_deref(), Some("piece-0"), "{tool:?} {kind}");
            }
            let edge_on = ["translate-yz", "rotate-x"];
            for kind in edge_on {
                assert!(
                    walls.get(&handle(0, kind)).is_none(),
                    "{kind} is seen edge-on from the camera"
                );
            }
            let prefix = match tool {
                Tool::Move => "translate-",
                Tool::Rotate => "rotate-",
                Tool::Raise => "translate-y",
                _ => "scale-",
            };
            assert!(
                gizmos(&wall)
                    .iter()
                    .all(|id| id.contains(&format!("/gizmo/{prefix}"))),
                "{tool:?} has only its own handles"
            );
        }
    }

    /// The length of the arm of the gizmo of the first wall.
    fn arm(castle: &Castle) -> f32 {
        wall_gizmo_size(castle.walls[0].wall.scaled_profile().top())
    }

    #[test]
    fn dragging_a_scale_handle_commits_the_snapped_factor_and_the_root_stays_unscaled() {
        let castle = scaling_castle();
        let x_axis = castle.walls[0].wall.pivot.rotation * Vec3::X;
        let reach = arm(&castle);
        let (dropped, preview) = drag(&castle, &handle(0, "scale-x"), |anchor| {
            anchor + x_axis * 1.02 * reach
        });
        let scale = dropped.walls[0].wall.scale;
        assert!(near(scale, Vec3::new(2.0, 1.0, 1.0)), "{scale}");
        let dropped_view = crate::view(&dropped);
        for view in [&preview, &dropped_view] {
            let root = view.get(&piece_id(0)).unwrap();
            assert_eq!(root.transform.scale, Vec3::ONE);
            for node in view
                .nodes()
                .iter()
                .filter(|node| node.id.contains("/gizmo/"))
            {
                assert_eq!(node.transform.scale, Vec3::ONE, "{}", node.id);
            }
        }
        assert!(
            dropped.walls[0]
                .wall
                .shape(&dropped.terrain)
                .bounds()
                .half_extents()
                .x
                > castle.walls[0]
                    .wall
                    .shape(&castle.terrain)
                    .bounds()
                    .half_extents()
                    .x
                    * 1.8
        );
    }

    #[test]
    fn dragging_the_uniform_handle_scales_all_three_axes() {
        let castle = scaling_castle();
        let pivot = castle.walls[0].wall.pivot.translation;
        let name = handle(0, "scale-uniform");
        let reach = arm(&castle);
        let (dropped, _) = drag(&castle, &name, |anchor| {
            // The pointer is on the plane of the drag, which passes through the handle.
            let out = (anchor - pivot).truncate();
            let at = pivot.truncate() + out.normalize() * (out.length() + 0.52 * reach);
            at.extend(anchor.z)
        });
        let scale = dropped.walls[0].wall.scale;
        assert!(near(scale, Vec3::splat(1.5)), "{scale}");
    }

    #[test]
    fn scaling_by_dragging_down_to_nothing_stops_at_the_minimum() {
        let castle = scaling_castle();
        let x_axis = Vec3::X;
        let (dropped, _) = drag(&castle, &handle(0, "scale-x"), |anchor| {
            anchor - x_axis * 5.0 * arm(&castle)
        });
        assert!(dropped.walls[0].wall.scale.x >= 0.2 - 1e-6);
    }

    #[test]
    fn the_gizmo_moves_and_lifts_a_piece_and_turns_it_about_the_vertical_axis() {
        let castle = tower_castle().with_selected(Some(0));
        let (moved, _) = drag(&castle, &handle(0, "translate-x"), |anchor| {
            anchor + Vec3::X * 0.52
        });
        let position = moved.piece(0).unwrap().transform.translation;
        assert!(near(position, Vec3::new(0.5, 0.0, 0.0)), "{position}");

        let (lifted, _) = drag(&castle, &handle(0, "translate-y"), |anchor| {
            anchor + Vec3::Y * 0.32
        });
        let height = lifted.piece(0).unwrap().transform.translation.y;
        assert!((height - 0.3).abs() < 1e-4, "{height}");

        let turned = castle.turned(0, Quat::from_rotation_y(FRAC_PI_2));
        let yaw = turned
            .piece(0)
            .unwrap()
            .transform
            .rotation
            .to_euler(EulerRot::YXZ)
            .0;
        assert!((yaw - FRAC_PI_2).abs() < 1e-4, "{yaw}");
        let tilted = castle.turned(0, Quat::from_rotation_x(1.0));
        assert_eq!(
            tilted.piece(0).unwrap().transform.rotation,
            castle.piece(0).unwrap().transform.rotation,
            "a piece does not tilt"
        );
    }

    #[test]
    fn a_wall_moves_and_turns_as_one_piece_through_the_castle() {
        let castle = wall_castle();
        let wall = &castle.walls[0].wall;
        let moved = castle.moved(0, Vec3::new(1.0, 0.0, -1.0));
        assert!(near(
            moved.walls[0].wall.point(0),
            wall.point(0) + Vec3::new(1.0, 0.0, -1.0)
        ));
        let turned = castle.turned(0, Quat::from_rotation_y(FRAC_PI_2));
        assert_eq!(turned.walls[0].wall.curve, wall.curve);
        assert_eq!(turned.moved(7, Vec3::X).walls[0].wall, turned.walls[0].wall);
    }

    #[test]
    fn a_tower_aligns_to_the_end_of_a_wall_after_the_wall_is_scaled() {
        let mut castle = Castle::empty().with_wall(Wall::straight((0.0, 0.0), (3.0, 0.0)));
        castle.add(PieceKind::Tower, 9.0, 9.0, 0.0);
        let scaled = castle.scaled(0, Vec3::new(2.0, 1.0, 1.0));
        let end = scaled.walls[0].wall.point(1);
        assert!((end.x - 4.5).abs() < 1e-3, "{end}");
        let near_end = Vec2::new(end.x + 0.4, end.z);
        let poses = alignments(&scaled, 1, near_end);
        assert!(
            poses
                .iter()
                .any(|pose| pose.position.x > end.x + 0.3 && pose.position.y.abs() < 0.2),
            "{poses:?}"
        );
        let before = alignments(&castle, 1, near_end);
        assert!(
            before.iter().all(|pose| pose.position.x < 3.0 + 0.8),
            "before scaling, the end was at 3: {before:?}"
        );
    }
}
