//! The `react_to` operator.

use crate::spec::DragSpec;

/// Calls `getter` on every frame and compiles the spec that `build` returns for its value. The
/// spec is compiled again, with its memory reset, whenever the value changes.
///
/// Use it to let a drag follow something outside the state, for example a modifier key or a
/// setting, without restarting the drag.
pub fn react_to<S, V>(
    getter: impl Fn() -> V + Clone + Send + Sync + 'static,
    build: impl Fn(&V) -> DragSpec<S> + Clone + Send + Sync + 'static,
) -> DragSpec<S>
where
    S: Clone + Send + Sync + 'static,
    V: PartialEq + Send + Sync + 'static,
{
    DragSpec::new(move |context| {
        let context = context.clone();
        let getter = getter.clone();
        let build = build.clone();
        let mut current = None;
        Box::new(move |pointer| {
            let value = getter();
            let (_, behavior) = match current.take() {
                Some((seen, behavior)) if seen == value => current.insert((seen, behavior)),
                _ => {
                    let behavior = build(&value).compile(&context);
                    current.insert((value, behavior))
                }
            };
            behavior(pointer)
        })
    })
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bevy::math::Vec2;

    use super::*;
    use crate::ops::state;
    use crate::ops::testing::{Slot, context};

    #[test]
    fn follows_the_value_of_the_getter() {
        static TARGET: AtomicUsize = AtomicUsize::new(1);
        let spec = react_to(|| TARGET.load(Ordering::SeqCst), |slot| state(Slot(*slot)));
        let mut behavior = spec.compile(&context());
        assert_eq!(behavior(Pointer::from(Vec2::ZERO)).drop_state, Slot(1));
        TARGET.store(2, Ordering::SeqCst);
        assert_eq!(behavior(Pointer::from(Vec2::ZERO)).drop_state, Slot(2));
    }

    #[test]
    fn rebuilds_only_when_the_value_changes() {
        static TARGET: AtomicUsize = AtomicUsize::new(0);
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let spec = react_to(
            || TARGET.load(Ordering::SeqCst),
            |slot| {
                BUILDS.fetch_add(1, Ordering::SeqCst);
                state(Slot(*slot))
            },
        );
        let mut behavior = spec.compile(&context());
        for _ in 0..3 {
            behavior(Pointer::from(Vec2::ZERO));
        }
        assert_eq!(BUILDS.load(Ordering::SeqCst), 1);
        TARGET.store(1, Ordering::SeqCst);
        behavior(Pointer::from(Vec2::ZERO));
        behavior(Pointer::from(Vec2::ZERO));
        assert_eq!(BUILDS.load(Ordering::SeqCst), 2);
    }
}
