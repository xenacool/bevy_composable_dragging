//! The `with_snap_radius` modifier.

use std::sync::Arc;

use crate::behavior::Chaining;
use crate::spec::DragSpec;

/// Options of [`with_snap_radius_options`](DragSpec::with_snap_radius_options). The defaults
/// match [`with_snap_radius`](DragSpec::with_snap_radius).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct SnapOptions {
    /// When set, snapping and unsnapping count as a switch of branch that blends the preview over
    /// this many seconds, instead of jumping.
    pub transition: Option<f32>,
    /// When set, a drag that is snapped to a state other than the one it started from continues
    /// as a new drag from that state (see [`with_chaining`](DragSpec::with_chaining)).
    pub chain: bool,
}

impl SnapOptions {
    /// The defaults: no blending and no chaining.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`transition`](Self::transition).
    #[must_use]
    pub fn with_transition(mut self, seconds: f32) -> Self {
        self.transition = Some(seconds);
        self
    }

    /// Turns on [`chain`](Self::chain).
    #[must_use]
    pub fn with_chain(mut self) -> Self {
        self.chain = true;
        self
    }
}

/// Decides whether a drop state differs from the start state, for chaining.
type Differs<S> = Arc<dyn Fn(&S, &S) -> bool + Send + Sync>;

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Snaps the preview to the drop state's own layout once the dragged node is within `radius`
    /// world units of where that layout places it. This only changes the preview, not the drop
    /// state, and is typically combined with [`with_floating`](Self::with_floating) so the
    /// floating node settles into place when it gets close.
    ///
    /// See [`with_snap_radius_options`](Self::with_snap_radius_options) to blend the change and
    /// to chain into a new drag.
    #[must_use]
    pub fn with_snap_radius(self, radius: f32) -> Self {
        self.snapping(radius, None, None)
    }

    fn snapping(self, radius: f32, transition: Option<f32>, chain: Option<Differs<S>>) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let view_fn = context.view_fn();
            let dragged = context.dragged().to_owned();
            let space = *context.space();
            let start = context.start_state().clone();
            let chain = chain.clone();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                let settled = view_fn(&frame.drop_state);
                let snapped = matches!(
                    (
                        frame.view.world_position(&dragged),
                        settled.world_position(&dragged)
                    ),
                    (Some(held), Some(home))
                        if space.project_point(held).distance(space.project_point(home)) <= radius
                );
                if snapped {
                    frame.view = settled;
                }
                if let Some(seconds) = transition {
                    frame = frame.in_branch(usize::from(snapped));
                    frame.branch_transition = Some(seconds);
                }
                if let Some(differs) = &chain
                    && snapped
                    && frame.chain.is_none()
                    && differs(&frame.drop_state, &start)
                {
                    let mut chaining = Chaining::new();
                    chaining.transition = transition;
                    frame.chain = Some(chaining);
                }
                frame
            })
        })
    }
}

impl<S: Clone + PartialEq + Send + Sync + 'static> DragSpec<S> {
    /// Like [`with_snap_radius`](Self::with_snap_radius), with the blending and chaining of
    /// [`SnapOptions`]. Chaining compares the drop state with the state the drag started from,
    /// so the state must be comparable.
    #[must_use]
    pub fn with_snap_radius_options(self, radius: f32, options: SnapOptions) -> Self {
        let chain: Option<Differs<S>> = options.chain.then(|| Arc::new(|a: &S, b: &S| a != b) as _);
        self.snapping(radius, options.transition, chain)
    }
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use bevy::math::Vec2;

    use super::SnapOptions;
    use crate::ops::closest_states;
    use crate::ops::testing::{Slot, context};

    #[test]
    fn snaps_the_preview_inside_the_radius() {
        let spec = closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .with_snap_radius(2.0);
        let mut behavior = spec.compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(9.0, 1.0)));
        assert_eq!(frame.drop_state, Slot(1));
        let item = frame.view.world_transform("item").unwrap().translation;
        assert!(item.truncate().distance(Vec2::new(10.0, 0.0)) < 1e-4);
        assert!(item.z.abs() < 1e-4, "the snapped item is not lifted");
    }

    #[test]
    fn keeps_floating_outside_the_radius() {
        let spec = closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .with_snap_radius(2.0);
        let mut behavior = spec.compile(&context());
        let frame = behavior(Pointer::from(Vec2::new(7.0, 0.0)));
        assert_eq!(frame.drop_state, Slot(1));
        assert!(
            frame
                .view
                .world_position("item")
                .unwrap()
                .truncate()
                .distance(Vec2::new(7.0, 0.0))
                < 1e-4
        );
    }

    fn snapping_with(options: SnapOptions) -> crate::DragSpec<Slot> {
        closest_states([Slot(0), Slot(1), Slot(2)])
            .with_floating()
            .with_snap_radius_options(2.0, options)
    }

    #[test]
    fn the_default_options_change_nothing() {
        let mut plain = snapping_with(SnapOptions::new()).compile(&context());
        let snapped = plain(Pointer::from(Vec2::new(9.0, 1.0)));
        let floating = plain(Pointer::from(Vec2::new(7.0, 0.0)));
        assert_eq!(snapped.branch, floating.branch);
        assert!(snapped.chain.is_none() && snapped.branch_transition.is_none());
    }

    #[test]
    fn a_transition_makes_snapping_a_branch_switch() {
        let mut behavior =
            snapping_with(SnapOptions::new().with_transition(0.2)).compile(&context());
        let snapped = behavior(Pointer::from(Vec2::new(9.0, 1.0)));
        let floating = behavior(Pointer::from(Vec2::new(7.0, 0.0)));
        assert_ne!(snapped.branch, floating.branch);
        assert_eq!(snapped.branch_transition, Some(0.2));
        assert_eq!(floating.branch_transition, Some(0.2));
    }

    #[test]
    fn chaining_continues_the_drag_once_snapped_to_another_state() {
        let mut behavior = snapping_with(SnapOptions::new().with_chain()).compile(&context());
        assert!(
            behavior(Pointer::from(Vec2::new(9.0, 1.0))).chain.is_some(),
            "snapped to slot 1"
        );
        assert!(
            behavior(Pointer::from(Vec2::new(7.0, 0.0))).chain.is_none(),
            "not snapped yet"
        );
        assert!(
            behavior(Pointer::from(Vec2::new(1.0, 0.0))).chain.is_none(),
            "snapped to the start state"
        );
    }

    #[test]
    fn the_chain_blends_over_the_snap_transition() {
        let options = SnapOptions::new().with_transition(0.3).with_chain();
        let frame = snapping_with(options).compile(&context())(Pointer::from(Vec2::new(9.0, 1.0)));
        assert_eq!(frame.chain.unwrap().transition, Some(0.3));
    }
}
