//! Drag specs: declarative descriptions of how a node can be dragged.

use std::fmt;
use std::sync::Arc;

use crate::behavior::{DragBehavior, DragContext};

/// Compiles a spec into the behavior of one drag.
type CompileFn<S> = dyn Fn(&DragContext<S>) -> DragBehavior<S> + Send + Sync;

/// A declarative description of a drag, compiled into a [`DragBehavior`] when a drag starts.
///
/// Specs are built with the operator functions in [`ops`](crate::ops) and combined with the
/// modifier methods on this type. Because a spec is just a compile function, new operators are
/// ordinary functions that return a `DragSpec`.
pub struct DragSpec<S>(Arc<CompileFn<S>>);

impl<S> Clone for DragSpec<S> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<S> fmt::Debug for DragSpec<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DragSpec(..)")
    }
}

impl<S> DragSpec<S> {
    /// Creates a spec from a function that compiles it into a behavior.
    pub fn new(
        compile: impl Fn(&DragContext<S>) -> DragBehavior<S> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(compile))
    }

    /// Compiles the spec into the behavior of one drag.
    pub fn compile(&self, context: &DragContext<S>) -> DragBehavior<S> {
        (self.0)(context)
    }
}
