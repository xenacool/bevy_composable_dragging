//! The `between` operator.

use bevy::math::Vec2;

use super::weights::{natural_neighbor_weights, triangle_weights, triangulate};
use crate::behavior::Frame;
use crate::lerp::blend_views;
use crate::spec::DragSpec;
use crate::view::View;

/// How [`between_with`] spreads weight over the targets, given where the pointer is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Interpolation {
    /// Barycentric weights in the smallest triangle of targets that contains the pointer. This is
    /// the weighting of [`between`].
    #[default]
    Hull,
    /// Barycentric weights in the triangle of the Delaunay triangulation of the targets that
    /// contains the pointer.
    Delaunay,
    /// Natural neighbor (Sibson) weights: smoother than the triangle schemes, because every
    /// target that borders the pointer in the Voronoi diagram shares the weight.
    NaturalNeighbor,
}

/// Which target a [`between`] drag drops onto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DropRule {
    /// The target with the largest blend weight.
    #[default]
    LargestWeight,
    /// The target whose position of the dragged node is nearest to the pointer. This is the rule
    /// of Dragology's `between`.
    NearestTarget,
}

/// Options of [`between_with`]. The defaults match [`between`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct BetweenOptions {
    /// How weight is spread over the targets.
    pub interpolation: Interpolation,
    /// Which target a drop commits to.
    pub drop_rule: DropRule,
    /// Raises every weight to this power and normalizes, which makes the dragged node pull
    /// towards the target it is nearest to: `1.0` adds no attraction, and the larger the value the
    /// closer the behavior gets to [`closest`](crate::ops::closest). It must be positive.
    pub sharpness: f32,
}

impl Default for BetweenOptions {
    fn default() -> Self {
        Self {
            interpolation: Interpolation::default(),
            drop_rule: DropRule::default(),
            sharpness: 1.0,
        }
    }
}

impl BetweenOptions {
    /// The defaults: [`Interpolation::Hull`], [`DropRule::LargestWeight`] and a sharpness of one.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`interpolation`](Self::interpolation).
    #[must_use]
    pub fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    /// Sets [`drop_rule`](Self::drop_rule).
    #[must_use]
    pub fn with_drop_rule(mut self, drop_rule: DropRule) -> Self {
        self.drop_rule = drop_rule;
        self
    }

    /// Sets [`sharpness`](Self::sharpness).
    ///
    /// # Panics
    ///
    /// Panics when `sharpness` is not positive.
    #[must_use]
    pub fn with_sharpness(mut self, sharpness: f32) -> Self {
        assert!(
            sharpness > 0.0,
            "between: sharpness ({sharpness}) must be positive"
        );
        self.sharpness = sharpness;
        self
    }
}

/// Blends the views of `targets` according to where the pointer is inside the convex hull of the
/// dragged node's positions in them. Dropping switches to the target with the largest weight
/// (see [`DropRule`] to drop onto the nearest target instead).
///
/// Targets that do not contain the dragged node are ignored. With no usable target the drag
/// shows the starting state.
pub fn between<S: Clone + Send + Sync + 'static>(
    targets: impl IntoIterator<Item = S>,
) -> DragSpec<S> {
    between_with(targets, BetweenOptions::new())
}

/// Like [`between`], with the interpolation mode, drop rule and sharpness of [`BetweenOptions`].
pub fn between_with<S: Clone + Send + Sync + 'static>(
    targets: impl IntoIterator<Item = S>,
    options: BetweenOptions,
) -> DragSpec<S> {
    let targets: Vec<S> = targets.into_iter().collect();
    DragSpec::new(move |context| {
        let mut states = Vec::new();
        let mut views: Vec<View<S>> = Vec::new();
        let mut anchors = Vec::new();
        let space = *context.space();
        for target in &targets {
            let view = context.render(target);
            if let Some(anchor) = context.anchor_in(&view) {
                states.push(target.clone());
                views.push(view);
                anchors.push(space.local(anchor));
            }
        }
        if states.is_empty() {
            let start = context.start_state().clone();
            let view = context.render(&start);
            return Box::new(move |_| Frame::new(view.clone(), start.clone(), f32::INFINITY));
        }
        let triangles = match options.interpolation {
            Interpolation::Delaunay => triangulate(&anchors),
            _ => Vec::new(),
        };
        Box::new(move |pointer| {
            // The weights are 2D math in the basis of the space of the drag.
            let pointer = space.local(pointer.point);
            let mut weights = match options.interpolation {
                Interpolation::Hull => hull_weights(&anchors, pointer),
                Interpolation::Delaunay => triangle_weights(&anchors, &triangles, pointer)
                    .unwrap_or_else(|| hull_weights(&anchors, pointer)),
                Interpolation::NaturalNeighbor => natural_neighbor_weights(&anchors, pointer),
            };
            sharpen(&mut weights, options.sharpness);
            let view_refs: Vec<&View<S>> = views.iter().collect();
            let blended_anchor: Vec2 = anchors.iter().zip(&weights).map(|(a, w)| *a * *w).sum();
            let nearest = match options.drop_rule {
                DropRule::LargestWeight => weights
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.total_cmp(b))
                    .map_or(0, |(index, _)| index),
                DropRule::NearestTarget => anchors
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.distance_squared(pointer)
                            .total_cmp(&b.distance_squared(pointer))
                    })
                    .map_or(0, |(index, _)| index),
            };
            Frame::new(
                blend_views(&view_refs, &weights),
                states[nearest].clone(),
                blended_anchor.distance(pointer),
            )
        })
    })
}

/// Raises the weights to `sharpness` and normalizes them. A sharpness of one changes nothing.
fn sharpen(weights: &mut [f32], sharpness: f32) {
    if (sharpness - 1.0).abs() < f32::EPSILON {
        return;
    }
    for weight in weights.iter_mut() {
        *weight = weight.powf(sharpness);
    }
    let total: f32 = weights.iter().sum();
    if total > 0.0 {
        for weight in weights.iter_mut() {
            *weight /= total;
        }
    }
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
    use crate::ops::testing::{Slot, context};
    use crate::space::Pointer;

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

    /// A dragged item at one of the corners of a 10 x 10 square.
    #[derive(Clone, Debug, PartialEq)]
    struct Corner(usize);

    fn corner_view(corner: &Corner) -> View<Corner> {
        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let (x, y) = corners[corner.0];
        View::new().with(crate::view::ViewNode::circle("item", 1.0).at_xy(Vec2::new(x, y)))
    }

    fn corner_frame(options: BetweenOptions, pointer: Vec2) -> Frame<Corner> {
        let context = crate::behavior::DragContext::new(
            std::sync::Arc::new(corner_view),
            "item".into(),
            Corner(0),
        );
        between_with((0..4).map(Corner), options).compile(&context)(Pointer::from(pointer))
    }

    #[test]
    fn the_default_options_match_plain_between() {
        let context = context();
        for x in [-3.0, 2.0, 7.0, 14.0, 26.0] {
            let pointer = Vec2::new(x, 4.0);
            let plain =
                between([Slot(0), Slot(1), Slot(2)]).compile(&context)(Pointer::from(pointer));
            let with = between_with([Slot(0), Slot(1), Slot(2)], BetweenOptions::new())
                .compile(&context)(Pointer::from(pointer));
            assert_eq!(plain.drop_state, with.drop_state);
            assert_eq!(plain.gap, with.gap);
            assert_eq!(
                plain.view.world_position("item").unwrap(),
                with.view.world_position("item").unwrap()
            );
        }
    }

    #[test]
    fn every_interpolation_keeps_the_item_on_the_pointer_inside_the_hull() {
        let pointer = Vec2::new(3.0, 4.0);
        for interpolation in [
            Interpolation::Hull,
            Interpolation::Delaunay,
            Interpolation::NaturalNeighbor,
        ] {
            let frame = corner_frame(
                BetweenOptions::new().with_interpolation(interpolation),
                pointer,
            );
            assert!(frame.gap < 1e-3, "{interpolation:?}: gap {}", frame.gap);
        }
    }

    #[test]
    fn every_interpolation_projects_an_outside_pointer_onto_the_hull() {
        for interpolation in [
            Interpolation::Hull,
            Interpolation::Delaunay,
            Interpolation::NaturalNeighbor,
        ] {
            let frame = corner_frame(
                BetweenOptions::new().with_interpolation(interpolation),
                Vec2::new(5.0, -20.0),
            );
            let item = frame.view.world_position("item").unwrap();
            assert!(
                item.truncate().distance(Vec2::new(5.0, 0.0)) < 1e-3,
                "{interpolation:?}: {item}"
            );
            assert!((frame.gap - 20.0).abs() < 1e-3);
        }
    }

    #[test]
    fn the_nearest_target_rule_drops_onto_the_corner_nearest_the_pointer() {
        let nearest = BetweenOptions::new().with_drop_rule(DropRule::NearestTarget);
        let near_corner = Vec2::new(1.0, 1.0);
        assert_eq!(
            corner_frame(nearest, near_corner).drop_state,
            corner_frame(BetweenOptions::new(), near_corner).drop_state
        );
        assert_eq!(corner_frame(nearest, near_corner).drop_state, Corner(0));
        let outside = Vec2::new(40.0, 9.0);
        assert_eq!(corner_frame(nearest, outside).drop_state, Corner(2));
    }

    #[test]
    fn sharpness_pulls_the_item_towards_the_nearest_target() {
        let pointer = Vec2::new(3.0, 2.0);
        let soft = corner_frame(BetweenOptions::new(), pointer);
        let sharp = corner_frame(BetweenOptions::new().with_sharpness(4.0), pointer);
        let from_corner =
            |frame: &Frame<Corner>| frame.view.world_position("item").unwrap().length();
        assert!(from_corner(&sharp) < from_corner(&soft));
        assert!(sharp.gap > soft.gap);
    }

    #[test]
    fn sharpness_of_one_leaves_the_weights_alone() {
        let mut weights = vec![0.2, 0.3, 0.5];
        sharpen(&mut weights, 1.0);
        assert_eq!(weights, vec![0.2, 0.3, 0.5]);
        sharpen(&mut weights, 2.0);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(weights[2] > 0.5);
    }

    #[test]
    #[should_panic(expected = "sharpness")]
    fn a_non_positive_sharpness_is_rejected() {
        let _ = BetweenOptions::new().with_sharpness(0.0);
    }
}
