//! Animated transitions between views.

use bevy::prelude::{Res, ResMut, Time};

use crate::lerp::lerp_views;
use crate::model::DragModel;
use crate::view::View;

/// How long the default drop animation takes, in seconds.
pub(crate) const DEFAULT_DURATION: f32 = 0.2;

/// An animation from one view to another.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
pub(crate) struct Transition<S> {
    from: View<S>,
    to: View<S>,
    elapsed: f32,
    duration: f32,
}

impl<S: Clone> Transition<S> {
    /// Starts a transition that takes `duration` seconds.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn new(from: View<S>, to: View<S>, duration: f32) -> Self {
        Self {
            from,
            to,
            elapsed: 0.0,
            duration,
        }
    }

    /// The view the transition ends in.
    pub(crate) fn target(&self) -> &View<S> {
        &self.to
    }

    /// Replaces the view the transition ends in, keeping the elapsed time.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn retarget(&mut self, to: View<S>) {
        self.to = to;
    }

    /// Moves the animation forward by `seconds`.
    pub(crate) fn advance(&mut self, seconds: f32) {
        self.elapsed += seconds;
    }

    /// Whether the animation has reached its target.
    pub(crate) fn is_finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// The view to show right now.
    pub(crate) fn current(&self) -> View<S> {
        lerp_views(&self.from, &self.to, ease_out(self.elapsed / self.duration))
    }
}

/// Decelerating curve: fast at the start, settling gently at the end.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Advances the running transition of the model, if any.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
pub(crate) fn advance_transitions<S: Clone + Send + Sync + 'static>(
    time: Res<Time>,
    mut model: ResMut<DragModel<S>>,
) {
    if model.is_transitioning() {
        model.advance_transition(time.delta_secs());
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::view::ViewNode;

    fn at(x: f32) -> View<()> {
        View::new().with(ViewNode::circle("n", 1.0).at(Vec2::new(x, 0.0)))
    }

    #[test]
    fn eases_from_start_to_target() {
        let mut transition = Transition::new(at(0.0), at(10.0), 1.0);
        assert!(transition.current().position("n").unwrap().x.abs() < 1e-5);
        transition.advance(0.5);
        let halfway = transition.current().position("n").unwrap().x;
        assert!(
            halfway > 5.0 && halfway < 10.0,
            "ease-out is ahead: {halfway}"
        );
        assert!(!transition.is_finished());
        transition.advance(0.5);
        assert!(transition.is_finished());
        assert!((transition.current().position("n").unwrap().x - 10.0).abs() < 1e-5);
    }
}
