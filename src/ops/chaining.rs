//! The `with_drop_transition` and `with_chaining` modifiers.

use crate::behavior::Chaining;
use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Sets how long, in seconds, the animation after a drop takes. Zero snaps instantly.
    /// Without this, drops use the model's transition duration (see
    /// [`DragModel::set_transition_duration`](crate::DragModel::set_transition_duration)).
    #[must_use]
    pub fn with_drop_transition(self, seconds: f32) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                frame.drop_transition = Some(seconds);
                frame
            })
        })
    }
}

impl<S: Clone + PartialEq + Send + Sync + 'static> DragSpec<S> {
    /// When the drop state differs from the state the drag started from, continues the drag
    /// right away from the new state, with the node's own spec in the new state. The state is
    /// committed on the spot, and the preview blends into the continued drag.
    ///
    /// This is how `between([state, ...neighbors]).with_chaining()` lets a single drag walk
    /// across many states. See [`with_chaining_options`](Self::with_chaining_options) to follow
    /// another node or spec.
    #[must_use]
    pub fn with_chaining(self) -> Self {
        self.with_chaining_options(Chaining::new())
    }

    /// Like [`with_chaining`](Self::with_chaining), with the node, spec and blend time of the
    /// continued drag chosen by `chaining`.
    #[must_use]
    pub fn with_chaining_options(self, chaining: Chaining<S>) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let start = context.start_state().clone();
            let chaining = chaining.clone();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                if frame.chain.is_none() && frame.drop_state != start {
                    frame.chain = Some(chaining.clone());
                }
                frame
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use crate::behavior::Chaining;
    use crate::ops::closest_states;
    use crate::ops::testing::{Slot, context};

    #[test]
    fn drop_transition_is_carried_on_the_frame() {
        let spec = closest_states([Slot(0)]).with_drop_transition(0.75);
        let frame = spec.compile(&context())(Vec2::ZERO);
        assert_eq!(frame.drop_transition, Some(0.75));
    }

    #[test]
    fn chains_only_when_the_drop_state_leaves_the_start_state() {
        let spec = closest_states([Slot(0), Slot(1)]).with_chaining();
        let mut behavior = spec.compile(&context());
        assert!(behavior(Vec2::new(0.0, 0.0)).chain.is_none());
        assert!(behavior(Vec2::new(10.0, 0.0)).chain.is_some());
    }

    #[test]
    fn chaining_options_are_passed_to_the_frame() {
        let spec = closest_states([Slot(1)])
            .with_chaining_options(Chaining::new().dragging("other").with_transition(0.0));
        let chain = spec.compile(&context())(Vec2::ZERO).chain.unwrap();
        assert_eq!(chain.dragged.as_deref(), Some("other"));
        assert_eq!(chain.transition, Some(0.0));
        assert!(chain.follow.is_none());
    }

    #[test]
    fn an_inner_chaining_request_wins() {
        let inner = closest_states([Slot(1)]).with_chaining_options(Chaining::new().dragging("a"));
        let spec = inner.with_chaining_options(Chaining::new().dragging("b"));
        let chain = spec.compile(&context())(Vec2::ZERO).chain.unwrap();
        assert_eq!(chain.dragged.as_deref(), Some("a"));
    }
}
