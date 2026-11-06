//! The `on_drop` modifiers.

use std::sync::Arc;

use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Keeps the preview but makes dropping switch to `state` instead.
    #[must_use]
    pub fn on_drop(self, state: S) -> Self {
        self.on_drop_with(move |_| state.clone())
    }

    /// Keeps the preview but makes dropping switch to the state computed from the preview's
    /// drop state, for example to round a free position to a grid.
    #[must_use]
    pub fn on_drop_with(self, transform: impl Fn(&S) -> S + Send + Sync + 'static) -> Self {
        let transform = Arc::new(transform);
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let transform = Arc::clone(&transform);
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                frame.drop_state = transform(&frame.drop_state);
                frame
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
    fn replaces_the_drop_state_but_keeps_the_preview() {
        let mut behavior = state(Slot(1)).on_drop(Slot(2)).compile(&context());
        let frame = behavior(Vec2::new(10.0, 0.0));
        assert_eq!(frame.drop_state, Slot(2));
        assert!((frame.view.position("item").unwrap().x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn computes_the_drop_state_from_the_preview() {
        let spec = closest_states([Slot(0), Slot(1)]).on_drop_with(|slot| Slot(slot.0 + 1));
        let mut behavior = spec.compile(&context());
        assert_eq!(behavior(Vec2::new(10.0, 0.0)).drop_state, Slot(2));
        assert_eq!(behavior(Vec2::new(0.0, 0.0)).drop_state, Slot(1));
    }
}
