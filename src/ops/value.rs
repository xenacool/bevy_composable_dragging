//! The value operators `along_axis`, `around_axis` and `across_plane`, and their [`Snap`].
//!
//! Where [`vary`](super::vary) searches for the parameters that bring a node closest to the
//! pointer, these operators measure the pointer directly: the distance along an axis, the angle
//! around an axis, or the offset across a plane. The measured value is handed to a function that
//! applies it to the state, so they need no solver and are exact. They are the building blocks
//! of the handles of a [`Gizmo`](crate::gizmo::Gizmo).
//!
//! The preview shows the state with the value pulled towards the nearest step of the [`Snap`],
//! and releasing commits the state with the exact step. All values are measured from where the
//! drag started, so steps are steps of change, not positions on a fixed grid.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use bevy::math::{Vec2, Vec3};

use crate::behavior::{DragContext, Frame};
use crate::space::{DragSpace, Pointer};
use crate::spec::DragSpec;

/// How a measured value settles onto steps: the preview is pulled towards the nearest multiple
/// of `step` once the value is within `radius` of it, and a drop always commits that multiple.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// The distance between steps. Zero or less turns snapping off.
    pub step: f32,
    /// How close to a step the preview starts to be pulled towards it, as a distance in the unit
    /// of the value.
    pub radius: f32,
}

impl Snap {
    /// No snapping: the value is the measured one.
    pub const NONE: Self = Self {
        step: 0.0,
        radius: 0.0,
    };

    /// Steps of `step`, with a pull radius of a quarter of a step.
    #[must_use]
    pub fn step(step: f32) -> Self {
        Self {
            step,
            radius: step * 0.25,
        }
    }

    /// The same steps with another pull radius.
    #[must_use]
    pub fn within(self, radius: f32) -> Self {
        Self { radius, ..self }
    }

    /// The value shown in the preview and the value committed on drop for a measured `raw`
    /// value. The shown value is `raw` far from a step, and moves towards the nearest step along
    /// a smoothstep as `raw` gets closer to it: `lerp(raw, step, smoothstep(1 - distance /
    /// radius))`.
    #[must_use]
    pub fn apply(self, raw: f32) -> (f32, f32) {
        if self.step <= 0.0 {
            return (raw, raw);
        }
        let snapped = (raw / self.step).round() * self.step;
        if self.radius <= 0.0 {
            return (raw, snapped);
        }
        let closeness = (1.0 - (raw - snapped).abs() / self.radius).clamp(0.0, 1.0);
        let pull = closeness * closeness * (3.0 - 2.0 * closeness);
        (raw + (snapped - raw) * pull, snapped)
    }
}

/// Turns the state a drag started from and a measured value into a new state.
type Apply<S, V> = Arc<dyn Fn(&S, V) -> S + Send + Sync>;

/// Builds the spec of a value operator from its [`Snap`].
type Build<S> = Arc<dyn Fn(Snap) -> DragSpec<S> + Send + Sync>;

/// A value operator whose snapping can still be chosen: turn it into a [`DragSpec`] with
/// `into()`, or set a [`Snap`] first with [`snap`](Self::snap).
pub struct Tracked<S> {
    build: Build<S>,
    snap: Snap,
}

impl<S> Clone for Tracked<S> {
    fn clone(&self) -> Self {
        Self {
            build: Arc::clone(&self.build),
            snap: self.snap,
        }
    }
}

impl<S> Tracked<S> {
    /// Snaps the value to steps. The default is [`Snap::NONE`].
    #[must_use]
    pub fn snap(mut self, snap: Snap) -> Self {
        self.snap = snap;
        self
    }
}

impl<S> From<Tracked<S>> for DragSpec<S> {
    fn from(tracked: Tracked<S>) -> Self {
        (tracked.build)(tracked.snap)
    }
}

/// Measures a pointer into the shown and the committed value of one drag.
type Measure<V> = Box<dyn FnMut(&Pointer) -> (V, V) + Send + Sync>;

/// Starts a [`Measure`] for a drag in `space` of a node that started at `anchor`, grabbed at the
/// point `grab_offset` away from it. The pointers it measures are the grab position of the node,
/// that is the pointer with `grab_offset` taken off.
type StartMeasure<V> = Arc<dyn Fn(&DragSpace, Vec3, Vec3) -> Measure<V> + Send + Sync>;

/// The spec of a drag that applies a measured value to the state.
fn value_spec<S, V>(apply: Apply<S, V>, start: StartMeasure<V>) -> DragSpec<S>
where
    S: Clone + Send + Sync + 'static,
    V: Copy + Send + Sync + 'static,
{
    DragSpec::new(move |context: &DragContext<S>| {
        let state = context.start_state().clone();
        let space = *context.space();
        let anchor = context
            .anchor_in(&context.render(&state))
            .map_or(space.origin(), |anchor| space.project_point(anchor));
        let mut measure = start(&space, anchor, context.grab_offset());
        let apply = Arc::clone(&apply);
        let context = context.clone();
        Box::new(move |pointer| {
            let (shown, committed) = measure(&pointer);
            Frame::new(
                context.render(&apply(&state, shown)),
                apply(&state, committed),
                0.0,
            )
        })
    })
}

fn tracked<S, V>(
    apply: impl Fn(&S, V) -> S + Send + Sync + 'static,
    start: impl Fn(&DragSpace, Vec3, Vec3, Snap) -> Measure<V> + Send + Sync + 'static,
) -> Tracked<S>
where
    S: Clone + Send + Sync + 'static,
    V: Copy + Send + Sync + 'static,
{
    let apply: Apply<S, V> = Arc::new(apply);
    let start = Arc::new(start);
    Tracked {
        build: Arc::new(move |snap| {
            let start = Arc::clone(&start);
            value_spec(
                Arc::clone(&apply),
                Arc::new(move |space, anchor, grab| start(space, anchor, grab, snap)),
            )
        }),
        snap: Snap::NONE,
    }
}

/// Drags the distance along the first axis of the space: the direction of an
/// [`Axis`](DragSpace::Axis) space. `apply` turns the distance travelled since the start of the
/// drag into a new state.
pub fn along_axis<S: Clone + Send + Sync + 'static>(
    apply: impl Fn(&S, f32) -> S + Send + Sync + 'static,
) -> Tracked<S> {
    tracked(apply, |space, anchor, _, snap| {
        let space = *space;
        let origin = space.local(anchor).x;
        Box::new(move |pointer| snap.apply(space.local(pointer.point).x - origin))
    })
}

/// Drags the angle, in radians, of the pointer around the normal of the space, measured from the
/// pivot at the origin of a [`Plane`](DragSpace::Plane) space. The angle keeps counting past half a turn, so
/// a drag may spin more than once. `apply` turns the angle turned since the start of the drag
/// into a new state.
pub fn around_axis<S: Clone + Send + Sync + 'static>(
    apply: impl Fn(&S, f32) -> S + Send + Sync + 'static,
) -> Tracked<S> {
    tracked(apply, |space, anchor, grab, snap| {
        let space = *space;
        let angle_of = move |world: Vec3| {
            let local = space.local(world);
            local.y.atan2(local.x)
        };
        // The angle is the one of the pointer itself, so the node may sit on the pivot, as the
        // ring of a rotate handle does.
        let mut last = angle_of(anchor + grab);
        let mut turned = 0.0;
        Box::new(move |pointer| {
            let angle = angle_of(pointer.point + grab);
            turned += (angle - last + PI).rem_euclid(TAU) - PI;
            last = angle;
            snap.apply(turned)
        })
    })
}

/// Drags the offset across the plane of the space, as a world vector in the plane. `apply` turns
/// the offset since the start of the drag into a new state. Steps apply to each of the two axes
/// of the plane separately.
pub fn across_plane<S: Clone + Send + Sync + 'static>(
    apply: impl Fn(&S, Vec3) -> S + Send + Sync + 'static,
) -> Tracked<S> {
    tracked(apply, |space, anchor, _, snap| {
        let space = *space;
        let start = space.local(anchor);
        let origin = space.from_local(Vec2::ZERO);
        Box::new(move |pointer| {
            let delta = space.local(pointer.point) - start;
            let (shown_x, snapped_x) = snap.apply(delta.x);
            let (shown_y, snapped_y) = snap.apply(delta.y);
            (
                space.from_local(Vec2::new(shown_x, shown_y)) - origin,
                space.from_local(Vec2::new(snapped_x, snapped_y)) - origin,
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use bevy::math::Dir3;

    use super::*;
    use crate::ops::{closest, state};
    use crate::view::{View, ViewNode};

    /// A node whose position, rotation and offset all come from the state.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Piece {
        x: f32,
        angle: f32,
        offset: Vec3,
    }

    fn view(piece: &Piece) -> View<Piece> {
        View::new().with(
            ViewNode::circle("item", 1.0)
                .at(Vec3::new(piece.x, 0.0, 0.0) + piece.offset)
                .with_rotation(bevy::math::Quat::from_rotation_z(piece.angle)),
        )
    }

    fn context(space: DragSpace, start: Piece) -> DragContext<Piece> {
        DragContext::new(Arc::new(view), "item".into(), start).with_space(space)
    }

    fn x_axis() -> DragSpace {
        DragSpace::axis(Vec3::ZERO, Dir3::X)
    }

    fn at(x: f32, y: f32) -> Pointer {
        Pointer::at(Vec3::new(x, y, 0.0))
    }

    #[test]
    fn no_snap_leaves_the_value_alone() {
        assert_eq!(Snap::NONE.apply(0.37), (0.37, 0.37));
        assert_eq!(Snap::step(0.0).apply(2.0), (2.0, 2.0));
    }

    #[test]
    fn a_value_far_from_a_step_is_shown_as_measured_and_commits_the_nearest_step() {
        let snap = Snap::step(1.0).within(0.2);
        let (shown, committed) = snap.apply(0.45);
        assert_eq!(shown, 0.45);
        assert_eq!(committed, 0.0);
        assert_eq!(snap.apply(0.55).1, 1.0);
        assert_eq!(snap.apply(-1.6).1, -2.0);
    }

    #[test]
    fn a_value_near_a_step_is_pulled_towards_it_smoothly() {
        let snap = Snap::step(1.0).within(0.2);
        let (at_edge, _) = snap.apply(0.2);
        assert!((at_edge - 0.2).abs() < 1e-6, "no pull at the edge");
        let (halfway, _) = snap.apply(0.1);
        assert!((halfway - 0.05).abs() < 1e-6, "smoothstep(0.5) is one half");
        let (on_step, _) = snap.apply(0.0);
        assert_eq!(on_step, 0.0);
        let pulled: Vec<f32> = [0.19, 0.15, 0.1, 0.05, 0.01]
            .iter()
            .map(|raw| snap.apply(*raw).0)
            .collect();
        assert!(
            pulled.windows(2).all(|pair| pair[1] < pair[0]),
            "{pulled:?}"
        );
        assert!((Snap::step(2.0).radius - 0.5).abs() < 1e-6);
        assert!((Snap::step(2.0).within(1.0).radius - 1.0).abs() < 1e-6);
    }

    #[test]
    fn along_axis_measures_the_distance_since_the_start() {
        let start = Piece::default();
        let spec: DragSpec<Piece> = along_axis(|piece: &Piece, distance| Piece {
            x: piece.x + distance,
            ..piece.clone()
        })
        .into();
        let mut behavior = spec.compile(&context(x_axis(), start));
        let frame = behavior(at(3.5, 7.0));
        assert!((frame.drop_state.x - 3.5).abs() < 1e-5);
        assert!((frame.view.world_position("item").unwrap().x - 3.5).abs() < 1e-5);
        assert_eq!(frame.gap, 0.0);
        let back = behavior(at(-2.0, 0.0));
        assert!((back.drop_state.x + 2.0).abs() < 1e-5);
    }

    #[test]
    fn along_axis_previews_the_pull_and_commits_the_step() {
        let spec: DragSpec<Piece> = along_axis(|piece: &Piece, distance| Piece {
            x: piece.x + distance,
            ..piece.clone()
        })
        .snap(Snap::step(0.5).within(0.1))
        .into();
        let mut behavior = spec.compile(&context(x_axis(), Piece::default()));
        let frame = behavior(at(1.53, 0.0));
        assert!((frame.drop_state.x - 1.5).abs() < 1e-5, "the drop is exact");
        let shown = frame.view.world_position("item").unwrap().x;
        assert!(
            shown > 1.5 && shown < 1.53,
            "the preview is pulled: {shown}"
        );
        let loose = behavior(at(1.7, 0.0));
        assert!((loose.view.world_position("item").unwrap().x - 1.7).abs() < 1e-5);
        assert!((loose.drop_state.x - 1.5).abs() < 1e-5);
    }

    #[test]
    fn around_axis_measures_the_angle_around_the_origin() {
        let start = Piece {
            x: 2.0,
            ..Piece::default()
        };
        let space = DragSpace::plane(Vec3::ZERO, Dir3::Z);
        let spec: DragSpec<Piece> = around_axis(|piece: &Piece, angle| Piece {
            angle: piece.angle + angle,
            ..piece.clone()
        })
        .snap(Snap::step(FRAC_PI_2).within(0.2))
        .into();
        let mut behavior = spec.compile(&context(space, start));
        // A pointer at 80 degrees around the origin settles on 90 degrees.
        let angle = 80_f32.to_radians();
        let frame = behavior(at(2.0 * angle.cos(), 2.0 * angle.sin()));
        assert!((frame.drop_state.angle - FRAC_PI_2).abs() < 1e-4);
        assert!((frame.view.world_position("item").unwrap().x - 2.0).abs() < 1e-4);
    }

    #[test]
    fn around_axis_keeps_counting_past_half_a_turn() {
        let space = DragSpace::plane(Vec3::ZERO, Dir3::Z);
        let spec: DragSpec<Piece> = around_axis(|_: &Piece, angle| Piece {
            angle,
            ..Piece::default()
        })
        .into();
        let start = Piece {
            x: 1.0,
            ..Piece::default()
        };
        let mut behavior = spec.compile(&context(space, start));
        let mut turned = 0.0;
        for degrees in (10..=370).step_by(20) {
            let angle = (degrees as f32).to_radians();
            turned = behavior(at(angle.cos(), angle.sin())).drop_state.angle;
        }
        assert!((turned - 370_f32.to_radians()).abs() < 1e-3, "{turned}");
        let back = behavior(at(1.0, -0.001));
        assert!(back.drop_state.angle > 6.0, "stays on the same turn");
    }

    #[test]
    fn across_plane_measures_an_offset_in_the_plane_and_snaps_each_axis() {
        let ground = DragSpace::plane(Vec3::ZERO, Dir3::Y);
        let spec: DragSpec<Piece> = across_plane(|piece: &Piece, offset: Vec3| Piece {
            offset: piece.offset + offset,
            ..piece.clone()
        })
        .snap(Snap::step(1.0).within(0.25))
        .into();
        let mut behavior = spec.compile(&context(ground, Piece::default()));
        let pointer = Pointer::at(Vec3::new(2.1, 0.0, 3.9));
        let frame = behavior(pointer);
        let committed = frame.drop_state.offset;
        assert!(
            committed.distance(Vec3::new(2.0, 0.0, 4.0)) < 1e-4,
            "{committed}"
        );
        assert!(committed.y.abs() < 1e-6, "the offset stays in the plane");
    }

    #[test]
    fn a_value_operator_wins_a_closest_against_a_state_with_a_gap() {
        let tracked: DragSpec<Piece> = along_axis(|piece: &Piece, distance| Piece {
            x: piece.x + distance,
            ..piece.clone()
        })
        .into();
        let home = Piece::default();
        let spec = closest([state(home.clone()).change_gap(|_| 3.0), tracked]);
        let frame = spec.compile(&context(x_axis(), home.clone()))(at(4.0, 0.0));
        assert!((frame.drop_state.x - 4.0).abs() < 1e-5);
        assert_ne!(frame.drop_state, home);
    }

    #[test]
    fn on_drop_replaces_the_committed_value() {
        let tracked: DragSpec<Piece> = along_axis(|piece: &Piece, distance| Piece {
            x: piece.x + distance,
            ..piece.clone()
        })
        .into();
        let replaced = Piece {
            x: 42.0,
            ..Piece::default()
        };
        let frame = tracked
            .on_drop(replaced.clone())
            .compile(&context(x_axis(), Piece::default()))(at(1.0, 0.0));
        assert_eq!(frame.drop_state, replaced);
        assert!((frame.view.world_position("item").unwrap().x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_value_operator_can_chain_into_a_new_drag() {
        let tracked: DragSpec<Piece> = along_axis(|piece: &Piece, distance| Piece {
            x: piece.x + distance,
            ..piece.clone()
        })
        .into();
        let frame = tracked
            .with_chaining()
            .compile(&context(x_axis(), Piece::default()))(at(2.0, 0.0));
        assert!(frame.chain.is_some());
    }

    #[test]
    fn a_missing_node_gives_the_value_without_a_preview_anchor() {
        let tracked: DragSpec<Piece> = along_axis(|piece: &Piece, distance| Piece {
            x: piece.x + distance,
            ..piece.clone()
        })
        .into();
        let context = context(x_axis(), Piece::default()).with_dragged("missing");
        let frame = tracked.compile(&context)(at(1.0, 0.0));
        assert!((frame.drop_state.x - 1.0).abs() < 1e-5);
    }
}
