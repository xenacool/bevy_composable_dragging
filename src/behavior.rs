//! The runtime contract between drag specs and the engine.

use std::sync::Arc;

use bevy::math::Vec2;

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

/// A running drag: maps the pointer position to the [`Frame`] to show.
///
/// The position is where the dragged node's origin should be, so the grab offset is already
/// removed. Behaviors may keep state between calls (for example to make `closest` sticky).
pub type DragBehavior<S> = Box<dyn FnMut(Vec2) -> Frame<S> + Send + Sync>;

/// What a [`DragSpec`](crate::DragSpec) can see when it is compiled into a [`DragBehavior`].
pub struct DragContext<S> {
    view_fn: ViewFn<S>,
    dragged: NodeId,
    start_state: S,
    grab_offset: Vec2,
}

impl<S> DragContext<S> {
    /// Creates the context for dragging the node `dragged` out of `start_state`.
    pub fn new(view_fn: ViewFn<S>, dragged: NodeId, start_state: S) -> Self {
        Self {
            view_fn,
            dragged,
            start_state,
            grab_offset: Vec2::ZERO,
        }
    }

    /// Sets the offset from the pointer to the dragged node's origin at the start of the drag.
    #[must_use]
    pub fn with_grab_offset(mut self, grab_offset: Vec2) -> Self {
        self.grab_offset = grab_offset;
        self
    }

    /// The offset, at the start of the drag, from the dragged node's origin to the pointer.
    /// Behaviors receive the position the node's origin should have; adding this offset gives
    /// the pointer itself.
    pub fn grab_offset(&self) -> Vec2 {
        self.grab_offset
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
    pub fn anchor_in(&self, view: &View<S>) -> Option<Vec2> {
        view.position(&self.dragged)
    }
}
