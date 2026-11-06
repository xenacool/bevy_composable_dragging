//! The `when_far` modifier.

use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Switches to `background` while the dragged node is more than `gap` world units away from
    /// where this spec would put it, for example from the nearest drop slot of a
    /// [`closest`](crate::ops::closest). This gives "discrete islands in a continuous sea":
    /// snap when close, behave differently when far.
    ///
    /// The drag starts in the foreground.
    #[must_use]
    pub fn when_far(self, background: Self, gap: f32) -> Self {
        self.when_far_hysteresis(background, gap, gap)
    }

    /// Like [`when_far`](Self::when_far) with separate thresholds: the drag leaves the
    /// foreground beyond `gap_out` and returns to it within `gap_in`, which avoids flicker at the
    /// boundary.
    ///
    /// # Panics
    ///
    /// Panics when `gap_in > gap_out`, because the behavior would oscillate.
    #[must_use]
    pub fn when_far_hysteresis(self, background: Self, gap_in: f32, gap_out: f32) -> Self {
        assert!(
            gap_in <= gap_out,
            "when_far: gap_in ({gap_in}) must not exceed gap_out ({gap_out})"
        );
        Self::new(move |context| {
            let mut foreground = self.compile(context);
            let mut background = background.compile(context);
            let mut in_foreground = true;
            Box::new(move |pointer| {
                let frame = foreground(pointer);
                let threshold = if in_foreground { gap_out } else { gap_in };
                in_foreground = frame.gap <= threshold;
                if in_foreground {
                    frame.in_branch(0)
                } else {
                    background(pointer).in_branch(1)
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use crate::ops::testing::{Slot, context};
    use crate::ops::{closest_states, state};
    #[test]
    fn switches_to_the_background_when_far_from_the_foreground() {
        let spec = closest_states([Slot(0), Slot(1)]).when_far(state(Slot(2)), 5.0);
        let mut behavior = spec.compile(&context());
        let near = behavior(Vec2::new(2.0, 0.0));
        assert_eq!(near.drop_state, Slot(0));
        let far = behavior(Vec2::new(10.0, 40.0));
        assert_eq!(far.drop_state, Slot(2));
        assert_ne!(near.branch, far.branch);
    }

    #[test]
    fn hysteresis_prevents_flicker_at_the_boundary() {
        let spec = closest_states([Slot(0)]).when_far_hysteresis(state(Slot(2)), 3.0, 6.0);
        let mut behavior = spec.compile(&context());
        let drop = |behavior: &mut crate::DragBehavior<Slot>, y: f32| {
            behavior(Vec2::new(0.0, y)).drop_state.0
        };
        assert_eq!(drop(&mut behavior, 4.0), 0, "starts in the foreground");
        assert_eq!(drop(&mut behavior, 7.0), 2, "beyond gap_out");
        assert_eq!(
            drop(&mut behavior, 4.0),
            2,
            "between the thresholds it stays"
        );
        assert_eq!(drop(&mut behavior, 2.0), 0, "within gap_in it returns");
    }

    #[test]
    #[should_panic(expected = "gap_in")]
    fn inverted_thresholds_are_rejected() {
        let _ = state(Slot(0)).when_far_hysteresis(state(Slot(1)), 5.0, 2.0);
    }
}
