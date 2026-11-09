//! A small derivative-free constrained minimizer.
//!
//! Dragology minimizes with COBYLA. The only COBYLA crate for Rust does not declare a license, so
//! this module implements Nelder-Mead with restarts instead. Constraints are handled by ranking:
//! a point that satisfies them always beats one that does not, and two infeasible points are
//! ranked by how much they violate. A first stage that charges violations as a penalty lets the
//! search move along curved constraints. Starting from a feasible point (the current drag state)
//! the ranked search that follows never settles on an infeasible result. The drags this is used
//! for have one or two parameters, where Nelder-Mead is fast and robust.

use std::cmp::Ordering;

/// The objective and constraint violation at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Evaluation {
    /// The value to minimize.
    pub(crate) cost: f32,
    /// How much the point violates the constraints; zero means feasible.
    pub(crate) violation: f32,
}

/// Violations up to this size count as satisfied.
const FEASIBILITY_TOLERANCE: f32 = 1e-5;
/// The search stops once the simplex is this small relative to the initial step sizes.
const CONVERGENCE_TOLERANCE: f32 = 1e-4;
const MAX_ITERATIONS: usize = 200;
/// Restarting from the best point escapes simplexes that collapsed against a constraint.
const ROUNDS: usize = 3;
/// How many units of cost one unit of violation costs while relaxing. Costs are squared
/// distances, so this must exceed twice the distances that are dragged over to dominate them.
const PENALTY: f32 = 1000.0;

impl Evaluation {
    fn is_feasible(self) -> bool {
        self.violation <= FEASIBILITY_TOLERANCE
    }

    /// Orders feasible points by cost, and infeasible points by violation after all feasible ones.
    fn rank(self, other: Self) -> Ordering {
        match (self.is_feasible(), other.is_feasible()) {
            (true, true) => self.cost.total_cmp(&other.cost),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self
                .violation
                .total_cmp(&other.violation)
                .then(self.cost.total_cmp(&other.cost)),
        }
    }
}

struct Vertex {
    point: Vec<f32>,
    evaluation: Evaluation,
}

/// Minimizes `evaluate` starting at `start`, with `steps` giving a typical size of each
/// parameter. Returns the best point found and its evaluation.
pub(crate) fn minimize(
    start: &[f32],
    steps: &[f32],
    mut evaluate: impl FnMut(&[f32]) -> Evaluation,
) -> (Vec<f32>, Evaluation) {
    let mut best = Vertex {
        point: start.to_vec(),
        evaluation: evaluate(start),
    };
    if start.is_empty() {
        return (best.point, best.evaluation);
    }
    best = relax(best.point, steps, &mut evaluate);
    let mut scale = 1.0;
    for _ in 0..ROUNDS {
        let previous = best.evaluation;
        best = nelder_mead(best.point, steps, scale, &mut evaluate);
        if best.evaluation.rank(previous) != Ordering::Less {
            break;
        }
        scale *= 0.3;
    }
    (best.point, best.evaluation)
}

/// Moves along the constraints towards the optimum before the exact search starts.
///
/// Ranking alone cannot follow a curved constraint, such as a fixed distance from a point: the
/// feasible points next to the current one are a sliver that a simplex hardly ever hits, so the
/// search crawls. Charging the violation as a penalty lets the simplex step through slightly
/// infeasible points. The result is only approximately feasible; the ranked rounds that follow
/// restore exact feasibility from it.
fn relax(
    start: Vec<f32>,
    steps: &[f32],
    evaluate: &mut impl FnMut(&[f32]) -> Evaluation,
) -> Vertex {
    let mut penalized = |point: &[f32]| {
        let evaluation = evaluate(point);
        Evaluation {
            cost: evaluation.cost + PENALTY * evaluation.violation,
            violation: 0.0,
        }
    };
    let relaxed = nelder_mead(start, steps, 1.0, &mut penalized);
    Vertex {
        evaluation: evaluate(&relaxed.point),
        point: relaxed.point,
    }
}

fn nelder_mead(
    start: Vec<f32>,
    steps: &[f32],
    scale: f32,
    evaluate: &mut impl FnMut(&[f32]) -> Evaluation,
) -> Vertex {
    let dimension = start.len();
    let mut simplex: Vec<Vertex> = Vec::with_capacity(dimension + 1);
    simplex.push(Vertex {
        evaluation: evaluate(&start),
        point: start.clone(),
    });
    for axis in 0..dimension {
        let mut point = start.clone();
        point[axis] += steps[axis] * scale;
        simplex.push(Vertex {
            evaluation: evaluate(&point),
            point,
        });
    }

    for _ in 0..MAX_ITERATIONS {
        simplex.sort_by(|a, b| a.evaluation.rank(b.evaluation));
        if relative_size(&simplex, steps) < CONVERGENCE_TOLERANCE {
            break;
        }
        let worst = simplex.len() - 1;
        let centroid = centroid(&simplex[..worst]);
        let along = |factor: f32| -> Vec<f32> {
            centroid
                .iter()
                .zip(&simplex[worst].point)
                .map(|(c, w)| c + factor * (c - w))
                .collect()
        };
        let probe = |point: Vec<f32>, evaluate: &mut dyn FnMut(&[f32]) -> Evaluation| {
            let evaluation = evaluate(&point);
            Vertex { point, evaluation }
        };

        let reflected = probe(along(1.0), evaluate);
        let replacement = if reflected.evaluation.rank(simplex[0].evaluation) == Ordering::Less {
            let expanded = probe(along(2.0), evaluate);
            if expanded.evaluation.rank(reflected.evaluation) == Ordering::Less {
                Some(expanded)
            } else {
                Some(reflected)
            }
        } else if reflected.evaluation.rank(simplex[worst - 1].evaluation) == Ordering::Less {
            Some(reflected)
        } else {
            let outside = reflected.evaluation.rank(simplex[worst].evaluation) == Ordering::Less;
            let contracted = probe(along(if outside { 0.5 } else { -0.5 }), evaluate);
            let reference = if outside {
                reflected.evaluation
            } else {
                simplex[worst].evaluation
            };
            (contracted.evaluation.rank(reference) == Ordering::Less).then_some(contracted)
        };

        if let Some(vertex) = replacement {
            simplex[worst] = vertex;
        } else {
            let anchor = simplex[0].point.clone();
            for vertex in simplex.iter_mut().skip(1) {
                for (coordinate, base) in vertex.point.iter_mut().zip(&anchor) {
                    *coordinate = base + 0.5 * (*coordinate - base);
                }
                vertex.evaluation = evaluate(&vertex.point);
            }
        }
    }

    simplex.sort_by(|a, b| a.evaluation.rank(b.evaluation));
    simplex.swap_remove(0)
}

fn centroid(vertices: &[Vertex]) -> Vec<f32> {
    let mut centroid = vec![0.0; vertices[0].point.len()];
    for vertex in vertices {
        for (sum, coordinate) in centroid.iter_mut().zip(&vertex.point) {
            *sum += coordinate;
        }
    }
    let count = vertices.len() as f32;
    for sum in &mut centroid {
        *sum /= count;
    }
    centroid
}

/// The largest distance from the best vertex, measured in units of the initial step sizes.
fn relative_size(simplex: &[Vertex], steps: &[f32]) -> f32 {
    simplex[1..]
        .iter()
        .flat_map(|vertex| {
            vertex
                .point
                .iter()
                .zip(&simplex[0].point)
                .zip(steps)
                .map(|((a, b), step)| (a - b).abs() / step.max(f32::EPSILON))
        })
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;

    fn feasible(cost: f32) -> Evaluation {
        Evaluation {
            cost,
            violation: 0.0,
        }
    }

    #[test]
    fn minimizes_an_unconstrained_quadratic() {
        let (point, _) = minimize(&[0.0, 0.0], &[1.0, 1.0], |p| {
            feasible((p[0] - 3.0).powi(2) + (p[1] + 1.0).powi(2))
        });
        assert!((point[0] - 3.0).abs() < 1e-3 && (point[1] + 1.0).abs() < 1e-3);
    }

    #[test]
    fn stops_at_an_upper_bound() {
        let (point, evaluation) = minimize(&[0.0], &[1.0], |p| Evaluation {
            cost: (p[0] - 5.0).powi(2),
            violation: (p[0] - 1.0).max(0.0),
        });
        assert!((point[0] - 1.0).abs() < 1e-3, "got {}", point[0]);
        assert!(evaluation.is_feasible());
    }

    #[test]
    fn respects_an_ordering_constraint_between_two_parameters() {
        // Minimize the distance to (5, 0) subject to x <= y; the optimum is (2.5, 2.5).
        let (point, evaluation) = minimize(&[0.0, 1.0], &[1.0, 1.0], |p| Evaluation {
            cost: (p[0] - 5.0).powi(2) + p[1].powi(2),
            violation: (p[0] - p[1]).max(0.0),
        });
        assert!(evaluation.is_feasible());
        assert!(
            (point[0] - 2.5).abs() < 0.02 && (point[1] - 2.5).abs() < 0.02,
            "{point:?}"
        );
    }

    #[test]
    fn keeps_two_parameters_equal() {
        // Minimize the distance to (4, 2) subject to x == y; the optimum is (3, 3).
        let (point, evaluation) = minimize(&[1.0, 1.0], &[1.0, 1.0], |p| Evaluation {
            cost: (p[0] - 4.0).powi(2) + (p[1] - 2.0).powi(2),
            violation: (p[0] - p[1]).abs(),
        });
        assert!(evaluation.is_feasible());
        assert!(
            (point[0] - 3.0).abs() < 0.05 && (point[1] - 3.0).abs() < 0.05,
            "{point:?}"
        );
    }

    #[test]
    fn follows_a_moving_target_from_a_warm_start() {
        let mut x = vec![0.0];
        for target in [0.2, 0.4, 0.6, 0.8] {
            (x, _) = minimize(&x, &[0.1], |p| feasible((p[0] - target).powi(2)));
            assert!((x[0] - target).abs() < 1e-3);
        }
    }

    #[test]
    fn terminates_with_a_finite_result_on_a_flat_or_oscillating_objective() {
        let (point, _) = minimize(&[0.0], &[1.0], |_| feasible(1.0));
        assert!(point[0].is_finite());
        let (point, _) = minimize(&[0.0], &[1.0], |p| feasible((p[0] * 40.0).cos()));
        assert!(point[0].is_finite());
    }

    #[test]
    fn no_parameters_returns_the_start() {
        let (point, evaluation) = minimize(&[], &[], |_| feasible(2.0));
        assert_eq!(point, Vec::<f32>::new());
        assert_eq!(evaluation, feasible(2.0));
    }

    // The cases below are ported from Dragology's `math/optimization.test.ts`. A drag minimizes
    // the squared distance between the pointer and a point that the parameters map to, from the
    // previous solution, as `vary` does.

    /// The pixel-sized first step that `vary` uses for parameters of scale 100.
    const STEP: f32 = 25.0;

    /// Solves once from `start`, then again to check that the result is stable.
    fn solve(
        start: &[f32],
        steps: &[f32],
        target: Vec2,
        to_point: impl Fn(&[f32]) -> Vec2,
        violation: impl Fn(&[f32]) -> f32,
    ) -> Vec<f32> {
        let mut evaluate = |params: &[f32]| Evaluation {
            cost: to_point(params).distance_squared(target),
            violation: violation(params),
        };
        let (first, _) = minimize(start, steps, &mut evaluate);
        let (second, _) = minimize(&first, steps, &mut evaluate);
        for (a, b) in first.iter().zip(&second) {
            assert!((a - b).abs() < 0.05, "unstable: {first:?} then {second:?}");
        }
        first
    }

    fn slider(value: &[f32]) -> Vec2 {
        Vec2::new(value[0], 0.0)
    }

    fn no_violation(_: &[f32]) -> f32 {
        0.0
    }

    fn within_zero_and(upper: f32) -> impl Fn(&[f32]) -> f32 {
        move |p| (-p[0]).max(0.0) + (p[0] - upper).max(0.0)
    }

    #[test]
    fn a_slider_parameter_moves_to_the_target() {
        let result = solve(
            &[100.0],
            &[STEP],
            Vec2::new(150.0, 0.0),
            slider,
            no_violation,
        );
        assert!((result[0] - 150.0).abs() < 0.1, "{result:?}");
    }

    #[test]
    fn a_warm_start_follows_a_nearby_target() {
        let first = solve(&[0.0], &[STEP], Vec2::new(100.0, 0.0), slider, no_violation);
        assert!((first[0] - 100.0).abs() < 0.1);
        let second = solve(&first, &[STEP], Vec2::new(105.0, 0.0), slider, no_violation);
        assert!((second[0] - 105.0).abs() < 0.1, "{second:?}");
    }

    #[test]
    fn a_slider_respects_both_bounds() {
        let above = solve(
            &[100.0],
            &[STEP],
            Vec2::new(300.0, 0.0),
            slider,
            within_zero_and(240.0),
        );
        assert!(
            (above[0] - 240.0).abs() < 0.1 && above[0] <= 240.0 + 1e-3,
            "{above:?}"
        );
        let below = solve(
            &[100.0],
            &[STEP],
            Vec2::new(-50.0, 0.0),
            slider,
            within_zero_and(240.0),
        );
        assert!(below[0].abs() < 0.1 && below[0] >= -1e-3, "{below:?}");
    }

    #[test]
    fn a_perpendicular_pointer_offset_is_ignored() {
        let result = solve(
            &[0.0],
            &[STEP],
            Vec2::new(120.0, 50.0),
            slider,
            no_violation,
        );
        assert!((result[0] - 120.0).abs() < 0.1, "{result:?}");
    }

    #[test]
    fn a_two_dimensional_position_finds_the_target() {
        let point = |p: &[f32]| Vec2::new(p[0], p[1]);
        let from_origin = solve(
            &[0.0, 0.0],
            &[STEP, STEP],
            Vec2::new(80.0, 60.0),
            point,
            no_violation,
        );
        assert!((from_origin[0] - 80.0).abs() < 0.1 && (from_origin[1] - 60.0).abs() < 0.1);
        let from_far = solve(
            &[200.0, 150.0],
            &[STEP, STEP],
            Vec2::new(50.0, 30.0),
            point,
            no_violation,
        );
        assert!((from_far[0] - 50.0).abs() < 0.1 && (from_far[1] - 30.0).abs() < 0.1);
    }

    #[test]
    fn an_angle_on_a_circle_reaches_the_top() {
        let circle = |p: &[f32]| Vec2::from_angle(p[0]) * 100.0;
        let result = solve(&[0.0], &[0.25], Vec2::new(0.0, 100.0), circle, no_violation);
        let point = circle(&result);
        // COBYLA is derivative-free too, so the reference only asks for one digit here.
        assert!(
            point.x.abs() < 5.0 && (point.y - 100.0).abs() < 5.0,
            "{point}"
        );
    }

    fn on_the_line(p: &[f32]) -> f32 {
        (p[0] + p[1] - 100.0).abs()
    }

    #[test]
    fn the_closest_point_on_a_line_is_the_projection() {
        let point = |p: &[f32]| Vec2::new(p[0], p[1]);
        for start in [[10.0, 10.0], [200.0, 200.0]] {
            let result = solve(
                &start,
                &[STEP, STEP],
                Vec2::new(80.0, 60.0),
                point,
                on_the_line,
            );
            assert!(
                (result[0] - 60.0).abs() < 0.5 && (result[1] - 40.0).abs() < 0.5,
                "{start:?} gave {result:?}"
            );
        }
    }

    #[test]
    fn a_far_target_shift_is_projected_onto_the_line() {
        let point = |p: &[f32]| Vec2::new(p[0], p[1]);
        let steps = [STEP, STEP];
        let at_start = solve(
            &[50.0, 50.0],
            &steps,
            Vec2::new(50.0, 50.0),
            point,
            on_the_line,
        );
        let moved = solve(
            &at_start,
            &steps,
            Vec2::new(250.0, 50.0),
            point,
            on_the_line,
        );
        assert!(
            (moved[0] - 150.0).abs() < 1.0 && (moved[1] + 50.0).abs() < 1.0,
            "{moved:?}"
        );
    }

    #[test]
    fn a_static_target_does_not_oscillate_over_many_frames() {
        let point = |p: &[f32]| Vec2::new(p[0], p[1]);
        let steps = [STEP, STEP];
        let target = Vec2::new(80.0, 60.0);
        let mut params = vec![10.0, 10.0];
        let mut results = Vec::new();
        for _ in 0..20 {
            params = minimize(&params, &steps, |p| Evaluation {
                cost: point(p).distance_squared(target),
                violation: on_the_line(p),
            })
            .0;
            results.push(params.clone());
        }
        for result in &results[1..] {
            assert!(
                (result[0] - results[0][0]).abs() < 0.1 && (result[1] - results[0][1]).abs() < 0.1
            );
        }
    }

    #[test]
    fn a_slowly_moving_target_moves_the_solution_monotonically() {
        let point = |p: &[f32]| Vec2::new(p[0], p[1]);
        let steps = [STEP, STEP];
        let mut params = vec![10.0, 10.0];
        let mut xs = Vec::new();
        for frame in 0..30 {
            let target = Vec2::new(80.0 + 40.0 * frame as f32 / 29.0, 60.0);
            params = minimize(&params, &steps, |p| Evaluation {
                cost: point(p).distance_squared(target),
                violation: on_the_line(p),
            })
            .0;
            xs.push(params[0]);
        }
        for window in xs.windows(3) {
            let (before, after) = (window[1] - window[0], window[2] - window[1]);
            assert!(before * after >= -0.01, "jitter in {xs:?}");
        }
    }

    // Emulates the reference timeline: `pos` maps 1:1 to x and `track` maps 1:42 to y.
    const TRACK_WIDTH: f32 = 340.0;
    const BLOCK_WIDTH: f32 = 80.0;
    const TRACKS: f32 = 4.0;
    const TRACK_PITCH: f32 = 42.0;

    fn timeline_point(p: &[f32]) -> Vec2 {
        Vec2::new(p[0], p[1] * TRACK_PITCH)
    }

    fn inside_the_timeline(p: &[f32]) -> f32 {
        (-p[0]).max(0.0)
            + (p[0] + BLOCK_WIDTH - TRACK_WIDTH).max(0.0)
            + (-p[1]).max(0.0)
            + (p[1] - (TRACKS - 1.0)).max(0.0)
    }

    fn timeline_step(params: &[f32], target: Vec2) -> Vec<f32> {
        minimize(params, &[STEP, 0.25], |p| Evaluation {
            cost: timeline_point(p).distance_squared(target),
            violation: inside_the_timeline(p),
        })
        .0
    }

    #[test]
    fn the_timeline_converges_at_the_top_right_corner() {
        let result = timeline_step(&[100.0, 1.0], Vec2::new(400.0, -50.0));
        assert!(
            (result[0] - 260.0).abs() < 0.5 && result[1].abs() < 0.02,
            "{result:?}"
        );
    }

    #[test]
    fn the_timeline_holds_the_corner_while_the_pointer_stays_past_it() {
        let mut params = vec![100.0, 1.0];
        for step in 0..5 {
            let t = step as f32 / 4.0;
            params = timeline_step(
                &params,
                Vec2::new(100.0 + 200.0 * t, (1.0 - t) * TRACK_PITCH),
            );
        }
        for _ in 0..20 {
            params = timeline_step(&params, Vec2::new(500.0, -100.0));
            assert!(
                (params[0] - 260.0).abs() < 0.5 && params[1].abs() < 0.02,
                "{params:?}"
            );
        }
    }

    #[test]
    fn the_timeline_stays_at_the_corner_while_the_pointer_sweeps_past_it() {
        let mut params = timeline_step(&[200.0, 0.0], Vec2::new(260.0, -20.0));
        for step in 0..30 {
            let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 29.0;
            let target = Vec2::new(260.0 + 80.0 * angle.cos(), -80.0 * angle.sin());
            params = timeline_step(&params, target);
            assert!(
                (params[0] - 260.0).abs() < 0.5 && params[1].abs() < 0.02,
                "{params:?} at step {step}"
            );
        }
    }
}
