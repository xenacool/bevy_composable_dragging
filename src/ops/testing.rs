//! Shared fixtures for the operator tests.

use bevy::math::Vec2;

use crate::behavior::DragContext;
use crate::view::{View, ViewNode};

/// The slot of a single draggable item, at x = 0, 10 or 20.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Slot(pub(crate) usize);

/// An item at its slot, another node that depends on the slot, and a 20 x 20 bin at x = 100.
pub(crate) fn slots(slot: &Slot) -> View<Slot> {
    View::new()
        .with(ViewNode::circle("item", 1.0).at_xy(Vec2::new(slot.0 as f32 * 10.0, 0.0)))
        .with(ViewNode::circle("other", 1.0).at_xy(Vec2::new(0.0, 50.0 + slot.0 as f32)))
        .with(ViewNode::rectangle("bin", Vec2::new(20.0, 20.0)).at_xy(Vec2::new(100.0, 0.0)))
}

/// Dragging "item" out of slot 0.
pub(crate) fn context() -> DragContext<Slot> {
    DragContext::new(slots, "item".into(), Slot(0))
}
