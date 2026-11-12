//! User-defined modifiers: wrappers around a spec.

use crate::behavior::{DragBehavior, DragContext};
use crate::spec::DragSpec;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Wraps the behavior of this spec in a behavior of your own. This is the extension point
    /// for new modifiers, the counterpart of Dragology's `modifier`: every built-in modifier
    /// such as [`during`](Self::during) or [`when_far`](Self::when_far) is a wrapper of this
    /// shape.
    ///
    /// `wrapper` runs once per drag with the context and the compiled behavior of this spec, and
    /// returns the behavior that replaces it. It may keep state between frames, call the inner
    /// behavior any number of times, change the pointer it passes in and change the frame it
    /// gets back. A wrapper that needs neither the context nor state is simpler as
    /// [`change_frame`](Self::change_frame) or [`change_result`](Self::change_result).
    ///
    /// ```
    /// use bevy_composable_dragging::DragSpec;
    ///
    /// /// Lets the wrapped spec see a pointer that trails the real one: each frame it covers the
    /// /// fraction `follow` (0 to 1) of the way from where it was to where the pointer is.
    /// fn smooth_pointer<S: Clone + Send + Sync + 'static>(
    ///     spec: DragSpec<S>,
    ///     follow: f32,
    /// ) -> DragSpec<S> {
    ///     spec.wrap(move |_context, mut inner| {
    ///         let mut trailing = None;
    ///         Box::new(move |pointer| {
    ///             let from = trailing.unwrap_or(pointer.point);
    ///             let point = from.lerp(pointer.point, follow.clamp(0.0, 1.0));
    ///             trailing = Some(point);
    ///             inner(pointer.moved_to(point))
    ///         })
    ///     })
    /// }
    /// # let _ = smooth_pointer::<u32>;
    /// ```
    #[must_use]
    pub fn wrap(
        self,
        wrapper: impl Fn(&DragContext<S>, DragBehavior<S>) -> DragBehavior<S>
        + Clone
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::new(move |context| wrapper(context, self.compile(context)))
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use crate::ops::closest_states;
    use crate::ops::testing::{Slot, context};
    use crate::space::Pointer;

    #[test]
    fn keeps_state_between_frames() {
        let spec = closest_states([Slot(0), Slot(1)]).wrap(|_, mut inner| {
            let mut frames = 0_u32;
            Box::new(move |pointer| {
                frames += 1;
                let mut frame = inner(pointer);
                frame.branch = u64::from(frames);
                frame
            })
        });
        let mut behavior = spec.compile(&context());
        let first = behavior(Pointer::from(Vec2::ZERO));
        let second = behavior(Pointer::from(Vec2::ZERO));
        assert_eq!((first.branch, second.branch), (1, 2));
    }

    #[test]
    fn each_drag_gets_its_own_state() {
        let spec = closest_states([Slot(0)]).wrap(|_, mut inner| {
            let mut frames = 0_u32;
            Box::new(move |pointer| {
                frames += 1;
                let mut frame = inner(pointer);
                frame.branch = u64::from(frames);
                frame
            })
        });
        let mut first = spec.compile(&context());
        first(Pointer::from(Vec2::ZERO));
        let second = spec.compile(&context())(Pointer::from(Vec2::ZERO));
        assert_eq!(second.branch, 1);
    }

    #[test]
    fn sees_the_context_of_the_drag() {
        let spec = closest_states([Slot(0)]).wrap(|context, mut inner| {
            let dragged = context.dragged().len();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                frame.branch = dragged as u64;
                frame
            })
        });
        let frame = spec.compile(&context())(Pointer::from(Vec2::ZERO));
        assert_eq!(frame.branch, "item".len() as u64);
    }

    #[test]
    fn can_trail_the_pointer() {
        let spec = closest_states([Slot(0), Slot(1)]).wrap(|_, mut inner| {
            let mut trailing = None;
            Box::new(move |pointer| {
                let from = trailing.unwrap_or(pointer.point);
                let point = from.lerp(pointer.point, 0.5);
                trailing = Some(point);
                inner(pointer.moved_to(point))
            })
        });
        let mut behavior = spec.compile(&context());
        assert_eq!(
            behavior(Pointer::from(Vec2::ZERO)).drop_state,
            Slot(0),
            "starts at the pointer"
        );
        // The pointer jumps to x = 8, which is nearest slot 1 at x = 10, but the wrapped spec
        // sees x = 4 first and x = 6 on the next frame.
        assert_eq!(
            behavior(Pointer::from(Vec2::new(8.0, 0.0))).drop_state,
            Slot(0),
            "4 is nearer slot 0"
        );
        assert_eq!(
            behavior(Pointer::from(Vec2::new(8.0, 0.0))).drop_state,
            Slot(1),
            "6 is nearer slot 1"
        );
    }
}
