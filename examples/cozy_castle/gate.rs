//! Gates: an arch gate dropped on a wall is not a piece any more but an opening in it.
//!
//! The wall as it is shown is derived from the castle, like the look of a stacked piece: it is
//! open under every arch gate that stands over it. So the opening is there in the preview while
//! the gate is still held, and dropping the gate only makes it permanent.

use crate::wall::Wall;
use crate::{Castle, PieceKind};

/// The width of a gate, and so of the opening it makes in a wall.
const GATE_WIDTH: f32 = 0.9;

impl Castle {
    /// The wall as it is shown: open under every arch gate that stands over it.
    pub fn wall_shown(&self, wall: &Wall) -> Wall {
        self.placed
            .iter()
            .filter(|piece| piece.kind == PieceKind::ArchGate)
            .fold(wall.clone(), |wall, gate| {
                wall.opened_at(gate.ground(), GATE_WIDTH / 2.0)
                    .unwrap_or(wall)
            })
    }

    /// The castle after gate `id` has been dropped. A gate that is dropped on a wall is not a
    /// piece any more but an opening in it, and the wall is selected. A gate anywhere else, and
    /// any other piece, stays as it is.
    pub fn gate_dropped(&self, id: u32) -> Self {
        let Some(gate) = self
            .piece(id)
            .filter(|piece| piece.kind == PieceKind::ArchGate)
        else {
            return self.clone();
        };
        let at = gate.ground();
        let Some((wall, opened)) = self.walls.iter().find_map(|piece| {
            piece
                .wall
                .opened_at(at, GATE_WIDTH / 2.0)
                .map(|opened| (piece.id, opened))
        }) else {
            return self.clone();
        };
        let mut castle = self.without(id);
        if let Some(piece) = castle.walls.iter_mut().find(|piece| piece.id == wall) {
            piece.wall = opened;
        }
        castle.with_selected(Some(wall))
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;

    fn walled() -> Castle {
        Castle::empty().with_wall(Wall::through(&[
            Vec3::new(-2.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        ]))
    }

    #[test]
    fn a_gate_over_a_wall_opens_it_in_the_view_while_the_gate_is_still_there() {
        let mut castle = walled();
        castle.add(PieceKind::ArchGate, 0.0, 0.0);
        let shown = castle.wall_shown(&castle.walls[0].wall);
        assert_ne!(shown.shape(), castle.walls[0].wall.shape());
        assert!(castle.piece(1).is_some());
    }

    #[test]
    fn a_gate_dropped_on_a_wall_becomes_an_opening_and_a_gate_elsewhere_stays() {
        let mut castle = walled();
        castle.add(PieceKind::ArchGate, 0.0, 0.0);
        let dropped = castle.gate_dropped(1);
        assert!(dropped.piece(1).is_none(), "the gate is now the opening");
        assert_eq!(dropped.selected, Some(0));
        assert_eq!(
            dropped.wall_shown(&dropped.walls[0].wall).shape(),
            castle.wall_shown(&castle.walls[0].wall).shape(),
            "the wall looks the same before and after the drop"
        );

        let mut apart = walled();
        apart.add(PieceKind::ArchGate, 0.0, 3.0);
        let id = apart.placed.last().unwrap().id;
        assert!(apart.gate_dropped(id).piece(id).is_some());
        assert_eq!(apart.wall_shown(&apart.walls[0].wall), apart.walls[0].wall);

        let mut plain = walled();
        plain.add(PieceKind::Tower, 0.0, 0.0);
        assert!(plain.gate_dropped(1).piece(1).is_some());
    }
}
