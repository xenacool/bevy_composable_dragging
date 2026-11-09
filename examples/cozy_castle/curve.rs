//! A curve through control points in 3D, the authored form of a wall.
//!
//! The control points go to `bevy_gradmesh` as an open or a closed Catmull-Rom spline, which is
//! baked into frames evenly spaced by arc length. An open curve has a frame on its last point,
//! and a loop wraps around.

use bevy::prelude::*;
use bevy_gradmesh::{BakedSpline, CatmullRomSpline};

/// How many baked frames a stretch between control points gets: the spacing of the frames
/// follows the size of the curve, so a small curve is as smooth as a big one.
const FRAMES_PER_SEGMENT: f32 = 64.0;

/// The spacing of the baked frames of a curve through `points`, with `segments` stretches
/// between them.
fn frame_step(points: &[Vec3], segments: usize) -> f32 {
    let length: f32 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(segments)
        .map(|(from, to)| from.distance(*to))
        .sum();
    if length > 0.0 {
        length / (segments as f32 * FRAMES_PER_SEGMENT)
    } else {
        1.0
    }
}

/// A curve through control points, in the local frame of whatever owns it. It needs at least
/// two points, and a closed curve at least three.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub points: Vec<Vec3>,
    pub closed: bool,
}

impl Curve {
    /// The curve a freehand `stroke` draws: points within `tolerance` of the one before are merged
    /// and points that lie within `tolerance` of a straight line between their neighbours are
    /// dropped. The end points are kept.
    pub fn simplified(stroke: &[Vec3], tolerance: f32) -> Self {
        let mut points: Vec<Vec3> = Vec::new();
        for point in stroke {
            if points
                .last()
                .is_none_or(|last| last.distance(*point) > tolerance)
            {
                points.push(*point);
            }
        }
        if let (Some(end), true) = (stroke.last(), points.len() > 1) {
            *points.last_mut().expect("there are points") = *end;
        }
        Self {
            points: douglas_peucker(&points, tolerance),
            closed: false,
        }
    }

    /// Whether the curve really is a loop.
    fn is_loop(&self) -> bool {
        self.closed && self.points.len() >= 3
    }

    /// The number of stretches between neighbouring control points.
    pub fn segments(&self) -> usize {
        if self.is_loop() {
            self.points.len()
        } else {
            self.points.len().saturating_sub(1)
        }
    }

    /// The point of the curve halfway, in the curve parameter, between the control points of
    /// `segment`.
    pub fn segment_middle(&self, segment: usize) -> Vec3 {
        let spline = self.spline(Vec3::ONE);
        spline.point((segment as f32 + 0.5) / spline.segment_count() as f32)
    }

    /// The curve with control point `index` at `point`.
    pub fn with_point(&self, index: usize, point: Vec3) -> Self {
        let mut curve = self.clone();
        curve.points[index] = point;
        curve
    }

    /// The curve with a new control point on the curve, in the middle of `segment`.
    pub fn with_midpoint_inserted(&self, segment: usize) -> Self {
        let mut curve = self.clone();
        curve
            .points
            .insert(segment + 1, self.segment_middle(segment));
        curve
    }

    /// The spline through the control points, scaled about the origin by `scale`.
    fn spline(&self, scale: Vec3) -> CatmullRomSpline {
        let points: Vec<Vec3> = self.points.iter().map(|point| *point * scale).collect();
        let spline = if self.is_loop() {
            CatmullRomSpline::closed(points)
        } else {
            CatmullRomSpline::open(points)
        };
        spline.expect("a curve has two points, and a loop three")
    }

    /// The curve baked for sampling, with its control points scaled about the origin by `scale`.
    pub fn baked(&self, scale: Vec3) -> Baked {
        let points: Vec<Vec3> = self.points.iter().map(|point| *point * scale).collect();
        let step = frame_step(&points, self.segments());
        Baked(self.spline(scale).bake(step))
    }
}

/// A curve baked for sampling by arc length.
pub struct Baked(BakedSpline);

impl Baked {
    /// The length of the curve.
    pub fn length(&self) -> f32 {
        self.0.length()
    }

    /// The position `distance` along the curve. An open curve ends where it ends and a loop
    /// wraps around.
    pub fn position(&self, distance: f32) -> Vec3 {
        self.0.sample_at_distance(distance).position
    }

    /// The unit direction of the curve `distance` along it.
    pub fn tangent(&self, distance: f32) -> Vec3 {
        self.0.sample_at_distance(distance).forward
    }
}

/// The distance from `point` to the segment from `start` to `end`.
fn distance_to_segment(point: Vec3, start: Vec3, end: Vec3) -> f32 {
    let line = end - start;
    let along =
        ((point - start).dot(line) / line.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    point.distance(start + line * along)
}

/// The points of a polyline that matter at `tolerance`, by the Douglas-Peucker rule.
fn douglas_peucker(points: &[Vec3], tolerance: f32) -> Vec<Vec3> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let (index, farthest) = points
        .iter()
        .enumerate()
        .skip(1)
        .take(points.len() - 2)
        .map(|(index, point)| (index, distance_to_segment(*point, first, last)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("there are points between the ends");
    if farthest <= tolerance {
        return vec![first, last];
    }
    let mut kept = douglas_peucker(&points[..=index], tolerance);
    kept.pop();
    kept.extend(douglas_peucker(&points[index..], tolerance));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(length: f32) -> Curve {
        Curve {
            points: vec![Vec3::ZERO, Vec3::new(length, 0.0, 0.0)],
            closed: false,
        }
    }

    fn bend() -> Curve {
        Curve {
            points: vec![
                Vec3::ZERO,
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(2.0, 0.5, 2.0),
                Vec3::new(4.0, 1.0, 2.0),
            ],
            closed: false,
        }
    }

    fn square() -> Curve {
        Curve {
            points: vec![
                Vec3::new(-1.0, 0.0, -1.0),
                Vec3::new(1.0, 0.0, -1.0),
                Vec3::new(1.0, 0.0, 1.0),
                Vec3::new(-1.0, 0.0, 1.0),
            ],
            closed: true,
        }
    }

    #[test]
    fn a_simplified_stroke_keeps_its_end_points_and_drops_collinear_ones() {
        let stroke: Vec<Vec3> = (0..=20)
            .map(|step| Vec3::new(step as f32 * 0.1, 0.0, 0.001 * (step % 2) as f32))
            .collect();
        let curve = Curve::simplified(&stroke, 0.05);
        assert_eq!(curve.points, vec![stroke[0], stroke[20]]);
        assert!(!curve.closed);
    }

    #[test]
    fn a_simplified_stroke_keeps_its_corners() {
        let mut stroke: Vec<Vec3> = (0..=10)
            .map(|step| Vec3::new(step as f32 * 0.2, 0.0, 0.0))
            .collect();
        stroke.extend((1..=10).map(|step| Vec3::new(2.0, 0.0, step as f32 * 0.2)));
        let curve = Curve::simplified(&stroke, 0.05);
        assert_eq!(
            curve.points,
            vec![
                Vec3::ZERO,
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 2.0)
            ]
        );
    }

    #[test]
    fn a_simplified_stroke_merges_repeated_points_and_still_ends_where_it_ended() {
        let end = Vec3::new(1.0, 0.0, 0.0);
        let stroke = [
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::new(0.5, 0.0, 0.0),
            end,
            end * 1.001,
        ];
        let curve = Curve::simplified(&stroke, 0.01);
        assert_eq!(curve.points.first(), Some(&Vec3::ZERO));
        assert_eq!(curve.points.last(), Some(&(end * 1.001)));
    }

    #[test]
    fn an_open_curve_runs_from_its_first_to_its_last_point() {
        for curve in [line(3.0), bend()] {
            let baked = curve.baked(Vec3::ONE);
            let first = curve.points[0];
            let last = *curve.points.last().unwrap();
            assert!(baked.position(0.0).distance(first) < 1e-3);
            assert!(baked.position(baked.length()).distance(last) < 1e-2);
            assert!(
                baked.position(baked.length() * 2.0).distance(last) < 1e-2,
                "it ends"
            );
            assert!(baked.position(-1.0).distance(first) < 1e-3, "it starts");
        }
    }

    #[test]
    fn a_straight_curve_is_as_long_as_the_distance_between_its_points() {
        let baked = line(3.0).baked(Vec3::ONE);
        assert!((baked.length() - 3.0).abs() < 1e-2, "{}", baked.length());
        let scaled = line(3.0).baked(Vec3::new(2.0, 1.0, 1.0));
        assert!((scaled.length() - 6.0).abs() < 2e-2);
    }

    #[test]
    fn samples_at_equal_distances_are_equally_far_apart() {
        let baked = bend().baked(Vec3::ONE);
        let step = baked.length() / 40.0;
        let positions: Vec<Vec3> = (0..=40).map(|i| baked.position(i as f32 * step)).collect();
        for pair in positions.windows(2) {
            let gap = pair[0].distance(pair[1]);
            assert!((gap - step).abs() < step * 0.2, "{gap} against {step}");
        }
    }

    #[test]
    fn the_tangent_of_a_straight_curve_points_along_it_and_is_a_unit_vector() {
        let baked = line(3.0).baked(Vec3::ONE);
        for distance in [0.0, 1.0, 2.0, 3.0] {
            let tangent = baked.tangent(distance);
            assert!(tangent.distance(Vec3::X) < 1e-2, "{tangent}");
        }
        let bent = bend().baked(Vec3::ONE);
        for i in 0..=20 {
            let tangent = bent.tangent(bent.length() * i as f32 / 20.0);
            assert!((tangent.length() - 1.0).abs() < 1e-3, "{tangent}");
        }
    }

    #[test]
    fn a_raised_control_point_lifts_the_curve() {
        let curve = bend().with_point(1, Vec3::new(2.0, 3.0, 0.0));
        let baked = curve.baked(Vec3::ONE);
        let top = (0..=100)
            .map(|i| baked.position(baked.length() * i as f32 / 100.0).y)
            .fold(0.0, f32::max);
        assert!(top > 2.0, "{top}");
        let scaled = curve.baked(Vec3::new(1.0, 2.0, 1.0));
        let scaled_top = (0..=100)
            .map(|i| scaled.position(scaled.length() * i as f32 / 100.0).y)
            .fold(0.0, f32::max);
        assert!(scaled_top > top * 1.5, "{scaled_top} against {top}");
    }

    #[test]
    fn a_closed_curve_wraps_around() {
        let baked = square().baked(Vec3::ONE);
        assert!(
            baked.length() > 8.0 && baked.length() < 10.0,
            "{}",
            baked.length()
        );
        let start = baked.position(0.0);
        assert!(baked.position(baked.length()).distance(start) < 1e-2);
        assert!(
            baked
                .position(baked.length() + 0.5)
                .distance(baked.position(0.5))
                < 1e-2
        );
        assert!(
            square()
                .points
                .iter()
                .any(|point| point.distance(start) < 1e-3)
        );
    }

    #[test]
    fn a_closed_curve_with_too_few_points_is_open() {
        let curve = Curve {
            points: vec![Vec3::ZERO, Vec3::X],
            closed: true,
        };
        assert_eq!(curve.segments(), 1);
        assert!((curve.baked(Vec3::ONE).length() - 1.0).abs() < 1e-2);
    }

    #[test]
    fn segments_and_midpoints_follow_the_control_points() {
        assert_eq!(bend().segments(), 3);
        assert_eq!(square().segments(), 4);
        // The closing side runs from (-1, -1) to (-1, 1). The Catmull-Rom arms there are 1/3 long,
        // so the middle of the segment, (b0 + 3 b1 + 3 b2 + b3) / 8, bulges out to x = -1.25.
        let middle = square().segment_middle(3);
        assert!(
            middle.distance(Vec3::new(-1.25, 0.0, 0.0)) < 0.01,
            "{middle}"
        );
        assert_eq!(line(2.0).segment_middle(0), Vec3::X);
        let inserted = bend().with_midpoint_inserted(1);
        assert_eq!(inserted.points.len(), 5);
        assert_eq!(inserted.points[2], bend().segment_middle(1));
        assert_eq!(inserted.points[3], bend().points[2]);
    }
}
