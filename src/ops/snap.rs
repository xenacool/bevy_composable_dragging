//! The `with_snap_radius` modifier.

use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Snaps the preview to the drop state's own layout once the dragged node is within `radius`
    /// world units of where that layout places it. This only changes the preview, not the drop
    /// state, and is typically combined with [`with_floating`](Self::with_floating) so the
    /// floating node settles into place when it gets close.
    #[must_use]
    pub fn with_snap_radius(self, radius: f32) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let view_fn = context.view_fn();
            let dragged = context.dragged().to_owned();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                let settled = view_fn(&frame.drop_state);
                if let (Some(held), Some(home)) =
                    (frame.view.position(&dragged), settled.position(&dragged))
                    && held.distance(home) <= radius
                {
                    frame.view = settled;
                }
                frame
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use crate::ops::closest_states;
    use crate::ops::testing::{Slot, context};
    #[test]
    fn snaps_the_preview_inside_the_radius() {
        let spec = closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .with_snap_radius(2.0);
        let mut behavior = spec.compile(&context());
        let frame = behavior(Vec2::new(9.0, 1.0));
        assert_eq!(frame.drop_state, Slot(1));
        let item = frame.view.world_transform("item").unwrap().translation;
        assert!(item.truncate().distance(Vec2::new(10.0, 0.0)) < 1e-4);
        assert!(item.z.abs() < 1e-4, "the snapped item is not lifted");
    }

    #[test]
    fn keeps_floating_outside_the_radius() {
        let spec = closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .with_snap_radius(2.0);
        let mut behavior = spec.compile(&context());
        let frame = behavior(Vec2::new(7.0, 0.0));
        assert_eq!(frame.drop_state, Slot(1));
        assert!(
            frame
                .view
                .position("item")
                .unwrap()
                .distance(Vec2::new(7.0, 0.0))
                < 1e-4
        );
    }
}
