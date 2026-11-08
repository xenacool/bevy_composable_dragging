//! Declarative, composable dragging for Bevy.
//!
//! The crate is a port of Dragology (*Declarative Dragging*). Describe the interface as a pure
//! function from state to a [`View`] and attach [`DragSpec`]s to the nodes that can be dragged.
//! The engine renders the target states speculatively, finds the dragged node by id, and
//! synthesizes the preview while dragging and the drop state on release.

mod behavior;
mod blend;
pub mod constraint;
mod easing;
mod geometry;
#[cfg(feature = "gradmesh")]
pub mod gizmo;
#[cfg(feature = "gradmesh")]
pub mod handles;
pub mod heightmap;
#[cfg(feature = "gradmesh")]
pub mod hud;
#[cfg(feature = "pbr")]
mod input;
mod lerp;
mod model;
pub mod ops;
mod param;
#[cfg(feature = "pbr")]
mod plugin;
#[cfg(feature = "gradmesh")]
pub mod radial;
#[cfg(feature = "pbr")]
mod render;
mod rendering;
#[cfg(feature = "gradmesh")]
mod sdf;
#[cfg(feature = "pbr")]
mod shapes;
mod solver;
mod space;
mod spec;
mod transition;
mod view;

pub use behavior::{Chaining, DragBehavior, DragContext, Frame, ViewFn};
pub use blend::{Blend, Presence};
pub use constraint::Constraint;
#[cfg(feature = "pbr")]
pub use geometry::MeshGeometry;
pub use geometry::{Bounds, Geometry};
#[cfg(feature = "pbr")]
pub use input::DragPlane;
pub use lerp::{blend_views, lerp_transform, lerp_views};
pub use model::DragModel;
pub use param::Param;
#[cfg(feature = "pbr")]
pub use plugin::DraggablePlugin;
pub use rendering::Rendering;
#[cfg(feature = "gradmesh")]
pub use sdf::SdfShape;
pub use space::{DragSpace, Pointer};
pub use spec::DragSpec;
pub use view::{DragFactory, GIZMO_LAYER, Hit, NodeId, View, ViewNode};
