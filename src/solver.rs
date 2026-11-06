//! A small derivative-free constrained minimizer.
//!
//! Dragology minimizes with COBYLA. The only COBYLA crate for Rust does not declare a license, so
//! this module implements Nelder–Mead with restarts instead. Constraints are handled by ranking:
//! a point that satisfies them always beats one that does not, and two infeasible points are
//! ranked by how much they violate. Starting from a feasible point (the current drag state) the
//! search therefore never settles on an infeasible result. The drags this is used for have one or
//! two parameters, where Nelder–Mead is fast and robust.

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
}
