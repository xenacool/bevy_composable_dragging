//! The drag operators.
//!
//! The functions in this module build [`DragSpec`](crate::DragSpec)s; modifiers such as
//! [`with_floating`](crate::DragSpec::with_floating) are methods on the spec.

mod between;
mod branch;
mod closest;
mod drop_target;
mod floating;
mod follow;
mod on_drop;
mod snap;
mod state;
#[cfg(test)]
mod testing;
mod vary;
mod when_far;

pub use between::between;
pub use closest::{closest, closest_states, closest_with_stickiness};
pub use drop_target::drop_target;
pub use follow::{switch_to_state_and_follow, switch_to_state_and_follow_with};
pub use state::state;
pub use vary::{vary, vary_constrained};
