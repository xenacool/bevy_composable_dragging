//! The `with_overlay` modifier.

use crate::behavior::Frame;
use crate::spec::DragSpec;
use crate::view::{NodeId, ViewNode};

/// How far overlay nodes are lifted by default so they are drawn above everything, including a
/// floating node (see [`with_floating`](DragSpec::with_floating)).
const DEFAULT_LIFT: f32 = 1.0;

/// The prefix that keeps the ids of overlay nodes apart from the ids of the preview.
const OVERLAY_PREFIX: &str = "overlay-";

/// Options of [`with_overlay_options`](DragSpec::with_overlay_options). The defaults match
/// [`with_overlay`](DragSpec::with_overlay).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct OverlayOptions {
    /// How far overlay nodes are lifted along the Z axis of their parent, so they are drawn above
    /// the preview, including a floating node. Zero leaves them where the overlay put them.
    pub lift: f32,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self { lift: DEFAULT_LIFT }
    }
}

impl OverlayOptions {
    /// The defaults: a lift of one unit.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`lift`](Self::lift).
    #[must_use]
    pub fn with_lift(mut self, lift: f32) -> Self {
        self.lift = lift;
        self
    }
}

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Draws extra nodes on top of the preview while this spec is active, for example marks at
    /// the drop targets. `overlay` sees the frame the spec produced and returns the nodes to
    /// add.
    ///
    /// Overlay nodes are never draggable, are lifted above the rest of the view, and disappear
    /// on drop because the dropped state does not draw them. In a branch of a
    /// [`closest`](crate::ops::closest) they show only while that branch is the active one.
    ///
    /// Overlay ids live in their own namespace: the preview shows an overlay node `"mark"` as
    /// `"overlay-mark"`, so overlay ids can never collide with the ids of the view. A parent that
    /// names another node of the same overlay stays within the overlay; any other parent names a
    /// node of the preview, so an overlay can be attached to the dragged node.
    #[must_use]
    pub fn with_overlay(
        self,
        overlay: impl Fn(&Frame<S>) -> Vec<ViewNode<S>> + Clone + Send + Sync + 'static,
    ) -> Self {
        self.with_overlay_options(overlay, OverlayOptions::new())
    }

    /// Like [`with_overlay`](Self::with_overlay), with the lift of [`OverlayOptions`].
    #[must_use]
    pub fn with_overlay_options(
        self,
        overlay: impl Fn(&Frame<S>) -> Vec<ViewNode<S>> + Clone + Send + Sync + 'static,
        options: OverlayOptions,
    ) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let overlay = overlay.clone();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                let nodes = overlay(&frame);
                let ids: Vec<NodeId> = nodes.iter().map(|node| node.id.clone()).collect();
                for mut node in nodes {
                    node.drag = None;
                    node.transform.translation.z += options.lift;
                    node.id = prefixed(&node.id);
                    node.parent = node.parent.map(|parent| {
                        if ids.contains(&parent) {
                            prefixed(&parent)
                        } else {
                            parent
                        }
                    });
                    frame.view.push(node);
                }
                frame
            })
        })
    }
}

fn prefixed(id: &str) -> NodeId {
    format!("{OVERLAY_PREFIX}{id}").into()
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use bevy::math::Vec2;

    use crate::ops::testing::{Slot, context};
    use crate::ops::{closest, state};
    use crate::view::ViewNode;

    #[test]
    fn appends_non_draggable_nodes_above_the_preview() {
        let spec = state(Slot(1)).with_overlay(|frame| {
            vec![
                ViewNode::circle("mark", 2.0)
                    .at_xy(Vec2::new(frame.gap, 0.0))
                    .draggable(|| state(Slot(0))),
            ]
        });
        let frame = spec.compile(&context())(Pointer::from(Vec2::new(10.0, 0.0)));
        let mark = frame.view.get("overlay-mark").unwrap();
        assert!(mark.drag.is_none());
        assert!(mark.transform.translation.z > 0.5);
        assert_eq!(frame.view.len(), 4);
    }

    #[test]
    fn the_lift_can_be_changed() {
        let spec = state(Slot(1)).with_overlay_options(
            |_| vec![ViewNode::circle("mark", 2.0)],
            super::OverlayOptions::new().with_lift(4.0),
        );
        let frame = spec.compile(&context())(Pointer::from(Vec2::ZERO));
        let mark = frame.view.get("overlay-mark").unwrap();
        assert!((mark.transform.translation.z - 4.0).abs() < 1e-6);
    }

    #[test]
    fn overlay_ids_never_collide_with_the_preview() {
        let spec = state(Slot(1)).with_overlay(|_| vec![ViewNode::circle("item", 2.0)]);
        let frame = spec.compile(&context())(Pointer::from(Vec2::new(10.0, 0.0)));
        assert!(frame.view.get("overlay-item").is_some());
        assert_eq!(
            frame
                .view
                .nodes()
                .iter()
                .filter(|node| node.id == "item")
                .count(),
            1
        );
        assert!((frame.view.world_position("item").unwrap().x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn parents_stay_inside_the_overlay_or_name_the_preview() {
        let spec = state(Slot(1)).with_overlay(|_| {
            vec![
                ViewNode::circle("base", 2.0),
                ViewNode::circle("child", 1.0).with_parent("base"),
                ViewNode::circle("badge", 1.0).with_parent("item"),
            ]
        });
        let frame = spec.compile(&context())(Pointer::from(Vec2::ZERO));
        let parent = |id: &str| {
            frame
                .view
                .get(id)
                .unwrap()
                .parent
                .as_deref()
                .map(str::to_owned)
        };
        assert_eq!(parent("overlay-child").as_deref(), Some("overlay-base"));
        assert_eq!(parent("overlay-badge").as_deref(), Some("item"));
    }

    #[test]
    fn a_branch_shows_its_overlay_only_while_active() {
        let marked = |slot: usize, id: &'static str| {
            state(Slot(slot)).with_overlay(move |_| vec![ViewNode::circle(id, 1.0)])
        };
        let spec = closest([marked(0, "left"), marked(1, "right")]);
        let mut behavior = spec.compile(&context());
        let left = behavior(Pointer::from(Vec2::new(0.0, 0.0)));
        assert!(
            left.view.get("overlay-left").is_some() && left.view.get("overlay-right").is_none()
        );
        let right = behavior(Pointer::from(Vec2::new(10.0, 0.0)));
        assert!(
            right.view.get("overlay-right").is_some() && right.view.get("overlay-left").is_none()
        );
    }
}
