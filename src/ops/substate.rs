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
/// The preview shows the whole scene with all nodes retaining their drag behavior. The dragged
/// node moves according to the inner spec, while sibling nodes remain draggable.
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
        let whole_view_fn = context.view_fn();
        let dragged = NodeId::from(context.dragged().to_owned());
        let part_view = {
            let view_fn = context.view_fn();
            let lens = lens.clone();
            let whole = whole.clone();
            ViewFn::new(move |part: &T| view_fn.call(&lens.set(&whole, part.clone())).rebind())
        };
        let part_context = DragContext::with_view_fn(part_view, dragged.clone(), lens.get(&whole))
            .with_grab_offset(context.grab_offset())
            .with_space(*context.space());
        let mut inner = spec.compile(&part_context);
        let lens = lens.clone();
        Box::new(move |pointer| lift(&lens, &whole, &whole_view_fn, &dragged, inner(pointer)))
    })
}

/// The frame of the whole state for a frame of its part.
///
/// Instead of rebinding the inner view (which would strip all drag factories), we render the
/// full view from the updated state and merge the dragged node's transform from the inner view.
/// This keeps all sibling nodes draggable - a zipper over the view's nodes.
fn lift<S, T>(
    lens: &Lens<S, T>,
    whole: &S,
    view_fn: &ViewFn<S>,
    dragged: &NodeId,
    frame: Frame<T>,
) -> Frame<S>
where
    S: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    let drop_state = lens.set(whole, frame.drop_state);
    // Render the full view with all drag factories intact.
    let mut full_view = view_fn.call(&drop_state);
    // Merge the dragged node's transform from the inner view.
    if let Some(inner_transform) = frame.view.world_transform(dragged) {
        full_view.set_world_transform(dragged, inner_transform);
    }
    let mut lifted = Frame::new(full_view, drop_state, frame.gap);
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

    /// Like [`two_view`], but every node carries a drag factory so the zipper can preserve them.
    fn two_view_draggable(two: &Two) -> View<Two> {
        View::new()
            .with(
                ViewNode::circle("item", 1.0)
                    .at_xy(Vec2::new(two.left as f32 * 10.0, 0.0))
                    .draggable(move || {
                        substate(
                            Lens::new(
                                |t: &Two| t.left,
                                |t: &Two, v| Two {
                                    left: v,
                                    ..t.clone()
                                },
                            ),
                            closest_states([0usize, 1, 2]),
                        )
                    }),
            )
            .with(
                ViewNode::circle("twin", 1.0)
                    .at_xy(Vec2::new(two.right as f32 * 10.0, 50.0))
                    .draggable(move || {
                        substate(
                            Lens::new(
                                |t: &Two| t.right,
                                |t: &Two, v| Two {
                                    right: v,
                                    ..t.clone()
                                },
                            ),
                            closest_states([0usize, 1, 2]),
                        )
                    }),
            )
    }

    fn two_context() -> DragContext<Two> {
        DragContext::new(two_view, "item".into(), Two { left: 0, right: 2 })
    }

    fn two_context_draggable() -> DragContext<Two> {
        DragContext::new(two_view_draggable, "item".into(), Two { left: 0, right: 2 })
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
    fn the_preview_keeps_sibling_nodes_draggable() {
        let spec = substate(left(), closest_states([0, 1]));
        let frame = spec.compile(&two_context_draggable())(Pointer::from(Vec2::ZERO));
        // The "twin" node is not dragged by the substate spec, so it keeps its drag factory
        // from the view function. The dragged "item" node also retains its drag factory.
        let item = frame.view.get("item").expect("item node exists");
        let twin = frame.view.get("twin").expect("twin node exists");
        // Both nodes retain their drag factories from the full view.
        assert!(
            item.drag.is_some(),
            "dragged node should keep its drag factory"
        );
        assert!(
            twin.drag.is_some(),
            "sibling node should keep its drag factory"
        );
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

    /// Verifies that `substate` propagates the parent's `DragSpace` to the inner spec.
    ///
    /// Without `.with_space()`, the inner spec would default to the XY world plane, so a drag
    /// on the XZ ground plane would move in the wrong direction.
    #[test]
    fn substate_propagates_drag_space() {
        use crate::behavior::DragContext;
        use crate::ops::VaryOptions;
        use crate::ops::vary_func;
        use crate::space::{DragSpace, Pointer};
        use bevy::math::{Dir3, Vec3};

        #[derive(Clone)]
        struct GroundPoint {
            x: f32,
            z: f32,
        }

        fn ground_view(state: &GroundPoint) -> View<GroundPoint> {
            let x = state.x;
            View::new().with(
                ViewNode::circle("dot", 1.0)
                    .at(Vec3::new(state.x, 0.0, state.z))
                    .draggable(move || {
                        substate(
                            Lens::new(
                                |s: &GroundPoint| s.x,
                                |s: &GroundPoint, x| GroundPoint { x, ..s.clone() },
                            ),
                            vary_func([x], |v| v[0], VaryOptions::new().with_scale(10.0)),
                        )
                    }),
            )
        }

        let xz_plane = DragSpace::plane(Vec3::ZERO, Dir3::Y);
        let context = DragContext::new(ground_view, "dot".into(), GroundPoint { x: 0.0, z: 0.0 })
            .with_space(xz_plane);

        let spec = ground_view(&GroundPoint { x: 0.0, z: 0.0 })
            .get("dot")
            .and_then(|node| node.drag_spec(&GroundPoint { x: 0.0, z: 0.0 }))
            .unwrap();

        let mut behavior = spec.compile(&context);
        let pointer = Pointer::at(Vec3::new(5.0, 0.0, 0.0));
        let frame = behavior(pointer);

        assert!(
            (frame.drop_state.x - 5.0).abs() < 1e-2,
            "substate inner spec should follow pointer on XZ plane: got {}, expected 5.0",
            frame.drop_state.x
        );
    }
}
