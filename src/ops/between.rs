//! The `between` operator.

use bevy::math::Vec2;

use crate::behavior::Frame;
use crate::lerp::blend_views;
use crate::spec::DragSpec;
use crate::view::View;

/// Blends the views of `targets` according to where the pointer is inside the convex hull of the
/// dragged node's positions in them. Dropping switches to the target with the largest weight.
///
/// Targets that do not contain the dragged node are ignored. With no usable target the drag
/// shows the starting state.
pub fn between<S: Clone + Send + Sync + 'static>(
    targets: impl IntoIterator<Item = S>,
) -> DragSpec<S> {
    let targets: Vec<S> = targets.into_iter().collect();
    DragSpec::new(move |context| {
        let mut states = Vec::new();
        let mut views: Vec<View<S>> = Vec::new();
        let mut anchors = Vec::new();
        for target in &targets {
            let view = context.render(target);
            if let Some(anchor) = context.anchor_in(&view) {
                states.push(target.clone());
                views.push(view);
                anchors.push(anchor);
            }
        }
        if states.is_empty() {
            let start = context.start_state().clone();
            let view = context.render(&start);
            return Box::new(move |_| Frame::new(view.clone(), start.clone(), f32::INFINITY));
        }
        Box::new(move |pointer| {
            let weights = hull_weights(&anchors, pointer);
            let view_refs: Vec<&View<S>> = views.iter().collect();
            let blended_anchor: Vec2 = anchors.iter().zip(&weights).map(|(a, w)| *a * *w).sum();
            let nearest = weights
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .map_or(0, |(index, _)| index);
            Frame::new(
                blend_views(&view_refs, &weights),
                states[nearest].clone(),
                blended_anchor.distance(pointer),
            )
        })
    })
}

/// Tolerance for treating two triangle corners as collinear or two distances as equal.
const EPSILON: f32 = 1e-6;

/// Weights (non-negative, summing to one) that express the point of the convex hull of `points`
/// that is nearest to `q` as a combination of the points.
///
/// Inside the hull the weights are barycentric coordinates in the smallest triangle that
/// contains `q`; outside, `q` is projected onto the nearest segment between two points.
pub(crate) fn hull_weights(points: &[Vec2], q: Vec2) -> Vec<f32> {
    let mut weights = vec![0.0; points.len()];
    match points.len() {
        0 => {}
        1 => weights[0] = 1.0,
        _ => {
            if let Some(([i, j, k], barycentric)) = enclosing_triangle(points, q) {
                weights[i] = barycentric[0];
                weights[j] = barycentric[1];
                weights[k] = barycentric[2];
            } else {
                let (i, j, s) = nearest_segment(points, q);
                weights[i] = 1.0 - s;
                weights[j] = s;
            }
        }
    }
    weights
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.perp_dot(b)
}

/// The non-degenerate triangle with the smallest area that contains `q`, with `q`'s barycentric
/// coordinates in it.
fn enclosing_triangle(points: &[Vec2], q: Vec2) -> Option<([usize; 3], [f32; 3])> {
    let mut best: Option<(f32, [usize; 3], [f32; 3])> = None;
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            for k in j + 1..points.len() {
                let (a, b, c) = (points[i], points[j], points[k]);
                let area = cross(b - a, c - a);
                if area.abs() < EPSILON {
                    continue;
                }
                let barycentric = [
                    cross(b - q, c - q) / area,
                    cross(c - q, a - q) / area,
                    cross(a - q, b - q) / area,
                ];
                let inside = barycentric.iter().all(|w| *w >= -EPSILON);
                if inside && best.is_none_or(|(smallest, _, _)| area.abs() < smallest) {
                    best = Some((area.abs(), [i, j, k], barycentric));
                }
            }
        }
    }
    best.map(|(_, corners, barycentric)| (corners, barycentric))
}

/// The segment between two points that is nearest to `q`, with the parameter of the nearest
/// point along it. Ties go to the shorter segment so collinear points blend neighbors.
fn nearest_segment(points: &[Vec2], q: Vec2) -> (usize, usize, f32) {
    let mut best = (0, 1, 0.0);
    let mut best_distance = f32::INFINITY;
    let mut best_length = f32::INFINITY;
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            let segment = points[j] - points[i];
            let length = segment.length_squared();
            let s = if length < EPSILON {
                0.0
            } else {
                ((q - points[i]).dot(segment) / length).clamp(0.0, 1.0)
            };
            let distance = q.distance_squared(points[i] + segment * s);
            let tolerance = EPSILON * (1.0 + best_distance.min(1e6));
            let closer = distance < best_distance - tolerance;
            let tied_but_shorter =
                (distance - best_distance).abs() <= tolerance && length < best_length;
            if closer || tied_but_shorter {
                best = (i, j, s);
                best_distance = distance;
                best_length = length;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn single_target_takes_all_weight() {
        assert_eq!(hull_weights(&[Vec2::new(3.0, 4.0)], Vec2::ZERO), vec![1.0]);
    }

    #[test]
    fn two_targets_project_onto_the_segment() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        assert!(close(
            &hull_weights(&points, Vec2::new(2.5, 7.0)),
            &[0.75, 0.25]
        ));
        assert!(close(
            &hull_weights(&points, Vec2::new(-5.0, 0.0)),
            &[1.0, 0.0]
        ));
        assert!(close(
            &hull_weights(&points, Vec2::new(50.0, 0.0)),
            &[0.0, 1.0]
        ));
    }

    #[test]
    fn collinear_targets_blend_the_two_neighbors() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(20.0, 0.0),
        ];
        assert!(close(
            &hull_weights(&points, Vec2::new(5.0, 0.0)),
            &[0.5, 0.5, 0.0]
        ));
        assert!(close(
            &hull_weights(&points, Vec2::new(15.0, 0.0)),
            &[0.0, 0.5, 0.5]
        ));
        assert!(close(
            &hull_weights(&points, Vec2::new(30.0, 1.0)),
            &[0.0, 0.0, 1.0]
        ));
    }

    #[test]
    fn inside_a_triangle_the_weights_are_barycentric() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(12.0, 0.0),
            Vec2::new(0.0, 12.0),
        ];
        let weights = hull_weights(&points, Vec2::new(3.0, 3.0));
        assert!(close(&weights, &[0.5, 0.25, 0.25]));
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn outside_a_triangle_the_point_is_projected_to_the_hull() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(12.0, 0.0),
            Vec2::new(0.0, 12.0),
        ];
        let weights = hull_weights(&points, Vec2::new(6.0, -5.0));
        assert!(close(&weights, &[0.5, 0.5, 0.0]));
    }

    #[test]
    fn weights_reproduce_the_query_inside_the_hull() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let q = Vec2::new(7.0, 3.0);
        let weights = hull_weights(&points, q);
        let reconstructed: Vec2 = points.iter().zip(&weights).map(|(p, w)| *p * *w).sum();
        assert!(reconstructed.distance(q) < 1e-4);
    }
}
