//! The `substate` operator.

use crate::behavior::{Chaining, DragContext, Frame, ViewFn};
use crate::owned::owned_fn;
use crate::spec::DragSpec;
use crate::view::NodeId;

owned_fn! {
    /// Reads the part of a state that a [`Lens`] focuses on.
    Get<S, T>, GetCall, (state: &S) -> T
}

owned_fn! {
    /// Puts a part back into a state for a [`Lens`].
    Set<S, T>, SetCall, (state: &S, part: T) -> S
}

/// A focus on a part of type `T` of a state of type `S`: how to read the part and how to put a
/// new one back.
pub struct Lens<S, T> {
    get: Get<S, T>,
    set: Set<S, T>,
}

impl<S, T> Clone for Lens<S, T> {
    fn clone(&self) -> Self {
        Self {
            get: self.get.clone(),
            set: self.set.clone(),
        }
    }
}

impl<S, T> Lens<S, T> {
    /// Creates a lens from a getter and a setter. `set` returns a copy of the state with the
    /// part replaced.
    pub fn new(
        get: impl Fn(&S) -> T + Clone + Send + Sync + 'static,
        set: impl Fn(&S, T) -> S + Clone + Send + Sync + 'static,
    ) -> Self {
        Self {
            get: Get::new(get),
            set: Set::new(set),
        }
    }

    /// Reads the part from `state`.
    pub fn get(&self, state: &S) -> T {
        self.get.call(state)
    }

    /// Returns a copy of `state` with the part replaced by `part`.
    pub fn set(&self, state: &S, part: T) -> S {
        self.set.call(state, part)
    }
}

/// Runs `spec`, a drag spec for a part of the state, on the part that `lens` focuses on. The
/// spec sees and changes only the part; the rest of the state stays as it was when the drag
/// started.
///
/// Because a spec for the part cannot drive nodes of the whole, the preview of a substate drag
/// shows the whole scene with non-draggable nodes (see [`View::rebind`](crate::View::rebind)).
/// That matters only while the drag is running, since the dropped state is rendered by the
/// view function as usual.
///
/// ```
/// use bevy_composable_dragging::ops::{Lens, closest_states, substate};
/// use bevy_composable_dragging::{DragSpec, ViewNode};
///
/// #[derive(Clone)]
/// struct Board {
///     cursor: usize,
///     label: usize,
/// }
///
/// // The spec for the cursor only sees, and only changes, a `usize`.
/// let cursor = Lens::new(
///     |board: &Board| board.cursor,
///     |board: &Board, cursor| Board { cursor, ..board.clone() },
/// );
/// let spec: DragSpec<Board> = substate(cursor, closest_states([0, 1, 2]))
///     .with_overlay(|_| vec![ViewNode::circle("halo", 2.0)]);
/// ```
#[must_use]
pub fn substate<S, T>(lens: Lens<S, T>, spec: DragSpec<T>) -> DragSpec<S>
where
    S: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    DragSpec::new(move |context: &DragContext<S>| {
        let whole = context.start_state().clone();
        let part_view = {
            let view_fn = context.view_fn();
            let lens = lens.clone();
            let whole = whole.clone();
            ViewFn::new(move |part: &T| view_fn.call(&lens.set(&whole, part.clone())).rebind())
        };
        let part_context = DragContext::with_view_fn(
            part_view,
            NodeId::from(context.dragged().to_owned()),
            lens.get(&whole),
        )
        .with_grab_offset(context.grab_offset());
        let mut inner = spec.compile(&part_context);
        let lens = lens.clone();
        Box::new(move |pointer| lift(&lens, &whole, inner(pointer)))
    })
}

/// The frame of the whole state for a frame of its part.
fn lift<S, T>(lens: &Lens<S, T>, whole: &S, frame: Frame<T>) -> Frame<S>
where
    S: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    let mut lifted = Frame::new(
        frame.view.rebind(),
        lens.set(whole, frame.drop_state),
        frame.gap,
    );
    lifted.branch = frame.branch;
    lifted.branch_transition = frame.branch_transition;
    lifted.drop_transition = frame.drop_transition;
    lifted.chain = frame.chain.map(|chain| Chaining {
        dragged: chain.dragged,
        follow: chain.follow.map(|spec| substate(lens.clone(), spec)),
        transition: chain.transition,
    });
    lifted
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;

    use bevy::math::Vec2;

    use super::*;
    use crate::ops::{closest_states, state};
    use crate::view::{View, ViewNode};

    /// The dragged item sits at `left`; a twin sits at `right`.
    #[derive(Clone, Debug, PartialEq)]
    struct Two {
        left: usize,
        right: usize,
    }

    fn two_view(two: &Two) -> View<Two> {
        View::new()
            .with(ViewNode::circle("item", 1.0).at_xy(Vec2::new(two.left as f32 * 10.0, 0.0)))
            .with(ViewNode::circle("twin", 1.0).at_xy(Vec2::new(two.right as f32 * 10.0, 50.0)))
    }

    fn two_context() -> DragContext<Two> {
        DragContext::new(two_view, "item".into(), Two { left: 0, right: 2 })
    }

    fn left() -> Lens<Two, usize> {
        Lens::new(
            |two: &Two| two.left,
            |two: &Two, left| Two {
                left,
                ..two.clone()
            },
        )
    }

    #[test]
    fn the_part_changes_and_the_rest_stays() {
        let spec = substate(left(), closest_states([0, 1, 2]));
        let mut behavior = spec.compile(&two_context());
        let frame = behavior(Pointer::from(Vec2::new(12.0, 0.0)));
        assert_eq!(frame.drop_state, Two { left: 1, right: 2 });
        assert!((frame.view.world_position("item").unwrap().x - 10.0).abs() < 1e-4);
        assert!((frame.view.world_position("twin").unwrap().x - 20.0).abs() < 1e-4);
        assert!(frame.gap < 3.0);
    }

    #[test]
    fn the_preview_nodes_are_not_draggable() {
        let spec = substate(left(), closest_states([0, 1]));
        let frame = spec.compile(&two_context())(Pointer::from(Vec2::ZERO));
        assert!(frame.view.nodes().iter().all(|node| node.drag.is_none()));
    }

    #[test]
    fn a_chained_follow_spec_is_lifted_to_the_whole_state() {
        let follow = state(1usize);
        let spec = substate(
            left(),
            closest_states([0usize, 1])
                .with_chaining_options(Chaining::new().dragging("twin").following(follow)),
        );
        let frame = spec.compile(&two_context())(Pointer::from(Vec2::new(10.0, 0.0)));
        let chain = frame.chain.expect("the drop state left the start state");
        assert_eq!(chain.dragged.as_deref(), Some("twin"));
        let mut follow = chain
            .follow
            .expect("the follow spec is kept")
            .compile(&two_context());
        assert_eq!(
            follow(Pointer::from(Vec2::ZERO)).drop_state,
            Two { left: 1, right: 2 }
        );
    }

    #[test]
    fn frame_options_survive_the_lens() {
        let spec = substate(
            left(),
            closest_states([0usize, 1])
                .with_branch_transition(0.5)
                .with_drop_transition(0.25),
        );
        let frame = spec.compile(&two_context())(Pointer::from(Vec2::ZERO));
        assert_eq!(frame.branch_transition, Some(0.5));
        assert_eq!(frame.drop_transition, Some(0.25));
    }
}
