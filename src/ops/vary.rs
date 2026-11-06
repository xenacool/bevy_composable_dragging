//! The `vary` operator.

use std::sync::Arc;

use crate::behavior::Frame;
use crate::constraint::Constraint;
use crate::param::Param;
use crate::solver::{Evaluation, minimize};
use crate::spec::DragSpec;

type Reader<S> = dyn Fn(&S) -> f32 + Send + Sync;

/// Options of [`vary_with`] and [`vary_func`]. The defaults add nothing to plain [`vary`].
pub struct VaryOptions<S> {
    constraint: Constraint<S>,
    pins: Vec<Arc<Reader<S>>>,
    scale: f32,
}

impl<S> Clone for VaryOptions<S> {
    fn clone(&self) -> Self {
        Self {
            constraint: self.constraint.clone(),
            pins: self.pins.clone(),
            scale: self.scale,
        }
    }
}

impl<S> Default for VaryOptions<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> VaryOptions<S> {
    /// No constraint, no pins, and a parameter scale of one.
    #[must_use]
    pub fn new() -> Self {
        Self {
            constraint: Constraint::none(),
            pins: Vec::new(),
            scale: 1.0,
        }
    }

    /// The solution must satisfy `constraint`, for example
    /// [`in_range`](crate::constraint::in_range) for a slider.
    #[must_use]
    pub fn with_constraint(mut self, constraint: Constraint<S>) -> Self {
        self.constraint = constraint;
        self
    }

    /// Holds the number that `read` extracts from a state at the value it has in the state the
    /// drag starts from, however the parameters move. Use it to keep a derived quantity fixed
    /// while other numbers vary. Pins can be added repeatedly.
    ///
    /// Pins may be curved, such as a fixed distance from a point: the dragged node then slides
    /// along the circle towards the pointer. The solver is derivative-free, so it holds a curved
    /// pin only approximately (to well under a pixel at the usual scales) and may trail a very
    /// fast pointer by a few frames; use [`vary_func`] to vary the one number that traces the
    /// curve when exactness matters.
    #[must_use]
    pub fn with_pin(mut self, read: impl Fn(&S) -> f32 + Send + Sync + 'static) -> Self {
        self.pins.push(Arc::new(read));
        self
    }

    /// Holds `param` at its starting value; a shortcut for [`with_pin`](Self::with_pin).
    #[must_use]
    pub fn with_pinned_param(self, param: Param<S>) -> Self
    where
        S: 'static,
    {
        self.with_pin(move |state| param.get(state))
    }

    /// Sets the typical magnitude of the numbers that [`vary_func`] varies, which sizes the
    /// solver's first steps. [`vary`] and [`vary_with`] use the scale of each [`Param`] instead.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
}

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
    vary_with(start, params, VaryOptions::new())
}

/// Like [`vary`], but the solution must satisfy `constraint`, for example
/// [`in_range`](crate::constraint::in_range) for a slider.
pub fn vary_constrained<S: Clone + Send + Sync + 'static>(
    start: S,
    params: impl IntoIterator<Item = Param<S>>,
    constraint: Constraint<S>,
) -> DragSpec<S> {
    vary_with(
        start,
        params,
        VaryOptions::new().with_constraint(constraint),
    )
}

/// Like [`vary`], with the constraint and pins of [`VaryOptions`].
pub fn vary_with<S: Clone + Send + Sync + 'static>(
    start: S,
    params: impl IntoIterator<Item = Param<S>>,
    options: VaryOptions<S>,
) -> DragSpec<S> {
    let params: Vec<Param<S>> = params.into_iter().collect();
    let initial: Vec<f32> = params.iter().map(|param| param.get(&start)).collect();
    let steps: Vec<f32> = params.iter().map(|param| 0.25 * param.scale()).collect();
    solve(
        initial,
        steps,
        move |values| {
            let mut state = start.clone();
            for (param, value) in params.iter().zip(values) {
                param.set(&mut state, *value);
            }
            state
        },
        options,
    )
}

/// Varies raw numbers instead of fields of a state: `build` makes the state from the numbers,
/// which start at `initial`. Use it when several parts of the state must change in concert, for
/// example an angle that sets both the position and the rotation of a node.
///
/// As with [`vary`], the numbers are only solver variables; the values in `initial` seed the
/// search and are not shown on their own.
pub fn vary_func<S: Clone + Send + Sync + 'static>(
    initial: impl IntoIterator<Item = f32>,
    build: impl Fn(&[f32]) -> S + Send + Sync + 'static,
    options: VaryOptions<S>,
) -> DragSpec<S> {
    let initial: Vec<f32> = initial.into_iter().collect();
    let steps = vec![0.25 * options.scale; initial.len()];
    solve(initial, steps, build, options)
}

/// The shared core: minimizes the pointer-to-node distance over the numbers that `build` turns
/// into states, under the constraint and with the pins fixed at their starting values.
fn solve<S: Clone + Send + Sync + 'static>(
    initial: Vec<f32>,
    steps: Vec<f32>,
    build: impl Fn(&[f32]) -> S + Send + Sync + 'static,
    options: VaryOptions<S>,
) -> DragSpec<S> {
    let build = Arc::new(build);
    DragSpec::new(move |context| {
        let build = Arc::clone(&build);
        let view_fn = context.view_fn();
        let dragged = context.dragged().to_owned();
        let steps = steps.clone();
        let constraint = options.constraint.clone();
        let pins = options.pins.clone();
        let initial_state = build(&initial);
        let targets: Vec<f32> = pins.iter().map(|read| read(&initial_state)).collect();
        let mut values = initial.clone();

        Box::new(move |pointer| {
            let (solution, evaluation) = minimize(&values, &steps, |candidate| {
                let state = build(candidate);
                let cost = view_fn(&state)
                    .position(&dragged)
                    .map_or(f32::INFINITY, |anchor| anchor.distance_squared(pointer));
                let pinned: f32 = pins
                    .iter()
                    .zip(&targets)
                    .map(|(read, target)| (read(&state) - target).abs())
                    .sum();
                Evaluation {
                    cost,
                    violation: constraint.violation(&state) + pinned,
                }
            });
            values = solution;
            let drop_state = build(&values);
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

    #[derive(Clone)]
    struct Point {
        x: f32,
        y: f32,
    }

    fn point_view(state: &Point) -> View<Point> {
        View::new().with(ViewNode::circle("dot", 10.0).at(Vec2::new(state.x, state.y)))
    }

    fn point_context() -> DragContext<Point> {
        DragContext::new(Arc::new(point_view), "dot".into(), Point { x: 0.0, y: 5.0 })
    }

    #[test]
    fn a_pinned_parameter_keeps_its_starting_value() {
        let spec = vary_with(
            Point { x: 0.0, y: 5.0 },
            [param!(Point, x), param!(Point, y)],
            VaryOptions::new().with_pinned_param(param!(Point, y)),
        );
        let frame = spec.compile(&point_context())(Vec2::new(30.0, 40.0));
        assert!((frame.drop_state.x - 30.0).abs() < 0.1);
        assert!((frame.drop_state.y - 5.0).abs() < 0.1);
    }

    #[test]
    fn a_pinned_derived_quantity_is_held() {
        let spec = vary_with(
            Point { x: 0.0, y: 5.0 },
            [param!(Point, x), param!(Point, y)],
            VaryOptions::new().with_pin(|point: &Point| point.y * 2.0),
        );
        let frame = spec.compile(&point_context())(Vec2::new(-12.0, 40.0));
        assert!((frame.drop_state.x + 12.0).abs() < 0.1);
        assert!((frame.drop_state.y - 5.0).abs() < 0.1);
    }

    #[test]
    fn a_fixed_distance_pin_slides_the_dot_along_the_circle() {
        let start = Point { x: 100.0, y: 0.0 };
        let context = DragContext::new(Arc::new(point_view), "dot".into(), start.clone());
        let spec = vary_with(
            start,
            [param!(Point, x), param!(Point, y)],
            VaryOptions::new()
                .with_pin(|point: &Point| point.x.hypot(point.y))
                .with_scale(100.0),
        );
        let mut behavior = spec.compile(&context);
        let mut last = None;
        for step in 0..=20 {
            let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 20.0;
            let frame = behavior(Vec2::from_angle(angle) * 140.0);
            let point = frame.drop_state;
            assert!((point.x.hypot(point.y) - 100.0).abs() < 0.5, "step {step}");
            last = Some(point);
        }
        let end = last.unwrap();
        assert!(
            end.x.abs() < 1.0 && (end.y - 100.0).abs() < 1.0,
            "({}, {})",
            end.x,
            end.y
        );
    }

    #[test]
    fn vary_func_varies_raw_numbers_through_a_builder() {
        // One number moves the dot along the line y = 2x.
        let spec = vary_func(
            [0.0],
            |values| Point {
                x: values[0],
                y: values[0] * 2.0,
            },
            VaryOptions::new().with_scale(10.0),
        );
        let frame = spec.compile(&point_context())(Vec2::new(10.0, 20.0));
        assert!((frame.drop_state.x - 10.0).abs() < 0.1);
        assert!((frame.drop_state.y - 20.0).abs() < 0.2);
        assert!(frame.gap < 0.2);
    }

    #[test]
    fn vary_func_honours_a_constraint() {
        let spec = vary_func(
            [0.0],
            |values| Slider { value: values[0] },
            VaryOptions::new().with_constraint(Constraint::new(|slider: &Slider| {
                (slider.value - 1.0).max(0.0)
            })),
        );
        let frame = spec.compile(&slider_context(0.0))(Vec2::new(500.0, 0.0));
        assert!((frame.drop_state.value - 1.0).abs() < 1e-3);
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
