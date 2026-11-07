//! The `during` modifier.

use std::sync::Arc;

use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Transforms the drop state on every frame and shows the transformed state as the preview.
    ///
    /// This is [`on_drop_with`](Self::on_drop_with) that also changes what the user sees. The gap
    /// is measured again from the dragged node of the transformed preview. A node that is missing
    /// from the transformed preview gives an infinite gap.
    #[must_use]
    pub fn during(self, transform: impl Fn(&S) -> S + Send + Sync + 'static) -> Self {
        let transform = Arc::new(transform);
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let context = context.clone();
            let transform = Arc::clone(&transform);
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                let transformed = transform(&frame.drop_state);
                frame.view = context.render(&transformed);
                frame.gap = context.gap_in(&frame.view, &pointer);
                frame.drop_state = transformed;
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
    fn transforms_the_drop_state_and_renders_it_as_the_preview() {
        let spec = closest_states([Slot(0), Slot(1)]).during(|slot| Slot(slot.0 + 1));
        let mut behavior = spec.compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(0.0, 0.0)));
        assert_eq!(frame.drop_state, Slot(1));
        assert!((frame.view.world_position("item").unwrap().x - 10.0).abs() < 1e-4);
        assert!((frame.view.world_position("other").unwrap().y - 51.0).abs() < 1e-4);
    }

    #[test]
    fn measures_the_gap_from_the_transformed_preview() {
        let spec = closest_states([Slot(0)]).during(|_| Slot(1));
        let mut behavior = spec.compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(10.0, 3.0)));
        assert!((frame.gap - 3.0).abs() < 1e-4);
    }
}
