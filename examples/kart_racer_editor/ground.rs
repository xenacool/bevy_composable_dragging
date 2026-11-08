//! The ground: a heightmap of the land around the road, with holes, which is not the road.
//!
//! The road is the [`TrackDesign`](crate::track::TrackDesign); the ground is what lies under and
//! around it. A kart drives on the road while it is on it and on the ground when it is not, falls
//! through a hole or off the edge of the map, and is put back at a checkpoint once it is below
//! the [`abyss`](Ground::abyss).
//!
//! Every vertex of the grid has one height, so there are no caves or overhangs, and the cells
//! can be cut out as holes. A hole is a mask over the cells, so the height under it is kept and
//! a hole that is filled again brings back the hill that was there. The grid is cut into chunks
//! of [`SIDE`] x [`SIDE`] vertices that are shared with `Arc`, so copying a ground is cheap and an
//! edit only copies the chunks it reaches. A chunk has a [`key`](Ground::chunk_key) that
//! changes whenever anything its mesh is built from changes, so only those meshes are built
//! again. How high the ground can rise, how low it can sink and where the abyss begins all come
//! from the width of the ground, so a map of any size has the same proportions.

use std::ops::Deref;
use std::sync::Arc;

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
/// How far below the lowest the ground can go the abyss lies, in reliefs.
const ABYSS_DEPTH: f32 = 1.0;
/// How far under the lowest edge of the road the land is cut along it, in metres.
const ROAD_CLEARANCE: f32 = 0.3;
/// How far beyond the edge of the road the land is cut, in cells. Every cell the road touches has
/// its corners within the square root of two cells of the road, so this is enough for the road
/// never to be covered by the surface between the vertices.
const ROAD_SHOULDER: f32 = 1.5;

/// The ground: a square of land, flat to begin with, that brushes raise and lower and that
/// holes can be cut out of. It is a [`Heightmap`] whose size, cells and range of heights all
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

impl Ground {
    /// Flat ground at height `base`, a square `2 * half` wide around `middle`, of `across` x
    /// `across` chunks.
    pub fn new(middle: Vec2, half: f32, across: usize, base: f32) -> Self {
        let across = across.max(1);
        let half = half.abs().max(f32::MIN_POSITIVE);
        let vertices = length((across * SIDE).cast_signed() - 1);
        let relief = half * RELIEF_SHARE;
        let chunks = UVec2::splat(u32::try_from(across).unwrap_or(u32::MAX));
        let grid = Grid::new(
            chunks,
            SIDE,
            middle - Vec2::splat(half),
            half * 2.0 / vertices,
        );
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

    /// The height below which nothing is held by the ground any more: a relief under its
    /// lowest point. A kart that is lower than this has fallen out of the map.
    pub fn abyss(&self) -> f32 {
        self.lowest() - self.relief() * ABYSS_DEPTH
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

    /// The ground with the cells within `radius` of `center` cut out as holes, or filled in again
    /// when `hole` is false. The cell under `center` is always one of them, however small the
    /// radius, so a press never does nothing.
    #[must_use]
    pub fn with_holes(&self, center: Vec2, radius: f32, hole: bool) -> Self {
        Self {
            map: self.map.with_holes(center, radius, hole),
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

    /// Where a ray meets the surface of the ground, if it does, holes included, so a hole can
    /// be pointed at and filled again. A ray that meets the land beyond the map misses.
    pub fn hit(&self, ray: Ray3d) -> Option<Vec3> {
        self.map
            .hit(ray)
            .filter(|point| self.map.contains(point.xz()))
    }

    /// The shape of chunk `chunk`: a heightfield with holes where the ground is cut out.
    /// Returns `None` only if every cell is a hole, so there is nothing to draw.
    pub fn chunk_shape(&self, chunk: usize) -> Option<Shape> {
        let heights = self.chunk_heights(chunk);
        let holes = self.chunk_holes(chunk);
        // If every cell is a hole, there's nothing to draw.
        if holes.iter().all(|&h| h) {
            return None;
        }
        let size = UVec2::splat(u32::try_from(SIDE + 1).unwrap_or(u32::MAX));
        let extent = self.chunk_extent();
        let key = self.chunk_key(chunk);
        let options = HeightfieldOptions::default().with_holes(holes);
        Some(Shape::heightfield_with_options(
            Arc::from(heights.as_slice()),
            size,
            extent,
            key,
            options,
        ))
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
    use bevy::mesh::{Indices, VertexAttributeValues};
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

    fn triangles(mesh: &Mesh) -> usize {
        mesh.indices().map_or(0, Indices::len) / 3
    }

    /// Whether the ground at `xz` holds a kart at `height`.
    fn holds(ground: &Ground, xz: Vec2, height: f32) -> bool {
        ground
            .height_at(xz)
            .is_some_and(|found| (found - height).abs() < 1e-4)
    }

    #[test]
    fn flat_ground_holds_a_kart_at_its_base_over_the_whole_square_and_nowhere_else() {
        let ground = flat();
        for xz in [Vec2::ZERO, Vec2::new(95.0, -95.0), Vec2::new(-96.0, 96.0)] {
            assert!(holds(&ground, xz, BASE), "{xz}");
        }
        for xz in [Vec2::new(96.5, 0.0), Vec2::new(0.0, -200.0)] {
            assert_eq!(ground.height_at(xz), None, "{xz}");
            assert!(!ground.is_hole(xz), "beyond the map is not a hole: {xz}");
        }
        assert!(ground.normal_at(Vec2::new(3.0, 4.0)).distance(Vec3::Y) < 1e-4);
    }

    #[test]
    fn the_cells_are_the_width_of_the_ground_over_its_vertices() {
        let ground = Ground::new(Vec2::new(40.0, -10.0), 50.0, 4, 2.0);
        assert!((ground.pitch() * 63.0 - 100.0).abs() < 1e-4);
        assert_eq!(ground.chunk_count(), 16);
        assert!(holds(&ground, Vec2::new(40.0, -10.0), 2.0));
        assert_eq!(ground.height_at(Vec2::new(-11.0, -10.0)), None);
        assert!(ground.contains(Vec2::new(90.0, 40.0)));
    }

    #[test]
    fn the_range_of_heights_and_the_abyss_grow_with_the_ground() {
        let small = Ground::new(Vec2::ZERO, 50.0, 4, 0.0);
        let large = Ground::new(Vec2::ZERO, 500.0, 4, 0.0);
        assert!((large.relief() / small.relief() - 10.0).abs() < 1e-4);
        for ground in [&small, &large] {
            assert!(ground.lowest() < ground.base() && ground.base() < ground.highest());
            assert!(ground.abyss() < ground.lowest());
            assert!((ground.highest() - ground.base() - ground.relief()).abs() < 1e-4);
        }
    }

    #[test]
    fn a_ray_finds_the_surface_straight_down_and_at_a_slant() {
        let ground = flat().raised(&mound(Vec2::new(10.0, 0.0), 3.0));
        let down = Ray3d::new(Vec3::new(10.0, 100.0, 0.0), Dir3::NEG_Y);
        let hit = ground.hit(down).unwrap();
        assert!((hit.y - ground.surface(Vec2::new(10.0, 0.0))).abs() < 1e-3);
        assert!(hit.xz().distance(Vec2::new(10.0, 0.0)) < 1e-3);
        let slanted = Ray3d::new(
            Vec3::new(-60.0, 100.0, -60.0),
            Dir3::new(Vec3::new(1.0, -1.0, 1.0)).unwrap(),
        );
        let met = ground.hit(slanted).unwrap();
        assert!((met.y - ground.surface(met.xz())).abs() < 1e-2, "{met}");
    }

    #[test]
    fn a_ray_misses_beyond_the_map_and_when_it_does_not_point_down() {
        let ground = flat();
        let beside = Ray3d::new(Vec3::new(300.0, 50.0, 0.0), Dir3::NEG_Y);
        assert_eq!(ground.hit(beside), None);
        let level = Ray3d::new(Vec3::new(0.0, 50.0, 0.0), Dir3::X);
        assert_eq!(ground.hit(level), None);
        let up = Ray3d::new(Vec3::new(0.0, 50.0, 0.0), Dir3::Y);
        assert_eq!(ground.hit(up), None);
    }

    #[test]
    fn a_ray_that_starts_under_the_surface_hits_where_it_starts() {
        let ground = flat();
        let low = Ray3d::new(Vec3::new(5.0, BASE - 1.0, 5.0), Dir3::NEG_Y);
        let hit = ground.hit(low).unwrap();
        assert!((hit.y - (BASE - 1.0)).abs() < 1e-4);
    }

    #[test]
    fn a_chunk_shape_has_its_vertices_and_two_triangles_for_every_cell() {
        let ground = flat();
        let mesh = ground.chunk_shape(0).unwrap().build();
        // The heightfield surface has (SIDE+1)² vertices and SIDE*SIDE*2 triangles;
        // gradmesh adds a skirt below each edge, so the totals are larger.
        let surface_vertices = (SIDE + 1) * (SIDE + 1);
        let surface_triangles = SIDE * SIDE * 2;
        assert!(mesh.count_vertices() >= surface_vertices);
        assert!(triangles(&mesh) >= surface_triangles);
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals expected");
        };
        // Surface normals face up; skirt normals may face sideways.
        let up_normals = normals.iter().filter(|n| n[1] > 0.99).count();
        assert!(up_normals >= surface_vertices);
    }

    #[test]
    fn the_last_chunks_leave_out_the_cells_beyond_the_grid() {
        let ground = flat();
        let last = ground.chunk_count() - 1;
        let mesh = ground.chunk_shape(last).unwrap().build();
        // The last chunk has one fewer cell in each direction because it sits at
        // the edge of the grid.  Gradmesh adds skirts, so the total is at least
        // the surface triangle count.
        assert!(triangles(&mesh) >= (SIDE - 1) * (SIDE - 1) * 2);
    }

    #[test]
    fn the_triangles_of_a_chunk_face_up_and_follow_the_heights() {
        let ground = flat().raised(&mound(Vec2::new(-80.0, -80.0), 3.0));
        let mesh = ground.chunk_shape(0).unwrap().build();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions expected");
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("indices expected");
        };
        // Surface triangles face up; skirt triangles may face sideways.
        let mut up_count = 0;
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(positions[triangle[k] as usize]));
            if (b - a).cross(c - a).y > 0.0 {
                up_count += 1;
            }
        }
        // At least the surface triangles (SIDE*SIDE*2) face up.
        assert!(
            up_count >= SIDE * SIDE * 2,
            "only {up_count} triangles face up"
        );
        let top = positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((top - (BASE + 3.0)).abs() < 0.2, "{top}");
    }

    #[test]
    fn holes_take_their_triangles_out_of_the_mesh() {
        let whole = flat();
        let holed = whole.with_holes(Vec2::new(-80.0, -80.0), 4.0, true);
        let before = triangles(&whole.chunk_shape(0).unwrap().build());
        let after = triangles(&holed.chunk_shape(0).unwrap().build());
        assert!(after < before && after > 0, "{before} {after}");
        assert_ne!(whole.chunk_key(0), holed.chunk_key(0));
        assert_eq!(whole.chunk_key(100), holed.chunk_key(100));
    }

    #[test]
    fn a_chunk_that_is_nothing_but_holes_has_no_shape() {
        let ground = flat();
        let hole = ground.with_holes(ground.position(8, 8), 40.0, true);
        assert!(hole.chunk_shape(0).is_none());
        assert!(hole.chunk_shape(ground.chunk_count() - 1).is_some());
    }

    #[test]
    fn the_ground_is_colored_by_height_in_four_rising_bands() {
        let ground = flat();
        let style = ground.style();
        assert_eq!(style.color_mode, ColorMode::HeightBands);
        let bands = style.height_bands.unwrap();
        assert_eq!(bands.count, 4);
        assert!((bands.heights[0] - ground.lowest()).abs() < 1e-5);
        assert!(bands.heights[..4].windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            bands.heights[1] < BASE && BASE < bands.heights[2],
            "flat ground is the grass band"
        );
        assert!(bands.heights[3] < ground.highest());
    }

    /// The upstream track raised to 10 m at every control point, so the ground at its base is
    /// well under it.
    fn high_track() -> BakedTrack {
        let mut design = TrackDesign::upstream();
        for point in &mut design.control_points {
            point.position.y = 10.0;
        }
        design.bake()
    }

    /// Flat land wide enough for the upstream track and its shoulders.
    fn upstream_land() -> Ground {
        Ground::new(Vec2::ZERO, 1200.0, CHUNKS, BASE)
    }

    /// A hill that covers the whole of `ground`.
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
        let ground = upstream_land();
        let hill = everywhere(&ground, 30.0);
        let low_road = track
            .frames()
            .iter()
            .map(|f| f.position.y)
            .fold(f32::MAX, f32::min);
        assert!(hill.surface(track.frames()[0].position.xz()) > low_road + 10.0);
        let cut = hill.cut_along(&track);
        for frame in track.frames() {
            for side in [-1.0, 0.0, 1.0] {
                let at = frame.surface_point(side * frame.width / 2.0);
                assert!(
                    cut.surface(at.xz()) <= at.y.max(low_road) - 0.1,
                    "{side}: the land is {} at the road at {}",
                    cut.surface(at.xz()),
                    at.y
                );
            }
        }
        let far = ground.middle() + Vec2::splat(ground.half() * 0.95);
        assert_eq!(cut.surface(far), hill.surface(far), "beyond the shoulder");
        assert_eq!(cut, cut.cut_along(&track), "cutting twice changes nothing");
    }

    #[test]
    fn land_under_the_road_is_left_alone_and_the_cut_never_goes_below_the_lowest_land() {
        let ground = upstream_land();
        assert_eq!(
            ground.cut_along(&high_track()),
            ground,
            "nothing covers a road above it"
        );
        let mut buried = TrackDesign::upstream();
        for point in &mut buried.control_points {
            point.position.y = ground.lowest() - 50.0;
        }
        let cut = everywhere(&ground, 30.0).cut_along(&buried.bake());
        let frame = buried.bake().frames()[0];
        assert!(cut.surface(frame.position.xz()) >= ground.lowest() - 1e-3);
    }

    #[test]
    fn the_cut_leaves_hills_standing_on_either_side_of_the_road() {
        let track = high_track();
        let ground = upstream_land();
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
        let cut = hill.cut_along(&track);
        assert!(cut.surface(aside) > BASE + 4.0, "{}", cut.surface(aside));
    }
}
