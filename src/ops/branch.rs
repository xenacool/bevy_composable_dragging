//! The `with_branch_transition` modifier.

use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Sets how long, in seconds, the preview takes to blend when the drag switches between
    /// branches, for example between the alternatives of a [`closest`](crate::ops::closest) or
    /// between the foreground and background of [`when_far`](Self::when_far). Zero switches
    /// instantly. Without this, switches use the model's transition duration.
    #[must_use]
    pub fn with_branch_transition(self, seconds: f32) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                frame.branch_transition = Some(seconds);
                frame
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use bevy::math::Vec2;

    use crate::ops::closest_states;
    use crate::ops::testing::{Slot, context};
    #[test]
    fn frames_carry_the_requested_duration_and_a_branch_per_alternative() {
        let spec = closest_states([Slot(0), Slot(1)]).with_branch_transition(0.5);
        let mut behavior = spec.compile(&context());
        let left = behavior(Pointer::from(Vec2::new(0.0, 0.0)));
        let right = behavior(Pointer::from(Vec2::new(10.0, 0.0)));
        assert_eq!(left.branch_transition, Some(0.5));
        assert_ne!(left.branch, right.branch);
        assert_eq!(
            behavior(Pointer::from(Vec2::new(1.0, 0.0))).branch,
            left.branch
        );
    }
}
