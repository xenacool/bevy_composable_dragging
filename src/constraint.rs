//! Constraints on the parameters that `vary` changes.
//!
//! A [`Constraint`] reports how much a state violates it: zero when satisfied, growing with the
//! amount of violation. The solver only needs that single number.

use std::sync::Arc;

use crate::param::Param;

type Violation<S> = dyn Fn(&S) -> f32 + Send + Sync;

/// A condition on a state that `vary` must keep satisfied.
pub struct Constraint<S>(Arc<Violation<S>>);

impl<S> Clone for Constraint<S> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<S> Constraint<S> {
    /// Creates a constraint from a function that returns how much a state violates it: zero when
    /// satisfied, a positive amount otherwise.
    pub fn new(violation: impl Fn(&S) -> f32 + Send + Sync + 'static) -> Self {
        Self(Arc::new(violation))
    }

    /// A constraint that every state satisfies.
    #[must_use]
    pub fn none() -> Self {
        Self::new(|_| 0.0)
    }

    /// How much `state` violates the constraint; zero when it is satisfied.
    pub fn violation(&self, state: &S) -> f32 {
        (self.0)(state)
    }
}

/// `a <= b`.
#[must_use]
pub fn less_than<S: 'static>(a: Param<S>, b: Param<S>) -> Constraint<S> {
    Constraint::new(move |state| (a.get(state) - b.get(state)).max(0.0))
}

/// `a >= b`.
#[must_use]
pub fn more_than<S: 'static>(a: Param<S>, b: Param<S>) -> Constraint<S> {
    less_than(b, a)
}

/// `a == b`.
#[must_use]
pub fn equal<S: 'static>(a: Param<S>, b: Param<S>) -> Constraint<S> {
    Constraint::new(move |state| (a.get(state) - b.get(state)).abs())
}

/// `params[0] <= params[1] <= ...`.
pub fn in_order<S: 'static>(params: impl IntoIterator<Item = Param<S>>) -> Constraint<S> {
    let params: Vec<Param<S>> = params.into_iter().collect();
    Constraint::new(move |state| {
        params
            .windows(2)
            .map(|pair| (pair[0].get(state) - pair[1].get(state)).max(0.0))
            .sum()
    })
}

/// `param >= minimum`.
#[must_use]
pub fn at_least<S: 'static>(param: Param<S>, minimum: f32) -> Constraint<S> {
    Constraint::new(move |state| (minimum - param.get(state)).max(0.0))
}

/// `param <= maximum`.
#[must_use]
pub fn at_most<S: 'static>(param: Param<S>, maximum: f32) -> Constraint<S> {
    Constraint::new(move |state| (param.get(state) - maximum).max(0.0))
}

/// `minimum <= param <= maximum`.
#[must_use]
pub fn in_range<S: 'static>(param: Param<S>, minimum: f32, maximum: f32) -> Constraint<S> {
    and([at_least(param.clone(), minimum), at_most(param, maximum)])
}

/// All of `constraints` at once.
pub fn and<S: 'static>(constraints: impl IntoIterator<Item = Constraint<S>>) -> Constraint<S> {
    let constraints: Vec<Constraint<S>> = constraints.into_iter().collect();
    Constraint::new(move |state| constraints.iter().map(|c| c.violation(state)).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param;

    #[derive(Clone)]
    struct Pair {
        a: f32,
        b: f32,
        c: f32,
    }

    fn pair(a: f32, b: f32, c: f32) -> Pair {
        Pair { a, b, c }
    }

    #[test]
    fn less_and_more_than_measure_the_overshoot() {
        let less = less_than(param!(Pair, a), param!(Pair, b));
        assert_eq!(less.violation(&pair(1.0, 2.0, 0.0)), 0.0);
        assert_eq!(less.violation(&pair(3.0, 2.0, 0.0)), 1.0);
        let more = more_than(param!(Pair, a), param!(Pair, b));
        assert_eq!(more.violation(&pair(1.0, 2.0, 0.0)), 1.0);
        assert_eq!(more.violation(&pair(3.0, 2.0, 0.0)), 0.0);
    }

    #[test]
    fn equal_measures_the_difference() {
        let constraint = equal(param!(Pair, a), param!(Pair, b));
        assert_eq!(constraint.violation(&pair(2.0, 2.0, 0.0)), 0.0);
        assert_eq!(constraint.violation(&pair(2.0, 5.0, 0.0)), 3.0);
    }

    #[test]
    fn in_order_sums_the_out_of_order_gaps() {
        let constraint = in_order([param!(Pair, a), param!(Pair, b), param!(Pair, c)]);
        assert_eq!(constraint.violation(&pair(1.0, 2.0, 3.0)), 0.0);
        assert_eq!(constraint.violation(&pair(1.0, 3.0, 2.0)), 1.0);
        assert_eq!(constraint.violation(&pair(3.0, 2.0, 0.0)), 3.0);
    }

    #[test]
    fn bounds_report_distance_outside_the_range() {
        let constraint = in_range(param!(Pair, a), 0.0, 1.0);
        assert_eq!(constraint.violation(&pair(0.5, 0.0, 0.0)), 0.0);
        assert_eq!(constraint.violation(&pair(-2.0, 0.0, 0.0)), 2.0);
        assert_eq!(constraint.violation(&pair(1.5, 0.0, 0.0)), 0.5);
    }

    #[test]
    fn and_adds_up_and_none_is_always_satisfied() {
        let both = and([
            at_least(param!(Pair, a), 1.0),
            at_most(param!(Pair, b), 1.0),
        ]);
        assert_eq!(both.violation(&pair(0.0, 3.0, 0.0)), 3.0);
        assert_eq!(
            Constraint::<Pair>::none().violation(&pair(9.0, 9.0, 9.0)),
            0.0
        );
    }
}
