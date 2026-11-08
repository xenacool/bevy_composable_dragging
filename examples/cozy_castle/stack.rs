//! Stacking: a piece that stands on another changes the one underneath.
//!
//! What a piece looks like depends on what stands on it, and that is worked out by a pure
//! function of the castle, [`Castle::variant`]. It only reads which pieces rest on which, which
//! follows from the heights of their tops (fixed by their kinds) and never from how they look,
//! so a piece cannot change what it rests on in a loop. A piece with something on it gives up its
//! roof or battlements for a platform, and thickens at the base for every layer above it.
//!
//! Because a drag is previewed by drawing whole castles, the lower piece changes in the preview
//! while the piece held over it is carried or settled onto it, and goes back when it is lifted
//! away.

use bevy::prelude::*;

use crate::{Castle, OVERLAP_EPSILON, Placed, polygons_overlap};

/// How far from the top of a piece the base of another may be and still rest on it.
const REST_TOLERANCE: f32 = 0.05;
/// Pieces further apart than this cannot overlap.
const NEAR: f32 = 2.5;
/// How much thicker a piece is for every layer above it, and for how many layers it counts.
const THICKEN: f32 = 0.05;
const MAX_LAYERS: u8 = 3;
/// How far from its center, as a share of its outline, a piece is on a wall.
const WALL_INSET: f32 = 0.5;

/// How a piece or a wall looks, given what stands on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variant {
    /// Something stands on it, so its top is a platform.
    pub open_top: bool,
    /// How much thicker its base is than usual.
    pub thickness: f32,
    /// It has its roof, or its battlements.
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
    /// The points of a footprint that decide whether a piece stands on a wall: its center and
    /// the middle of its outline, pulled halfway to the center. A piece that only touches the end
    /// of a wall, such as a gate set between two walls, does not stand on it.
    pub fn wall_points(outline: &[Vec2], center: Vec2) -> Vec<Vec2> {
        outline
            .iter()
            .map(|corner| center.lerp(*corner, WALL_INSET))
            .chain(std::iter::once(center))
            .collect()
    }

    /// The points of piece `id` that decide whether it stands on a wall.
    pub fn footprint_points(piece: &Placed) -> Vec<Vec2> {
        Self::wall_points(&piece.outline(), piece.pose().position)
    }

    /// Whether `upper` rests on the top of `lower`.
    fn rests_on(upper: &Placed, lower: &Placed) -> bool {
        lower.kind.top() > 0.0
            && upper.pose().position.distance(lower.pose().position) < NEAR
            && (upper.transform.translation.y - (lower.transform.translation.y + lower.kind.top()))
                .abs()
                <= REST_TOLERANCE
            && polygons_overlap(&upper.outline(), &lower.outline(), OVERLAP_EPSILON)
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

    /// The pieces and the walls that piece `id` stands on.
    pub fn resting_on(&self, id: u32) -> Vec<u32> {
        let Some(upper) = self.piece(id) else {
            return Vec::new();
        };
        let points = Self::footprint_points(upper);
        let pieces = self
            .placed
            .iter()
            .filter(|lower| lower.id != id && Self::rests_on(upper, lower))
            .map(|lower| lower.id);
        let walls = self.walls.iter().filter_map(|piece| {
            piece
                .wall
                .top_under(&self.terrain, &points)
                .filter(|top| (upper.transform.translation.y - top).abs() <= REST_TOLERANCE)
                .map(|_| piece.id)
        });
        pieces.chain(walls).collect()
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

    /// How wall `id` looks, given the pieces that stand on it.
    pub fn wall_variant(&self, id: u32) -> Variant {
        if !self.walls.iter().any(|piece| piece.id == id) {
            return Variant::STANDING;
        }
        let layers = self
            .placed
            .iter()
            .filter(|upper| self.resting_on(upper.id).contains(&id))
            .map(|upper| self.layers_above(upper.id).saturating_add(1))
            .max()
            .unwrap_or(0);
        Variant::with_layers(layers)
    }

    /// The top of the highest wall under the footprint of a piece at `pose`, if it stands on one.
    pub fn wall_top_under(&self, outline: &[Vec2], center: Vec2) -> Option<f32> {
        let points = Self::wall_points(outline, center);
        self.walls
            .iter()
            .filter_map(|piece| piece.wall.top_under(&self.terrain, &points))
            .reduce(f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wall::Wall;
    use crate::{PieceKind, piece_id};

    fn tower_stack(layers: usize) -> Castle {
        let mut castle = Castle::empty();
        for _ in 0..layers {
            castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        }
        castle
    }

    #[test]
    fn a_piece_with_nothing_on_it_stands_as_it_is() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Tower, 2.0, 0.0, 0.0);
        assert_eq!(castle.variant(0), Variant::STANDING);
        assert_eq!(castle.supports(0), Vec::<u32>::new());
        assert_eq!(castle.resting_on(1), Vec::<u32>::new());
    }

    #[test]
    fn a_piece_on_top_turns_the_roof_into_a_platform_and_thickens_the_base() {
        let castle = tower_stack(2);
        assert_eq!(castle.supports(0), vec![1]);
        assert_eq!(castle.resting_on(1), vec![0]);
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
        assert!(castle.variant(0).open_top && castle.variant(1).open_top);
        assert_eq!(castle.variant(2), Variant::STANDING);
        assert_eq!(castle.supports(0), vec![1]);
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
        castle.add(PieceKind::Keep, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Keep, 0.9, 0.0, 0.0);
        assert_eq!(castle.variant(0), Variant::STANDING);
        assert_eq!(castle.variant(1), Variant::STANDING);
    }

    #[test]
    fn a_path_has_nothing_stand_on_it() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Cobble, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Cobble, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        assert!(castle.supports(0).is_empty() && castle.supports(1).is_empty());
        assert_eq!(castle.variant(0), Variant::STANDING);
    }

    #[test]
    fn overlapping_pieces_never_loop() {
        let mut castle = Castle::empty();
        for kind in [
            PieceKind::ArchGate,
            PieceKind::SquareGate,
            PieceKind::Torch,
            PieceKind::Banner,
            PieceKind::Cobble,
            PieceKind::Cottage,
        ] {
            castle.add(kind, 0.1, 0.0, 0.0);
            castle.add(kind, 0.0, 0.1, 0.0);
        }
        for piece in &castle.placed {
            let _ = castle.variant(piece.id);
        }
    }

    #[test]
    fn the_variant_ignores_the_terrain_under_the_stack() {
        let castle = tower_stack(2);
        let raised = castle.sculpted(&crate::terrain::Brush {
            center: Vec2::new(0.0, 0.0),
            radius: 1.0,
            amount: 0.6,
            falloff: 1.0,
        });
        assert!(raised.piece(0).unwrap().transform.translation.y > 0.3);
        assert_eq!(raised.variant(0), castle.variant(0));
        assert_eq!(raised.variant(1), castle.variant(1));
    }

    #[test]
    fn carrying_a_piece_over_another_changes_the_one_underneath_in_the_preview() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Keep, 3.0, 0.0, 0.0);
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

    #[test]
    fn the_candidates_of_a_drag_include_the_lower_piece_changed() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Banner, 3.0, 0.0, 0.0);
        // A banner dropped on the tower: every candidate is a whole castle, so the tower in
        // the candidate of the carried banner is shown as it would be with the banner on it.
        let over = castle.moved(1, Vec3::new(-3.0, 0.0, 0.0));
        assert!(over.variant(0).open_top);
        assert!(
            crate::candidates(&over, 1, Vec2::ZERO)
                .iter()
                .all(|candidate| candidate.piece(0).is_some())
        );
    }

    #[test]
    fn a_piece_stands_on_a_wall_and_changes_it() {
        let wall = Wall::straight((-2.0, 0.0), (2.0, 0.0));
        let top = wall.profile.top();
        let mut castle = Castle::empty().with_wall(wall);
        assert_eq!(castle.wall_variant(0), Variant::STANDING);
        castle.add(PieceKind::Outpost, 0.0, 0.0, 0.0);
        let y = castle.piece(1).unwrap().transform.translation.y;
        assert!((y - top).abs() < 1e-4, "{y} against {top}");
        assert_eq!(castle.resting_on(1), vec![0]);
        let variant = castle.wall_variant(0);
        assert!(variant.open_top && !variant.roof);
        assert!(variant.thickness > 1.0);
        let walkway = castle.walls[0].wall.stacked(variant);
        assert!(walkway.profile.battlements.is_none());
        assert!((walkway.profile.top() - top).abs() < 1e-5, "same height");
        assert!(walkway.profile.width > castle.walls[0].wall.profile.width);
        assert_eq!(castle.without(1).wall_variant(0), Variant::STANDING);
    }

    #[test]
    fn a_piece_beside_a_wall_does_not_stand_on_it() {
        let wall = Wall::straight((-2.0, 0.0), (2.0, 0.0));
        let mut castle = Castle::empty().with_wall(wall);
        castle.add(PieceKind::Banner, 0.0, 1.5, 0.0);
        assert_eq!(castle.resting_on(1), Vec::<u32>::new());
        assert!(castle.piece(1).unwrap().transform.translation.y.abs() < 1e-6);
        assert_eq!(castle.wall_variant(0), Variant::STANDING);
    }

    #[test]
    fn a_piece_on_a_wall_on_a_hill_stands_at_the_top_of_the_wall() {
        let wall = Wall::straight((-2.0, 0.0), (2.0, 0.0));
        let mut castle = Castle::empty().with_wall(wall);
        castle = castle.sculpted(&crate::terrain::Brush {
            center: Vec2::new(0.0, 0.0),
            radius: 1.2,
            amount: 0.5,
            falloff: 1.0,
        });
        castle.add(PieceKind::Outpost, 0.0, 0.0, 0.0);
        let y = castle.piece(1).unwrap().transform.translation.y;
        let ground = castle.terrain.surface(Vec2::ZERO);
        let top = castle.walls[0].wall.profile.top();
        assert!(
            (y - (ground + top)).abs() < 0.05,
            "{y} against {ground} + {top}"
        );
    }
}
