//! Selection, and the gizmo that scales the selected wall.
//!
//! Selection is state, `Castle::selected`. Pressing a wall and releasing it selects it through a
//! fixed drop state, so it needs no click event and works for a drag of any length, including
//! none. The gizmo is the library [`Gizmo`], added to the view for the selected wall while the
//! Scale tool is chosen. Its handles edit the state through [`Castle::scaled`]: they never scale
//! a node, because the handles are children of the node and a scaled node would stretch them.

use bevy::prelude::*;
use bevy_composable_dragging::DragSpec;
use bevy_composable_dragging::gizmo::{Gizmo, GizmoReveal};
use bevy_composable_dragging::ops::{Snap, state};

use crate::menu::Tool;
use crate::wall::Wall;
use crate::{Castle, piece_id};

/// How much bigger than the height of a wall its gizmo is.
const GIZMO_PER_HEIGHT: f32 = 2.5;
/// How far beyond the reach of a wall its handles are, so the pointer grabs a handle and not
/// the wall.
const GIZMO_MARGIN: f32 = 0.6;
/// The smallest gizmo.
const MIN_GIZMO: f32 = 1.0;

impl Castle {
    /// The castle with `selected` selected. Choosing something else puts the tool back to Select.
    pub fn with_selected(&self, selected: Option<u32>) -> Self {
        let mut castle = self.clone();
        if castle.selected != selected {
            castle.tool = Tool::Select;
        }
        castle.selected = selected;
        castle
    }

    /// The castle with piece `id` moved by the ground part of the world `offset`. A piece stays
    /// on what it rests on.
    pub fn moved(&self, id: u32, offset: Vec3) -> Self {
        let Some(piece) = self.piece(id) else {
            return self.clone();
        };
        self.posed(id, piece.ground() + Vec2::new(offset.x, offset.z))
    }

    /// The castle with wall `id` scaled by `factor` on each of its axes.
    pub fn scaled(&self, id: u32, factor: Vec3) -> Self {
        let mut castle = self.clone();
        if let Some(piece) = castle.walls.iter_mut().find(|piece| piece.id == id) {
            piece.wall = piece.wall.scaled(factor);
        }
        castle
    }

    /// Whether the Scale tool is chosen for the wall that is selected.
    pub fn scaling(&self) -> bool {
        self.tool == Tool::Scale && self.selected.is_some_and(|id| self.wall(id).is_some())
    }
}

impl GizmoReveal for Castle {
    /// Only the Scale tool shows the gizmo of the selected wall.
    fn reveals_gizmo(&self) -> bool {
        self.scaling()
    }
}

/// The spec of a press on a wall: whatever the pointer does, releasing selects the wall.
pub fn select_spec(castle: &Castle, id: u32) -> DragSpec<Castle> {
    state(castle.with_selected(Some(id)))
}

/// How big the gizmo of a wall is: its handles have to reach out of the wall, which is `top`
/// tall and reaches `reach` from its pivot, or the pointer would grab the wall instead.
fn gizmo_size(top: f32, reach: f32) -> f32 {
    (top * GIZMO_PER_HEIGHT)
        .max(reach + GIZMO_MARGIN)
        .max(MIN_GIZMO)
}

/// The gizmo that scales `wall`, whose root node is at `transform`: handles for each axis and
/// one for all three. Handles the scene camera sees edge-on or end-on are left out.
pub fn wall_gizmo(id: u32, wall: &Wall, transform: Transform) -> Gizmo<Castle> {
    let top = wall.profile().top();
    Gizmo::new(piece_id(id), transform)
        .with_size(gizmo_size(top, wall.reach()))
        .raised(top / 2.0)
        .seen_from(crate::scene_camera().translation)
        .scale(Snap::NONE, move |castle: &Castle, factor| {
            castle.scaled(id, factor)
        })
        .scale_uniform(Snap::NONE, move |castle: &Castle, factor| {
            castle.scaled(id, Vec3::splat(factor))
        })
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, DragSpace, Pointer, View};

    use super::*;
    use crate::{PieceKind, move_spec};

    fn walled() -> Castle {
        let mut castle = Castle::empty()
            .with_wall(Wall::through(&[
                Vec3::new(-1.5, 0.0, 1.0),
                Vec3::new(1.5, 0.0, 1.0),
            ]))
            .with_selected(Some(0));
        castle.tool = Tool::Scale;
        castle
    }

    fn handle(name: &str) -> String {
        format!("{}/gizmo/{name}", piece_id(0))
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    /// Drags `node` of `castle` with the spec its node has. The pointer goes from the node to
    /// `to(anchor)`; a camera-facing handle drags in the plane through its pivot facing +Z.
    fn drag(castle: &Castle, node: &str, to: impl Fn(Vec3) -> Vec3) -> (Castle, View<Castle>) {
        let view = crate::view(castle);
        let node_ref = view.get(node).unwrap();
        let spec = node_ref.drag_spec(castle).unwrap();
        let anchor = view.world_position(node).unwrap();
        let space = match node_ref.drag_space.unwrap() {
            DragSpace::Screen => DragSpace::plane(anchor, Dir3::Z),
            other => other,
        };
        let context =
            DragContext::new(crate::view, node.to_owned().into(), castle.clone()).with_space(space);
        let frame = spec.compile(&context)(Pointer::at(to(anchor)));
        (frame.drop_state, frame.view)
    }

    /// The length of the arm of the gizmo of the wall.
    fn arm(castle: &Castle) -> f32 {
        let wall = &castle.walls[0].wall;
        gizmo_size(wall.profile().top(), wall.reach())
    }

    #[test]
    fn the_gizmo_is_shown_only_for_the_scale_tool_of_a_selected_wall() {
        let gizmos = |castle: &Castle| {
            crate::view(castle)
                .nodes()
                .iter()
                .filter(|node| node.id.contains("/gizmo/"))
                .count()
        };
        assert!(gizmos(&walled()) > 0);
        let mut selecting = walled();
        selecting.tool = Tool::Select;
        assert_eq!(gizmos(&selecting), 0);
        assert_eq!(gizmos(&walled().with_selected(None)), 0);
        let view = crate::view(&walled());
        for kind in ["scale-x", "scale-uniform"] {
            let node = view.get(&handle(kind)).unwrap();
            assert_eq!(node.parent.as_deref(), Some("piece-0"), "{kind}");
        }
    }

    #[test]
    fn choosing_something_else_puts_the_tool_back_to_select() {
        assert_eq!(walled().with_selected(Some(0)).tool, Tool::Scale);
        assert_eq!(walled().with_selected(None).tool, Tool::Select);
    }

    #[test]
    fn pressing_a_wall_selects_it_and_releasing_a_piece_selects_it_too() {
        let mut castle = walled().with_selected(None);
        castle.add(PieceKind::Tower, 0.0, 0.0);
        let (selected, _) = drag(&castle, &piece_id(0), |anchor| anchor);
        assert_eq!(selected.selected, Some(0));

        let context = DragContext::new(crate::view, piece_id(1).into(), castle.clone())
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let still = move_spec(1).compile(&context)(Pointer::at(Vec3::ZERO));
        assert_eq!(still.drop_state.selected, Some(1));
        let kept = still.drop_state.piece(1).unwrap().position;
        assert!(near(kept, castle.piece(1).unwrap().position));
    }

    #[test]
    fn dragging_a_scale_handle_scales_the_wall_and_the_root_stays_unscaled() {
        let castle = walled();
        let reach = arm(&castle);
        let (dropped, preview) = drag(&castle, &handle("scale-x"), |anchor| {
            anchor + Vec3::X * 1.02 * reach
        });
        let scale = dropped.walls[0].wall.scale;
        assert!(
            scale.x > 1.5 && near(scale, Vec3::new(scale.x, 1.0, 1.0)),
            "{scale}"
        );
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
            dropped.walls[0].wall.shape().bounds().half_extents().x
                > castle.walls[0].wall.shape().bounds().half_extents().x * 1.5
        );
    }

    #[test]
    fn scaling_by_dragging_down_to_nothing_stops_at_the_minimum() {
        let castle = walled();
        let (dropped, _) = drag(&castle, &handle("scale-x"), |anchor| {
            anchor - Vec3::X * 5.0 * arm(&castle)
        });
        assert!(dropped.walls[0].wall.scale.x >= 0.2 - 1e-6);
    }
}
