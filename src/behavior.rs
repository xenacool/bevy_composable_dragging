//! The runtime contract between drag specs and the engine.

use std::sync::Arc;

use bevy::math::Vec3;

use crate::space::{DragSpace, Pointer};
use crate::spec::DragSpec;
use crate::view::{NodeId, View};

/// Builds the [`View`] of a state. This is the pure `state -> view` function of an interface.
pub type ViewFn<S> = Arc<dyn Fn(&S) -> View<S> + Send + Sync>;

/// What a drag shows for one pointer position.
///
/// Build frames with [`Frame::new`] so custom operators keep working when fields are added.
#[non_exhaustive]
pub struct Frame<S> {
    /// The preview shown while dragging.
    pub view: View<S>,
    /// The state the interface switches to if the drag ends now.
    pub drop_state: S,
    /// How far the dragged node is from the pointer in `view`. Operators such as `closest` use it
    /// to compare alternatives; it is infinite when the dragged node is missing from `view`.
    pub gap: f32,
    /// Identifies which branch of the spec tree produced the frame. When it changes between
    /// frames the preview blends from the old frame to the new one (see
    /// [`with_branch_transition`](crate::DragSpec::with_branch_transition)).
    pub branch: u64,
    /// How long, in seconds, to blend when `branch` changes. `None` uses the model's default.
    pub branch_transition: Option<f32>,
    /// How long, in seconds, the animation after a drop takes. `None` uses the model's default
    /// (see [`with_drop_transition`](crate::DragSpec::with_drop_transition)).
    pub drop_transition: Option<f32>,
    /// Asks the engine to continue the drag from `drop_state` right now (see
    /// [`with_chaining`](crate::DragSpec::with_chaining)).
    pub chain: Option<Chaining<S>>,
}

/// How a drag continues from the state it has just switched to.
///
/// Build it with [`Chaining::new`] and refine it with the `with_*` methods.
#[non_exhaustive]
pub struct Chaining<S> {
    /// The node to keep dragging. `None` keeps the node that is being dragged.
    pub dragged: Option<NodeId>,
    /// The spec to continue with. `None` uses the spec attached to the dragged node in the new
    /// state.
    pub follow: Option<DragSpec<S>>,
    /// How long, in seconds, the preview blends into the continued drag. `None` uses the model's
    /// default; zero switches instantly.
    pub transition: Option<f32>,
}

impl<S> Clone for Chaining<S> {
    fn clone(&self) -> Self {
        Self {
            dragged: self.dragged.clone(),
            follow: self.follow.clone(),
            transition: self.transition,
        }
    }
}

impl<S> Default for Chaining<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Chaining<S> {
    /// Continues with the same node and the spec attached to it in the new state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dragged: None,
            follow: None,
            transition: None,
        }
    }

    /// Continues the drag with the node `dragged` of the new state.
    #[must_use]
    pub fn dragging(mut self, dragged: impl Into<NodeId>) -> Self {
        self.dragged = Some(dragged.into());
        self
    }

    /// Continues the drag with `spec` instead of the spec attached to the node.
    #[must_use]
    pub fn following(mut self, spec: DragSpec<S>) -> Self {
        self.follow = Some(spec);
        self
    }

    /// Sets how long, in seconds, the preview blends into the continued drag.
    #[must_use]
    pub fn with_transition(mut self, seconds: f32) -> Self {
        self.transition = Some(seconds);
        self
    }
}

impl<S> Frame<S> {
    /// A frame on the default branch.
    pub fn new(view: View<S>, drop_state: S, gap: f32) -> Self {
        Self {
            view,
            drop_state,
            gap,
            branch: 0,
            branch_transition: None,
            drop_transition: None,
            chain: None,
        }
    }

    /// Records that this frame came from branch `index` of an operator, nested inside whatever
    /// branch it already came from.
    #[must_use]
    pub fn in_branch(mut self, index: usize) -> Self {
        self.branch = self
            .branch
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(index as u64 + 1);
        self
    }
}

/// A running drag: maps the [`Pointer`] to the [`Frame`] to show.
///
/// The point of the pointer is where the dragged node's origin should be, so the grab offset is
/// already removed. Behaviors may keep state between calls (for example to make `closest`
/// sticky).
pub type DragBehavior<S> = Box<dyn FnMut(Pointer) -> Frame<S> + Send + Sync>;

/// What a [`DragSpec`](crate::DragSpec) can see when it is compiled into a [`DragBehavior`].
pub struct DragContext<S> {
    view_fn: ViewFn<S>,
    dragged: NodeId,
    start_state: S,
    grab_offset: Vec3,
    space: DragSpace,
}

impl<S: Clone> Clone for DragContext<S> {
    fn clone(&self) -> Self {
        Self {
            view_fn: Arc::clone(&self.view_fn),
            dragged: self.dragged.clone(),
            start_state: self.start_state.clone(),
            grab_offset: self.grab_offset,
            space: self.space,
        }
    }
}

impl<S> DragContext<S> {
    /// Creates the context for dragging the node `dragged` out of `start_state`.
    pub fn new(view_fn: ViewFn<S>, dragged: NodeId, start_state: S) -> Self {
        Self {
            view_fn,
            dragged,
            start_state,
            grab_offset: Vec3::ZERO,
            space: DragSpace::default(),
        }
    }

    /// Sets the offset from the pointer to the dragged node's origin at the start of the drag.
    #[must_use]
    pub fn with_grab_offset(mut self, grab_offset: Vec3) -> Self {
        self.grab_offset = grab_offset;
        self
    }

    /// Sets the space the drag happens in.
    #[must_use]
    pub fn with_space(mut self, space: DragSpace) -> Self {
        self.space = space;
        self
    }

    /// Sets the id of the node being dragged.
    #[must_use]
    pub fn with_dragged(mut self, dragged: impl Into<NodeId>) -> Self {
        self.dragged = dragged.into();
        self
    }

    /// Sets the state the drag started from.
    #[must_use]
    pub fn with_start_state(mut self, start_state: S) -> Self {
        self.start_state = start_state;
        self
    }

    /// The offset, at the start of the drag, from the dragged node's origin to the pointer.
    /// Behaviors receive the position the node's origin should have; adding this offset gives
    /// the pointer itself.
    pub fn grab_offset(&self) -> Vec3 {
        self.grab_offset
    }

    /// The space the drag happens in. Pointers are world points in it, and gaps are measured
    /// after projecting positions onto it.
    pub fn space(&self) -> &DragSpace {
        &self.space
    }

    /// Speculatively renders `state`. This only builds plain data.
    pub fn render(&self, state: &S) -> View<S> {
        (self.view_fn)(state)
    }

    /// The view function, for operators that evaluate many states while a drag runs.
    pub fn view_fn(&self) -> ViewFn<S> {
        Arc::clone(&self.view_fn)
    }

    /// The id of the node being dragged.
    pub fn dragged(&self) -> &str {
        &self.dragged
    }

    /// The state the drag started from.
    pub fn start_state(&self) -> &S {
        &self.start_state
    }

    /// The world position of the dragged node in `view`, if the view contains it.
    pub fn anchor_in(&self, view: &View<S>) -> Option<Vec3> {
        view.world_position(&self.dragged)
    }

    /// The distance between the pointer and `anchor` projected onto the space of the drag.
    pub fn gap_to(&self, anchor: Vec3, pointer: &Pointer) -> f32 {
        self.space.project_point(anchor).distance(pointer.point)
    }

    /// How far the dragged node is from the pointer in `view`: [`gap_to`](Self::gap_to) its
    /// position, and infinite when the view does not contain the node.
    pub fn gap_in(&self, view: &View<S>, pointer: &Pointer) -> f32 {
        self.anchor_in(view)
            .map_or(f32::INFINITY, |anchor| self.gap_to(anchor, pointer))
    }
}
