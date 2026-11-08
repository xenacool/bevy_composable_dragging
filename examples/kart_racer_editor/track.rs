//! The track as plain data: a closed Catmull-Rom loop in three dimensions, with a width and a
//! banking at every control point, and everything built from it.
//!
//! Ported from the `bevy_gradmesh` kart racer example, whose hard-coded loop, road widths,
//! checkpoints, coins and scenery the tests measure against. Nothing here touches the
//! ECS, so a track editor can edit a [`TrackDesign`] with the `with_point_*` operations, ask
//! [`TrackDesign::validate`] whether it can be driven and rebuild every derived value from it.
//!
//! Between control points the elevation follows the spline itself, while the width and the roll
//! are Catmull-Rom interpolated scalars that wrap around the loop. The parameter of
//! [`BakedTrack::sample`] is the fraction of the arc length.

use std::f32::consts::PI;
use std::fmt;

use bevy::prelude::*;
use bevy_gradmesh::{BakedSpline, CatmullRomSpline, Shape, ShapeDetail, SweepFrame, SweepOptions};

use crate::kart::KART_WIDTH;

/// Spacing of the baked frames the road, barriers and physics are built from.
pub const FRAME_STEP: f32 = 0.5;
/// Spacing of the frames used to measure how close two parts of the road come.
const CLEARANCE_STEP: f32 = 2.0;
/// How high a coin rests above the road surface.
const COIN_HOVER: f32 = 0.8;
/// How many trees the scenery has.
pub const TREE_COUNT: usize = 24;
/// How many bushes the scenery has.
pub const BUSH_COUNT: usize = 8;
/// Free space between the road edge and scattered scenery.
const DECORATION_CLEARANCE: f32 = 12.0;
/// How far beyond the clearance scattered scenery may stand.
const DECORATION_SPREAD: f32 = 60.0;
/// How often a scattered piece of scenery is placed again when it lands on a road.
const PLACEMENT_ATTEMPTS: usize = 16;
/// Curvature (1 / radius) below which the road counts as a straight.
const STRAIGHT_CURVATURE: f32 = 1.0 / 150.0;
/// Curvature (1 / radius) below which the road counts as a gentle bend.
const BEND_CURVATURE: f32 = 1.0 / 50.0;
/// The fraction of its length a loop has to keep when its tight corners are straightened;
/// straightening shrinks a loop that is far too sharp into nothing.
const MIN_STRAIGHTENED_SIZE: f32 = 0.5;

/// The fewest control points a loop can have.
pub const MIN_POINTS: usize = 3;
/// The smallest width scale of a control point.
pub const MIN_POINT_WIDTH: f32 = 0.5;
/// The largest width scale of a control point.
pub const MAX_POINT_WIDTH: f32 = 2.0;
/// The largest banking of a control point, in radians either way.
pub const MAX_ROLL: f32 = 0.6;
/// The narrowest road the kart can be driven on: three kart widths.
pub const MIN_ROAD_WIDTH: f32 = 3.0 * KART_WIDTH;
/// How far apart in height two crossing roads must be for a bridge to be drivable.
pub const BRIDGE_CLEARANCE: f32 = 6.0;
/// The steepest slope (rise over run) the road may have.
pub const MAX_GRADE: f32 = 0.6;

/// The hand-placed control points of the upstream loop; tight corners are straightened later.
const UPSTREAM_CONTROL_POINTS: [Vec3; 23] = [
    Vec3::new(0.0, 0.5, 100.0),
    Vec3::new(80.0, 0.8, 100.0),
    Vec3::new(160.0, 1.2, 100.0),
    Vec3::new(240.0, 1.5, 80.0),
    Vec3::new(280.0, 1.8, 20.0),
    Vec3::new(260.0, 1.5, -60.0),
    Vec3::new(200.0, 0.8, -100.0),
    Vec3::new(120.0, 0.3, -80.0),
    Vec3::new(80.0, -0.2, -120.0),
    Vec3::new(40.0, 0.2, -180.0),
    Vec3::new(-20.0, 0.8, -200.0),
    Vec3::new(-100.0, 1.3, -180.0),
    Vec3::new(-160.0, 1.6, -220.0),
    Vec3::new(-200.0, 1.0, -160.0),
    Vec3::new(-240.0, 0.3, -200.0),
    Vec3::new(-280.0, -0.3, -140.0),
    Vec3::new(-300.0, -0.5, -60.0),
    Vec3::new(-280.0, -0.2, 20.0),
    Vec3::new(-240.0, 0.3, 80.0),
    Vec3::new(-180.0, 0.8, 120.0),
    Vec3::new(-100.0, 1.0, 100.0),
    Vec3::new(-40.0, 0.8, 140.0),
    Vec3::new(20.0, 0.6, 120.0),
];

/// The hand-placed trees of the upstream scenery.
const UPSTREAM_TREES: [Vec3; TREE_COUNT] = [
    Vec3::new(-250.0, 0.0, -250.0),
    Vec3::new(-280.0, 0.0, -230.0),
    Vec3::new(-230.0, 0.0, -270.0),
    Vec3::new(-100.0, 0.0, -260.0),
    Vec3::new(50.0, 0.0, -270.0),
    Vec3::new(200.0, 0.0, -250.0),
    Vec3::new(250.0, 0.0, -200.0),
    Vec3::new(320.0, 0.0, -50.0),
    Vec3::new(340.0, 0.0, 30.0),
    Vec3::new(250.0, 0.0, 180.0),
    Vec3::new(300.0, 0.0, 200.0),
    Vec3::new(100.0, 0.0, 200.0),
    Vec3::new(150.0, 0.0, 220.0),
    Vec3::new(-150.0, 0.0, 200.0),
    Vec3::new(-180.0, 0.0, 180.0),
    Vec3::new(-340.0, 0.0, -30.0),
    Vec3::new(-330.0, 0.0, 50.0),
    Vec3::new(100.0, 0.0, -150.0),
    Vec3::new(-80.0, 0.0, -130.0),
    Vec3::new(150.0, 0.0, 50.0),
    Vec3::new(-120.0, 0.0, 50.0),
    Vec3::new(-200.0, 0.0, -100.0),
    Vec3::new(80.0, 0.0, -200.0),
    Vec3::new(180.0, 0.0, 150.0),
];

/// The hand-placed bushes of the upstream scenery.
const UPSTREAM_BUSHES: [Vec3; BUSH_COUNT] = [
    Vec3::new(-220.0, 0.0, -220.0),
    Vec3::new(-80.0, 0.0, -240.0),
    Vec3::new(180.0, 0.0, -230.0),
    Vec3::new(300.0, 0.0, -30.0),
    Vec3::new(220.0, 0.0, 180.0),
    Vec3::new(80.0, 0.0, 220.0),
    Vec3::new(-120.0, 0.0, 160.0),
    Vec3::new(-260.0, 0.0, -10.0),
];

/// One control point of the loop: where the road passes, how wide it is and how it leans.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoint {
    /// Where the road centerline passes; `y` is the elevation.
    pub position: Vec3,
    /// Scales the curvature-based road width here: `1.0` keeps the width of the upstream track.
    pub width: f32,
    /// Banking in radians about the direction of travel; positive lowers the right edge.
    pub roll: f32,
}

impl ControlPoint {
    /// A control point at `position` with the upstream width and no banking.
    pub const fn new(position: Vec3) -> Self {
        Self {
            position,
            width: 1.0,
            roll: 0.0,
        }
    }
}

/// The width of the road on straights, in gentle bends and in tight corners, before the width
/// scale of the control points is applied.
///
/// Roads are wider on straights and narrower in corners.
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
            straight: 16.0,
            bend: 14.0,
            corner: 12.0,
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

    /// The widest the road gets at width scale one.
    pub fn widest(self) -> f32 {
        self.straight.max(self.bend).max(self.corner)
    }

    fn all_positive(self) -> bool {
        [self.straight, self.bend, self.corner]
            .into_iter()
            .all(is_positive)
    }
}

/// What a piece of scenery is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    /// A trunk with a wobbling canopy.
    Tree,
    /// A small pulsing bush.
    Bush,
}

/// Where a piece of scenery stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorationSite {
    /// What stands there.
    pub kind: DecorationKind,
    /// The spot on the ground.
    pub position: Vec3,
}

/// An arch the kart has to drive through, standing on the road.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    /// The road surface at the gate, turned to face along the road and leaning with it.
    pub transform: Transform,
    /// The width of the road at the gate.
    pub width: f32,
    /// How far along the road the gate is, in metres from the start line.
    pub distance: f32,
}

/// The road at one place: where it is, how it is oriented and how wide it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadFrame {
    /// The point of the centerline.
    pub position: Vec3,
    /// Unit direction of travel.
    pub tangent: Vec3,
    /// Unit normal of the road surface, leaning with the roll.
    pub up: Vec3,
    /// Unit vector across the road towards the right edge, as seen along the tangent.
    pub right: Vec3,
    /// The width of the road, with the width scale applied.
    pub width: f32,
    /// The banking in radians; positive lowers the right edge.
    pub roll: f32,
    /// Signed horizontal curvature (1 / radius); positive turns left.
    pub curvature: f32,
}

impl RoadFrame {
    fn oriented(position: Vec3, tangent: Vec3, curvature: f32, width: f32, roll: f32) -> Self {
        let level_right = tangent.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let level_up = level_right.cross(tangent).normalize_or(Vec3::Y);
        let (sin, cos) = roll.sin_cos();
        Self {
            position,
            tangent,
            up: level_up * cos + level_right * sin,
            right: level_right * cos - level_up * sin,
            width,
            roll,
            curvature,
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

    /// The orientation that faces along the road like the kart model does (`+Z` forward) with
    /// `+Y` up.
    pub fn rotation(&self) -> Quat {
        Quat::from_mat3(&Mat3::from_cols(-self.right, self.up, self.tangent))
    }
}

/// Why a [`TrackDesign`] cannot be driven.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackError {
    /// A closed loop needs at least [`MIN_POINTS`] control points.
    TooFewPoints(usize),
    /// A setting is not a positive, finite number or a control point is not finite.
    InvalidParameter(&'static str),
    /// The width scale of a control point is outside [`MIN_POINT_WIDTH`] to [`MAX_POINT_WIDTH`].
    WidthOutOfRange(usize),
    /// The roll of a control point is beyond [`MAX_ROLL`] either way.
    RollTooLarge(usize),
    /// Two control points are closer than the widest road in plan and at the same height.
    PointsTooClose {
        /// Index of the first control point.
        a: usize,
        /// Index of the second control point.
        b: usize,
    },
    /// The loop bends tighter than the minimum turn radius in plan, even after straightening it,
    /// or straightening it would shrink it to less than half its length.
    TooSharp {
        /// The tightest turn radius of the loop.
        radius: f32,
    },
    /// The road somewhere is narrower than [`MIN_ROAD_WIDTH`].
    RoadTooNarrow {
        /// The narrowest width.
        width: f32,
    },
    /// The road somewhere climbs more steeply than [`MAX_GRADE`].
    TooSteep {
        /// The steepest slope, rise over run.
        grade: f32,
    },
    /// Two parts of the road that are far apart along the loop overlap in plan without a
    /// [`BRIDGE_CLEARANCE`] between them.
    SegmentsTooClose {
        /// Distance along the loop of the first part.
        a: f32,
        /// Distance along the loop of the second part.
        b: f32,
    },
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewPoints(count) => write!(
                f,
                "a closed track needs at least {MIN_POINTS} control points, got {count}"
            ),
            Self::InvalidParameter(what) => write!(f, "invalid track setting: {what}"),
            Self::WidthOutOfRange(index) => write!(
                f,
                "the width of control point {index} is outside {MIN_POINT_WIDTH} to {MAX_POINT_WIDTH}"
            ),
            Self::RollTooLarge(index) => write!(
                f,
                "the roll of control point {index} is beyond {MAX_ROLL} radians"
            ),
            Self::PointsTooClose { a, b } => write!(
                f,
                "control points {a} and {b} are closer than the road is wide"
            ),
            Self::TooSharp { radius } => {
                write!(f, "the track bends with a radius of only {radius:.1}")
            }
            Self::RoadTooNarrow { width } => write!(
                f,
                "the road is only {width:.1} wide, the kart needs {MIN_ROAD_WIDTH:.1}"
            ),
            Self::TooSteep { grade } => {
                write!(
                    f,
                    "the road climbs with a grade of {grade:.2}, at most {MAX_GRADE}"
                )
            }
            Self::SegmentsTooClose { a, b } => write!(
                f,
                "the road at {a:.0} and at {b:.0} along the track overlaps without a bridge"
            ),
        }
    }
}

impl std::error::Error for TrackError {}

/// Everything that defines a track: the control points of the closed loop and the settings the
/// road, checkpoints, coins and scenery are derived from.
///
/// The default is the track of the upstream example: its control points with their upstream
/// heights, the upstream road widths, no banking and a width scale of one everywhere.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct TrackDesign {
    /// The control points of the closed Catmull-Rom loop; the last connects back to the first.
    pub control_points: Vec<ControlPoint>,
    /// The tightest turn (world units) the road may take in plan; the spline relaxes anything
    /// sharper.
    pub min_turn_radius: f32,
    /// The road width for each kind of curvature.
    pub widths: RoadWidths,
    /// How many checkpoint arches stand along the loop; the first one is the start line.
    pub checkpoint_count: usize,
    /// How many coins hover along the loop.
    pub coin_count: usize,
    /// Scatters the scenery randomly with this seed; `None` keeps the hand-placed scenery of the
    /// upstream track.
    pub decoration_seed: Option<u64>,
}

impl TrackDesign {
    /// A design without control points, with the usual settings of a track. It cannot be driven
    /// until it has [`MIN_POINTS`] points. There is no default design: whoever builds a world
    /// says which track it has.
    pub fn empty() -> Self {
        Self {
            control_points: Vec::new(),
            min_turn_radius: 14.0,
            widths: RoadWidths::default(),
            checkpoint_count: 8,
            coin_count: 24,
            decoration_seed: None,
        }
    }

    /// The track of the `bevy_gradmesh` kart racer example, which the tests measure against.
    pub fn upstream() -> Self {
        Self {
            control_points: UPSTREAM_CONTROL_POINTS
                .into_iter()
                .map(ControlPoint::new)
                .collect(),
            ..Self::empty()
        }
    }

    /// The spline with width and roll channels added at the control points.
    ///
    /// Channel 0 is the width scale, channel 1 is the roll in radians.
    fn spline_with_channels(&self) -> CatmullRomSpline {
        let points: Vec<Vec3> = self.control_points.iter().map(|p| p.position).collect();
        let widths: Vec<f32> = self.control_points.iter().map(|p| p.width).collect();
        let rolls: Vec<f32> = self.control_points.iter().map(|p| p.roll).collect();
        CatmullRomSpline::closed(points)
            .with_channel(&widths)
            .with_channel(&rolls)
    }

    /// The loop the kart drives: the control points with tight corners straightened in plan,
    /// with width and roll channels.
    ///
    /// # Panics
    /// Panics if there are fewer than [`MIN_POINTS`] control points; see
    /// [`TrackDesign::validate`].
    pub fn spline(&self) -> CatmullRomSpline {
        self.spline_with_channels().relaxed(self.min_turn_radius)
    }

    /// The road width at a signed `curvature` (1 / radius) at width scale one.
    pub fn road_width(&self, curvature: f32) -> f32 {
        self.widths.at(curvature)
    }

    /// The road at the spline parameter `t`, which wraps around `[0, 1)`; control point `i` of
    /// `n` sits at `i / n`.
    #[cfg(test)]
    pub fn sample_frame(&self, t: f32) -> RoadFrame {
        let spline = self.spline();
        let position = spline.point(t);
        let tangent = spline.tangent(t);
        let curvature = spline.curvature(t);
        let width_scale = spline
            .channel(0, t)
            .map(|w| w.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH))
            .unwrap_or(1.0);
        let roll = spline
            .channel(1, t)
            .map(|r| r.clamp(-MAX_ROLL, MAX_ROLL))
            .unwrap_or(0.0);
        let width = self.widths.at(curvature) * width_scale;
        RoadFrame::oriented(position, tangent, curvature, width, roll)
    }

    /// The road at every control point, in order.
    ///
    /// # Panics
    /// Panics if there are fewer than [`MIN_POINTS`] control points.
    pub fn control_frames(&self) -> Vec<RoadFrame> {
        let spline = self.spline();
        let count = self.control_points.len();
        (0..count)
            .map(|i| {
                let t = i as f32 / count as f32;
                let position = spline.point(t);
                let tangent = spline.tangent(t);
                let curvature = spline.curvature(t);
                let width_scale = spline
                    .channel(0, t)
                    .map_or(1.0, |w| w.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH));
                let roll = spline
                    .channel(1, t)
                    .map_or(0.0, |r| r.clamp(-MAX_ROLL, MAX_ROLL));
                let width = self.widths.at(curvature) * width_scale;
                RoadFrame::oriented(position, tangent, curvature, width, roll)
            })
            .collect()
    }

    /// Bakes the loop into arc-length-spaced frames for fast, even sampling.
    ///
    /// # Panics
    /// Panics if there are fewer than [`MIN_POINTS`] control points; see
    /// [`TrackDesign::validate`].
    pub fn bake(&self) -> BakedTrack {
        let baked = self.spline().bake(FRAME_STEP);
        BakedTrack::from_baked(&baked, self)
    }

    fn bake_for_clearance(&self) -> BakedTrack {
        let baked = self
            .spline_with_channels()
            .relaxed(self.min_turn_radius)
            .bake(CLEARANCE_STEP);
        BakedTrack::from_baked(&baked, self)
    }

    /// Builds the road shape from the baked spline using [`Shape::sweep_frames`].
    ///
    /// The profile is a thin rectangle; width and roll come from the spline channels,
    /// with curvature-based width modulation applied at each frame.
    pub fn road_shape(&self) -> Shape {
        let baked = self.spline().bake(FRAME_STEP);
        let frames = BakedTrack::sweep_frames_for(&baked, self);
        let profile = road_profile();
        Shape::sweep_frames(
            profile,
            &frames,
            SweepOptions::default()
                .with_closed(true)
                .with_detail(ShapeDetail::Low),
        )
    }

    /// The checkpoint arches, evenly spaced along the loop starting at its beginning.
    pub fn checkpoint_gates(&self, sampler: &BakedTrack) -> Vec<Gate> {
        (0..self.checkpoint_count)
            .map(|i| {
                let frame = sampler.sample(i as f32 / self.checkpoint_count as f32);
                Gate {
                    transform: Transform::from_translation(frame.position)
                        .with_rotation(frame.rotation()),
                    width: frame.width,
                    distance: sampler.length() * i as f32 / self.checkpoint_count as f32,
                }
            })
            .collect()
    }

    /// The resting poses of the coins, evenly spaced along the centerline and hovering just
    /// above the road surface, standing across the road.
    pub fn coin_transforms(&self, sampler: &BakedTrack) -> Vec<Transform> {
        (0..self.coin_count)
            .map(|i| {
                let frame = sampler.sample((i as f32 + 0.5) / self.coin_count as f32);
                Transform::from_translation(frame.position + frame.up * COIN_HOVER)
                    .with_rotation(frame.rotation() * Quat::from_rotation_x(PI / 2.0))
            })
            .collect()
    }

    /// The scenery beside the road: the hand-placed upstream scenery, or a seeded scatter that
    /// keeps clear of every part of the road.
    pub fn decoration_sites(&self, sampler: &BakedTrack) -> Vec<DecorationSite> {
        match self.decoration_seed {
            None => UPSTREAM_TREES
                .iter()
                .map(|&position| DecorationSite {
                    kind: DecorationKind::Tree,
                    position,
                })
                .chain(UPSTREAM_BUSHES.iter().map(|&position| DecorationSite {
                    kind: DecorationKind::Bush,
                    position,
                }))
                .collect(),
            Some(seed) => self.scattered_sites(sampler, seed),
        }
    }

    fn scattered_sites(&self, sampler: &BakedTrack, seed: u64) -> Vec<DecorationSite> {
        let mut random = SplitMix(seed);
        let clearance = self.widths.widest() * MAX_POINT_WIDTH / 2.0 + DECORATION_CLEARANCE;
        let mut sites = Vec::with_capacity(TREE_COUNT + BUSH_COUNT);
        for index in 0..TREE_COUNT + BUSH_COUNT {
            let kind = if index < TREE_COUNT {
                DecorationKind::Tree
            } else {
                DecorationKind::Bush
            };
            for _ in 0..PLACEMENT_ATTEMPTS {
                let frame = sampler.sample_at_distance(random.unit() * sampler.length());
                let side = if random.unit() < 0.5 { -1.0 } else { 1.0 };
                let reach = clearance + random.unit() * DECORATION_SPREAD;
                let position = (frame.position + frame.right * side * reach).with_y(0.0);
                let nearest = sampler
                    .frames()
                    .iter()
                    .map(|f| f.position.xz().distance(position.xz()))
                    .fold(f32::MAX, f32::min);
                if nearest >= clearance {
                    sites.push(DecorationSite { kind, position });
                    break;
                }
            }
        }
        sites
    }

    /// A copy with control point `index` moved to `position`, elevation included.
    ///
    /// Out-of-range indices and non-finite positions leave the design unchanged, and so do the
    /// other `with_point_*` operations.
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

    /// A copy with the roll of control point `index` set to `roll` radians, clamped to
    /// plus or minus [`MAX_ROLL`].
    #[must_use]
    pub fn with_point_roll(&self, index: usize, roll: f32) -> Self {
        self.with_point(index, |point| {
            if !roll.is_nan() {
                point.roll = roll.clamp(-MAX_ROLL, MAX_ROLL);
            }
        })
    }

    /// A copy with control point `index` raised by `dy`, or lowered if it is negative.
    #[must_use]
    pub fn with_point_raised(&self, index: usize, dy: f32) -> Self {
        self.with_point(index, |point| {
            if dy.is_finite() {
                point.position.y += dy;
            }
        })
    }

    /// A copy with a new control point on the curve halfway along the segment that follows
    /// `after_index`, with the elevation, width and roll interpolated there. The new point has
    /// index `after_index + 1`.
    #[must_use]
    pub fn with_point_inserted(&self, after_index: usize) -> Self {
        let count = self.control_points.len();
        if after_index >= count || count < MIN_POINTS {
            return self.clone();
        }
        let t = (after_index as f32 + 0.5) / count as f32;
        let spline = self.spline();
        let width = spline
            .channel(0, t)
            .unwrap_or(1.0)
            .clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH);
        let roll = spline
            .channel(1, t)
            .unwrap_or(0.0)
            .clamp(-MAX_ROLL, MAX_ROLL);
        let mut design = self.clone();
        design.control_points.insert(
            after_index + 1,
            ControlPoint {
                position: spline.point(t),
                width,
                roll,
            },
        );
        design
    }

    /// A copy with a new control point at `position` after the last one, with the standard
    /// width and no banking. This is how a track is laid down from nothing; it is unchanged
    /// when `position` is not finite.
    #[must_use]
    pub fn with_point_appended(&self, position: Vec3) -> Self {
        let mut design = self.clone();
        if position.is_finite() {
            design.control_points.push(ControlPoint::new(position));
        }
        design
    }

    /// A copy without control point `index`, unless that would leave fewer than
    /// [`MIN_POINTS`].
    #[must_use]
    pub fn with_point_removed(&self, index: usize) -> Self {
        let mut design = self.clone();
        if index < design.control_points.len() && design.control_points.len() > MIN_POINTS {
            design.control_points.remove(index);
        }
        design
    }

    fn with_point(&self, index: usize, edit: impl FnOnce(&mut ControlPoint)) -> Self {
        let mut design = self.clone();
        if let Some(point) = design.control_points.get_mut(index) {
            edit(point);
        }
        design
    }

    /// Checks that the track can be driven.
    ///
    /// The loop is closed by construction. It is drivable when the settings make sense, every
    /// control point has a width scale and a roll within their limits, no two control points at
    /// the same height are closer than the widest road, no turn of the straightened loop is
    /// tighter than the minimum turn radius in plan, the road is nowhere narrower than
    /// [`MIN_ROAD_WIDTH`] or steeper than [`MAX_GRADE`], and no two parts of the road that are
    /// far apart along the loop overlap in plan unless they are [`BRIDGE_CLEARANCE`] apart in
    /// height.
    pub fn validate(&self) -> Result<(), TrackError> {
        if self.control_points.len() < MIN_POINTS {
            return Err(TrackError::TooFewPoints(self.control_points.len()));
        }
        if !self
            .control_points
            .iter()
            .all(|point| point.position.is_finite())
        {
            return Err(TrackError::InvalidParameter(
                "control points must be finite",
            ));
        }
        if !is_positive(self.min_turn_radius) {
            return Err(TrackError::InvalidParameter(
                "the minimum turn radius must be positive",
            ));
        }
        if !self.widths.all_positive() {
            return Err(TrackError::InvalidParameter("road widths must be positive"));
        }
        if self.checkpoint_count < 2 {
            return Err(TrackError::InvalidParameter(
                "a lap needs at least two checkpoints",
            ));
        }
        for (index, point) in self.control_points.iter().enumerate() {
            if !(MIN_POINT_WIDTH..=MAX_POINT_WIDTH).contains(&point.width) {
                return Err(TrackError::WidthOutOfRange(index));
            }
            if !(-MAX_ROLL..=MAX_ROLL).contains(&point.roll) {
                return Err(TrackError::RollTooLarge(index));
            }
        }

        let widest = self.widths.widest() * MAX_POINT_WIDTH;
        for (a, first) in self.control_points.iter().enumerate() {
            for (b, second) in self.control_points.iter().enumerate().skip(a + 1) {
                let level = (first.position.y - second.position.y).abs() < BRIDGE_CLEARANCE;
                let close = first.position.xz().distance(second.position.xz())
                    < widest * first.width.max(second.width) / MAX_POINT_WIDTH;
                if level && close {
                    return Err(TrackError::PointsTooClose { a, b });
                }
            }
        }

        let raw = self.spline_with_channels();
        let relaxed = raw.clone().relaxed(self.min_turn_radius);
        if plan_length(relaxed.points()) < MIN_STRAIGHTENED_SIZE * plan_length(raw.points()) {
            return Err(TrackError::TooSharp {
                radius: raw.min_turn_radius(),
            });
        }
        let radius = relaxed.min_turn_radius();
        if radius < self.min_turn_radius {
            return Err(TrackError::TooSharp { radius });
        }

        self.check_road(&self.bake_for_clearance())
    }

    fn check_road(&self, baked: &BakedTrack) -> Result<(), TrackError> {
        let frames = baked.frames();
        let narrowest = frames.iter().map(|f| f.width).fold(f32::MAX, f32::min);
        if narrowest < MIN_ROAD_WIDTH {
            return Err(TrackError::RoadTooNarrow { width: narrowest });
        }
        let steepest = frames
            .iter()
            .map(|f| f.tangent.y.abs() / f.tangent.xz().length().max(1e-3))
            .fold(0.0, f32::max);
        if steepest > MAX_GRADE {
            return Err(TrackError::TooSteep { grade: steepest });
        }

        let length = baked.length();
        let separation = PI * self.min_turn_radius.max(self.widths.widest());
        for (i, first) in frames.iter().enumerate() {
            for (j, second) in frames.iter().enumerate().skip(i + 1) {
                let along = baked.distances()[j] - baked.distances()[i];
                if along.min(length - along) < separation {
                    continue;
                }
                let level = (first.position.y - second.position.y).abs() < BRIDGE_CLEARANCE;
                let close = first.position.xz().distance(second.position.xz())
                    < first.width.max(second.width);
                if level && close {
                    return Err(TrackError::SegmentsTooClose {
                        a: baked.distances()[i],
                        b: baked.distances()[j],
                    });
                }
            }
        }
        Ok(())
    }
}

/// The length of the closed polygon through `points` in plan.
fn plan_length(points: &[Vec3]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.xz().distance(b.xz()))
        .sum()
}

/// A thin rectangular road profile, one unit wide and very thin.
fn road_profile() -> Vec<Vec2> {
    let thickness = 0.01;
    vec![
        Vec2::new(-0.5, -thickness),
        Vec2::new(0.5, -thickness),
        Vec2::new(0.5, 0.0),
        Vec2::new(-0.5, 0.0),
    ]
}

/// A track baked into arc-length-spaced road frames for fast, even sampling.
///
/// Wraps [`BakedSpline`] so the spline handles frame interpolation, channel
/// sampling, and arc-length lookup, while the track layer adds curvature-based
/// road width on top.
#[derive(Resource, Clone, Debug)]
pub struct BakedTrack {
    baked: BakedSpline,
    widths: RoadWidths,
    /// Total loop length.
    length: f32,
    /// Frame spacing.
    step: f32,
}

impl BakedTrack {
    /// Builds baked frames from a [`BakedSpline`] that has width (channel 0) and
    /// roll (channel 1) at the control points.
    fn from_baked(baked: &BakedSpline, design: &TrackDesign) -> Self {
        Self {
            baked: baked.clone(),
            widths: design.widths,
            length: baked.length(),
            step: FRAME_STEP,
        }
    }

    /// Converts the baked spline into [`SweepFrame`]s for [`Shape::sweep_frames`].
    ///
    /// Width comes from the spline channel scaled by curvature-based road width;
    /// roll comes from the spline channel.
    fn sweep_frames_for(baked: &BakedSpline, design: &TrackDesign) -> Vec<SweepFrame> {
        let frames = baked.sweep_frames(Some(0), Some(1));
        let baked_frames = baked.frames();
        frames
            .into_iter()
            .enumerate()
            .map(|(i, frame)| {
                let bf = baked_frames[i];
                let width_scale = baked
                    .channel_at_distance(0, bf.distance)
                    .map_or(1.0, |w| w.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH));
                let road_width = design.widths.at(bf.curvature) * width_scale;
                frame.with_scale(Vec2::new(road_width, frame.scale.y))
            })
            .collect()
    }

    /// Total loop length in world units.
    pub fn length(&self) -> f32 {
        self.length
    }

    /// The frames for validation and rendering, computed on demand.
    pub fn frames(&self) -> Vec<RoadFrame> {
        self.baked
            .frames()
            .iter()
            .map(|frame| self.frame_from_spline(frame))
            .collect()
    }

    /// The arc-length distance from the start of the loop of each frame.
    pub fn distances(&self) -> Vec<f32> {
        self.baked.frames().iter().map(|f| f.distance).collect()
    }

    /// Samples at the normalized loop progress `t`, which wraps around `[0, 1)`.
    pub fn sample(&self, t: f32) -> RoadFrame {
        self.sample_at_distance(t.rem_euclid(1.0) * self.length)
    }

    /// Samples at an arc-length `distance` from the start, which wraps around the loop.
    pub fn sample_at_distance(&self, distance: f32) -> RoadFrame {
        let frame = self.baked.sample_at_distance(distance);
        self.frame_from_spline(&frame)
    }

    fn frame_from_spline(&self, frame: &bevy_gradmesh::SplineFrame) -> RoadFrame {
        let width_scale = self
            .baked
            .channel_at_distance(0, frame.distance)
            .map_or(1.0, |w| w.clamp(MIN_POINT_WIDTH, MAX_POINT_WIDTH));
        let roll = self
            .baked
            .channel_at_distance(1, frame.distance)
            .map_or(0.0, |r| r.clamp(-MAX_ROLL, MAX_ROLL));
        let width = self.widths.at(frame.curvature) * width_scale;
        RoadFrame::oriented(frame.position, frame.forward, frame.curvature, width, roll)
    }

    /// The normalized loop progress of the frame nearest to `point` in three dimensions.
    #[cfg(test)]
    pub fn nearest_t(&self, point: Vec3) -> f32 {
        let count = self.baked.frames().len();
        let dist_sq = |i: usize| {
            let f = &self.baked.frames()[i % count];
            f.position.distance_squared(point)
        };
        let step = (count / 100).max(1);
        let coarse = (0..count)
            .step_by(step)
            .min_by(|&a, &b| dist_sq(a).total_cmp(&dist_sq(b)))
            .unwrap_or(0);
        let span = step.max(2);
        let start = (coarse + count - span / 2) % count;
        (0..span)
            .map(|o| (start + o) % count)
            .min_by(|&a, &b| dist_sq(a).total_cmp(&dist_sq(b)))
            .map(|best| self.baked.frames()[best].distance / self.length)
            .unwrap_or(0.0)
    }

    /// The arc-length distance of the frame nearest to `point` in three dimensions among those
    /// within `window` of the distance `around`, so a road above or below the one being driven
    /// is never mistaken for it.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "frame indices are small, non-negative whole numbers"
    )]
    pub fn nearest_distance_near(&self, point: Vec3, around: f32, window: f32) -> f32 {
        let count = self.baked.frames().len();
        let start = ((around - window).rem_euclid(self.length) / self.step) as usize % count;
        let span = ((2.0 * window / self.step).ceil() as usize + 1).min(count);
        let dist_sq = |i: usize| {
            let f = &self.baked.frames()[i % count];
            f.position.distance_squared(point)
        };
        let best = (0..span)
            .map(|offset| (start + offset) % count)
            .min_by(|&a, &b| dist_sq(a).total_cmp(&dist_sq(b)))
            .unwrap_or(0);
        self.baked.frames()[best].distance
    }
}

fn is_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

/// A small seeded random number generator, so a seed always scatters the same scenery.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..1`.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;

    const EPSILON: f32 = 1e-3;

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions expected");
        };
        positions.iter().map(|&p| Vec3::from(p)).collect()
    }

    fn square(side: f32) -> TrackDesign {
        let half = side / 2.0;
        TrackDesign {
            control_points: [
                Vec3::new(-half, 0.0, -half),
                Vec3::new(half, 0.0, -half),
                Vec3::new(half, 0.0, half),
                Vec3::new(-half, 0.0, half),
            ]
            .map(ControlPoint::new)
            .to_vec(),
            ..TrackDesign::upstream()
        }
    }

    fn flat_default() -> TrackDesign {
        let mut design = TrackDesign::upstream();
        for point in &mut design.control_points {
            point.position.y = 0.0;
        }
        design
    }

    /// A figure eight whose two passes cross at the origin; points 7 to 10 form the second pass.
    fn figure_eight(second_pass_height: f32) -> TrackDesign {
        let control_points = (0..12)
            .map(|i| {
                let angle = (15.0 + 30.0 * i as f32).to_radians();
                let height = if (7..=10).contains(&i) {
                    second_pass_height
                } else {
                    0.0
                };
                ControlPoint::new(Vec3::new(
                    400.0 * angle.cos(),
                    height,
                    200.0 * (2.0 * angle).sin(),
                ))
            })
            .collect();
        TrackDesign {
            control_points,
            ..TrackDesign::upstream()
        }
    }

    #[test]
    fn default_design_reproduces_the_upstream_track() {
        let design = TrackDesign::upstream();
        assert_eq!(design.control_points.len(), 23);
        assert_eq!(
            design.control_points[0].position,
            Vec3::new(0.0, 0.5, 100.0)
        );
        assert_eq!(
            design.control_points[22].position,
            Vec3::new(20.0, 0.6, 120.0)
        );
        assert!(design.control_points.iter().all(|p| p.width == 1.0));
        assert!(design.control_points.iter().all(|p| p.roll == 0.0));
        assert!((design.min_turn_radius - 14.0).abs() < f32::EPSILON);
        assert_eq!(design.checkpoint_count, 8);
        assert_eq!(design.coin_count, 24);
        assert_eq!(design.decoration_seed, None);
    }

    #[test]
    fn default_frames_have_the_upstream_widths_and_no_banking() {
        let baked = TrackDesign::upstream().bake();
        let design = TrackDesign::upstream();
        for frame in baked.frames() {
            assert!((frame.width - design.road_width(frame.curvature)).abs() < EPSILON);
            assert!((frame.roll).abs() < EPSILON);
            assert!(frame.right.y.abs() < EPSILON);
            assert!((frame.up.dot(frame.right)).abs() < EPSILON);
            assert!(frame.up.y > 0.9);
        }
    }

    #[test]
    fn default_design_is_closed_and_drivable() {
        let design = TrackDesign::upstream();
        assert_eq!(design.validate(), Ok(()));
        assert!(design.validate().is_ok());

        let baked = design.bake();
        let first = baked.frames()[0];
        let last = baked.sample_at_distance(baked.length() - 1e-3);
        assert!(first.position.distance(last.position) < 0.1);
        assert!(first.tangent.dot(last.tangent) > 0.99);
        assert!(design.spline().min_turn_radius() >= design.min_turn_radius);
    }

    #[test]
    fn too_few_control_points_are_rejected() {
        let design = TrackDesign {
            control_points: vec![
                ControlPoint::new(Vec3::ZERO),
                ControlPoint::new(Vec3::X * 100.0),
            ],
            ..TrackDesign::upstream()
        };
        assert_eq!(design.validate(), Err(TrackError::TooFewPoints(2)));
    }

    #[test]
    fn control_points_closer_than_the_road_is_wide_are_rejected() {
        let mut design = TrackDesign::upstream();
        design.control_points[5].position = design.control_points[4].position + Vec3::X * 5.0;
        assert!(matches!(
            design.validate(),
            Err(TrackError::PointsTooClose { a: 4, b: 5 })
        ));
        assert!(!design.validate().is_ok());
    }

    #[test]
    fn loops_that_are_too_sharp_are_rejected() {
        let design = square(20.0);
        assert!(matches!(
            design.validate(),
            Err(TrackError::TooSharp { radius }) if radius < design.min_turn_radius
        ));
        assert!(square(300.0).validate().is_ok());
    }

    #[test]
    fn roads_that_cross_without_a_bridge_are_rejected() {
        for height in [0.0, BRIDGE_CLEARANCE - 2.0] {
            assert!(matches!(
                figure_eight(height).validate(),
                Err(TrackError::SegmentsTooClose { .. })
            ));
        }
    }

    #[test]
    fn a_bridge_is_drivable_when_the_roads_are_far_enough_apart_in_height() {
        let design = figure_eight(BRIDGE_CLEARANCE * 2.0);
        assert_eq!(design.validate(), Ok(()));
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let invalid = [
            TrackDesign {
                min_turn_radius: 0.0,
                ..TrackDesign::upstream()
            },
            TrackDesign {
                widths: RoadWidths {
                    corner: -1.0,
                    ..RoadWidths::default()
                },
                ..TrackDesign::upstream()
            },
            TrackDesign {
                checkpoint_count: 1,
                ..TrackDesign::upstream()
            },
            TrackDesign {
                control_points: vec![ControlPoint::new(Vec3::NAN); 4],
                ..TrackDesign::upstream()
            },
        ];
        for design in invalid {
            assert!(matches!(
                design.validate(),
                Err(TrackError::InvalidParameter(_))
            ));
            assert!(!design.validate().unwrap_err().to_string().is_empty());
        }
    }

    #[test]
    fn width_and_roll_beyond_their_limits_are_rejected() {
        let mut wide = TrackDesign::upstream();
        wide.control_points[3].width = MAX_POINT_WIDTH + 0.5;
        assert_eq!(wide.validate(), Err(TrackError::WidthOutOfRange(3)));

        let mut leaning = TrackDesign::upstream();
        leaning.control_points[7].roll = -MAX_ROLL - 0.1;
        assert_eq!(leaning.validate(), Err(TrackError::RollTooLarge(7)));
    }

    #[test]
    fn roads_narrower_than_the_kart_needs_are_rejected() {
        let design = TrackDesign {
            widths: RoadWidths {
                straight: 3.0,
                bend: 3.0,
                corner: 3.0,
            },
            ..TrackDesign::upstream()
        };
        assert!(matches!(
            design.validate(),
            Err(TrackError::RoadTooNarrow { width }) if width < MIN_ROAD_WIDTH
        ));
    }

    #[test]
    fn roads_that_are_too_steep_are_rejected() {
        let design = TrackDesign::upstream().with_point_raised(10, 80.0);
        assert!(matches!(
            design.validate(),
            Err(TrackError::TooSteep { grade }) if grade > MAX_GRADE
        ));
    }

    #[test]
    fn road_width_shrinks_with_curvature() {
        let design = TrackDesign::upstream();
        assert!((design.road_width(0.0) - 16.0).abs() < f32::EPSILON);
        assert!((design.road_width(1.0 / 100.0) - 14.0).abs() < f32::EPSILON);
        assert!((design.road_width(1.0 / 30.0) - 12.0).abs() < f32::EPSILON);
        assert!((design.road_width(-1.0 / 30.0) - 12.0).abs() < f32::EPSILON);
        assert!((design.widths.widest() - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn width_scale_interpolates_smoothly_between_control_points() {
        let mut design = TrackDesign::upstream();
        for index in [5, 6] {
            design.control_points[index].width = 2.0;
        }
        let frames = design.control_frames();
        assert_eq!(
            frames[5].width,
            design.road_width(frames[5].curvature) * 2.0
        );
        assert_eq!(frames[4].width, design.road_width(frames[4].curvature));

        let between = design.sample_frame(4.5 / 23.0);
        let scale = between.width / design.road_width(between.curvature);
        assert!(scale > 1.0 && scale < 2.0, "{scale}");

        let mut plateau = TrackDesign::upstream();
        for index in [4, 5, 6, 7] {
            plateau.control_points[index].width = 1.5;
        }
        let middle = plateau.sample_frame(5.5 / 23.0);
        assert!((middle.width / plateau.road_width(middle.curvature) - 1.5).abs() < EPSILON);
    }

    #[test]
    fn roll_interpolates_and_wraps_around_the_loop() {
        let mut design = TrackDesign::upstream();
        for index in [21, 22, 0, 1] {
            design.control_points[index].roll = 0.3;
        }
        assert!((design.sample_frame(22.5 / 23.0).roll - 0.3).abs() < EPSILON);
        assert!((design.sample_frame(0.0).roll - 0.3).abs() < EPSILON);

        design.control_points[1].roll = 0.0;
        design.control_points[0].roll = 0.4;
        let between = design.sample_frame(0.5 / 23.0).roll;
        assert!(between > 0.0 && between < 0.4);
        assert!((design.sample_frame(2.0 / 23.0).roll).abs() < EPSILON);
    }

    #[test]
    fn frames_lean_with_the_roll() {
        let mut design = flat_default();
        design.control_points[4].roll = 0.3;
        let frame = design.control_frames()[4];
        assert!((frame.roll - 0.3).abs() < EPSILON);
        assert!((frame.up.y - 0.3_f32.cos()).abs() < 1e-2);
        assert!(frame.edge(1.0).y < frame.edge(-1.0).y);
        assert!((frame.up.dot(frame.right)).abs() < EPSILON);
        assert!((frame.rotation() * Vec3::Y).distance(frame.up) < EPSILON);
        assert!((frame.rotation() * Vec3::Z).distance(frame.tangent) < EPSILON);
    }

    #[test]
    fn elevation_follows_the_control_points() {
        let design = TrackDesign::upstream();
        for (i, frame) in design.control_frames().iter().enumerate() {
            assert!((frame.position.y - design.control_points[i].position.y).abs() < EPSILON);
        }
        let raised = design.with_point_raised(3, 4.0);
        let frame = raised.control_frames()[3];
        assert!((frame.position.y - (design.control_points[3].position.y + 4.0)).abs() < EPSILON);

        let baked = raised.bake();
        let top = baked
            .frames()
            .iter()
            .map(|f| f.position.y)
            .fold(f32::MIN, f32::max);
        assert!(top > 4.0);
    }

    #[test]
    fn baked_track_has_length_and_wraps() {
        let baked = TrackDesign::upstream().bake();
        let length = baked.length();
        assert!(length > 0.0);
        for distance in [0.0, 12.5, length * 0.5] {
            let at = baked.sample_at_distance(distance).position;
            assert!(at.distance(baked.sample_at_distance(distance + length).position) < 1e-2);
            assert!(at.distance(baked.sample_at_distance(distance - length).position) < 1e-2);
        }
        assert!(
            baked
                .sample(0.0)
                .position
                .distance(baked.sample(1.0).position)
                < EPSILON
        );
        assert_eq!(baked.frames().len(), baked.distances().len());
    }

    #[test]
    fn nearest_frames_are_found_globally_and_locally() {
        let baked = TrackDesign::upstream().bake();
        let target = baked.sample(0.4);
        let point = target.position + target.right * 2.0;
        assert!((baked.nearest_t(point) - 0.4).abs() < 1e-3);

        let around = baked.length() * 0.4;
        let near = baked.nearest_distance_near(point, around + 5.0, 12.0);
        assert!((near - around).abs() < FRAME_STEP);

        let wrapped = baked.sample_at_distance(1.0);
        let at_start = baked.nearest_distance_near(wrapped.position, baked.length() - 3.0, 12.0);
        assert!((at_start - 1.0).abs() < FRAME_STEP);
    }

    #[test]
    fn the_nearest_frame_distinguishes_a_road_from_the_bridge_above_it() {
        let design = figure_eight(BRIDGE_CLEARANCE * 2.0);
        let baked = design.bake();
        let below = baked.nearest_t(Vec3::new(0.0, 0.5, 0.0));
        let above = baked.nearest_t(Vec3::new(0.0, BRIDGE_CLEARANCE * 2.0, 0.0));
        assert!((baked.sample(below).position.y).abs() < 1.0);
        assert!((baked.sample(above).position.y - BRIDGE_CLEARANCE * 2.0).abs() < 1.0);
        let along = baked.length() * below;
        let local = baked.nearest_distance_near(Vec3::new(0.0, 0.5, 0.0), along, 20.0);
        assert!((local - along).abs() < FRAME_STEP);
    }

    #[test]
    fn road_shape_has_vertices_and_covers_the_track() {
        let design = TrackDesign::upstream();
        let mesh = design.road_shape().build();
        let n = mesh.count_vertices();
        assert!(n > 0, "road shape should produce vertices");

        let points = positions(&mesh);
        let min = points.iter().copied().fold(Vec3::MAX, Vec3::min);
        let max = points.iter().copied().fold(Vec3::MIN, Vec3::max);
        for point in design.spline().points() {
            assert!(point.cmpge(min).all() && point.cmple(max).all(), "{point}");
        }
        assert!(max.x - min.x > 500.0 && max.z - min.z > 300.0);
    }

    #[test]
    fn banked_road_frames_tilt_by_the_roll() {
        let mut design = flat_default();
        design.control_points[4].roll = 0.3;
        let frame = design.control_frames()[4];
        let drop = frame.edge(-1.0).y - frame.edge(1.0).y;
        assert!((drop - frame.width * 0.3_f32.sin()).abs() < EPSILON);

        let baked = design.bake();
        let frames = baked.frames();
        let mut steepest: f32 = 0.0;
        for frame in frames {
            let drop = frame.edge(-1.0).y - frame.edge(1.0).y;
            assert!((drop - frame.width * frame.roll.sin()).abs() < EPSILON * 2.0);
            steepest = steepest.max(drop);
        }
        assert!(steepest > 0.99 * frame.width * 0.3_f32.sin());
    }

    #[test]
    fn road_shape_follows_the_elevation() {
        let design = TrackDesign::upstream().with_point_raised(3, 4.0);
        let mesh = design.road_shape().build();
        let points = positions(&mesh);
        let top = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let bottom = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        assert!(top > 5.0 && bottom < 0.0);
    }

    #[test]
    fn road_triangles_face_up() {
        use bevy::mesh::Indices;
        let design = TrackDesign::upstream();
        let mesh = design.road_shape().build();
        let points = positions(&mesh);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("indices expected");
        };
        // The sweep produces a closed surface with top and bottom faces.
        // The top-facing triangles (road surface) should face up.
        let facing_up = indices
            .chunks(3)
            .filter(|triangle| {
                let [a, b, c] = [0, 1, 2].map(|i| points[triangle[i] as usize]);
                let normal = (b - a).cross(c - a);
                // Count triangles whose normal points mostly up
                normal.length() > 1e-6 && normal.y > normal.xz().length()
            })
            .count();
        assert!(facing_up > 0, "expected some triangles facing up");
        // The sweep produces a closed surface with top, bottom and side faces.
        // The road surface triangles should face up.
        assert!(
            facing_up > indices.len() / 3 / 10,
            "road surface triangles should face up: {facing_up}/{}",
            indices.len() / 3
        );
    }

    #[test]
    fn checkpoints_are_evenly_spaced_along_the_road() {
        let design = TrackDesign::upstream();
        let baked = design.bake();
        let gates = design.checkpoint_gates(&baked);
        assert_eq!(gates.len(), 8);
        for (i, gate) in gates.iter().enumerate() {
            let frame = baked.sample(i as f32 / 8.0);
            assert!(gate.transform.translation.distance(frame.position) < EPSILON);
            assert!((gate.transform.rotation * Vec3::Z).dot(frame.tangent) > 0.999);
            assert_eq!(gate.width, frame.width);
        }
    }

    #[test]
    fn coins_hover_above_the_road_surface() {
        let design = TrackDesign::upstream();
        let baked = design.bake();
        let coins = design.coin_transforms(&baked);
        assert_eq!(coins.len(), 24);
        for (i, coin) in coins.iter().enumerate() {
            let frame = baked.sample((i as f32 + 0.5) / 24.0);
            let expected = frame.position + frame.up * COIN_HOVER;
            assert!(coin.translation.distance(expected) < EPSILON);
        }
    }

    #[test]
    fn default_decorations_are_the_hand_placed_scenery() {
        let design = TrackDesign::upstream();
        let sites = design.decoration_sites(&design.bake());
        assert_eq!(sites.len(), TREE_COUNT + BUSH_COUNT);
        assert_eq!(sites[0].kind, DecorationKind::Tree);
        assert_eq!(sites[0].position, Vec3::new(-250.0, 0.0, -250.0));
        assert_eq!(sites[TREE_COUNT].kind, DecorationKind::Bush);
    }

    #[test]
    fn seeded_decorations_are_repeatable_and_keep_clear_of_the_road() {
        let seeded = |seed| TrackDesign {
            decoration_seed: Some(seed),
            ..TrackDesign::upstream()
        };
        let design = seeded(7);
        let baked = design.bake();
        let sites = design.decoration_sites(&baked);
        assert!(sites.len() > TREE_COUNT);
        assert_eq!(sites, design.decoration_sites(&baked));
        assert_ne!(sites, seeded(8).decoration_sites(&baked));
        for site in &sites {
            let nearest = baked
                .frames()
                .iter()
                .map(|f| f.position.xz().distance(site.position.xz()))
                .fold(f32::MAX, f32::min);
            assert!(nearest >= 8.0 + DECORATION_CLEARANCE - 1.0);
        }
    }

    #[test]
    fn moving_a_point_changes_only_that_point() {
        let design = TrackDesign::upstream();
        let target = Vec3::new(10.0, 3.0, 99.0);
        let moved = design.with_point_moved(2, target);
        assert_eq!(moved.control_points[2].position, target);
        assert_eq!(moved.control_points[2].width, 1.0);
        for index in (0..23).filter(|&i| i != 2) {
            assert_eq!(moved.control_points[index], design.control_points[index]);
        }
        assert_eq!(design, TrackDesign::upstream());

        assert_eq!(design.with_point_moved(99, target), design);
        assert_eq!(design.with_point_moved(2, Vec3::NAN), design);
    }

    #[test]
    fn point_width_is_clamped() {
        let design = TrackDesign::upstream();
        assert_eq!(design.with_point_width(1, 1.4).control_points[1].width, 1.4);
        assert_eq!(
            design.with_point_width(1, 99.0).control_points[1].width,
            MAX_POINT_WIDTH
        );
        assert_eq!(
            design.with_point_width(1, -3.0).control_points[1].width,
            MIN_POINT_WIDTH
        );
        assert_eq!(design.with_point_width(1, f32::NAN), design);
        assert_eq!(design.with_point_width(40, 1.4), design);
    }

    #[test]
    fn point_roll_is_clamped() {
        let design = TrackDesign::upstream();
        assert_eq!(design.with_point_roll(1, 0.2).control_points[1].roll, 0.2);
        assert_eq!(
            design.with_point_roll(1, 9.0).control_points[1].roll,
            MAX_ROLL
        );
        assert_eq!(
            design.with_point_roll(1, -9.0).control_points[1].roll,
            -MAX_ROLL
        );
        assert_eq!(design.with_point_roll(1, f32::NAN), design);
        assert_eq!(design.with_point_roll(40, 0.2), design);
    }

    #[test]
    fn raising_a_point_moves_only_its_height() {
        let design = TrackDesign::upstream();
        let raised = design.with_point_raised(5, 2.5);
        let (before, after) = (design.control_points[5], raised.control_points[5]);
        assert!((after.position.y - before.position.y - 2.5).abs() < EPSILON);
        assert_eq!(after.position.xz(), before.position.xz());
        let lowered = raised.with_point_raised(5, -2.5);
        assert!((lowered.control_points[5].position.y - before.position.y).abs() < EPSILON);
        assert_eq!(design.with_point_raised(40, 1.0), design);
    }

    #[test]
    fn inserting_a_point_adds_it_on_the_curve_with_interpolated_channels() {
        let mut design = TrackDesign::upstream();
        design.control_points[3].width = 1.0;
        design.control_points[4].width = 1.0;
        design.control_points[3].roll = 0.2;
        design.control_points[4].roll = 0.2;
        let inserted = design.with_point_inserted(3);
        assert_eq!(inserted.control_points.len(), 24);

        let new = inserted.control_points[4];
        let midpoint = design.spline().point(3.5 / 23.0);
        assert!(new.position.distance(midpoint) < EPSILON);
        assert!(new.width >= MIN_POINT_WIDTH && new.width <= MAX_POINT_WIDTH);
        assert!((new.roll - 0.2).abs() < 0.05);
        assert_eq!(inserted.control_points[3], design.control_points[3]);
        assert_eq!(inserted.control_points[5], design.control_points[4]);
        assert!(inserted.validate().is_ok());

        let wrapped = design.with_point_inserted(22);
        assert_eq!(wrapped.control_points.len(), 24);
        assert_eq!(
            wrapped.control_points[23]
                .position
                .distance(design.spline().point(22.5 / 23.0)),
            0.0
        );
        assert_eq!(design.with_point_inserted(23), design);
    }

    #[test]
    fn a_track_is_laid_down_from_nothing_by_appending_points() {
        let mut design = TrackDesign::empty();
        assert!(design.control_points.is_empty());
        assert_eq!(design.validate(), Err(TrackError::TooFewPoints(0)));
        for (index, corner) in [(40.0, 0.0), (40.0, 80.0), (-40.0, 80.0), (-40.0, 0.0)]
            .into_iter()
            .enumerate()
        {
            design = design.with_point_appended(Vec3::new(corner.0, 0.0, corner.1));
            assert_eq!(design.control_points.len(), index + 1);
            assert_eq!(design.control_points[index].width, 1.0);
            assert_eq!(design.control_points[index].roll, 0.0);
        }
        assert_eq!(
            design.validate(),
            Ok(()),
            "four points make a drivable loop"
        );
        assert_eq!(
            design.with_point_appended(Vec3::NAN),
            design,
            "a point that is not a place is not added"
        );
    }

    #[test]
    fn removing_a_point_refuses_below_the_minimum() {
        let design = TrackDesign::upstream();
        let removed = design.with_point_removed(4);
        assert_eq!(removed.control_points.len(), 22);
        assert_eq!(removed.control_points[4], design.control_points[5]);
        assert_eq!(design.with_point_removed(99), design);

        let mut small = design.clone();
        small.control_points.truncate(MIN_POINTS);
        assert_eq!(small.with_point_removed(0), small);
        assert_eq!(
            small.with_point_inserted(0).control_points.len(),
            MIN_POINTS + 1
        );
    }

    #[test]
    fn editing_keeps_a_valid_design_valid() {
        let design = TrackDesign::upstream()
            .with_point_width(2, 1.5)
            .with_point_roll(2, 0.25)
            .with_point_raised(8, 1.0)
            .with_point_moved(0, Vec3::new(0.0, 0.5, 110.0));
        assert_eq!(design.validate(), Ok(()));
    }
}
