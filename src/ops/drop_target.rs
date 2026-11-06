//! The `drop_target` operator.

use crate::behavior::Frame;
use crate::spec::DragSpec;
use crate::view::NodeId;

/// Drops into `state` when the pointer is over the node `target`, for example a trash bin.
///
/// The gap is zero while the pointer is inside the target and infinite outside, so inside
/// [`closest`](super::closest) the target wins whenever the pointer is over it. The target's
/// bounds are taken from the state the drag started in, which keeps them stable even when
/// dropping removes or moves the target.
pub fn drop_target<S: Clone + Send + Sync + 'static>(
    target: impl Into<NodeId>,
    state: S,
) -> DragSpec<S> {
    let target: NodeId = target.into();
    DragSpec::new(move |context| {
        let bounds = context.render(context.start_state());
        let view = context.render(&state);
        let state = state.clone();
        let target = target.clone();
        let grab_offset = context.grab_offset();
        Box::new(move |anchor| {
            let inside = bounds.contains_point(&target, anchor + grab_offset);
            let gap = if inside { 0.0 } else { f32::INFINITY };
            Frame::new(view.clone(), state.clone(), gap)
        })
    })
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::ops::testing::{Slot, context};
    use crate::ops::{closest, state};
    #[test]
    fn the_gap_is_zero_inside_the_target_and_infinite_outside() {
        let mut behavior = drop_target("bin", Slot(2)).compile(&context());
        let inside = behavior(Vec2::new(105.0, 5.0));
        assert_eq!(inside.gap, 0.0);
        assert_eq!(inside.drop_state, Slot(2));
        assert!(behavior(Vec2::new(50.0, 0.0)).gap.is_infinite());
    }

    #[test]
    fn the_pointer_not_the_node_origin_is_tested() {
        // The node was grabbed 10 units left of its origin, so the pointer is 10 to the right
        // of the position the behavior receives.
        let context = context().with_grab_offset(Vec2::new(10.0, 0.0));
        let mut behavior = drop_target("bin", Slot(2)).compile(&context);
        assert_eq!(behavior(Vec2::new(95.0, 0.0)).gap, 0.0);
        assert!(behavior(Vec2::new(101.0, 0.0)).gap.is_infinite());
    }

    #[test]
    fn wins_a_closest_when_the_pointer_is_over_it() {
        let spec = closest([drop_target("bin", Slot(2)), state(Slot(1))]);
        let mut behavior = spec.compile(&context());
        assert_eq!(behavior(Vec2::new(100.0, 0.0)).drop_state, Slot(2));
        assert_eq!(behavior(Vec2::new(12.0, 0.0)).drop_state, Slot(1));
    }
}
