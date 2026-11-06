//! Declarative, composable dragging for Bevy.
//!
//! The crate is a port of Dragology (*Declarative Dragging*). Describe the interface as a pure
//! function from state to a [`View`] and attach [`DragSpec`]s to the nodes that can be dragged.
//! The engine renders the target states speculatively, finds the dragged node by id, and
//! synthesizes the preview while dragging and the drop state on release.

mod behavior;
pub mod constraint;
#[cfg(feature = "pbr")]
mod input;
mod lerp;
mod model;
pub mod ops;
mod param;
#[cfg(feature = "pbr")]
mod plugin;
#[cfg(feature = "pbr")]
mod render;
#[cfg(feature = "pbr")]
mod shapes;
mod solver;
mod spec;
mod transition;
mod view;

pub use behavior::{DragBehavior, DragContext, Frame, ViewFn};
pub use constraint::Constraint;
#[cfg(feature = "pbr")]
pub use input::DragPlane;
pub use lerp::{blend_views, lerp_transform, lerp_views};
pub use model::DragModel;
pub use param::Param;
#[cfg(feature = "pbr")]
pub use plugin::DraggablePlugin;
pub use spec::DragSpec;
pub use view::{DragFactory, NodeId, ShapeKey, View, ViewNode};
