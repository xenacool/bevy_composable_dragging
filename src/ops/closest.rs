//! The `closest` operator.

use crate::behavior::Frame;
use crate::ops::state;
use crate::spec::DragSpec;

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
        let mut current = None;
        Box::new(move |pointer| {
            let mut frames: Vec<Frame<S>> = behaviors
                .iter_mut()
                .map(|behavior| behavior(pointer))
                .collect();
            let gaps: Vec<f32> = frames.iter().map(|frame| frame.gap).collect();
            let chosen = select_branch(&gaps, current, stickiness);
            current = Some(chosen);
            frames.swap_remove(chosen).in_branch(chosen)
        })
    })
}

/// Like [`closest`] for plain states: each state is a [`state`] branch.
pub fn closest_states<S: Clone + Send + Sync + 'static>(
    states: impl IntoIterator<Item = S>,
) -> DragSpec<S> {
    closest(states.into_iter().map(state))
}

/// Picks the index of the branch to show: the one with the smallest gap, unless the `current`
/// branch is within `stickiness` of it.
pub(crate) fn select_branch(gaps: &[f32], current: Option<usize>, stickiness: f32) -> usize {
    let nearest = gaps
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map_or(0, |(index, _)| index);
    match current {
        Some(current) if gaps[current] <= gaps[nearest] + stickiness => current,
        _ => nearest,
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::ops::testing::{Slot, context};

    #[test]
    fn snaps_to_the_state_nearest_the_pointer() {
        let mut behavior = closest_states([Slot(0), Slot(1), Slot(2)]).compile(&context());
        let frame = behavior(Vec2::new(13.0, 4.0));
        assert_eq!(frame.drop_state.0, 1);
        assert!((frame.view.position("item").unwrap().x - 10.0).abs() < 1e-4);
        assert_eq!(behavior(Vec2::new(19.0, 0.0)).drop_state.0, 2);
    }

    #[test]
    fn floating_keeps_the_item_under_the_pointer_and_previews_the_rest() {
        let mut behavior = closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .compile(&context());
        let frame = behavior(Vec2::new(13.0, 4.0));
        assert_eq!(frame.drop_state.0, 1);
        let item = frame.view.position("item").unwrap();
        assert!(item.distance(Vec2::new(13.0, 4.0)) < 1e-4);
        assert!((frame.view.position("other").unwrap().y - 51.0).abs() < 1e-4);
    }

    #[test]
    fn sticky_closest_resists_small_moves_past_the_boundary() {
        let mut behavior =
            closest_with_stickiness([state(Slot(0)), state(Slot(1))], 4.0).compile(&context());
        assert_eq!(behavior(Vec2::new(2.0, 0.0)).drop_state.0, 0);
        // Slightly nearer to slot 1, but not by more than the stickiness.
        assert_eq!(behavior(Vec2::new(6.0, 0.0)).drop_state.0, 0);
        assert_eq!(behavior(Vec2::new(9.0, 0.0)).drop_state.0, 1);
    }

    #[test]
    fn no_branches_shows_the_start_state() {
        let mut behavior = closest::<Slot>([]).compile(&context());
        let frame = behavior(Vec2::ZERO);
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
