//! The `vary` operator.

use crate::behavior::{DragContext, Frame};
use crate::constraint::Constraint;
use crate::param::Param;
use crate::solver::{Evaluation, minimize};
use crate::spec::DragSpec;

/// Moves the dragged node as close to the pointer as the numeric `params` of `start` allow.
///
/// Every frame the solver searches for the parameter values that minimize the distance between
/// the pointer and the dragged node in the rendered view; everything else in `start` stays as
/// it is. It starts from the previous frame's solution, so the drag tracks the pointer
/// continuously. Passing the state to vary from lets branches of [`closest`](super::closest)
/// vary different variants of the state.
pub fn vary<S: Clone + Send + Sync + 'static>(
    start: S,
    params: impl IntoIterator<Item = Param<S>>,
) -> DragSpec<S> {
    vary_constrained(start, params, Constraint::none())
}

/// Like [`vary`], but the solution must satisfy `constraint`, for example
/// [`in_range`](crate::constraint::in_range) for a slider.
pub fn vary_constrained<S: Clone + Send + Sync + 'static>(
    start: S,
    params: impl IntoIterator<Item = Param<S>>,
    constraint: Constraint<S>,
) -> DragSpec<S> {
    let params: Vec<Param<S>> = params.into_iter().collect();
    DragSpec::new(move |context: &DragContext<S>| {
        let start = start.clone();
        let view_fn = context.view_fn();
        let dragged = context.dragged().to_owned();
        let params = params.clone();
        let constraint = constraint.clone();
        let steps: Vec<f32> = params.iter().map(|param| 0.25 * param.scale()).collect();
        let mut values: Vec<f32> = params.iter().map(|param| param.get(&start)).collect();

        Box::new(move |pointer| {
            let state_at = |values: &[f32]| {
                let mut state = start.clone();
                for (param, value) in params.iter().zip(values) {
                    param.set(&mut state, *value);
                }
                state
            };
            let (solution, evaluation) = minimize(&values, &steps, |candidate| {
                let state = state_at(candidate);
                let cost = view_fn(&state)
                    .position(&dragged)
                    .map_or(f32::INFINITY, |anchor| anchor.distance_squared(pointer));
                Evaluation {
                    cost,
                    violation: constraint.violation(&state),
                }
            });
            values = solution;
            let drop_state = state_at(&values);
            Frame::new(view_fn(&drop_state), drop_state, evaluation.cost.sqrt())
        })
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::math::Vec2;

    use super::*;
    use crate::behavior::DragContext;
    use crate::constraint::{in_order, in_range};
    use crate::param;
    use crate::view::{View, ViewNode};

    #[derive(Clone)]
    struct Slider {
        value: f32,
    }

    /// The knob sits at x = 100 * value.
    fn slider_view(state: &Slider) -> View<Slider> {
        View::new().with(ViewNode::circle("knob", 10.0).at(Vec2::new(state.value * 100.0, 0.0)))
    }

    fn slider_context(value: f32) -> DragContext<Slider> {
        DragContext::new(Arc::new(slider_view), "knob".into(), Slider { value })
    }

    #[test]
    fn keeps_the_knob_under_the_pointer() {
        let mut behavior =
            vary(Slider { value: 0.2 }, [param!(Slider, value)]).compile(&slider_context(0.2));
        for x in [30.0, 45.0, 62.0, 80.0] {
            let frame = behavior(Vec2::new(x, 7.0));
            assert!((frame.drop_state.value * 100.0 - x).abs() < 0.1, "x = {x}");
            // The pointer is off the slider's line by 7, which no parameter can fix.
            assert!((frame.gap - 7.0).abs() < 0.1);
        }
    }

    #[test]
    fn constraints_clamp_the_knob() {
        let value = param!(Slider, value);
        let mut behavior = vary_constrained(
            Slider { value: 0.5 },
            [value.clone()],
            in_range(value, 0.0, 1.0),
        )
        .compile(&slider_context(0.5));
        let beyond = behavior(Vec2::new(250.0, 0.0));
        assert!((beyond.drop_state.value - 1.0).abs() < 1e-3);
        let before = behavior(Vec2::new(-80.0, 0.0));
        assert!(before.drop_state.value.abs() < 1e-3);
        let inside = behavior(Vec2::new(40.0, 0.0));
        assert!((inside.drop_state.value - 0.4).abs() < 1e-3);
    }

    #[derive(Clone)]
    struct Range {
        start: f32,
        end: f32,
    }

    /// The handle sits on the range's end, at x = 100 * end.
    fn range_view(state: &Range) -> View<Range> {
        View::new().with(ViewNode::circle("end", 10.0).at(Vec2::new(state.end * 100.0, 0.0)))
    }

    #[test]
    fn the_end_cannot_cross_the_start() {
        let initial = Range {
            start: 0.4,
            end: 0.8,
        };
        let context = DragContext::new(Arc::new(range_view), "end".into(), initial.clone());
        let constraint = in_order([param!(Range, start), param!(Range, end)]);
        let mut behavior =
            vary_constrained(initial, [param!(Range, end)], constraint).compile(&context);
        let frame = behavior(Vec2::new(10.0, 0.0));
        assert!((frame.drop_state.end - 0.4).abs() < 1e-3);
        assert!((frame.drop_state.start - 0.4).abs() < 1e-6);
    }

    #[test]
    fn a_missing_dragged_node_reports_an_infinite_gap() {
        let context = DragContext::new(
            Arc::new(slider_view),
            "missing".into(),
            Slider { value: 0.5 },
        );
        let mut behavior = vary(Slider { value: 0.5 }, [param!(Slider, value)]).compile(&context);
        assert!(behavior(Vec2::ZERO).gap.is_infinite());
    }
}
