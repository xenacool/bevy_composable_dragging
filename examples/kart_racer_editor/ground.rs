//! The ground: a heightmap of the land around the road, which is not the road.
//!
//! The road is the [`TrackDesign`](crate::track::TrackDesign); the ground is what lies under and
//! around it. Every vertex of the grid has one height, so there are no caves or overhangs. The
//! grid is cut into chunks of [`SIDE`] x [`SIDE`] vertices that are stored in two flat buffers,
//! so copying a ground is two memory copies and an edit names the chunks it reaches. A chunk has
//! a [`key`](Ground::chunk_key) that changes whenever anything its mesh is built from changes,
//! so only those meshes are built again. How high the ground can rise and how low it can sink
//! come from the width of the ground, so a map of any size has the same proportions.

use std::ops::Deref;

use bevy::prelude::*;
pub use bevy_composable_dragging::heightmap::Brush;
use bevy_composable_dragging::heightmap::{Grid, Heightmap};
use bevy_gradmesh::{HeightfieldOptions, Shape, Style};

use crate::track::BakedTrack;

/// The vertices along a side of a chunk.
pub const SIDE: usize = 16;
/// The chunks along a side of the ground, which sets how fine the grid is: the cells are the
/// width of the ground divided by the vertices along it.
pub const CHUNKS: usize = 12;
/// The height of flat ground.
pub const BASE: f32 = -1.0;
/// How far the ground rises above its base and sinks below it at most, as a share of its
/// half-width.
const RELIEF_SHARE: f32 = 0.1;
/// How far under the lowest edge of the road the land is cut along it, in metres.
const ROAD_CLEARANCE: f32 = 0.3;
/// How far beyond the edge of the road the land is cut, in cells. Every cell the road touches has
/// its corners within the square root of two cells of the road, so this is enough for the road
/// never to be covered by the surface between the vertices.
const ROAD_SHOULDER: f32 = 1.5;

/// The ground: a square of land, flat to begin with, that brushes raise and lower. It is a
/// [`Heightmap`] whose size, cells and range of heights all
/// come from the width of the square, and it reads as one: every query of the heightmap is
/// available on it.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Ground {
    map: Heightmap,
    /// The middle of the ground and the half-width of its square.
    middle: Vec2,
    half: f32,
}

impl Deref for Ground {
    type Target = Heightmap;

    fn deref(&self) -> &Heightmap {
        &self.map
    }
}

/// How many frames of a track whose frames are `spacing` apart make up half a cell of `pitch`,
/// which is how often the land is cut along the road so no stretch of it is missed.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a count of frames, which is at least one"
)]
fn frames_per_half_cell(pitch: f32, spacing: f32) -> usize {
    ((pitch / 2.0 / spacing.max(f32::MIN_POSITIVE)).ceil() as usize).max(1)
}

/// A count of vertices or cells as a length.
#[allow(
    clippy::cast_precision_loss,
    reason = "vertex indices are a few hundred"
)]
fn length(count: isize) -> f32 {
    count as f32
}

/// The grid of a square of ground `2 * half` wide around `middle`, of `across` x `across` chunks.
fn square(middle: Vec2, half: f32, across: usize) -> Grid {
    let across = across.max(1);
    let vertices = length((across * SIDE).cast_signed() - 1);
    Grid::new(
        UVec2::splat(u32::try_from(across).unwrap_or(u32::MAX)),
        SIDE,
        middle - Vec2::splat(half),
        half * 2.0 / vertices,
    )
}

impl Ground {
    /// Flat ground at height `base`, a square `2 * half` wide around `middle`, of `across` x
    /// `across` chunks.
    pub fn new(middle: Vec2, half: f32, across: usize, base: f32) -> Self {
        let half = half.abs().max(f32::MIN_POSITIVE);
        let grid = square(middle, half, across);
        let relief = half * RELIEF_SHARE;
        Self {
            map: Heightmap::new(grid, base, base - relief, base + relief),
            middle,
            half,
        }
    }

    /// The middle of the ground on the ground plane.
    pub fn middle(&self) -> Vec2 {
        self.middle
    }

    /// Half the width of the ground, which is a square.
    pub fn half(&self) -> f32 {
        self.half
    }

    /// How far the ground rises above its base and sinks below it at most.
    pub fn relief(&self) -> f32 {
        self.half * RELIEF_SHARE
    }

    /// The ground after `brush` raised or lowered it, between the lowest and the highest the
    /// ground goes.
    #[must_use]
    pub fn raised(&self, brush: &Brush) -> Self {
        Self {
            map: self.map.raised(brush),
            ..self.clone()
        }
    }

    /// The ground cut down along `track`: every vertex under the road, and within a shoulder of
    /// [`ROAD_SHOULDER`] cells of it, is lowered to just under the lowest edge of the road there if
    /// it is higher, and never below the lowest the ground goes. A hill can be raised on both
    /// sides of a road and the road runs through it, but nothing can cover the road.
    #[must_use]
    pub fn cut_along(&self, track: &BakedTrack) -> Self {
        let frames = track.frames();
        let spacing = track.length() / length(frames.len().cast_signed().max(1));
        let stride = frames_per_half_cell(self.pitch(), spacing);
        let mut ground = self.clone();
        for frame in frames.iter().step_by(stride) {
            let reach = frame.width / 2.0 + self.pitch() * ROAD_SHOULDER;
            let limit = frame.edge(-1.0).y.min(frame.edge(1.0).y) - ROAD_CLEARANCE;
            ground.map.cap_near(frame.position.xz(), reach, limit);
        }
        ground
    }

    /// Where a ray meets the surface of the ground, if it does. A ray that meets the land beyond
    /// the map misses.
    pub fn hit(&self, ray: Ray3d) -> Option<Vec3> {
        self.map
            .hit(ray)
            .filter(|point| self.map.contains(point.xz()))
    }

    /// Where the middle of chunk `chunk` is on the ground plane, at height zero: the mesh of a
    /// chunk is centred on its own origin and stands there.
    pub fn chunk_center(&self, chunk: usize) -> Vec3 {
        self.map.chunk_center(chunk)
    }

    /// The shape of chunk `chunk`: a heightfield.
    pub fn chunk_shape(&self, chunk: usize) -> Shape {
        let heights = self.chunk_heights(chunk);
        let size = UVec2::splat(u32::try_from(SIDE + 1).unwrap_or(u32::MAX));
        Shape::heightfield_with_options(
            heights.as_slice().into(),
            size,
            self.chunk_extent(),
            HeightfieldOptions::default(),
        )
    }

    /// The look of the ground: earth in the hollows, grass on the plains, bare rock on the hills
    /// and pale stone on the heights, each from a height that is a share of the relief.
    pub fn style(&self) -> Style {
        let relief = self.relief();
        Style::opaque(Color::srgb(0.34, 0.52, 0.26)).with_height_bands(&[
            (self.lowest(), Color::srgb(0.36, 0.27, 0.18)),
            (self.base() - relief * 0.25, Color::srgb(0.34, 0.52, 0.26)),
            (self.base() + relief * 0.35, Color::srgb(0.5, 0.46, 0.4)),
            (self.base() + relief * 0.7, Color::srgb(0.88, 0.9, 0.92)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::Indices;
    use bevy_gradmesh::ColorMode;

    use super::*;
    use crate::track::TrackDesign;

    /// A ground 192 wide whose cells are about a metre, around the origin.
    fn flat() -> Ground {
        Ground::new(Vec2::ZERO, 96.0, CHUNKS, BASE)
    }

    fn mound(center: Vec2, amount: f32) -> Brush {
        Brush {
            center,
            radius: 8.0,
            amount,
            falloff: 1.0,
        }
    }

    #[test]
    fn flat_ground_is_at_its_base_over_the_whole_square_and_nowhere_else() {
        let ground = flat();
        for xz in [Vec2::ZERO, Vec2::new(95.0, -95.0), Vec2::new(-96.0, 96.0)] {
            assert!(ground.contains(xz), "{xz}");
            assert!((ground.surface(xz) - BASE).abs() < 1e-4, "{xz}");
        }
        for xz in [Vec2::new(96.5, 0.0), Vec2::new(0.0, -200.0)] {
            assert!(!ground.contains(xz), "{xz}");
        }
    }

    #[test]
    fn the_range_of_heights_grows_with_the_ground() {
        let small = Ground::new(Vec2::ZERO, 50.0, 4, 0.0);
        let large = Ground::new(Vec2::ZERO, 500.0, 4, 0.0);
        assert!((large.relief() / small.relief() - 10.0).abs() < 1e-4);
        for ground in [&small, &large] {
            assert!(ground.lowest() < ground.base() && ground.base() < ground.highest());
            assert!((ground.highest() - ground.base() - ground.relief()).abs() < 1e-4);
        }
    }

    #[test]
    fn a_ray_finds_the_surface_and_misses_beyond_the_map() {
        let ground = flat().raised(&mound(Vec2::new(10.0, 0.0), 3.0));
        let down = Ray3d::new(Vec3::new(10.0, 100.0, 0.0), Dir3::NEG_Y);
        let hit = ground.hit(down).unwrap();
        assert!((hit.y - ground.surface(Vec2::new(10.0, 0.0))).abs() < 1e-3);
        let slanted = Ray3d::new(
            Vec3::new(-60.0, 100.0, -60.0),
            Dir3::new(Vec3::new(1.0, -1.0, 1.0)).unwrap(),
        );
        let met = ground.hit(slanted).unwrap();
        assert!((met.y - ground.surface(met.xz())).abs() < 1e-2, "{met}");
        let beside = Ray3d::new(Vec3::new(300.0, 50.0, 0.0), Dir3::NEG_Y);
        assert_eq!(ground.hit(beside), None);
        let level = Ray3d::new(Vec3::new(0.0, 50.0, 0.0), Dir3::X);
        assert_eq!(ground.hit(level), None);
    }

    #[test]
    fn a_chunk_has_a_surface_that_follows_the_heights() {
        let ground = flat().raised(&mound(Vec2::new(-80.0, -80.0), 3.0));
        let mesh = ground.chunk_shape(0).build();
        let triangles = mesh.indices().map_or(0, Indices::len) / 3;
        assert!(triangles >= SIDE * SIDE * 2);
        let top = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|positions| positions.as_float3())
            .unwrap()
            .iter()
            .map(|position| position[1])
            .fold(f32::MIN, f32::max);
        assert!((top - (BASE + 3.0)).abs() < 0.2, "{top}");
        assert_ne!(ground.chunk_key(0), flat().chunk_key(0));
        assert_eq!(ground.chunk_key(100), flat().chunk_key(100));
    }

    #[test]
    fn the_ground_is_colored_by_height_in_four_rising_bands() {
        let ground = flat();
        let style = ground.style();
        assert_eq!(style.color_mode, ColorMode::HeightBands);
        let bands = style.height_bands.unwrap();
        assert_eq!(bands.count, 4);
        assert!(bands.heights[..4].windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            bands.heights[1] < BASE && BASE < bands.heights[2],
            "flat ground is the grass band"
        );
    }

    /// The initial track raised to 10 m at every control point, so the ground at its base is
    /// well under it.
    fn high_track() -> BakedTrack {
        let mut design = TrackDesign::initial();
        for point in &mut design.control_points {
            point.position.y = 10.0;
        }
        design.bake()
    }

    fn everywhere(ground: &Ground, amount: f32) -> Ground {
        ground.raised(&Brush {
            center: ground.middle(),
            radius: ground.half() * 2.0,
            amount,
            falloff: 0.0,
        })
    }

    #[test]
    fn land_that_covers_the_road_is_cut_down_along_it_and_nowhere_else() {
        let track = high_track();
        let ground = Ground::new(Vec2::ZERO, 600.0, CHUNKS, BASE);
        let hill = everywhere(&ground, 30.0);
        assert!(hill.surface(track.frames()[0].position.xz()) > 20.0);
        let cut = hill.cut_along(&track);
        for frame in track.frames() {
            for side in [-1.0, 0.0, 1.0] {
                let at = frame.surface_point(side * frame.width / 2.0);
                assert!(cut.surface(at.xz()) <= at.y - 0.1, "{side}");
            }
        }
        let far = ground.middle() + Vec2::splat(ground.half() * 0.95);
        assert_eq!(cut.surface(far), hill.surface(far), "beyond the shoulder");
        assert_eq!(cut, cut.cut_along(&track), "cutting twice changes nothing");
        assert_eq!(
            ground.cut_along(&track),
            ground,
            "nothing covers a road above it"
        );
    }

    #[test]
    fn the_cut_leaves_hills_standing_beside_the_road() {
        let track = high_track();
        let ground = Ground::new(Vec2::ZERO, 600.0, CHUNKS, BASE);
        let frame = track.frames()[100];
        let aside = frame
            .surface_point(frame.width / 2.0 + ground.pitch() * 6.0)
            .xz();
        let hill = ground.raised(&Brush {
            center: aside,
            radius: ground.pitch() * 2.0,
            amount: 8.0,
            falloff: 1.0,
        });
        assert!(hill.cut_along(&track).surface(aside) > BASE + 4.0);
    }
}
