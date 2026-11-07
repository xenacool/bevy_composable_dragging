//! The drag operators.
//!
//! The functions in this module build [`DragSpec`](crate::DragSpec)s; modifiers such as
//! [`with_floating`](crate::DragSpec::with_floating) are methods on the spec.

mod between;
mod branch;
mod chaining;
mod change;
mod closest;
mod drop_target;
mod during;
mod floating;
mod follow;
mod init_context;
mod on_drop;
mod overlay;
mod react_to;
mod snap;
mod state;
mod substate;
#[cfg(test)]
mod testing;
mod value;
mod vary;
mod weights;
mod when_far;

pub use between::{BetweenOptions, DropRule, Interpolation, between, between_with};
pub use closest::{
    ClosestOptions, closest, closest_states, closest_with_options, closest_with_stickiness,
};
pub use drop_target::{drop_target, drop_target_with_bounds};
pub use floating::FloatingOptions;
pub use follow::{switch_to_state_and_follow, switch_to_state_and_follow_with};
pub use init_context::custom;
pub use react_to::react_to;
pub use snap::SnapOptions;
pub use state::state;
pub use substate::{Lens, substate};
pub use value::{Snap, Tracked, across_plane, along_axis, around_axis};
pub use vary::{VaryOptions, vary, vary_constrained, vary_func, vary_with};
