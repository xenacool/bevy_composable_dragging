//! An editable heightmap in chunks, for terrain that a brush raises, lowers and cuts holes in.
//!
//! Every vertex of the grid has one height, so there are no caves or overhangs. The cells can be
//! cut out as holes: a hole is a mask over the cells, so the height under it is kept and a hole
//! that is filled again brings back the hill that was there. The grid is cut into chunks that
//! are shared with [`Arc`], so copying a [`Heightmap`] is cheap and an edit only copies the
//! chunks it reaches. [`Heightmap::chunk_key`] changes whenever anything a chunk's mesh is
//! built from changes, which is how a renderer knows which meshes to build again.
//!
//! Nothing here is tied to a look or to a game: the size of a chunk, the distance between
//! vertices, the height the ground starts at and how high and low it can go are all given in
//! [`Grid`] and [`Heightmap::new`], so a map of any size and scale works the same way.
//!
//! ```
//! use bevy::math::{UVec2, Vec2};
//! use bevy_composable_dragging::heightmap::{Brush, Grid, Heightmap};
//!
//! let grid = Grid::new(UVec2::new(2, 2), 16, Vec2::ZERO, 0.5);
//! let flat = Heightmap::new(grid, 0.0, -1.0, 3.0);
//! let hill = flat.raised(&Brush {
//!     center: Vec2::new(4.0, 4.0),
//!     radius: 2.0,
//!     amount: 1.0,
//!     falloff: 1.0,
//! });
//! assert!(hill.surface(Vec2::new(4.0, 4.0)) > flat.surface(Vec2::new(4.0, 4.0)));
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bevy::math::{UVec2, Vec2, Vec3, Vec3Swizzles};
use bevy::prelude::Ray3d;

/// A round brush that raises (`amount` above zero) or lowers (below zero) the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// Where the brush is on the ground.
    pub center: Vec2,
    /// How far the brush reaches from its center.
    pub radius: f32,
    /// How much the ground changes under the middle of the brush.
    pub amount: f32,
    /// The part of the radius, from the edge inwards, over which the brush fades out. One fades
    /// from the middle, zero has a hard edge.
    pub falloff: f32,
}

impl Brush {
    /// How strongly the brush acts at `distance` from its center: one in the middle, zero at the
    /// edge, with a smooth fade between.
    fn weight(&self, distance: f32) -> f32 {
        let inner = self.radius * (1.0 - self.falloff.clamp(0.0, 1.0));
        let fade = ((distance - inner) / (self.radius - inner).max(1e-6)).clamp(0.0, 1.0);
        1.0 - fade * fade * (3.0 - 2.0 * fade)
    }
}

/// The shape of the grid of a [`Heightmap`]: how many chunks, how big they are and where the
/// grid is.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Grid {
    /// The chunks along X and along Z.
    pub chunks: UVec2,
    /// The vertices along a side of a chunk. Neighbouring chunks share the edge between them,
    /// so a chunk's mesh reads one more vertex than this along each side.
    pub side: usize,
    /// Where the vertex at the corner of the grid is on the ground plane.
    pub origin: Vec2,
    /// The distance between neighbouring vertices, which is the size of a cell.
    pub pitch: f32,
}

impl Grid {
    /// A grid of `chunks` chunks of `side` x `side` vertices, `pitch` apart, whose corner is at
    /// `origin`. A side below two and a pitch that is not positive are raised to the least that
    /// makes a grid.
    #[must_use]
    pub fn new(chunks: UVec2, side: usize, origin: Vec2, pitch: f32) -> Self {
        Self {
            chunks: chunks.max(UVec2::ONE),
            side: side.max(2),
            origin,
            pitch: pitch.max(f32::MIN_POSITIVE),
        }
    }
}

/// A square of the ground: the heights of its vertices and which of its cells are holes, row
/// by row.
#[derive(Clone, Debug, PartialEq)]
struct Chunk {
    heights: Vec<f32>,
    holes: Vec<bool>,
}

/// The whole index of a position on the grid, which is rounded down.
#[allow(
    clippy::cast_possible_truncation,
    reason = "positions are within a few thousand vertices of the grid"
)]
fn grid_index(value: f32) -> isize {
    value.floor() as isize
}

/// A count of vertices or cells as a length.
fn length(count: isize) -> f32 {
    count as f32
}

/// A grid of heights in shared chunks, with holes, that brushes edit.
#[derive(Clone, Debug, PartialEq)]
pub struct Heightmap {
    chunks: Vec<Arc<Chunk>>,
    grid: Grid,
    base: f32,
    lowest: f32,
    highest: f32,
}

impl Heightmap {
    /// Flat ground at height `base` over `grid`, which brushes keep between `lowest` and
    /// `highest`. The limits are ordered around `base`.
    #[must_use]
    pub fn new(grid: Grid, base: f32, lowest: f32, highest: f32) -> Self {
        let cells = grid.side * grid.side;
        let chunk = Arc::new(Chunk {
            heights: vec![base; cells],
            holes: vec![false; cells],
        });
        let count = (grid.chunks.x * grid.chunks.y) as usize;
        Self {
            chunks: vec![chunk; count],
            grid,
            base,
            lowest: lowest.min(base),
            highest: highest.max(base),
        }
    }

    /// This ground laid over another `grid`, which brushes keep between `lowest` and `highest`
    /// from now on: every vertex takes the height of the surface under it, and every cell keeps
    /// the hole mask of the cell under its middle. What `grid` covers beyond this ground is flat
    /// at its base height with no holes, so the ground can be grown by laying it over a larger
    /// grid, or cropped by laying it over a smaller one. A grid with a different pitch smooths
    /// or sharpens the relief to what its vertices can hold.
    #[must_use]
    pub fn resampled(&self, grid: Grid, lowest: f32, highest: f32) -> Self {
        let mut map = Self::new(grid, self.base, lowest, highest);
        let side = grid.side;
        let across = grid.chunks.x as usize;
        let cell_middle = Vec2::splat(grid.pitch / 2.0);
        for (index, slot) in map.chunks.iter_mut().enumerate() {
            let (x0, z0) = (index % across * side, index / across * side);
            let mut chunk = Chunk {
                heights: Vec::with_capacity(side * side),
                holes: Vec::with_capacity(side * side),
            };
            for z in z0..z0 + side {
                for x in x0..x0 + side {
                    let at = grid.origin
                        + Vec2::new(length(x.cast_signed()), length(z.cast_signed())) * grid.pitch;
                    let height = if self.contains(at) {
                        self.surface(at)
                    } else {
                        self.base
                    };
                    chunk.heights.push(height.clamp(map.lowest, map.highest));
                    chunk.holes.push(self.is_hole(at + cell_middle));
                }
            }
            *slot = Arc::new(chunk);
        }
        map
    }

    /// The shape of the grid.
    #[must_use]
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// The height of flat ground.
    #[must_use]
    pub fn base(&self) -> f32 {
        self.base
    }

    /// The lowest the ground goes.
    #[must_use]
    pub fn lowest(&self) -> f32 {
        self.lowest
    }

    /// The highest the ground goes.
    #[must_use]
    pub fn highest(&self) -> f32 {
        self.highest
    }

    /// The distance between neighbouring vertices, which is the size of a cell.
    #[must_use]
    pub fn pitch(&self) -> f32 {
        self.grid.pitch
    }

    /// How many chunks the ground has, row by row.
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// The vertices along X and along Z.
    #[must_use]
    pub fn vertices(&self) -> (usize, usize) {
        (
            self.grid.chunks.x as usize * self.grid.side,
            self.grid.chunks.y as usize * self.grid.side,
        )
    }

    /// How many cells there are along X and along Z.
    fn cells(&self) -> (isize, isize) {
        let (width, depth) = self.vertices();
        (width.cast_signed() - 1, depth.cast_signed() - 1)
    }

    /// Whether `xz` is over the grid, edges included. The edge is allowed the rounding error of
    /// finding a position in cells, which grows with the size of the grid.
    #[must_use]
    pub fn contains(&self, xz: Vec2) -> bool {
        let (across, deep) = self.cells();
        let cells = Vec2::new(length(across), length(deep));
        let slack = cells * f32::EPSILON * 2.0;
        let local = self.local(xz);
        local.cmpge(-slack).all() && local.cmple(cells + slack).all()
    }

    /// Where `xz` is on the grid, in cells from its corner.
    fn local(&self, xz: Vec2) -> Vec2 {
        (xz - self.grid.origin) / self.grid.pitch
    }

    /// The chunk and the place in it of vertex `(x, z)`, which is clamped to the grid.
    fn slot(&self, x: isize, z: isize) -> (usize, usize) {
        let (width, depth) = self.vertices();
        let x = usize::try_from(x.max(0)).unwrap_or(0).min(width - 1);
        let z = usize::try_from(z.max(0)).unwrap_or(0).min(depth - 1);
        let side = self.grid.side;
        (
            (z / side) * self.grid.chunks.x as usize + x / side,
            (z % side) * side + x % side,
        )
    }

    /// The height of vertex `(x, z)`. Beyond the grid the ground continues flat at the height
    /// of its edge.
    #[must_use]
    pub fn vertex_height(&self, x: isize, z: isize) -> f32 {
        let (chunk, place) = self.slot(x, z);
        self.chunks[chunk].heights[place]
    }

    /// Whether the cell whose corner is vertex `(x, z)` is a hole. The row and the column
    /// beyond the last cells have no cell, which counts as a hole.
    #[must_use]
    pub fn cell_is_hole(&self, x: isize, z: isize) -> bool {
        let (across, deep) = self.cells();
        if x < 0 || z < 0 || x >= across || z >= deep {
            return true;
        }
        let (chunk, place) = self.slot(x, z);
        self.chunks[chunk].holes[place]
    }

    /// Where vertex `(x, z)` is on the ground plane.
    #[must_use]
    pub fn position(&self, x: isize, z: isize) -> Vec2 {
        self.grid.origin + Vec2::new(length(x), length(z)) * self.grid.pitch
    }

    /// The height of the surface at `xz`, between the vertices around it, whether or not there
    /// is a hole there. Beyond the grid it is the height of the nearest edge.
    #[must_use]
    pub fn surface(&self, xz: Vec2) -> f32 {
        let (across, deep) = self.cells();
        let local = self
            .local(xz)
            .clamp(Vec2::ZERO, Vec2::new(length(across), length(deep)));
        let (x, z) = (grid_index(local.x), grid_index(local.y));
        let (fx, fz) = (local.x - length(x), local.y - length(z));
        let near = self.vertex_height(x, z) * (1.0 - fx) + self.vertex_height(x + 1, z) * fx;
        let far = self.vertex_height(x, z + 1) * (1.0 - fx) + self.vertex_height(x + 1, z + 1) * fx;
        near * (1.0 - fz) + far * fz
    }

    /// The cell `xz` is over, which is the nearest one for a point on the edge of the grid.
    fn cell_at(&self, xz: Vec2) -> (isize, isize) {
        let (across, deep) = self.cells();
        let local = self.local(xz);
        (
            grid_index(local.x).clamp(0, across - 1),
            grid_index(local.y).clamp(0, deep - 1),
        )
    }

    /// Whether there is a hole at `xz`. Beyond the grid there is none.
    #[must_use]
    pub fn is_hole(&self, xz: Vec2) -> bool {
        if !self.contains(xz) {
            return false;
        }
        let (x, z) = self.cell_at(xz);
        self.cell_is_hole(x, z)
    }

    /// The height of the ground at `xz`, or `None` where nothing holds anything up: over a
    /// hole, or beyond the grid.
    #[must_use]
    pub fn height_at(&self, xz: Vec2) -> Option<f32> {
        if !self.contains(xz) {
            return None;
        }
        let (x, z) = self.cell_at(xz);
        (!self.cell_is_hole(x, z)).then(|| self.surface(xz))
    }

    /// The direction the ground faces at `xz`.
    #[must_use]
    pub fn normal_at(&self, xz: Vec2) -> Vec3 {
        let reach = self.grid.pitch;
        let slope_x = (self.surface(xz + Vec2::X * reach) - self.surface(xz - Vec2::X * reach))
            / (2.0 * reach);
        let slope_z = (self.surface(xz + Vec2::Y * reach) - self.surface(xz - Vec2::Y * reach))
            / (2.0 * reach);
        Vec3::new(-slope_x, 1.0, -slope_z).normalize()
    }

    /// The tallest the ground is under any of `points`, which is the lowest it goes when there
    /// are none.
    #[must_use]
    pub fn highest_under(&self, points: &[Vec2]) -> f32 {
        points
            .iter()
            .map(|point| self.surface(*point))
            .reduce(f32::max)
            .unwrap_or(self.lowest)
    }

    /// The range of vertices, along X and then along Z, within `reach` of `center` and over the
    /// grid, widened by `extra` on the high side.
    fn span(
        &self,
        center: Vec2,
        reach: f32,
        extra: isize,
    ) -> (
        std::ops::RangeInclusive<isize>,
        std::ops::RangeInclusive<isize>,
    ) {
        let (across, deep) = self.cells();
        let low = self.local(center - Vec2::splat(reach));
        let high = self.local(center + Vec2::splat(reach));
        (
            grid_index(low.x).max(0)..=(grid_index(high.x) + extra).min(across),
            grid_index(low.y).max(0)..=(grid_index(high.y) + extra).min(deep),
        )
    }

    /// Raises or lowers the ground under `brush`, between the lowest and the highest the ground
    /// goes, and returns the chunks whose meshes read a vertex that changed. That includes the
    /// neighbours that share an edge with a changed chunk. Only the chunks the brush reaches
    /// are copied.
    pub fn apply(&mut self, brush: &Brush) -> Vec<usize> {
        let (x_range, z_range) = self.span(brush.center, brush.radius, 1);
        let mut changed: Option<(isize, isize, isize, isize)> = None;
        for z in z_range {
            for x in x_range.clone() {
                let weight = brush.weight(self.position(x, z).distance(brush.center));
                if weight <= 0.0 {
                    continue;
                }
                let old = self.vertex_height(x, z);
                let new = (old + brush.amount * weight).clamp(self.lowest, self.highest);
                if (new - old).abs() <= f32::EPSILON {
                    continue;
                }
                let (chunk, place) = self.slot(x, z);
                Arc::make_mut(&mut self.chunks[chunk]).heights[place] = new;
                changed = Some(changed.map_or((x, x, z, z), |(x0, x1, z0, z1)| {
                    (x0.min(x), x1.max(x), z0.min(z), z1.max(z))
                }));
            }
        }
        let Some((x0, x1, z0, z1)) = changed else {
            return Vec::new();
        };
        let side = self.grid.side.cast_signed();
        let reaches = |chunk: isize, low: isize, high: isize| {
            chunk * side <= high && chunk * side + side >= low
        };
        let mut touched = Vec::new();
        let across = self.grid.chunks.x as usize;
        for cz in 0..self.grid.chunks.y as usize {
            for cx in 0..across {
                if reaches(cx.cast_signed(), x0, x1) && reaches(cz.cast_signed(), z0, z1) {
                    touched.push(cz * across + cx);
                }
            }
        }
        touched
    }

    /// The ground after `brush` raised or lowered it.
    #[must_use]
    pub fn raised(&self, brush: &Brush) -> Self {
        let mut map = self.clone();
        map.apply(brush);
        map
    }

    /// The ground with the cells within `radius` of `center` cut out as holes, or filled in again
    /// when `hole` is false. The cell under `center` is always one of them, however small the
    /// radius, so a press never does nothing.
    #[must_use]
    pub fn with_holes(&self, center: Vec2, radius: f32, hole: bool) -> Self {
        if !self.contains(center) {
            return self.clone();
        }
        let mut map = self.clone();
        let half_cell = Vec2::splat(self.grid.pitch / 2.0);
        let under = self.local(center);
        let under = (grid_index(under.x), grid_index(under.y));
        let (x_range, z_range) = self.span(center, radius, 0);
        let (across, deep) = self.cells();
        for z in z_range.start().to_owned()..=(*z_range.end()).min(deep - 1) {
            for x in x_range.start().to_owned()..=(*x_range.end()).min(across - 1) {
                let middle = self.position(x, z) + half_cell;
                if (x, z) != under && middle.distance(center) > radius {
                    continue;
                }
                let (chunk, place) = self.slot(x, z);
                if self.chunks[chunk].holes[place] != hole {
                    Arc::make_mut(&mut map.chunks[chunk]).holes[place] = hole;
                }
            }
        }
        map
    }

    /// The ground with every vertex within `reach` of `center` that is higher than `limit`
    /// lowered to `limit`, and never below the lowest the ground goes.
    #[must_use]
    pub fn capped_near(&self, center: Vec2, reach: f32, limit: f32) -> Self {
        let mut map = self.clone();
        map.cap_near(center, reach, limit);
        map
    }

    /// Lowers in place what [`capped_near`](Self::capped_near) would.
    pub fn cap_near(&mut self, center: Vec2, reach: f32, limit: f32) {
        let limit = limit.max(self.lowest);
        let (x_range, z_range) = self.span(center, reach, 1);
        for z in z_range {
            for x in x_range.clone() {
                if self.position(x, z).distance(center) > reach || self.vertex_height(x, z) <= limit
                {
                    continue;
                }
                let (chunk, place) = self.slot(x, z);
                Arc::make_mut(&mut self.chunks[chunk]).heights[place] = limit;
            }
        }
    }

    /// Where a ray meets the surface of the ground, if it does, holes included, so a hole can
    /// be pointed at and filled again. The ray is followed through the band of heights the
    /// ground goes between, in steps of half a cell. Beyond the grid the ground continues flat,
    /// as [`surface`](Self::surface) says, so a ray can hit it there; [`contains`](Self::contains)
    /// tells whether the point is over the grid.
    #[must_use]
    pub fn hit(&self, ray: Ray3d) -> Option<Vec3> {
        let slope = ray.direction.y;
        if slope.abs() < f32::EPSILON {
            return None;
        }
        let at_high = (self.highest - ray.origin.y) / slope;
        let at_low = (self.lowest - ray.origin.y) / slope;
        let (enter, leave) = (at_high.min(at_low).max(0.0), at_high.max(at_low));
        if leave < enter {
            return None;
        }
        let above = |distance: f32| {
            let point = ray.get_point(distance);
            point.y - self.surface(point.xz())
        };
        let step = self.grid.pitch / 2.0;
        let found = if above(enter) <= 0.0 {
            enter
        } else {
            let mut previous = enter;
            let mut distance = (enter + step).min(leave);
            loop {
                if above(distance) <= 0.0 {
                    let (mut near, mut far) = (previous, distance);
                    for _ in 0..16 {
                        let middle = f32::midpoint(near, far);
                        if above(middle) > 0.0 {
                            near = middle;
                        } else {
                            far = middle;
                        }
                    }
                    break far;
                }
                if distance >= leave {
                    return None;
                }
                previous = distance;
                distance = (distance + step).min(leave);
            }
        };
        Some(ray.get_point(found))
    }

    /// The first vertex, in both directions, of chunk `chunk`.
    #[must_use]
    pub fn chunk_corner(&self, chunk: usize) -> (isize, isize) {
        let (across, side) = (self.grid.chunks.x as usize, self.grid.side);
        (
            (chunk % across * side).cast_signed(),
            (chunk / across * side).cast_signed(),
        )
    }

    /// The size of a chunk on the ground plane.
    #[must_use]
    pub fn chunk_extent(&self) -> Vec2 {
        Vec2::splat(length(self.grid.side.cast_signed()) * self.grid.pitch)
    }

    /// Where the middle of chunk `chunk` is on the ground plane, at height zero.
    #[must_use]
    pub fn chunk_center(&self, chunk: usize) -> Vec3 {
        let (x, z) = self.chunk_corner(chunk);
        let middle = self.position(x, z) + self.chunk_extent() / 2.0;
        Vec3::new(middle.x, 0.0, middle.y)
    }

    /// The heights a mesh of chunk `chunk` is built from: its vertices and the edge it shares with
    /// each neighbour, `side + 1` along each side, row by row.
    #[must_use]
    pub fn chunk_heights(&self, chunk: usize) -> Vec<f32> {
        let (x0, z0) = self.chunk_corner(chunk);
        let side = self.grid.side.cast_signed();
        let mut heights = Vec::with_capacity((self.grid.side + 1).pow(2));
        for z in z0..=z0 + side {
            for x in x0..=x0 + side {
                heights.push(self.vertex_height(x, z));
            }
        }
        heights
    }

    /// The hole mask for chunk `chunk`: one flag per cell, `side * side` of them, row by row.
    /// A `true` value means the cell is cut out and has no triangles.
    #[must_use]
    pub fn chunk_holes(&self, chunk: usize) -> Vec<bool> {
        let (x0, z0) = self.chunk_corner(chunk);
        let side = self.grid.side.cast_signed();
        let mut holes = Vec::with_capacity(self.grid.side * self.grid.side);
        for z in z0..z0 + side {
            for x in x0..x0 + side {
                holes.push(self.cell_is_hole(x, z));
            }
        }
        holes
    }

    /// A number that is the same for two chunks exactly when the same mesh is built from them: it
    /// follows the place of the grid, every height the mesh reads, including those it shares
    /// with its neighbours, and every cell it leaves out.
    #[must_use]
    pub fn chunk_key(&self, chunk: usize) -> u64 {
        let (x0, z0) = self.chunk_corner(chunk);
        let side = self.grid.side.cast_signed();
        let mut hasher = DefaultHasher::new();
        chunk.hash(&mut hasher);
        for value in [self.grid.origin.x, self.grid.origin.y, self.grid.pitch] {
            value.to_bits().hash(&mut hasher);
        }
        for height in self.chunk_heights(chunk) {
            height.to_bits().hash(&mut hasher);
        }
        for z in z0..z0 + side {
            for x in x0..x0 + side {
                self.cell_is_hole(x, z).hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Dir3;

    use super::*;

    fn flat() -> Heightmap {
        Heightmap::new(
            Grid::new(UVec2::new(3, 2), 8, Vec2::new(-1.0, 2.0), 0.25),
            0.5,
            -1.0,
            3.0,
        )
    }

    fn mound(at: Vec2, amount: f32) -> Brush {
        Brush {
            center: at,
            radius: 0.6,
            amount,
            falloff: 1.0,
        }
    }

    #[test]
    fn a_grid_is_raised_to_the_least_that_makes_one() {
        let grid = Grid::new(UVec2::ZERO, 0, Vec2::ZERO, -3.0);
        assert_eq!((grid.chunks, grid.side), (UVec2::ONE, 2));
        assert!(grid.pitch > 0.0);
    }

    #[test]
    fn flat_ground_is_at_its_base_everywhere_and_beyond_its_edge() {
        let map = flat();
        assert_eq!(map.vertices(), (24, 16));
        assert_eq!(map.chunk_count(), 6);
        for point in [
            Vec2::new(-1.0, 2.0),
            Vec2::new(2.3, 3.1),
            Vec2::new(-40.0, 90.0),
        ] {
            assert!((map.surface(point) - 0.5).abs() < 1e-6, "{point}");
        }
        assert!(map.contains(Vec2::new(0.0, 3.0)) && !map.contains(Vec2::new(-1.1, 3.0)));
    }

    #[test]
    fn limits_are_ordered_around_the_base() {
        let map = Heightmap::new(Grid::new(UVec2::ONE, 4, Vec2::ZERO, 1.0), 1.0, 2.0, 0.0);
        assert!((map.lowest() - 1.0).abs() < 1e-6 && (map.highest() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_brush_raises_lowers_and_stops_at_the_limits() {
        let mut map = flat();
        let at = Vec2::new(0.5, 3.0);
        map.apply(&mound(at, 1.0));
        assert!(map.surface(at) > 1.0);
        map.apply(&mound(at, 100.0));
        assert!((map.surface(at) - map.highest()).abs() < 1e-6);
        map.apply(&mound(at, -100.0));
        assert!((map.surface(at) - map.lowest()).abs() < 1e-6);
        assert_eq!(
            map.apply(&mound(Vec2::new(900.0, 900.0), 1.0)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn an_edit_copies_only_the_chunks_it_reaches_and_names_the_ones_that_share_its_edge() {
        let map = flat();
        let inside = Vec2::new(-1.0 + 0.25 * 3.0, 2.0 + 0.25 * 3.0);
        let mut edited = map.clone();
        assert_eq!(edited.apply(&mound(inside, 0.5)), vec![0]);
        assert!(Arc::ptr_eq(&map.chunks[5], &edited.chunks[5]));
        assert!(!Arc::ptr_eq(&map.chunks[0], &edited.chunks[0]));
        let on_edge = Vec2::new(-1.0 + 0.25 * 8.0, 2.0 + 0.25 * 3.0);
        let mut edited = map.clone();
        assert_eq!(edited.apply(&mound(on_edge, 0.5)), vec![0, 1]);
    }

    #[test]
    fn a_chunk_key_follows_exactly_what_its_mesh_reads() {
        let map = flat();
        let keys: Vec<u64> = (0..6).map(|chunk| map.chunk_key(chunk)).collect();
        assert!(keys[1..].iter().all(|key| *key != keys[0]) || keys.len() == 6);
        let edited = map.raised(&mound(Vec2::new(-1.0 + 0.25 * 3.0, 2.0 + 0.25 * 3.0), 0.5));
        assert_ne!(edited.chunk_key(0), keys[0]);
        assert_eq!(edited.chunk_key(5), keys[5]);
        let holed = map.with_holes(Vec2::new(-0.8, 2.2), 0.0, true);
        assert_ne!(holed.chunk_key(0), keys[0]);
        assert_eq!(map.chunk_key(0), flat().chunk_key(0));
    }

    #[test]
    fn a_chunk_reads_one_more_vertex_than_its_side_and_stands_on_its_square() {
        let map = flat();
        assert_eq!(map.chunk_heights(4).len(), 81);
        let center = map.chunk_center(4);
        assert!(
            (center.x - (-1.0 + 0.25 * 8.0 * 1.5)).abs() < 1e-5,
            "{center}"
        );
        assert!(
            (center.z - (2.0 + 0.25 * 8.0 * 1.5)).abs() < 1e-5,
            "{center}"
        );
        assert!((map.chunk_extent().x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn holes_are_a_mask_that_keeps_the_hill_under_them() {
        let hill = flat().raised(&mound(Vec2::new(0.5, 3.0), 1.0));
        let at = Vec2::new(0.5, 3.0);
        let height = hill.surface(at);
        let cut = hill.with_holes(at, 0.3, true);
        assert!(cut.is_hole(at) && cut.height_at(at).is_none());
        assert!(!hill.is_hole(at) && hill.height_at(at).is_some());
        assert!(!cut.is_hole(Vec2::new(-0.9, 2.1)));
        let filled = cut.with_holes(at, 0.3, false);
        assert!(!filled.is_hole(at));
        assert!((filled.surface(at) - height).abs() < 1e-6);
        assert_eq!(cut.with_holes(Vec2::new(900.0, 0.0), 1.0, true), cut);
    }

    #[test]
    fn a_press_always_cuts_the_cell_under_it() {
        let map = flat().with_holes(Vec2::new(0.51, 3.01), 0.0, true);
        assert!(map.is_hole(Vec2::new(0.51, 3.01)));
    }

    #[test]
    fn the_far_row_and_column_have_no_cell() {
        let map = flat();
        assert!(map.cell_is_hole(23, 0) && map.cell_is_hole(0, 15) && map.cell_is_hole(-1, 0));
        assert!(!map.cell_is_hole(22, 14));
    }

    #[test]
    fn capping_lowers_only_what_is_higher_and_never_below_the_lowest() {
        let at = Vec2::new(0.5, 3.0);
        let hill = flat().raised(&mound(at, 2.0));
        let capped = hill.capped_near(at, 0.4, 0.2);
        assert!(capped.surface(at) <= 0.2 + 1e-6);
        assert!(capped.surface(Vec2::new(-0.9, 2.1)) > 0.0);
        let deep = hill.capped_near(at, 0.4, -50.0);
        assert!((deep.surface(at) - hill.lowest()).abs() < 1e-6);
        let lower = flat().capped_near(at, 0.4, 5.0);
        assert_eq!(lower, flat());
    }

    #[test]
    fn the_normal_leans_away_from_a_rise() {
        let map = flat().raised(&mound(Vec2::new(0.5, 3.0), 2.0));
        let west = map.normal_at(Vec2::new(0.3, 3.0));
        let east = map.normal_at(Vec2::new(0.7, 3.0));
        assert!(west.x < 0.0 && east.x > 0.0, "{west} {east}");
        assert!((map.normal_at(Vec2::new(-0.9, 2.1)) - Vec3::Y).length() < 1e-5);
    }

    #[test]
    fn a_ray_finds_the_surface_and_a_hole_can_be_pointed_at() {
        let at = Vec2::new(0.5, 3.0);
        let map = flat().raised(&mound(at, 1.0)).with_holes(at, 0.3, true);
        let top = map.surface(at);
        let ray = Ray3d::new(Vec3::new(at.x, 20.0, at.y), Dir3::NEG_Y);
        let hit = map.hit(ray).unwrap();
        assert!((hit.y - top).abs() < 1e-3, "{hit} {top}");
        let slanted = Ray3d::new(
            Vec3::new(-0.5, 3.5, 2.5),
            Dir3::new(Vec3::new(1.0, -1.0, 0.5)).unwrap(),
        );
        let found = map.hit(slanted).unwrap();
        assert!((found.y - map.surface(found.xz())).abs() < 1e-3);
        assert!(
            map.hit(Ray3d::new(Vec3::new(at.x, 20.0, at.y), Dir3::Y))
                .is_none()
        );
        let beyond = map
            .hit(Ray3d::new(Vec3::new(90.0, 20.0, 90.0), Dir3::NEG_Y))
            .unwrap();
        assert!(!map.contains(beyond.xz()) && (beyond.y - 0.5).abs() < 1e-3);
        assert!(
            map.hit(Ray3d::new(Vec3::new(0.0, 1.0, 3.0), Dir3::X))
                .is_none()
        );
    }

    #[test]
    fn laying_the_ground_over_a_larger_grid_keeps_its_hills_and_holes_and_is_flat_beyond() {
        let hill = Vec2::new(1.0, 3.0);
        let pit = Vec2::new(1.5, 4.0);
        let map = flat().raised(&mound(hill, 1.0)).with_holes(pit, 0.3, true);
        let grid = Grid::new(UVec2::new(4, 4), 8, Vec2::new(-3.0, 0.0), 0.25);
        let grown = map.resampled(grid, -2.0, 5.0);
        assert_eq!((grown.lowest(), grown.highest()), (-2.0, 5.0));
        assert!((grown.surface(hill) - map.surface(hill)).abs() < 1e-4);
        assert!(grown.is_hole(pit) && !grown.is_hole(Vec2::new(-2.0, 0.5)));
        for outside in [Vec2::new(-2.5, 0.5), Vec2::new(4.0, 7.0)] {
            assert!(
                (grown.surface(outside) - map.base()).abs() < 1e-6,
                "{outside}"
            );
        }
    }

    #[test]
    fn laying_the_ground_over_a_smaller_grid_crops_it() {
        let hill = Vec2::new(1.0, 3.0);
        let map = flat().raised(&mound(hill, 1.0));
        let grid = Grid::new(UVec2::ONE, 8, Vec2::new(0.0, 2.5), 0.25);
        let cropped = map.resampled(grid, map.lowest(), 0.8);
        assert!(cropped.contains(hill) && !cropped.contains(Vec2::new(-0.5, 3.0)));
        assert!(
            (cropped.surface(hill) - 0.8).abs() < 1e-4,
            "kept within the limits"
        );
    }

    #[test]
    fn the_tallest_under_nothing_is_the_lowest() {
        let map = flat();
        assert!((map.highest_under(&[]) - map.lowest()).abs() < 1e-6);
        assert!((map.highest_under(&[Vec2::new(0.0, 3.0)]) - 0.5).abs() < 1e-6);
    }
}
