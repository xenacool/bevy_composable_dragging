//! Stacking: a piece that stands on another changes the one underneath.
//!
//! What a piece looks like depends on what stands on it, and that is worked out by a pure
//! function of the castle, [`Castle::variant`]. It only reads which pieces rest on which, which
//! follows from the heights of their tops (fixed by their kinds) and never from how they look,
//! so a piece cannot change what it rests on in a loop. A piece with something on it gives up its
//! roof for a platform, and thickens at the base for every layer above it.
//!
//! Because a drag is previewed by drawing whole castles, the lower piece changes in the preview
//! while the piece held over it is carried onto it, and goes back when it is lifted away.

use crate::{Castle, Placed};

/// How far from the top of a piece the base of another may be and still rest on it.
const REST_TOLERANCE: f32 = 0.05;
/// How much thicker a piece is for every layer above it, and for how many layers it counts.
const THICKEN: f32 = 0.05;
const MAX_LAYERS: u8 = 3;

/// How a piece looks, given what stands on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variant {
    /// Something stands on it, so its top is a platform.
    pub open_top: bool,
    /// How much thicker its base is than usual.
    pub thickness: f32,
    /// It has its roof.
    pub roof: bool,
}

impl Variant {
    /// A piece with nothing on it.
    pub const STANDING: Self = Self {
        open_top: false,
        thickness: 1.0,
        roof: true,
    };

    /// A piece with `layers` layers above it.
    fn with_layers(layers: u8) -> Self {
        if layers == 0 {
            return Self::STANDING;
        }
        Self {
            open_top: true,
            thickness: 1.0 + THICKEN * f32::from(layers.min(MAX_LAYERS)),
            roof: false,
        }
    }
}

impl Castle {
    /// Whether `upper` rests on the top of `lower`.
    fn rests_on(upper: &Placed, lower: &Placed) -> bool {
        (upper.position.y - (lower.position.y + lower.kind.top())).abs() <= REST_TOLERANCE
            && upper.overlaps(lower)
    }

    /// The pieces that stand on piece `id`.
    pub fn supports(&self, id: u32) -> Vec<u32> {
        let Some(lower) = self.piece(id) else {
            return Vec::new();
        };
        self.placed
            .iter()
            .filter(|upper| upper.id != id && Self::rests_on(upper, lower))
            .map(|upper| upper.id)
            .collect()
    }

    /// How many layers of pieces stand above piece `id`.
    fn layers_above(&self, id: u32) -> u8 {
        self.supports(id)
            .into_iter()
            .map(|above| self.layers_above(above).saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// How piece `id` looks, given what stands on it.
    pub fn variant(&self, id: u32) -> Variant {
        Variant::with_layers(self.layers_above(id))
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::{PieceKind, piece_id};

    fn tower_stack(layers: usize) -> Castle {
        let mut castle = Castle::empty();
        for _ in 0..layers {
            castle.add(PieceKind::Tower, 0.0, 0.0);
        }
        castle
    }

    #[test]
    fn a_piece_with_nothing_on_it_stands_as_it_is() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0);
        castle.add(PieceKind::Tower, 2.0, 0.0);
        assert_eq!(castle.variant(0), Variant::STANDING);
        assert_eq!(castle.supports(0), Vec::<u32>::new());
    }

    #[test]
    fn a_piece_on_top_turns_the_roof_into_a_platform_and_thickens_the_base() {
        let castle = tower_stack(2);
        assert_eq!(castle.supports(0), vec![1]);
        let lower = castle.variant(0);
        assert!(lower.open_top && !lower.roof);
        assert!((lower.thickness - 1.05).abs() < 1e-6);
        assert_eq!(castle.variant(1), Variant::STANDING);
    }

    #[test]
    fn three_layers_thicken_the_base_twice_and_the_middle_once() {
        let castle = tower_stack(3);
        assert!((castle.variant(0).thickness - 1.10).abs() < 1e-6);
        assert!((castle.variant(1).thickness - 1.05).abs() < 1e-6);
        assert_eq!(castle.variant(2), Variant::STANDING);
        assert_eq!(castle.supports(1), vec![2]);
    }

    #[test]
    fn the_thickening_stops_after_a_few_layers() {
        let castle = tower_stack(7);
        let most = 1.0 + THICKEN * f32::from(MAX_LAYERS);
        assert!((castle.variant(0).thickness - most).abs() < 1e-6);
        assert!((castle.variant(1).thickness - most).abs() < 1e-6);
    }

    #[test]
    fn removing_the_upper_piece_puts_the_lower_one_back() {
        let castle = tower_stack(2);
        assert!(castle.variant(0).open_top);
        assert_eq!(castle.without(1).variant(0), Variant::STANDING);
    }

    #[test]
    fn a_piece_beside_another_does_not_stack_on_it() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Keep, 0.0, 0.0);
        castle.add(PieceKind::Keep, 0.9, 0.0);
        assert_eq!(castle.variant(0), Variant::STANDING);
        assert_eq!(castle.variant(1), Variant::STANDING);
    }

    #[test]
    fn overlapping_pieces_never_loop() {
        let mut castle = Castle::empty();
        for kind in [PieceKind::ArchGate, PieceKind::Tower, PieceKind::Keep] {
            castle.add(kind, 0.1, 0.0);
            castle.add(kind, 0.0, 0.1);
        }
        for piece in &castle.placed {
            let _ = castle.variant(piece.id);
        }
    }

    #[test]
    fn carrying_a_piece_over_another_changes_the_one_underneath_in_the_preview() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Keep, 0.0, 0.0);
        castle.add(PieceKind::Tower, 3.0, 0.0);
        let children = |castle: &Castle| {
            crate::view(castle)
                .nodes()
                .iter()
                .filter(|node| node.parent.as_deref() == Some(piece_id(0).as_str()))
                .count()
        };
        let over = castle.moved(1, Vec3::new(-3.0, 0.0, 0.0));
        assert_eq!(castle.variant(0), Variant::STANDING);
        assert!(over.variant(0).open_top);
        assert_ne!(children(&castle), children(&over));
        let held_away = over.moved(1, Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(held_away.variant(0), Variant::STANDING);
        assert_eq!(children(&held_away), children(&castle));
    }
}
