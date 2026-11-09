//! The `change_gap`, `change_result` and `change_frame` modifiers.

use bevy::math::Vec3;

use crate::behavior::Frame;
use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Changes the gap this spec reports, for example to reweight one branch of a
    /// [`closest`](crate::ops::closest): a smaller gap makes the branch win from further away.
    #[must_use]
    pub fn change_gap(self, change: impl Fn(f32) -> f32 + Clone + Send + Sync + 'static) -> Self {
        self.change_result(move |mut frame| {
            frame.gap = change(frame.gap);
            frame
        })
    }

    /// Transforms the whole [`Frame`] this spec produces on every pointer position. This is the
    /// most general wrapper: it can change the preview, the drop state, the gap and the rest.
    #[must_use]
    pub fn change_result(
        self,
        change: impl Fn(Frame<S>) -> Frame<S> + Clone + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let change = change.clone();
            Box::new(move |pointer| change(inner(pointer)))
        })
    }

    /// Remaps the pointer position (a world point in the space of the drag) before it reaches
    /// this spec, for example to scale or offset the motion of the dragged node.
    #[must_use]
    pub fn change_frame(
        self,
        remap: impl Fn(Vec3) -> Vec3 + Clone + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let remap = remap.clone();
            Box::new(move |pointer| inner(pointer.moved_to(remap(pointer.point))))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use bevy::math::{Vec2, Vec3};

    use crate::ops::testing::{Slot, context};
    use crate::ops::{closest, state};

    #[test]
    fn change_gap_rescales_the_reported_gap() {
        let mut behavior = state(Slot(0))
            .change_gap(|gap| gap * 2.0)
            .compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(3.0, 4.0)));
        assert!((frame.gap - 10.0).abs() < 1e-4);
    }

    #[test]
    fn change_gap_reweights_a_branch_of_closest() {
        let spec = closest([state(Slot(0)), state(Slot(1)).change_gap(|gap| gap * 0.1)]);
        let mut behavior = spec.compile(&context());
        // Slot 0 is nearer, but slot 1's gap is shrunk tenfold.
        assert_eq!(
            behavior(Pointer::from(Vec2::new(4.0, 0.0))).drop_state,
            Slot(1)
        );
    }

    #[test]
    fn change_result_can_rewrite_the_frame() {
        let spec = state(Slot(0)).change_result(|mut frame| {
            frame.drop_state = Slot(9);
            frame.branch_transition = Some(0.25);
            frame
        });
        let frame = spec.compile(&context())(Pointer::from(Vec2::ZERO));
        assert_eq!(frame.drop_state, Slot(9));
        assert_eq!(frame.branch_transition, Some(0.25));
    }

    #[test]
    fn change_frame_remaps_the_pointer_for_the_inner_spec() {
        let spec = state(Slot(0)).change_frame(|point| point + Vec3::new(5.0, 0.0, 0.0));
        let frame = spec.compile(&context())(Pointer::from(Vec2::ZERO));
        assert!((frame.gap - 5.0).abs() < 1e-4);
    }
}
