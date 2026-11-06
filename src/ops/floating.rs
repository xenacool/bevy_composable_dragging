//! The `with_floating` modifier.

use crate::spec::DragSpec;

/// How far a floating node is lifted towards the camera so it is drawn above the others.
const FLOAT_LIFT: f32 = 0.5;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Makes the dragged node float freely under the pointer while the rest of the preview
    /// shows what dropping would do. The dragged node is lifted by half a unit so it is drawn
    /// above its neighbors; the drop state is unchanged.
    #[must_use]
    pub fn with_floating(self) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let dragged = context.dragged().to_owned();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                frame.view.set_position(&dragged, pointer);
                if let Some(node) = frame.view.get_mut(&dragged) {
                    node.transform.translation.z += FLOAT_LIFT;
                }
                frame
            })
        })
    }
}
