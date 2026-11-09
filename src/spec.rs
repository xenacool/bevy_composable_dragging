//! Drag specs: declarative descriptions of how a node can be dragged.

use std::fmt;

use crate::behavior::{DragBehavior, DragContext};
use crate::owned::owned_fn;

owned_fn! {
    /// Compiles a spec into the behavior of one drag.
    CompileFn<S>, CompileCall, (context: &DragContext<S>) -> DragBehavior<S>
}

/// A declarative description of a drag, compiled into a [`DragBehavior`] when a drag starts.
///
/// Specs are built with the operator functions in [`ops`](crate::ops) and combined with the
/// modifier methods on this type. Because a spec is just a compile function, new operators are
/// ordinary functions that return a `DragSpec`.
pub struct DragSpec<S>(CompileFn<S>);

impl<S> Clone for DragSpec<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
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
        compile: impl Fn(&DragContext<S>) -> DragBehavior<S> + Clone + Send + Sync + 'static,
    ) -> Self {
        Self(CompileFn::new(compile))
    }

    /// Compiles the spec into the behavior of one drag.
    pub fn compile(&self, context: &DragContext<S>) -> DragBehavior<S> {
        self.0.call(context)
    }
}
