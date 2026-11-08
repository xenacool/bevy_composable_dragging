//! The `button` operator.

use crate::behavior::{DragContext, Frame};
use crate::spec::DragSpec;

/// A button: releasing it switches to `target`, unless the pointer has moved further than
/// `radius` from where it pressed, which cancels the press.
///
/// The preview never leaves the state the press started from, so nothing around the button
/// moves while it is held, however much the pointer jitters. Without any movement at all, a
/// click, it behaves the same.
pub fn button<S: Clone + Send + Sync + 'static>(target: S, radius: f32) -> DragSpec<S> {
    DragSpec::new(move |context: &DragContext<S>| {
        let start = context.start_state().clone();
        let view = context.render(&start);
        let anchor = context
            .anchor_in(&view)
            .map(|anchor| context.space().project_point(anchor));
        let target = target.clone();
        Box::new(move |pointer| {
            let moved = anchor.map_or(0.0, |anchor| anchor.distance(pointer.point));
            let drop_state = if moved <= radius {
                target.clone()
            } else {
                start.clone()
            };
            Frame::new(view.clone(), drop_state, moved)
        })
    })
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::ops::testing::{Slot, context};
    use crate::space::Pointer;

    #[test]
    fn releasing_on_the_button_switches_and_the_preview_stays() {
        let mut behavior = button(Slot(2), 5.0).compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(1.0, 1.0)));
        assert_eq!(frame.drop_state, Slot(2));
        assert_eq!(frame.view.world_position("item").unwrap().x, 0.0);
        assert_eq!(frame.view.world_position("other").unwrap().y, 50.0);
    }

    #[test]
    fn releasing_far_from_the_press_cancels() {
        let mut behavior = button(Slot(2), 5.0).compile(&context());
        assert_eq!(
            behavior(Pointer::from(Vec2::new(40.0, 0.0))).drop_state,
            Slot(0)
        );
        assert_eq!(
            behavior(Pointer::from(Vec2::new(3.0, 0.0))).drop_state,
            Slot(2)
        );
    }
}
