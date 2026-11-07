//! The `closest` operator.

use bevy::math::Vec3;

use crate::behavior::{DragBehavior, DragContext, Frame};
use crate::ops::state;
use crate::space::Pointer;
use crate::spec::DragSpec;

/// Options of [`closest_with_options`]. The defaults match [`closest`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ClosestOptions {
    /// The branch that is currently shown is kept until another branch is nearer to the pointer
    /// by more than this many world units.
    pub stickiness: f32,
    /// Once the shown branch has carried the dragged node more than this many world units from
    /// where it started, `closest` locks onto that branch and ignores the others until the node
    /// is back within this distance. Use it for branches that all start from the current state,
    /// such as one [`between`](crate::ops::between) per target, so a drag heading towards one
    /// target cannot jump partway onto another. `None` never locks.
    pub lock_past: Option<f32>,
}

impl ClosestOptions {
    /// The defaults: no stickiness and no locking.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`stickiness`](Self::stickiness).
    #[must_use]
    pub fn with_stickiness(mut self, stickiness: f32) -> Self {
        self.stickiness = stickiness;
        self
    }

    /// Sets [`lock_past`](Self::lock_past).
    #[must_use]
    pub fn with_lock_past(mut self, distance: f32) -> Self {
        self.lock_past = Some(distance);
        self
    }
}

/// Shows the branch whose preview puts the dragged node nearest to the pointer.
///
/// Every branch is a full spec, so discrete choices can be combined with continuous ones. The
/// preview snaps from branch to branch; use [`DragSpec::with_floating`] to let the dragged node
/// follow the pointer while the rest snaps.
pub fn closest<S: Clone + Send + Sync + 'static>(
    branches: impl IntoIterator<Item = DragSpec<S>>,
) -> DragSpec<S> {
    closest_with_stickiness(branches, 0.0)
}

/// Like [`closest`], but the branch that is currently shown is kept until another branch is
/// nearer to the pointer by more than `stickiness` world units, which avoids flicker at the
/// boundary between two branches.
pub fn closest_with_stickiness<S: Clone + Send + Sync + 'static>(
    branches: impl IntoIterator<Item = DragSpec<S>>,
    stickiness: f32,
) -> DragSpec<S> {
    closest_with_options(branches, ClosestOptions::new().with_stickiness(stickiness))
}

/// Like [`closest`], with the stickiness and branch locking of [`ClosestOptions`].
pub fn closest_with_options<S: Clone + Send + Sync + 'static>(
    branches: impl IntoIterator<Item = DragSpec<S>>,
    options: ClosestOptions,
) -> DragSpec<S> {
    let branches: Vec<DragSpec<S>> = branches.into_iter().collect();
    DragSpec::new(move |context| {
        let mut behaviors: Vec<_> = branches
            .iter()
            .map(|branch| branch.compile(context))
            .collect();
        if behaviors.is_empty() {
            let start = context.start_state().clone();
            let view = context.render(&start);
            return Box::new(move |_| Frame::new(view.clone(), start.clone(), f32::INFINITY));
        }
        let lock = options.lock_past.and_then(|distance| {
            let home = context.anchor_in(&context.render(context.start_state()))?;
            Some(Lock {
                context: context.clone(),
                home,
                distance,
            })
        });
        let mut current = None;
        let mut locked: Option<usize> = None;
        Box::new(move |pointer| {
            let mut frames: Vec<Option<Frame<S>>> = behaviors.iter().map(|_| None).collect();
            let is_past = |frame: &Frame<S>| lock.as_ref().is_some_and(|lock| lock.is_past(frame));
            if let Some(index) = locked
                && !is_past(run(&mut frames, &mut behaviors, index, pointer))
            {
                locked = None;
            }
            let chosen = if let Some(index) = locked {
                index
            } else {
                let gaps: Vec<f32> = (0..behaviors.len())
                    .map(|index| run(&mut frames, &mut behaviors, index, pointer).gap)
                    .collect();
                let chosen = select_branch(&gaps, current, options.stickiness);
                if is_past(run(&mut frames, &mut behaviors, chosen, pointer)) {
                    locked = Some(chosen);
                }
                chosen
            };
            current = Some(chosen);
            run(&mut frames, &mut behaviors, chosen, pointer);
            frames
                .swap_remove(chosen)
                .expect("the chosen branch ran")
                .in_branch(chosen)
        })
    })
}

/// Where the dragged node started, for [`ClosestOptions::lock_past`].
struct Lock<S> {
    context: DragContext<S>,
    home: Vec3,
    distance: f32,
}

impl<S> Lock<S> {
    /// Whether `frame` has carried the dragged node out of the lock radius. A preview without the
    /// node counts as far away.
    fn is_past(&self, frame: &Frame<S>) -> bool {
        self.context
            .anchor_in(&frame.view)
            .is_none_or(|anchor| anchor.distance(self.home) > self.distance)
    }
}

/// The frame of branch `index` for `pointer`, running the branch only the first time.
fn run<'a, S>(
    frames: &'a mut [Option<Frame<S>>],
    behaviors: &mut [DragBehavior<S>],
    index: usize,
    pointer: Pointer,
) -> &'a Frame<S> {
    frames[index].get_or_insert_with(|| behaviors[index](pointer))
}

/// Like [`closest`] for plain states: each state is a [`state`] branch.
pub fn closest_states<S: Clone + Send + Sync + 'static>(
    states: impl IntoIterator<Item = S>,
) -> DragSpec<S> {
    closest(states.into_iter().map(state))
}

/// Picks the index of the branch to show: the one with the smallest gap, where the `current`
/// branch counts `stickiness` smaller. Ties go to the first branch.
pub(crate) fn select_branch(gaps: &[f32], current: Option<usize>, stickiness: f32) -> usize {
    gaps.iter()
        .enumerate()
        .map(|(index, gap)| {
            gap - if Some(index) == current {
                stickiness
            } else {
                0.0
            }
        })
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map_or(0, |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use bevy::math::Vec2;

    use super::*;
    use crate::ops::between;
    use crate::ops::testing::{Slot, context};

    /// One branch that blends between slots 0 and 1 and one that jumps to slot 2.
    fn locking(distance: f32) -> DragSpec<Slot> {
        closest_with_options(
            [between([Slot(0), Slot(1)]), state(Slot(2))],
            ClosestOptions::new().with_lock_past(distance),
        )
    }

    #[test]
    fn lock_past_keeps_the_branch_that_carried_the_node_away() {
        let mut behavior = locking(5.0).compile(&context());
        assert_eq!(
            behavior(Pointer::from(Vec2::new(11.0, 0.0))).drop_state,
            Slot(1)
        );
        // Slot 2 is nearer now, but the first branch holds the node far from home.
        assert_eq!(
            behavior(Pointer::from(Vec2::new(19.0, 0.0))).drop_state,
            Slot(1)
        );
    }

    #[test]
    fn lock_past_releases_when_the_node_returns_home() {
        let mut behavior = locking(5.0).compile(&context());
        behavior(Pointer::from(Vec2::new(11.0, 0.0)));
        assert_eq!(
            behavior(Pointer::from(Vec2::new(3.0, 0.0))).drop_state,
            Slot(0)
        );
        assert_eq!(
            behavior(Pointer::from(Vec2::new(19.0, 0.0))).drop_state,
            Slot(2)
        );
    }

    #[test]
    fn without_lock_past_the_nearest_branch_always_wins() {
        let spec = closest([between([Slot(0), Slot(1)]), state(Slot(2))]);
        let mut behavior = spec.compile(&context());
        behavior(Pointer::from(Vec2::new(11.0, 0.0)));
        assert_eq!(
            behavior(Pointer::from(Vec2::new(19.0, 0.0))).drop_state,
            Slot(2)
        );
    }

    /// A dot at a point of the plane.
    #[derive(Clone, Debug, PartialEq)]
    struct Dot(f32, f32);

    fn dot_context() -> DragContext<Dot> {
        use crate::view::{View, ViewNode};
        DragContext::new(
            std::sync::Arc::new(|dot: &Dot| {
                View::new().with(ViewNode::circle("dot", 5.0).at_xy(Vec2::new(dot.0, dot.1)))
            }),
            "dot".into(),
            Dot(0.0, 0.0),
        )
    }

    /// Two spokes out of the start state, to the right and down; this is the setup of the
    /// `closest lockPast` tests in Dragology.
    fn spokes(options: ClosestOptions) -> DragSpec<Dot> {
        closest_with_options(
            [
                between([Dot(0.0, 0.0), Dot(100.0, 0.0)]),
                between([Dot(0.0, 0.0), Dot(0.0, 100.0)]),
            ],
            options,
        )
    }

    #[test]
    fn without_lock_past_a_drag_jumps_between_spokes() {
        let mut behavior = spokes(ClosestOptions::new()).compile(&dot_context());
        let right = behavior(Pointer::from(Vec2::new(60.0, 5.0))).branch;
        let down = behavior(Pointer::from(Vec2::new(40.0, 60.0))).branch;
        assert_ne!(right, down);
    }

    #[test]
    fn with_lock_past_a_drag_stays_on_its_spoke_until_the_dot_returns_home() {
        let mut behavior =
            spokes(ClosestOptions::new().with_lock_past(10.0)).compile(&dot_context());
        let right = behavior(Pointer::from(Vec2::new(60.0, 5.0))).branch;
        // Closer to the other spoke, but still out on this one.
        assert_eq!(behavior(Pointer::from(Vec2::new(40.0, 60.0))).branch, right);
        // Projects back onto the start of this spoke, so it unlocks.
        assert_ne!(
            behavior(Pointer::from(Vec2::new(-10.0, 60.0))).branch,
            right
        );
    }

    #[test]
    fn lock_past_is_free_to_switch_within_its_distance_of_home() {
        let mut behavior =
            spokes(ClosestOptions::new().with_lock_past(10.0)).compile(&dot_context());
        let right = behavior(Pointer::from(Vec2::new(6.0, 1.0))).branch;
        let down = behavior(Pointer::from(Vec2::new(1.0, 6.0))).branch;
        assert_ne!(right, down);
    }

    #[test]
    fn the_options_default_to_plain_closest() {
        let options = ClosestOptions::new();
        assert_eq!(options.stickiness, 0.0);
        assert_eq!(options.lock_past, None);
    }

    #[test]
    fn snaps_to_the_state_nearest_the_pointer() {
        let mut behavior = closest_states([Slot(0), Slot(1), Slot(2)]).compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(13.0, 4.0)));
        assert_eq!(frame.drop_state.0, 1);
        assert!((frame.view.world_position("item").unwrap().x - 10.0).abs() < 1e-4);
        assert_eq!(
            behavior(Pointer::from(Vec2::new(19.0, 0.0))).drop_state.0,
            2
        );
    }

    #[test]
    fn floating_keeps_the_item_under_the_pointer_and_previews_the_rest() {
        let mut behavior = closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(13.0, 4.0)));
        assert_eq!(frame.drop_state.0, 1);
        let item = frame.view.world_position("item").unwrap();
        assert!(item.truncate().distance(Vec2::new(13.0, 4.0)) < 1e-4);
        assert!((frame.view.world_position("other").unwrap().y - 51.0).abs() < 1e-4);
    }

    #[test]
    fn sticky_closest_resists_small_moves_past_the_boundary() {
        let mut behavior =
            closest_with_stickiness([state(Slot(0)), state(Slot(1))], 4.0).compile(&context());
        assert_eq!(behavior(Pointer::from(Vec2::new(2.0, 0.0))).drop_state.0, 0);
        // Slightly nearer to slot 1, but not by more than the stickiness.
        assert_eq!(behavior(Pointer::from(Vec2::new(6.0, 0.0))).drop_state.0, 0);
        assert_eq!(behavior(Pointer::from(Vec2::new(9.0, 0.0))).drop_state.0, 1);
    }

    #[test]
    fn no_branches_shows_the_start_state() {
        let mut behavior = closest::<Slot>([]).compile(&context());
        let frame = behavior(Pointer::from(Vec2::ZERO));
        assert_eq!(frame.drop_state.0, 0);
        assert!(frame.gap.is_infinite());
    }

    #[test]
    fn picks_the_smallest_gap() {
        assert_eq!(select_branch(&[5.0, 2.0, 9.0], None, 0.0), 1);
    }

    #[test]
    fn ties_go_to_the_first_branch() {
        assert_eq!(select_branch(&[3.0, 3.0], None, 0.0), 0);
    }

    #[test]
    fn missing_nodes_with_infinite_gaps_lose() {
        assert_eq!(select_branch(&[f32::INFINITY, 4.0], None, 0.0), 1);
    }

    #[test]
    fn stickiness_keeps_the_current_branch_until_beaten_by_the_margin() {
        let gaps = [10.0, 7.0];
        assert_eq!(select_branch(&gaps, Some(0), 2.0), 1);
        assert_eq!(select_branch(&gaps, Some(0), 3.0), 0);
        assert_eq!(select_branch(&gaps, Some(0), 5.0), 0);
        assert_eq!(select_branch(&gaps, Some(1), 5.0), 1);
    }

    #[test]
    fn without_stickiness_the_nearest_always_wins() {
        assert_eq!(select_branch(&[10.0, 7.0], Some(0), 0.0), 1);
    }
}
