//! The track as plain data: a closed Catmull-Rom loop in three dimensions with a width scale at
//! every control point, and the road built from it.
//!
//! Nothing here touches the ECS, so the editor edits a [`TrackDesign`] with the `with_point_*`
//! operations and rebuilds every derived value from it. Between control points the elevation
//! follows the spline itself, while the width scale is a Catmull-Rom interpolated scalar that
//! wraps around the loop. The parameter of [`BakedTrack::sample`] is the fraction of the arc
//! length.

use std::f32::consts::FRAC_1_SQRT_2;

use bevy::prelude::*;
use bevy_gradmesh::{BakedSpline, CatmullRomSpline, Shape, ShapeDetail, SweepFrame, SweepOptions};

/// Spacing of the baked frames the road is built from.
pub const FRAME_STEP: f32 = 0.5;
/// Curvature (1 / radius) below which the road counts as a straight.
const STRAIGHT_CURVATURE: f32 = 1.0 / 150.0;
/// Curvature (1 / radius) below which the road counts as a gentle bend.
const BEND_CURVATURE: f32 = 1.0 / 50.0;

/// The smallest width scale of a control point.
pub const MIN_POINT_WIDTH: f32 = 0.5;
/// The largest width scale of a control point.
pub const MAX_POINT_WIDTH: f32 = 2.0;

/// The control points of the loop the editor starts with, as how far to the right and how far
/// towards the viewer each is on the screen of the map, and how high: a wide loop that fills a
/// screen from the corner.
const INITIAL_CONTROL_POINTS: [(f32, f32, f32); 8] = [
    (200.0, 1.5, 0.0),
    (127.0, 2.5, 51.0),
    (0.0, 0.5, 84.0),
    (-134.0, 1.5, 54.0),
    (-200.0, 2.5, 0.0),
    (-127.0, 1.0, -51.0),
    (0.0, 0.5, -84.0),
    (134.0, 1.0, -54.0),
];

/// The point `right` metres to the right of the middle of the map, `height` metres up and
/// `toward` metres towards the viewer, for a map that is seen from the corner at +X and +Z.
pub fn on_the_map(right: f32, height: f32, toward: f32) -> Vec3 {
    Vec3::new(
        FRAC_1_SQRT_2 * (right + toward),
        height,
        FRAC_1_SQRT_2 * (toward - right),
    )
}

/// One control point of the loop: where the road passes and how wide it is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoint {
    /// Where the road centerline passes; `y` is the elevation.
    pub position: Vec3,
    /// Scales the curvature-based road width here: `1.0` is the width the bends give.
    pub width: f32,
}

impl ControlPoint {
    /// A control point at `position` with the usual width.
    pub const fn new(position: Vec3) -> Self {
        Self {
            position,
            width: 1.0,
        }
    }
}

/// The width of the road on straights, in gentle bends and in tight corners, before the width
/// scale of the control points is applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadWidths {
    /// Width where the curvature is below `1 / 150`.
    pub straight: f32,
    /// Width where the curvature is below `1 / 50`.
    pub bend: f32,
    /// Width everywhere else.
    pub corner: f32,
}

impl Default for RoadWidths {
    fn default() -> Self {
        Self {
            straight: 30.0,
            bend: 26.0,
            corner: 22.0,
        }
    }
}

impl RoadWidths {
    /// The road width at a signed `curvature` (1 / radius).
    pub fn at(self, curvature: f32) -> f32 {
        match curvature.abs() {
            c if c < STRAIGHT_CURVATURE => self.straight,
            c if c < BEND_CURVATURE => self.bend,
            _ => self.corner,
        }
    }
}

/// The road at one place: where it is, how it is oriented and how wide it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadFrame {
    /// The point of the centerline.
    pub position: Vec3,
    /// Unit direction of travel.
    pub tangent: Vec3,
    /// Unit normal of the road surface.
    pub up: Vec3,
    /// Unit vector across the road towards the right edge, as seen along the tangent.
    pub right: Vec3,
    /// The width of the road, with the width scale applied.
    pub width: f32,
}

impl RoadFrame {
    fn oriented(position: Vec3, tangent: Vec3, width: f32) -> Self {
        let right = tangent.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let up = right.cross(tangent).normalize_or(Vec3::Y);
        Self {
            position,
            tangent,
            up,
            right,
            width,
        }
    }

    /// The point of the road surface `lateral` units to the right of the centerline.
    pub fn surface_point(&self, lateral: f32) -> Vec3 {
        self.position + self.right * lateral
    }

    /// The edge of the road: `1.0` is the right one, `-1.0` the left one.
    pub fn edge(&self, side: f32) -> Vec3 {
        self.surface_point(side * self.width / 2.0)
    }
}

/// Everything that defines a track: the control points of the closed loop and the road widths
/// the road is derived from.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackDesign {
    /// The control points of the closed Catmull-Rom loop, at least three; the last connects
    /// back to the first.
    pub control_points: Vec<ControlPoint>,
    /// The tightest turn (world units) the road may take in plan; the spline relaxes anything
    /// sharper.
    pub min_turn_radius: f32,
    /// The road width for each kind of curvature.
    pub widths: RoadWidths,
}

impl TrackDesign {
    /// The track the editor starts with.
    pub fn initial() -> Self {
        Self {
            control_points: INITIAL_CONTROL_POINTS
                .into_iter()
                .map(|(right, height, toward)| ControlPoint::new(on_the_map(right, height, toward)))
                .collect(),
            min_turn_radius: 14.0,
            widths: RoadWidths::default(),
        }
    }

    /// The loop the road follows: the control points with tight corners straightened in plan,
    /// with the width scale as a channel.
    pub fn spline(&self) -> CatmullRomSpline {
        let points: Vec<Vec3> = self.control_points.iter().map(|p| p.position).collect();
        let widths: Vec<f32> = self.control_points.iter().map(|p| p.width).collect();
        CatmullRomSpline::closed(points)
            .expect("a track has at least three control points")
            .with_channel(&widths)
            .relaxed(self.min_turn_radius)
    }

    /// The road at every control point, in order.
    pub fn control_frames(&self) -> Vec<RoadFrame> {
        let spline = self.spline();
        let count = self.control_points.len();
        (0..count)
            .map(|index| {
                let t = index as f32 / count as f32;
                let scale = spline
                    .channel(0, t)
                    .map_or(1.0, |w| w.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH));
                RoadFrame::oriented(
                    spline.point(t),
                    spline.tangent(t),
                    self.widths.at(spline.curvature(t)) * scale,
                )
            })
            .collect()
    }

    /// Bakes the loop into arc-length-spaced frames for even sampling.
    pub fn bake(&self) -> BakedTrack {
        BakedTrack::from_baked(self.spline().bake(FRAME_STEP), self.widths)
    }

    /// The road: a thin strip swept along the frames of the baked loop, as wide as they say.
    pub fn road_shape(&self) -> Shape {
        let frames: Vec<SweepFrame> = self
            .bake()
            .frames()
            .iter()
            .map(|frame| {
                SweepFrame::new(frame.position, -frame.right, frame.up)
                    .with_scale(Vec2::new(frame.width, 1.0))
            })
            .collect();
        let thickness = 0.01;
        let profile = vec![
            Vec2::new(-0.5, -thickness),
            Vec2::new(0.5, -thickness),
            Vec2::new(0.5, 0.0),
            Vec2::new(-0.5, 0.0),
        ];
        Shape::sweep_frames(
            profile,
            &frames,
            SweepOptions::default()
                .with_closed(true)
                .with_detail(ShapeDetail::Low),
        )
    }

    /// A copy with control point `index` moved to `position`, elevation included. Out-of-range
    /// indices and non-finite positions leave the design unchanged, and so does
    /// [`with_point_width`](Self::with_point_width).
    #[must_use]
    pub fn with_point_moved(&self, index: usize, position: Vec3) -> Self {
        self.with_point(index, |point| {
            if position.is_finite() {
                point.position = position;
            }
        })
    }

    /// A copy with the width scale of control point `index` set to `width`, clamped to
    /// [`MIN_POINT_WIDTH`] to [`MAX_POINT_WIDTH`].
    #[must_use]
    pub fn with_point_width(&self, index: usize, width: f32) -> Self {
        self.with_point(index, |point| {
            if !width.is_nan() {
                point.width = width.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH);
            }
        })
    }

    fn with_point(&self, index: usize, edit: impl FnOnce(&mut ControlPoint)) -> Self {
        let mut design = self.clone();
        if let Some(point) = design.control_points.get_mut(index) {
            edit(point);
        }
        design
    }
}

/// A track baked into arc-length-spaced road frames for even sampling.
///
/// Wraps [`BakedSpline`] so the spline handles frame interpolation, channel sampling and
/// arc-length lookup, while the track layer adds curvature-based road width on top.
#[derive(Clone, Debug)]
pub struct BakedTrack {
    baked: BakedSpline,
    widths: RoadWidths,
}

impl BakedTrack {
    fn from_baked(baked: BakedSpline, widths: RoadWidths) -> Self {
        Self { baked, widths }
    }

    /// Total loop length in world units.
    pub fn length(&self) -> f32 {
        self.baked.length()
    }

    /// The frames of the road, computed on demand.
    pub fn frames(&self) -> Vec<RoadFrame> {
        self.baked
            .frames()
            .iter()
            .map(|frame| self.frame_from_spline(frame))
            .collect()
    }

    fn frame_from_spline(&self, frame: &bevy_gradmesh::SplineFrame) -> RoadFrame {
        let scale = self
            .baked
            .channel_at_distance(0, frame.distance)
            .map_or(1.0, |w| w.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH));
        RoadFrame::oriented(
            frame.position,
            frame.forward,
            self.widths.at(frame.curvature) * scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_initial_track_is_a_loop_of_at_least_three_points() {
        let design = TrackDesign::initial();
        assert!(design.control_points.len() >= 3);
        let baked = design.bake();
        assert!(baked.length() > 800.0);
        let frames = baked.frames();
        let (start, end) = (frames[0], frames[frames.len() - 1]);
        assert!(
            start.position.distance(end.position) < 2.0 * FRAME_STEP,
            "it closes"
        );
    }

    #[test]
    fn a_wider_control_point_widens_the_road_there_and_only_there() {
        let design = TrackDesign::initial();
        let wider = design.with_point_width(2, 2.0);
        let (before, after) = (design.control_frames(), wider.control_frames());
        assert!((after[2].width / before[2].width - 2.0).abs() < 1e-3);
        assert!((after[6].width - before[6].width).abs() < 1e-3);
        let widest = |track: &TrackDesign| {
            track
                .bake()
                .frames()
                .iter()
                .fold(0.0, |widest, frame| f32::max(widest, frame.width))
        };
        assert!(widest(&wider) > widest(&design) * 1.5);
    }

    #[test]
    fn the_width_stays_within_its_limits_and_a_bad_edit_changes_nothing() {
        let design = TrackDesign::initial();
        assert_eq!(
            design.with_point_width(1, 50.0).control_points[1].width,
            MAX_POINT_WIDTH
        );
        assert_eq!(
            design.with_point_width(1, 0.0).control_points[1].width,
            MIN_POINT_WIDTH
        );
        assert_eq!(design.with_point_width(1, f32::NAN), design);
        assert_eq!(design.with_point_width(99, 1.5), design);
        assert_eq!(design.with_point_moved(1, Vec3::NAN), design);
        assert_eq!(design.with_point_moved(99, Vec3::ONE), design);
    }

    #[test]
    fn moving_a_control_point_bends_the_road_through_it() {
        let design = TrackDesign::initial();
        let target = design.control_points[2].position + Vec3::new(40.0, 0.0, 0.0);
        let moved = design.with_point_moved(2, target);
        let frame = moved.control_frames()[2];
        assert!(frame.position.distance(target) < 1.0, "{}", frame.position);
    }

    #[test]
    fn the_road_shape_is_a_mesh_with_vertices() {
        let mesh = TrackDesign::initial().road_shape().build();
        assert!(mesh.count_vertices() > 1000);
    }

    #[test]
    fn the_edges_of_a_frame_are_a_width_apart_across_the_road() {
        let frame = TrackDesign::initial().control_frames()[3];
        let span = frame.edge(1.0).distance(frame.edge(-1.0));
        assert!((span - frame.width).abs() < 1e-3);
        assert!(frame.right.dot(frame.tangent).abs() < 1e-3);
    }
}
