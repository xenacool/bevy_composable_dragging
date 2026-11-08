//! Cuts: one operation that makes gates, doors and breaches.
//!
//! A [`Cut`] is a volume and the shape of the opening it makes. Every solid thing in the castle
//! takes the same cut in its own way, through [`Solid`]: a wall opens along the stretch of its
//! curve inside the volume, and a tower gets a door on the side facing it. A gate dropped on a
//! wall, a gate dropped on a tower and the Cutaway tool all make the same cut.
//!
//! Cuts act through the whole height, so a volume only matters on the ground plane.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy_composable_dragging::ops::custom;
use bevy_composable_dragging::{DragSpec, Frame, Pointer};

use crate::wall::{Opening, OpeningShape, Wall};
use crate::{Castle, PieceKind, Placed, normalize_angle};

/// The width of a gate, and so of the opening it makes in a wall.
const GATE_WIDTH: f32 = 0.9;
/// How much of the radius of a cut counts when it reaches for a tower or a keep, beyond the
/// reach of the footprint. A gate that stands beside a tower does not make a door in it.
const DOOR_REACH: f32 = 0.5;
/// The cut of the Cutaway tool is this part of the brush of the precision wide.
const CUTAWAY_WIDTH: f32 = 0.3;
/// The distance between the points of a wall that are tested against a cut.
const SAMPLE_GAP: f32 = 0.05;

/// The part of the ground, and everything over it, that a cut takes away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Volume {
    Sphere {
        center: Vec3,
        radius: f32,
    },
    /// Everything within `radius` of the line from `from` to `to`.
    Capsule {
        from: Vec3,
        to: Vec3,
        radius: f32,
    },
}

impl Volume {
    pub fn radius(self) -> f32 {
        match self {
            Self::Sphere { radius, .. } | Self::Capsule { radius, .. } => radius,
        }
    }

    /// The point of the axis of the volume that is nearest to `point` on the ground plane.
    pub fn nearest(self, point: Vec2) -> Vec2 {
        match self {
            Self::Sphere { center, .. } => Vec2::new(center.x, center.z),
            Self::Capsule { from, to, .. } => {
                let (from, to) = (Vec2::new(from.x, from.z), Vec2::new(to.x, to.z));
                let line = to - from;
                let along = ((point - from).dot(line) / line.length_squared().max(f32::EPSILON))
                    .clamp(0.0, 1.0);
                from + line * along
            }
        }
    }

    /// Whether `point` on the ground plane is inside the volume.
    pub fn contains(self, point: Vec2) -> bool {
        self.nearest(point).distance(point) <= self.radius()
    }
}

/// A volume, and the opening it makes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub volume: Volume,
    pub shape: OpeningShape,
}

/// A thing that a cut can open.
pub trait Solid {
    /// Takes the cut, and says whether it changed.
    fn apply(&mut self, cut: &Cut) -> bool;
}

impl Solid for Wall {
    /// The stretch of the curve that is inside the volume becomes an opening.
    fn apply(&mut self, cut: &Cut) -> bool {
        let baked = self.curve.baked(self.scale);
        let steps = crate::wall::pieces_along(baked.length(), SAMPLE_GAP, f32::ceil);
        let inside: Vec<f32> = (0..=steps)
            .map(|step| step as f32 / steps as f32)
            .filter(|ratio| {
                let world = self
                    .pivot
                    .transform_point(baked.position(ratio * baked.length()));
                cut.volume.contains(Vec2::new(world.x, world.z))
            })
            .collect();
        let (Some(first), Some(last)) = (inside.first(), inside.last()) else {
            return false;
        };
        *self = self.with_opening(Opening {
            t: *first..*last,
            shape: cut.shape,
        });
        true
    }
}

impl Solid for Placed {
    /// A tower or a keep gets a door on the side facing the cut. A keep has its doors in the
    /// middle of a side.
    fn apply(&mut self, cut: &Cut) -> bool {
        if !matches!(self.kind, PieceKind::Tower | PieceKind::Keep) {
            return false;
        }
        let center = self.pose().position;
        let reach = self
            .outline()
            .iter()
            .map(|point| point.distance(center))
            .fold(0.0, f32::max);
        let nearest = cut.volume.nearest(center);
        if nearest.distance(center) > reach + cut.volume.radius() * DOOR_REACH {
            return false;
        }
        let toward = nearest - center;
        let world = if toward.length() < 1e-3 {
            0.0
        } else {
            toward.x.atan2(toward.y)
        };
        let local = normalize_angle(world + self.pose().angle);
        self.door = Some(if self.kind == PieceKind::Keep {
            (local / FRAC_PI_2).round() * FRAC_PI_2
        } else {
            local
        });
        true
    }
}

impl PieceKind {
    /// The opening a gate of this kind makes in what it is dropped on.
    pub fn opening(self) -> Option<OpeningShape> {
        match self {
            Self::ArchGate => Some(OpeningShape::Arch),
            Self::SquareGate => Some(OpeningShape::Square),
            Self::WoodGate => Some(OpeningShape::Timber),
            _ => None,
        }
    }
}

impl Castle {
    /// The castle with every wall, tower and keep opened by `cut`, and the ids of those that
    /// changed.
    pub fn cut(&self, cut: &Cut) -> (Self, Vec<u32>) {
        let mut castle = self.clone();
        let mut changed = Vec::new();
        for piece in &mut castle.walls {
            if piece.wall.apply(cut) {
                changed.push(piece.id);
            }
        }
        for piece in &mut castle.placed {
            if piece.apply(cut) {
                changed.push(piece.id);
            }
        }
        (castle, changed)
    }

    /// The castle after gate `id` has been dropped. A gate that is dropped on a wall or on a
    /// tower is not a piece any more but an opening in it, and what it opened is selected. A
    /// gate anywhere else, and any other piece, stays as it is.
    pub fn gate_dropped(&self, id: u32) -> Self {
        let Some(piece) = self.piece(id) else {
            return self.clone();
        };
        let Some(shape) = piece.kind.opening() else {
            return self.clone();
        };
        let at = piece.pose().position;
        let cut = Cut {
            volume: Volume::Sphere {
                center: Vec3::new(at.x, 0.0, at.y),
                radius: GATE_WIDTH / 2.0,
            },
            shape,
        };
        let (mut castle, changed) = self.without(id).cut(&cut);
        let Some(opened) = changed.first() else {
            return self.clone();
        };
        castle.selected = Some(*opened);
        castle
    }
}

/// The Cutaway tool: dragging across the ground cuts a capsule from where the pointer was
/// pressed to where it is, through every wall and tower under it. The castle is shown as it
/// will be when the pointer is released.
pub fn cutaway(castle: &Castle) -> DragSpec<Castle> {
    let castle = castle.clone();
    custom(move |context| {
        let context = context.clone();
        let castle = castle.clone();
        let mut from: Option<Vec3> = None;
        Box::new(move |pointer: Pointer| {
            let point = pointer.point + context.grab_offset();
            let to = Vec3::new(point.x, 0.0, point.z);
            let from = *from.get_or_insert(to);
            let cut = Cut {
                volume: Volume::Capsule {
                    from,
                    to,
                    radius: castle.precision.brush() * CUTAWAY_WIDTH,
                },
                shape: OpeningShape::Breach,
            };
            let (cutaway, _) = castle.cut(&cut);
            Frame::new(context.render(&cutaway), cutaway, 0.0)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy_composable_dragging::{DragContext, DragSpace};

    use super::*;
    use crate::menu::Tool;

    fn walled() -> Castle {
        Castle::empty().with_wall(Wall::straight((-2.0, 0.0), (2.0, 0.0)))
    }

    fn sphere(x: f32, z: f32, radius: f32) -> Cut {
        Cut {
            volume: Volume::Sphere {
                center: Vec3::new(x, 0.0, z),
                radius,
            },
            shape: OpeningShape::Arch,
        }
    }

    #[test]
    fn a_volume_contains_the_ground_points_within_its_radius() {
        let ball = Volume::Sphere {
            center: Vec3::new(1.0, 5.0, 1.0),
            radius: 0.5,
        };
        assert!(ball.contains(Vec2::new(1.3, 1.0)));
        assert!(!ball.contains(Vec2::new(1.6, 1.0)));
        let capsule = Volume::Capsule {
            from: Vec3::ZERO,
            to: Vec3::new(2.0, 0.0, 0.0),
            radius: 0.2,
        };
        assert!(capsule.contains(Vec2::new(1.0, 0.15)));
        assert!(capsule.contains(Vec2::new(2.1, 0.0)));
        assert!(!capsule.contains(Vec2::new(1.0, 0.3)));
        assert!(!capsule.contains(Vec2::new(-0.3, 0.0)));
    }

    #[test]
    fn a_cut_opens_the_stretch_of_a_wall_inside_it() {
        let (castle, changed) = walled().cut(&sphere(0.0, 0.0, 0.45));
        assert_eq!(changed, vec![0]);
        let openings = &castle.walls[0].wall.openings;
        assert_eq!(openings.len(), 1);
        let t = &openings[0].t;
        assert!(
            (t.start - 0.39).abs() < 0.03 && (t.end - 0.61).abs() < 0.03,
            "{t:?}"
        );
        assert_eq!(openings[0].shape, OpeningShape::Arch);
    }

    #[test]
    fn a_cut_away_from_everything_changes_nothing() {
        let before = walled();
        let (after, changed) = before.cut(&sphere(0.0, 3.0, 0.45));
        assert_eq!(changed, Vec::<u32>::new());
        assert_eq!(after.walls[0].wall, before.walls[0].wall);
    }

    #[test]
    fn a_cut_opens_a_wall_in_the_same_place_after_it_has_been_scaled_and_moved() {
        let wall = Wall::straight((-2.0, 0.0), (2.0, 0.0))
            .scaled(Vec3::new(1.5, 1.0, 1.0))
            .moved(Vec3::new(1.0, 0.0, 1.0));
        let (castle, _) = Castle::empty().with_wall(wall).cut(&sphere(1.0, 1.0, 0.45));
        let t = &castle.walls[0].wall.openings[0].t;
        assert!((t.start + t.end - 1.0).abs() < 0.03, "{t:?}");
        let rescaled = castle.walls[0].wall.scaled(Vec3::new(2.0, 1.5, 1.0));
        assert_eq!(rescaled.openings, castle.walls[0].wall.openings);
    }

    #[test]
    fn a_cut_near_a_tower_gives_it_a_door_facing_the_cut() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        let (opened, changed) = castle.cut(&sphere(0.5, 0.0, 0.3));
        assert_eq!(changed, vec![0]);
        let door = opened.piece(0).unwrap().door.unwrap();
        assert!((door - FRAC_PI_2).abs() < 1e-3, "{door}");
        let (far, changed) = castle.cut(&sphere(2.0, 0.0, 0.3));
        assert_eq!(changed, Vec::<u32>::new());
        assert_eq!(far.piece(0).unwrap().door, None);
    }

    #[test]
    fn a_keep_gets_its_door_in_the_middle_of_a_side_and_other_pieces_get_none() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Keep, 0.0, 0.0, 0.0);
        castle.add(PieceKind::Banner, 1.5, 0.0, 0.0);
        let (opened, changed) = castle.cut(&sphere(0.5, 0.1, 0.4));
        assert_eq!(changed, vec![0]);
        let door = opened.piece(0).unwrap().door.unwrap();
        assert!((door - FRAC_PI_2).abs() < 1e-3, "{door}");
    }

    #[test]
    fn a_door_shows_as_a_child_of_its_tower_and_goes_with_a_copy() {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        let id = crate::piece_id(0);
        let door = format!("{id}/door");
        assert!(crate::view(&castle).get(&door).is_none());
        let (opened, _) = castle.cut(&sphere(0.5, 0.0, 0.3));
        let view = crate::view(&opened);
        assert_eq!(
            view.get(&door).unwrap().parent.as_deref(),
            Some(id.as_str())
        );
    }

    #[test]
    fn a_gate_dropped_on_a_wall_becomes_an_opening_and_a_gate_elsewhere_stays() {
        let mut castle = walled();
        castle.add(PieceKind::ArchGate, 0.0, 0.0, 0.0);
        let gate = castle.placed.last().unwrap().id;
        let dropped = castle.gate_dropped(gate);
        assert!(dropped.piece(gate).is_none(), "the gate is now the opening");
        assert_eq!(dropped.walls[0].wall.openings.len(), 1);
        assert_eq!(dropped.walls[0].wall.openings[0].shape, OpeningShape::Arch);
        assert_eq!(dropped.selected, Some(0));

        let mut apart = walled();
        apart.add(PieceKind::SquareGate, 0.0, 3.0, 0.0);
        let id = apart.placed.last().unwrap().id;
        let kept = apart.gate_dropped(id);
        assert!(kept.piece(id).is_some());
        assert_eq!(kept.walls[0].wall.openings, Vec::new());

        let mut tower = Castle::empty();
        tower.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        tower.add(PieceKind::WoodGate, 0.5, 0.0, 0.0);
        let wooden = tower.placed.last().unwrap().id;
        let doored = tower.gate_dropped(wooden);
        assert!(doored.piece(wooden).is_none());
        assert!(doored.piece(0).unwrap().door.is_some());

        let mut plain = walled();
        plain.add(PieceKind::Banner, 0.0, 0.0, 0.0);
        let banner = plain.placed.last().unwrap().id;
        assert!(plain.gate_dropped(banner).piece(banner).is_some());
    }

    #[test]
    fn only_gates_make_openings() {
        assert_eq!(PieceKind::ArchGate.opening(), Some(OpeningShape::Arch));
        assert_eq!(PieceKind::SquareGate.opening(), Some(OpeningShape::Square));
        assert_eq!(PieceKind::WoodGate.opening(), Some(OpeningShape::Timber));
        assert_eq!(PieceKind::Tower.opening(), None);
    }

    #[test]
    fn dragging_the_cutaway_tool_across_a_wall_breaches_it_in_the_preview_and_on_release() {
        let mut castle = walled();
        castle.tool = Tool::Cutaway;
        let eye = Vec3::new(0.0, 6.0, 6.0);
        let looking = |at: Vec3| Pointer {
            ray: Ray3d::new(eye, Dir3::new(at - eye).unwrap()),
            point: at,
        };
        let context = DragContext::new(Arc::new(crate::view), "terrain/0".into(), castle.clone())
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let mut behavior = cutaway(&castle).compile(&context);
        behavior(looking(Vec3::new(-0.5, 0.0, 1.0)));
        let frame = behavior(looking(Vec3::new(-0.5, 0.0, -1.0)));
        let wall = &frame.drop_state.walls[0].wall;
        assert_eq!(wall.openings.len(), 1);
        assert_eq!(wall.openings[0].shape, OpeningShape::Breach);
        assert!(wall.opening_at(0.38).is_some());
        assert!(wall.opening_at(0.9).is_none());
        let still = behavior(looking(Vec3::new(-0.5, 0.0, 1.0)));
        assert!(
            still.drop_state.walls[0].wall.openings.is_empty() || {
                let t = &still.drop_state.walls[0].wall.openings[0].t;
                t.end - t.start < 0.1
            }
        );
    }
}
