//! Walls: a Catmull-Rom curve with a cross-section, placed by a pivot and a scale. The mesh is
//! derived from the state and never stored, so scaling the wall or opening it re-derives it and
//! the engine blends the two states.
//!
//! The control points of the curve are in the local frame of the wall. The world curve is the
//! scaled local curve moved to the pivot, and the root node of the wall carries only the pivot
//! (never a scale), so the handles of the gizmo that are parented to it are not stretched.
//!
//! A wall is the cross-section of its [`Profile`] swept along the baked curve with
//! `Shape::sweep`. The openings are cut through the swept body, and the merlons of the
//! battlements are cuboids that join it in a compound shape, each turned to follow the curve.

use std::ops::Range;

use bevy::prelude::*;
use bevy_gradmesh::SweepOptions;
use bevy_gradmesh::{BakedSpline, CatmullRomSpline, Shape, ShapeDetail, ShapePart, SweepOpening};

/// The distance between the points of the path a wall is swept along.
const SWEEP_GAP: f32 = 0.1;
/// The pieces the round top of an arch is made of.
const ROUND_SEGMENTS: u32 = 10;
/// How many baked frames a stretch between control points gets: the spacing of the frames
/// follows the size of the curve, so a small curve is as smooth as a big one.
const FRAMES_PER_SEGMENT: f32 = 64.0;
/// The distance between the samples of the curve that an opening is measured against.
const SAMPLE_GAP: f32 = 0.05;
/// A wall is never scaled below this on any axis, so it cannot collapse.
const MIN_SCALE: f32 = 0.2;
/// How much of the height of the wall an arch clears, from the ground up.
const ARCH_CLEARANCE: f32 = 0.62;
/// The tint of the stone of a wall.
const TINT: Color = Color::srgb(0.96, 0.96, 0.94);

/// How many pieces of about `size` fit along `length`, at least one. `round` says whether the
/// last, partial piece counts (`f32::ceil`) or not (`f32::floor`).
fn pieces_along(length: f32, size: f32, round: fn(f32) -> f32) -> usize {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the count is a small, positive whole number"
    )]
    let count = round(length / size).max(1.0) as usize;
    count
}

/// The cross-section of a stone wall with battlements, as it is after scaling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// How thick the wall is.
    pub width: f32,
    /// How far the body reaches above its base.
    pub height: f32,
    /// The length of a merlon along the wall.
    block: f32,
    /// The length of the gap between two merlons.
    gap: f32,
    /// How far a merlon rises above the top of the body.
    rise: f32,
}

impl Profile {
    /// How far the wall reaches above its base, to the top of its merlons.
    pub fn top(&self) -> f32 {
        self.height + self.rise
    }

    /// The outline of the cross-section, with `x` across the wall and `y` up from its base.
    fn outline(&self) -> Vec<Vec2> {
        let half = self.width / 2.0;
        vec![
            Vec2::new(-half, 0.0),
            Vec2::new(half, 0.0),
            Vec2::new(half, self.height),
            Vec2::new(-half, self.height),
        ]
    }
}

/// A wall along a curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    /// The control points of the base of the wall, in its local frame.
    points: Vec<Vec3>,
    /// Where the local frame is in the world. It is never turned or scaled.
    pub pivot: Vec3,
    /// How much the local curve is stretched along each of its axes.
    pub scale: Vec3,
    /// The stretches of the wall that are open, as ranges of the length of the curve from zero at
    /// the start to one at the end, so an opening stays where it is along the wall when the wall
    /// is scaled. They do not overlap.
    openings: Vec<Range<f32>>,
}

impl Wall {
    /// A wall through the world `points`, which are its base. The pivot is put at the middle of
    /// the points on the ground.
    pub fn through(points: &[Vec3]) -> Self {
        let middle = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let pivot = Vec3::new(middle.x, 0.0, middle.z);
        Self {
            points: points.iter().map(|point| *point - pivot).collect(),
            pivot,
            scale: Vec3::ONE,
            openings: Vec::new(),
        }
    }

    /// The wall with its scale multiplied by `factor` on each axis, never below [`MIN_SCALE`].
    /// The footprint stretches about the pivot on X and Z and the height on Y, while the thickness
    /// follows the mean of the footprint factors, so the cross-section keeps its proportions.
    pub fn scaled(&self, factor: Vec3) -> Self {
        Self {
            scale: (self.scale * factor).max(Vec3::splat(MIN_SCALE)),
            ..self.clone()
        }
    }

    /// How far the farthest control point of the scaled wall is from its pivot.
    pub fn reach(&self) -> f32 {
        self.points
            .iter()
            .map(|point| (*point * self.scale).length())
            .fold(0.0, f32::max)
    }

    /// The cross-section as it is after scaling.
    pub fn profile(&self) -> Profile {
        let across = (self.scale.x * self.scale.z).sqrt();
        let up = self.scale.y;
        Profile {
            width: 0.3 * across,
            height: 0.7 * up,
            block: 0.22 * across,
            gap: 0.14 * across,
            rise: 0.12 * up,
        }
    }

    /// The curve baked for sampling by arc length, with its control points scaled.
    fn baked(&self) -> BakedSpline {
        let points: Vec<Vec3> = self
            .points
            .iter()
            .map(|point| *point * self.scale)
            .collect();
        let length: f32 = points
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .sum();
        let step = if length > 0.0 {
            length / ((points.len() - 1) as f32 * FRAMES_PER_SEGMENT)
        } else {
            1.0
        };
        CatmullRomSpline::open(points)
            .expect("a wall has at least two points")
            .bake(step)
    }

    /// The opening the wall has at `ratio` of its length, if any.
    fn is_open(&self, ratio: f32) -> bool {
        self.openings.iter().any(|opening| opening.contains(&ratio))
    }

    /// The wall with an opening over `t`. An opening that overlaps others joins them.
    fn with_opening(&self, t: Range<f32>) -> Self {
        let mut wall = self.clone();
        let mut t = t;
        wall.openings.retain(|other| {
            let overlaps = other.start <= t.end && t.start <= other.end;
            if overlaps {
                t = t.start.min(other.start)..t.end.max(other.end);
            }
            !overlaps
        });
        wall.openings.push(t);
        wall.openings.sort_by(|a, b| a.start.total_cmp(&b.start));
        wall
    }

    /// The wall with an arch opened through the stretch of its curve that is inside the disc of
    /// `radius` about `center` on the ground, or `None` when the curve does not enter the disc.
    pub fn opened_at(&self, center: Vec2, radius: f32) -> Option<Self> {
        let baked = self.baked();
        let length = baked.length();
        let steps = pieces_along(length, SAMPLE_GAP, f32::ceil);
        let inside: Vec<f32> = (0..=steps)
            .map(|step| step as f32 / steps as f32)
            .filter(|ratio| {
                let world = self.pivot + baked.sample_at_distance(ratio * length).position;
                Vec2::new(world.x, world.z).distance(center) <= radius
            })
            .collect();
        let (first, last) = (*inside.first()?, *inside.last()?);
        (first < last).then(|| self.with_opening(first..last))
    }

    /// The mesh of the wall in the frame of its pivot, already scaled: the profile swept along
    /// the curve with the openings cut through it, and the merlons of the battlements standing on
    /// top of the body.
    pub fn shape(&self) -> Shape {
        let baked = self.baked();
        let length = baked.length();
        let profile = self.profile();
        let steps = pieces_along(length, SWEEP_GAP, f32::ceil);
        let path = (0..=steps)
            .map(|step| {
                baked
                    .sample_at_distance(length * step as f32 / steps as f32)
                    .position
            })
            .collect();
        let clear = profile.height * ARCH_CLEARANCE;
        let options = SweepOptions::default()
            .with_tint_by_length(vec![(0.0, TINT), (1.0, TINT)])
            .with_openings(
                self.openings
                    .iter()
                    .map(|t| SweepOpening::arch(t.clone(), 0.0, clear * 0.6, clear, ROUND_SEGMENTS))
                    .collect(),
            );
        let body = Shape::sweep(profile.outline(), path, options);
        let mut parts = vec![ShapePart::new(body)];
        parts.extend(self.merlons(&baked, &profile));
        Shape::compound(parts)
    }

    /// The merlons on top of the wall. There are none over an opening.
    fn merlons(&self, baked: &BakedSpline, profile: &Profile) -> Vec<ShapePart> {
        let length = baked.length();
        let count = pieces_along(length, profile.block + profile.gap, f32::floor);
        let pitch = length / count as f32;
        (0..count)
            .map(|merlon| (merlon as f32 + 0.5) * pitch)
            .filter(|distance| !self.is_open(distance / length))
            .map(|distance| {
                let sample = baked.sample_at_distance(distance);
                let rotation = Quat::from_rotation_arc(Vec3::X, sample.forward);
                let center = sample.position + Vec3::Y * (profile.height + profile.rise / 2.0);
                let size = Vec3::new(profile.block, profile.rise, profile.width + 0.02);
                ShapePart::new(Shape::cuboid(size, 0.04, ShapeDetail::Low))
                    .with_transform(Transform::from_translation(center).with_rotation(rotation))
                    .with_tint(TINT)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;

    fn straight(length: f32) -> Wall {
        Wall::through(&[Vec3::ZERO, Vec3::new(length, 0.0, 0.0)])
    }

    fn bent() -> Wall {
        Wall::through(&[
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 2.0),
        ])
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    /// The vertices of the mesh of `shape` that a triangle uses.
    fn vertices(shape: &Shape) -> Vec<Vec3> {
        let mesh = shape.build();
        let Some(VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions expected");
        };
        let indices: Vec<usize> = mesh.indices().expect("indices expected").iter().collect();
        let mut used = vec![false; points.len()];
        for index in indices {
            used[index] = true;
        }
        points
            .iter()
            .zip(used)
            .filter_map(|(point, used)| used.then(|| Vec3::from(*point)))
            .collect()
    }

    #[test]
    fn a_wall_puts_its_pivot_in_the_middle_of_its_points_on_the_ground() {
        let wall = bent();
        assert!(near(wall.pivot, Vec3::new(4.0 / 3.0, 0.0, 2.0 / 3.0)));
        let bounds = wall.shape().bounds();
        assert!(bounds.half_extents().x * 2.0 > 2.0, "{bounds:?}");
        assert!(bounds.half_extents().z * 2.0 > 2.0, "{bounds:?}");
    }

    #[test]
    fn a_straight_wall_is_as_long_wide_and_tall_as_its_profile() {
        let bounds = straight(3.0).shape().bounds();
        let (center, half) = (bounds.center(), bounds.half_extents());
        assert!((half.x * 2.0 - 3.05).abs() < 0.1, "{bounds:?}");
        assert!((half.z * 2.0 - 0.32).abs() < 0.05, "{bounds:?}");
        assert!((half.y * 2.0 - 0.82).abs() < 0.05, "{bounds:?}");
        assert!(center.y - half.y > -1e-3, "it stands on its base");
        assert!(center.x.abs() < 0.1, "centered on its pivot: {bounds:?}");
    }

    #[test]
    fn scaling_stretches_the_footprint_and_keeps_the_proportions_of_the_profile() {
        let wall = bent();
        let scaled = wall.scaled(Vec3::new(2.0, 3.0, 0.5));
        assert_eq!(scaled.pivot, wall.pivot);
        let (before, after) = (wall.profile(), scaled.profile());
        assert!((after.height - before.height * 3.0).abs() < 1e-5);
        assert!((after.width - before.width).abs() < 1e-5, "sqrt(2 * 0.5)");
        let wide = wall.scaled(Vec3::new(4.0, 1.0, 1.0)).profile();
        assert!((wide.width - before.width * 2.0).abs() < 1e-5);
        assert!(scaled.shape().bounds().half_extents().x > wall.shape().bounds().half_extents().x);
    }

    #[test]
    fn scaling_by_one_changes_nothing_and_never_goes_below_the_minimum() {
        let wall = bent();
        assert_eq!(wall.scaled(Vec3::ONE), wall);
        let tiny = wall.scaled(Vec3::splat(0.001));
        assert!(tiny.scale.min_element() >= MIN_SCALE - 1e-6);
        assert!(tiny.shape().bounds().half_extents().min_element() > 0.0);
    }

    #[test]
    fn a_disc_on_the_wall_opens_an_arch_in_the_stretch_it_covers() {
        let wall = straight(4.0).through_center_opened(0.0, 0.45);
        let t = &wall.openings[0];
        assert!(
            (t.start - 0.39).abs() < 0.03 && (t.end - 0.61).abs() < 0.03,
            "{t:?}"
        );
        assert_eq!(
            straight(4.0).opened_at(Vec2::new(0.0, 3.0), 0.45),
            None,
            "a disc away from the wall opens nothing"
        );
    }

    impl Wall {
        /// The straight wall opened by a disc at `x` along it, centered on the pivot.
        fn through_center_opened(&self, x: f32, radius: f32) -> Self {
            self.opened_at(Vec2::new(self.pivot.x + x, self.pivot.z), radius)
                .expect("the disc is on the wall")
        }
    }

    #[test]
    fn an_arch_is_cut_through_the_wall_up_to_its_lintel() {
        let solid = straight(4.0);
        let height = solid.profile().height;
        let open = solid.through_center_opened(0.0, 0.45);
        let clear = height * ARCH_CLEARANCE;
        let middle = |shape: &Shape| -> Vec<f32> {
            vertices(shape)
                .into_iter()
                .filter(|vertex| vertex.x.abs() < 0.02)
                .map(|vertex| vertex.y)
                .collect()
        };
        let closed = middle(&solid.shape());
        assert!(
            closed.iter().all(|y| *y < 0.01 || *y > height - 0.01),
            "the solid wall has nothing between its base and its top: {closed:?}"
        );
        let heights = middle(&open.shape());
        assert!(
            heights.iter().any(|y| (*y - clear).abs() < 0.02),
            "{heights:?}"
        );
        assert!(
            heights.iter().all(|y| *y < 0.01 || *y > clear - 0.02),
            "open up to its lintel: {heights:?}"
        );
        assert_ne!(open.shape(), solid.shape());
    }

    #[test]
    fn openings_merge_when_they_overlap_and_stay_in_order() {
        let wall = straight(4.0)
            .with_opening(0.7..0.8)
            .with_opening(0.1..0.2)
            .with_opening(0.15..0.3);
        assert_eq!(wall.openings, vec![0.1..0.3, 0.7..0.8]);
        assert!(wall.is_open(0.25) && !wall.is_open(0.5));
    }

    #[test]
    fn an_opening_stays_where_it_is_along_the_wall_when_the_wall_is_scaled() {
        let wall = straight(4.0).with_opening(0.4..0.6);
        let scaled = wall.scaled(Vec3::new(2.0, 1.5, 1.0));
        assert_eq!(scaled.openings, wall.openings);
        let length = |wall: &Wall| wall.baked().length();
        assert!((length(&scaled) / length(&wall) - 2.0).abs() < 0.02);
    }

    #[test]
    fn the_merlons_leave_the_opening_free() {
        let wall = straight(4.0).with_opening(0.4..0.6);
        let top = |wall: &Wall| {
            vertices(&wall.shape())
                .into_iter()
                .filter(|vertex| vertex.x.abs() < 0.2 && vertex.y > wall.profile().height + 0.01)
                .count()
        };
        assert_eq!(top(&wall), 0);
        assert!(top(&straight(4.0)) > 0);
    }
}
