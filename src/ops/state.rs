//! The `state` operator.

use crate::behavior::Frame;
use crate::spec::DragSpec;

/// A single target: the preview always shows `target`, and dropping switches to it.
///
/// This is the leaf operator that the others combine.
pub fn state<S: Clone + Send + Sync + 'static>(target: S) -> DragSpec<S> {
    DragSpec::new(move |context| {
        let view = context.render(&target);
        let anchor = context.anchor_in(&view);
        let target = target.clone();
        Box::new(move |pointer| {
            Frame::new(
                view.clone(),
                target.clone(),
                anchor.map_or(f32::INFINITY, |anchor| anchor.distance(pointer)),
            )
        })
    })
}
