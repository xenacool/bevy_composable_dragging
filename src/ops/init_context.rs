//! The `with_init_context` modifier and the `custom` operator.

use crate::behavior::{DragBehavior, DragContext};
use crate::spec::DragSpec;

/// Defines a drag from scratch: `compile` turns the [`DragContext`] into the behavior of one
/// drag. This is the escape hatch for behaviors the other operators cannot express, and the
/// counterpart of Dragology's `custom`; it is the same as [`DragSpec::new`].
pub fn custom<S>(
    compile: impl Fn(&DragContext<S>) -> DragBehavior<S> + Send + Sync + 'static,
) -> DragSpec<S> {
    DragSpec::new(compile)
}

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Transforms the [`DragContext`] before this spec compiles, for example to drag another
    /// node with [`DragContext::with_dragged`] or to start from another state with
    /// [`DragContext::with_start_state`].
    #[must_use]
    pub fn with_init_context(
        self,
        change: impl Fn(&DragContext<S>) -> DragContext<S> + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |context| self.compile(&change(context)))
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::behavior::Frame;
    use crate::ops::state;
    use crate::ops::testing::{Slot, context};

    #[test]
    fn custom_builds_a_behavior_from_scratch() {
        let spec = custom(|context| {
            let view = context.render(&Slot(2));
            Box::new(move |_| Frame::new(view.clone(), Slot(2), 0.0))
        });
        let frame = spec.compile(&context())(Vec2::ZERO);
        assert_eq!(frame.drop_state, Slot(2));
        assert!((frame.view.position("item").unwrap().x - 20.0).abs() < 1e-4);
    }

    #[test]
    fn the_inner_spec_sees_the_changed_context() {
        let spec =
            state(Slot(1)).with_init_context(|context| context.clone().with_dragged("other"));
        let frame = spec.compile(&context())(Vec2::new(0.0, 51.0));
        // "other" sits at (0, 51) in slot 1, so the gap is measured from it instead of "item".
        assert!(frame.gap < 1e-4);
    }

    #[test]
    fn the_start_state_can_be_replaced() {
        let probe = custom(|context: &DragContext<Slot>| {
            let start = context.start_state().clone();
            let view = context.render(&start);
            Box::new(move |_| Frame::new(view.clone(), start.clone(), 0.0))
        })
        .with_init_context(|context| context.clone().with_start_state(Slot(2)));
        assert_eq!(probe.compile(&context())(Vec2::ZERO).drop_state, Slot(2));
    }

    // The two tests below are ported from Dragology's `userOps.test.tsx`: an operator and a
    // modifier written as a library user would, with only the public API.

    /// Jumps to whichever state puts the dragged node nearest the pointer.
    fn nearest(states: Vec<Slot>) -> DragSpec<Slot> {
        custom(move |context| {
            let candidates: Vec<(Slot, Vec2)> = states
                .iter()
                .filter_map(|slot| {
                    let view = context.render(slot);
                    Some((slot.clone(), context.anchor_in(&view)?))
                })
                .collect();
            let context = context.clone();
            Box::new(move |pointer| {
                let (slot, anchor) = candidates
                    .iter()
                    .min_by(|(_, a), (_, b)| a.distance(pointer).total_cmp(&b.distance(pointer)))
                    .expect("at least one state");
                Frame::new(context.render(slot), slot.clone(), anchor.distance(pointer))
            })
        })
    }

    #[test]
    fn runs_a_user_defined_operator() {
        let frame = nearest(vec![Slot(0), Slot(1)]).compile(&context())(Vec2::new(7.0, 0.0));
        assert_eq!(frame.drop_state, Slot(1));
    }

    #[test]
    fn runs_a_user_defined_modifier_around_it() {
        let shifted =
            nearest(vec![Slot(0), Slot(1)]).change_frame(|pointer| pointer + Vec2::new(3.0, 0.0));
        // 4 is nearer slot 0, but shifted to 7 it is nearer slot 1.
        assert_eq!(
            nearest(vec![Slot(0), Slot(1)]).compile(&context())(Vec2::new(4.0, 0.0)).drop_state,
            Slot(0)
        );
        assert_eq!(
            shifted.compile(&context())(Vec2::new(4.0, 0.0)).drop_state,
            Slot(1)
        );
    }
}
