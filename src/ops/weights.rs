//! Weighting schemes for [`between`](super::between): a Delaunay triangulation and natural
//! neighbor (Sibson) coordinates.
//!
//! Dragology computes natural neighbor weights from a Delaunay triangulation (a port of
//! Tinfour's interpolator). This module evaluates the same quantity from its definition: the
//! weight of a target is the share of the Voronoi area that a new point at the pointer would take
//! over from that target. Targets are few, so clipping Voronoi cells every frame is cheap.

use bevy::math::{DVec2, Vec2};

use super::between::hull_weights;

/// Indices of the three targets that form a triangle.
pub(crate) type Triangle = [usize; 3];

/// Tolerance for treating a point as inside a triangle or on the boundary of the box.
const EPSILON: f64 = 1e-9;

/// Delaunay triangulation of `points` by incremental insertion (Bowyer–Watson).
///
/// Returns no triangles for fewer than three points or when all points are collinear. The
/// triangulation is built once, so every pointer position is located in the same triangles.
pub(crate) fn triangulate(points: &[Vec2]) -> Vec<Triangle> {
    let count = points.len();
    if count < 3 {
        return Vec::new();
    }
    let mut all: Vec<DVec2> = points.iter().map(|point| point.as_dvec2()).collect();
    let (low, high) = bounds(&all);
    let middle = (low + high) / 2.0;
    let far = (high - low).max_element().max(1.0) * 1e5;
    all.extend([
        middle + DVec2::new(-far, -far),
        middle + DVec2::new(far, -far),
        middle + DVec2::new(0.0, far),
    ]);
    let mut triangles: Vec<Triangle> = vec![[count, count + 1, count + 2]];
    for point in 0..count {
        let (bad, mut kept): (Vec<Triangle>, Vec<Triangle>) =
            triangles.into_iter().partition(|triangle| {
                circumcircle_contains(
                    all[triangle[0]],
                    all[triangle[1]],
                    all[triangle[2]],
                    all[point],
                )
            });
        let mut boundary: Vec<(usize, usize)> = Vec::new();
        for triangle in &bad {
            for (from, to) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                match boundary
                    .iter()
                    .position(|&(a, b)| (a == to && b == from) || (a == from && b == to))
                {
                    Some(shared) => {
                        boundary.swap_remove(shared);
                    }
                    None => boundary.push((from, to)),
                }
            }
        }
        kept.extend(boundary.into_iter().map(|(from, to)| [from, to, point]));
        triangles = kept;
    }
    triangles
        .into_iter()
        .filter(|triangle| triangle.iter().all(|&corner| corner < count))
        .collect()
}

/// Whether `p` is strictly inside the circle through `a`, `b` and `c`.
fn circumcircle_contains(a: DVec2, b: DVec2, c: DVec2, p: DVec2) -> bool {
    let (a, b, c) = (a - p, b - p, c - p);
    let determinant = (a.length_squared()) * b.perp_dot(c) - b.length_squared() * a.perp_dot(c)
        + c.length_squared() * a.perp_dot(b);
    let orientation = (b - a).perp_dot(c - a);
    determinant * orientation.signum() > EPSILON
}

/// Barycentric weights of `q` in the triangle of `triangles` that contains it, or `None` when
/// `q` lies outside the triangulation.
#[expect(
    clippy::cast_possible_truncation,
    reason = "weights are computed in f64 and lie in [0, 1]"
)]
pub(crate) fn triangle_weights(
    points: &[Vec2],
    triangles: &[Triangle],
    q: Vec2,
) -> Option<Vec<f32>> {
    let q = q.as_dvec2();
    triangles.iter().find_map(|&[i, j, k]| {
        let (a, b, c) = (
            points[i].as_dvec2(),
            points[j].as_dvec2(),
            points[k].as_dvec2(),
        );
        let area = (b - a).perp_dot(c - a);
        if area.abs() < EPSILON {
            return None;
        }
        let barycentric = [
            (b - q).perp_dot(c - q) / area,
            (c - q).perp_dot(a - q) / area,
            (a - q).perp_dot(b - q) / area,
        ];
        barycentric.iter().all(|w| *w >= -EPSILON).then(|| {
            let mut weights = vec![0.0; points.len()];
            for (corner, weight) in [i, j, k].into_iter().zip(barycentric) {
                weights[corner] = weight.max(0.0) as f32;
            }
            weights
        })
    })
}

/// Natural neighbor (Sibson) weights of `q`.
///
/// A pointer outside the hull of `points` is first projected onto the hull, and on the hull
/// boundary the weights are the linear weights of the two points of the nearest edge, as in
/// Dragology. With fewer than three points, or collinear ones, there is no area to share and
/// the hull weights are returned.
#[expect(
    clippy::cast_possible_truncation,
    reason = "weights are computed in f64 and lie in [0, 1]"
)]
pub(crate) fn natural_neighbor_weights(points: &[Vec2], q: Vec2) -> Vec<f32> {
    let fallback = hull_weights(points, q);
    if points.len() < 3 {
        return fallback;
    }
    let sites: Vec<DVec2> = points.iter().map(|point| point.as_dvec2()).collect();
    let projected: DVec2 = sites
        .iter()
        .zip(&fallback)
        .map(|(site, weight)| *site * f64::from(*weight))
        .sum();
    let (low, high) = bounds(&sites);
    let margin = (high - low).max_element().max(1.0) * 100.0;
    let (low, high) = (low - DVec2::splat(margin), high + DVec2::splat(margin));
    let frame = vec![
        low,
        DVec2::new(high.x, low.y),
        high,
        DVec2::new(low.x, high.y),
    ];

    let mut cell = frame;
    for site in &sites {
        cell = clip_closer_to(&cell, projected, *site);
    }
    let unbounded = cell.iter().any(|corner| {
        (corner.x - low.x).abs() < EPSILON * margin
            || (corner.x - high.x).abs() < EPSILON * margin
            || (corner.y - low.y).abs() < EPSILON * margin
            || (corner.y - high.y).abs() < EPSILON * margin
    });
    if cell.is_empty() || unbounded {
        return fallback;
    }

    let stolen: Vec<f64> = (0..sites.len())
        .map(|index| {
            let mut part = cell.clone();
            for (other, site) in sites.iter().enumerate() {
                if other != index {
                    part = clip_closer_to(&part, sites[index], *site);
                }
            }
            area(&part)
        })
        .collect();
    let total: f64 = stolen.iter().sum();
    if total < EPSILON {
        return fallback;
    }
    stolen
        .into_iter()
        .map(|part| (part / total) as f32)
        .collect()
}

/// The part of `polygon` that is at least as close to `site` as to `other`.
fn clip_closer_to(polygon: &[DVec2], site: DVec2, other: DVec2) -> Vec<DVec2> {
    let normal = other - site;
    let offset = (other.length_squared() - site.length_squared()) / 2.0;
    let side = |point: DVec2| point.dot(normal) - offset;
    let mut clipped = Vec::with_capacity(polygon.len() + 1);
    for (index, &from) in polygon.iter().enumerate() {
        let to = polygon[(index + 1) % polygon.len()];
        let (side_from, side_to) = (side(from), side(to));
        if side_from <= 0.0 {
            clipped.push(from);
        }
        if (side_from < 0.0 && side_to > 0.0) || (side_from > 0.0 && side_to < 0.0) {
            clipped.push(from + (to - from) * (side_from / (side_from - side_to)));
        }
    }
    clipped
}

/// The area of a counter-clockwise polygon.
fn area(polygon: &[DVec2]) -> f64 {
    let doubled: f64 = polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .map(|(a, b)| a.perp_dot(*b))
        .sum();
    doubled / 2.0
}

fn bounds(points: &[DVec2]) -> (DVec2, DVec2) {
    points.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(low, high), point| (low.min(*point), high.max(*point)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec2> {
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ]
    }

    fn reconstruct(points: &[Vec2], weights: &[f32]) -> Vec2 {
        points.iter().zip(weights).map(|(p, w)| *p * *w).sum()
    }

    #[test]
    fn a_square_triangulates_into_two_triangles() {
        assert_eq!(triangulate(&square()).len(), 2);
    }

    #[test]
    fn too_few_or_collinear_points_have_no_triangles() {
        assert_eq!(triangulate(&square()[..2]).len(), 0);
        let line = [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)];
        assert_eq!(triangulate(&line).len(), 0);
    }

    #[test]
    fn the_triangulation_is_delaunay() {
        let points = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(5.0, 1.0),
            Vec2::new(5.0, 9.0),
        ];
        let triangles = triangulate(&points);
        for [i, j, k] in &triangles {
            for (other, point) in points.iter().enumerate() {
                if [*i, *j, *k].contains(&other) {
                    continue;
                }
                assert!(!circumcircle_contains(
                    points[*i].as_dvec2(),
                    points[*j].as_dvec2(),
                    points[*k].as_dvec2(),
                    point.as_dvec2()
                ));
            }
        }
    }

    #[test]
    fn triangle_weights_are_barycentric_in_the_located_triangle() {
        let points = square();
        let triangles = triangulate(&points);
        let q = Vec2::new(3.0, 4.0);
        let weights = triangle_weights(&points, &triangles, q).unwrap();
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(reconstruct(&points, &weights).distance(q) < 1e-4);
        assert_eq!(weights.iter().filter(|w| **w > 1e-6).count(), 3);
    }

    #[test]
    fn a_point_outside_the_triangulation_has_no_triangle() {
        let points = square();
        let triangles = triangulate(&points);
        assert!(triangle_weights(&points, &triangles, Vec2::new(30.0, 4.0)).is_none());
    }

    #[test]
    fn natural_neighbor_weights_reproduce_the_point_and_use_every_neighbor() {
        let points = square();
        let q = Vec2::new(3.0, 4.0);
        let weights = natural_neighbor_weights(&points, q);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(reconstruct(&points, &weights).distance(q) < 1e-3);
        assert!(weights.iter().all(|w| *w > 1e-3), "{weights:?}");
    }

    #[test]
    fn natural_neighbor_weights_of_a_triangle_are_barycentric() {
        let points = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(12.0, 0.0),
            Vec2::new(0.0, 12.0),
        ];
        let weights = natural_neighbor_weights(&points, Vec2::new(3.0, 3.0));
        for (weight, expected) in weights.iter().zip([0.5, 0.25, 0.25]) {
            assert!((weight - expected).abs() < 1e-3, "{weights:?}");
        }
    }

    #[test]
    fn natural_neighbor_weights_at_the_center_of_a_square_are_equal() {
        let weights = natural_neighbor_weights(&square(), Vec2::new(5.0, 5.0));
        for weight in weights {
            assert!((weight - 0.25).abs() < 1e-3);
        }
    }

    #[test]
    fn natural_neighbor_projects_outside_points_onto_the_hull() {
        let points = square();
        let weights = natural_neighbor_weights(&points, Vec2::new(5.0, -20.0));
        assert!(reconstruct(&points, &weights).distance(Vec2::new(5.0, 0.0)) < 1e-3);
        assert_eq!(weights.iter().filter(|w| **w > 1e-6).count(), 2);
    }

    #[test]
    fn natural_neighbor_with_two_points_falls_back_to_the_segment() {
        let points = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
        let weights = natural_neighbor_weights(&points, Vec2::new(2.5, 3.0));
        assert!((weights[0] - 0.75).abs() < 1e-5 && (weights[1] - 0.25).abs() < 1e-5);
    }

    // The cases below are ported from Dragology's `math/natural-neighbor.test.ts` and
    // `math/delaunay.test.ts`.

    fn plus_square() -> Vec<Vec2> {
        let mut points = square();
        points.push(Vec2::new(5.0, 5.0));
        points
    }

    #[test]
    fn weights_sum_to_one_and_are_non_negative() {
        let weights = natural_neighbor_weights(&plus_square(), Vec2::new(3.0, 4.0));
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(weights.iter().all(|w| *w >= -1e-6), "{weights:?}");
    }

    #[test]
    fn a_pointer_on_a_vertex_gives_that_vertex_all_the_weight() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(5.0, 10.0),
        ];
        let weights = natural_neighbor_weights(&points, Vec2::ZERO);
        assert!((weights[0] - 1.0).abs() < 1e-4, "{weights:?}");
        assert!(weights[1..].iter().all(|w| w.abs() < 1e-4));
    }

    #[test]
    fn a_grid_reproduces_linear_functions() {
        let points: Vec<Vec2> = (0..=10)
            .flat_map(|x| (0..=10).map(move |y| Vec2::new(x as f32, y as f32)))
            .collect();
        for q in [
            Vec2::new(3.7, 4.2),
            Vec2::new(7.1, 2.9),
            Vec2::new(5.3, 5.7),
            Vec2::new(1.3, 8.6),
            Vec2::new(6.5, 3.3),
        ] {
            let weights = natural_neighbor_weights(&points, q);
            assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-4);
            assert!(reconstruct(&points, &weights).distance(q) < 1e-3, "{q}");
        }
    }

    #[test]
    fn random_point_sets_reproduce_linear_functions_at_their_centroid() {
        // The deterministic generator of the reference test (Lehmer, multiplier 16807).
        let mut seed: u64 = 12345;
        let mut random = || {
            seed = seed * 16807 % 2_147_483_647;
            seed as f32 / 2_147_483_647.0
        };
        for _ in 0..10 {
            let points: Vec<Vec2> = (0..10)
                .map(|_| Vec2::new(random() * 100.0, random() * 100.0))
                .collect();
            let centroid = points.iter().copied().sum::<Vec2>() / 10.0;
            let weights = natural_neighbor_weights(&points, centroid);
            assert!(weights.iter().all(|w| *w >= -1e-5), "{weights:?}");
            assert!(reconstruct(&points, &weights).distance(centroid) < 1e-2);
        }
    }

    #[test]
    fn the_interior_point_weighs_little_near_a_hull_edge() {
        let points = [
            Vec2::new(150.0, 150.0),
            Vec2::new(450.0, 150.0),
            Vec2::new(450.0, 450.0),
            Vec2::new(150.0, 450.0),
            Vec2::new(300.0, 300.0),
        ];
        let q = Vec2::new(300.0, 155.0);
        let weights = natural_neighbor_weights(&points, q);
        assert!(weights[0] + weights[1] > 0.8, "{weights:?}");
        assert!(weights[4] < 0.1, "{weights:?}");
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(reconstruct(&points, &weights).distance(q) < 1e-1);
    }

    #[test]
    fn a_pointer_exactly_on_a_hull_edge_uses_the_edge_vertices() {
        let points = [
            Vec2::new(150.0, 150.0),
            Vec2::new(450.0, 150.0),
            Vec2::new(450.0, 450.0),
            Vec2::new(150.0, 450.0),
            Vec2::new(300.0, 300.0),
        ];
        let q = Vec2::new(300.0, 150.0);
        let weights = natural_neighbor_weights(&points, q);
        assert!(weights[0] + weights[1] > 0.95, "{weights:?}");
        assert!(reconstruct(&points, &weights).distance(q) < 1e-1);
    }

    #[test]
    fn a_pointer_outside_the_hull_is_projected_and_weights_sum_to_one() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(5.0, 10.0),
        ];
        let weights = natural_neighbor_weights(&points, Vec2::new(5.0, -5.0));
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_regular_polygon_gives_equal_weights_at_its_center() {
        let points: Vec<Vec2> = (0..6)
            .map(|i| Vec2::from_angle(std::f32::consts::TAU * i as f32 / 6.0) * 10.0)
            .collect();
        let weights = natural_neighbor_weights(&points, Vec2::ZERO);
        for weight in &weights {
            assert!((weight - 1.0 / 6.0).abs() < 1e-3, "{weights:?}");
        }
    }

    #[test]
    fn collinear_points_have_no_triangle_to_find() {
        for (points, q) in [
            (
                vec![Vec2::new(250.0, 0.0), Vec2::new(250.0, 50.0)],
                Vec2::new(250.0, 55.0),
            ),
            (
                vec![Vec2::new(250.0, 0.0), Vec2::new(250.0, 50.0)],
                Vec2::new(300.0, 55.0),
            ),
            (
                vec![
                    Vec2::new(250.0, 0.0),
                    Vec2::new(250.0, 50.0),
                    Vec2::new(250.0, 100.0),
                ],
                Vec2::new(250.0, 55.0),
            ),
        ] {
            let triangles = triangulate(&points);
            assert!(triangle_weights(&points, &triangles, q).is_none());
        }
    }
}
