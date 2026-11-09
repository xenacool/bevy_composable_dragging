//! The terrain: a heightmap in chunks that a brush raises and lowers.
//!
//! The grid, the brush, the surface and the ray hit are the library's
//! [`Heightmap`](bevy_composable_dragging::heightmap::Heightmap), whose chunks are shared with
//! `Arc`, so copying a terrain is cheap and a brush only copies the chunks it reaches. Each chunk
//! is a `bevy_gradmesh` height field that only the chunks an edit reaches build again, and its key
//! is the chunk's key in the heightmap, so equal terrain shares one mesh asset and an edited
//! chunk gets a new one. The grass, earth, rock and snow come from the height bands of the style
//! of the chunk nodes, which the gradient material of `bevy_gradmesh` colors by world height.
//!
//! The grid is fixed: there is no level-of-detail loading.

use std::sync::Arc;

use bevy::prelude::*;
pub use bevy_composable_dragging::heightmap::Brush;
use bevy_composable_dragging::heightmap::{Grid, Heightmap};
use bevy_composable_dragging::ops::custom;
use bevy_composable_dragging::{DragSpace, DragSpec, Frame, Pointer, View, ViewNode};
use bevy_gradmesh::{Shape, Style};

use crate::Castle;
use crate::kit::Fabric;
use crate::menu::ground_spec;

/// The vertices along a side of a chunk.
const SIDE: usize = 16;
/// The distance between neighbouring vertices.
const PITCH: f32 = 0.1;
/// The lowest and the highest the ground goes.
const LOWEST: f32 = -1.0;
const HIGHEST: f32 = 3.0;

/// The ground: a [`Heightmap`] with one height field shape per chunk.
#[derive(Clone, Debug)]
pub struct Terrain {
    map: Heightmap,
    shapes: Vec<Shape>,
}

/// The style of the ground: earth in the hollows, grass on the plains, bare rock on the hills and
/// pale stone on the heights, each from the height it is listed with.
fn ground_style() -> Style {
    Fabric::Terrain.style().with_height_bands(&[
        (LOWEST, Color::srgb(0.36, 0.27, 0.18)),
        (-0.05, Color::srgb(0.34, 0.52, 0.26)),
        (1.4, Color::srgb(0.5, 0.46, 0.4)),
        (2.2, Color::srgb(0.88, 0.9, 0.92)),
    ])
}

impl Terrain {
    /// Flat ground of `across` x `deep` chunks, whose corner is at `origin`.
    pub fn flat(across: usize, deep: usize, origin: Vec2) -> Self {
        let chunks = UVec2::new(
            u32::try_from(across).unwrap_or(u32::MAX),
            u32::try_from(deep).unwrap_or(u32::MAX),
        );
        let map = Heightmap::new(Grid::new(chunks, SIDE, origin, PITCH), 0.0, LOWEST, HIGHEST);
        let shapes = (0..map.chunk_count())
            .map(|chunk| build_shape(&map, chunk))
            .collect();
        Self { map, shapes }
    }

    /// The shapes of the chunks, in the order of the chunks. Each is centered on the origin in X
    /// and Z, and [`chunk_center`](Self::chunk_center) says where it stands.
    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    /// Where the shape of chunk `chunk` stands: the middle of the chunk on the ground plane.
    pub fn chunk_center(&self, chunk: usize) -> Vec3 {
        self.map.chunk_center(chunk)
    }

    /// The height of the ground at `xz`, between the vertices around it.
    pub fn surface(&self, xz: Vec2) -> f32 {
        self.map.surface(xz)
    }

    /// The tallest the ground is under any of `points`, which is zero when there are none.
    pub fn highest_under(&self, points: &[Vec2]) -> f32 {
        if points.is_empty() {
            return 0.0;
        }
        self.map.highest_under(points)
    }

    /// Raises or lowers the ground under `brush`, and returns the chunks whose shapes were built
    /// again. Only the chunks the brush reaches are copied, and only they and their neighbours
    /// that share an edge get new shapes.
    pub fn apply(&mut self, brush: &Brush) -> Vec<usize> {
        let rebuilt = self.map.apply(brush);
        for chunk in &rebuilt {
            self.shapes[*chunk] = build_shape(&self.map, *chunk);
        }
        rebuilt
    }

    /// Where a ray meets the ground, if it does.
    pub fn hit(&self, ray: Ray3d) -> Option<Vec3> {
        self.map.hit(ray)
    }
}

/// The height field of chunk `chunk`: its vertices and the edge it shares with each neighbour.
fn build_shape(map: &Heightmap, chunk: usize) -> Shape {
    let vertices = u32::try_from(map.grid().side + 1).unwrap_or(u32::MAX);
    Shape::heightfield(
        Arc::from(map.chunk_heights(chunk)),
        UVec2::splat(vertices),
        map.chunk_extent(),
    )
}

impl Castle {
    /// The castle with the ground raised or lowered under `brush`. The pieces stand on the new
    /// ground, the lowest first so what stands on them follows.
    pub fn sculpted(&self, brush: &Brush) -> Self {
        let mut castle = self.clone();
        if castle.terrain.apply(brush).is_empty() {
            return castle;
        }
        let mut order: Vec<(u32, f32)> = castle
            .placed
            .iter()
            .map(|piece| (piece.id, piece.transform.translation.y))
            .collect();
        order.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (id, _) in order {
            if let Some(piece) = castle.piece(id) {
                let pose = piece.pose();
                castle = castle.posed(id, pose);
            }
        }
        castle
    }
}

/// Where a sculpting drag took hold: the point of the ground that was pressed, and the vertical
/// line through it that the pointer is measured along.
struct Grip {
    line: DragSpace,
    center: Vec2,
    start: f32,
}

/// Sculpts the terrain. Pressing the ground takes hold of the point under the pointer, and
/// dragging up raises the ground around it while dragging down lowers it, in the steps of the
/// precision and over its brush. The ground is shown as it will be when the pointer is released.
pub fn sculpt(castle: &Castle) -> DragSpec<Castle> {
    let castle = castle.clone();
    custom(move |context| {
        let context = context.clone();
        let castle = castle.clone();
        let mut grip: Option<Grip> = None;
        Box::new(move |pointer: Pointer| {
            let grip = grip.get_or_insert_with(|| {
                let hit = castle
                    .terrain
                    .hit(pointer.ray)
                    .unwrap_or(pointer.point + context.grab_offset());
                let line = DragSpace::axis(Vec3::new(hit.x, 0.0, hit.z), Dir3::Y);
                let start = line.resolve(pointer.ray).map_or(hit.y, |point| point.y);
                Grip {
                    line,
                    center: Vec2::new(hit.x, hit.z),
                    start,
                }
            });
            let raw = grip
                .line
                .resolve(pointer.ray)
                .map_or(0.0, |point| point.y - grip.start);
            let (shown, committed) = castle.precision.snap().apply(raw);
            let brush = |amount: f32| Brush {
                center: grip.center,
                radius: castle.precision.brush(),
                amount,
                falloff: 0.9,
            };
            Frame::new(
                context.render(&castle.sculpted(&brush(shown))),
                castle.sculpted(&brush(committed)),
                0.0,
            )
        })
    })
}

/// Adds the chunks of the terrain to `view`. Pressing the ground does what the tool says
/// ([`ground_spec`]).
pub fn push_terrain(view: &mut View<Castle>, snapshot: &Arc<Castle>) {
    for (index, shape) in snapshot.terrain.shapes().iter().enumerate() {
        let snapshot = Arc::clone(snapshot);
        view.push(
            ViewNode::grad(format!("terrain/{index}"), shape.clone())
                .at(snapshot.terrain.chunk_center(index))
                .with_style(ground_style())
                .draggable_in(DragSpace::plane(Vec3::ZERO, Dir3::Y), move || {
                    ground_spec(&snapshot)
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::{Indices, VertexAttributeValues};
    use bevy_composable_dragging::Geometry;
    use bevy_gradmesh::ColorMode;

    use super::*;

    fn flat() -> Terrain {
        Terrain::flat(3, 2, Vec2::ZERO)
    }

    fn mound(center: Vec2, amount: f32) -> Brush {
        Brush {
            center,
            radius: 0.5,
            amount,
            falloff: 1.0,
        }
    }

    /// The vertices of the grid of a chunk shape, row by row, without its skirts.
    fn positions(shape: &Shape) -> Vec<[f32; 3]> {
        match shape.build().attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(points)) => points[..17 * 17].to_vec(),
            other => panic!("positions expected, found {other:?}"),
        }
    }

    /// The normals of the grid of a chunk shape, row by row, without its skirts.
    fn normals(shape: &Shape) -> Vec<[f32; 3]> {
        match shape.build().attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(points)) => points[..17 * 17].to_vec(),
            other => panic!("normals expected, found {other:?}"),
        }
    }

    /// How many chunks have the same shape before and after, which is how many were not touched.
    fn kept(before: &Terrain, after: &Terrain) -> usize {
        before
            .shapes()
            .iter()
            .zip(after.shapes())
            .filter(|(a, b)| a == b)
            .count()
    }

    #[test]
    fn flat_ground_is_at_height_zero_with_one_shape_per_chunk() {
        let terrain = flat();
        assert_eq!(terrain.shapes().len(), 6);
        for point in [
            Vec2::new(0.3, 0.2),
            Vec2::new(4.0, 3.1),
            Vec2::new(-7.0, 9.0),
        ] {
            assert!(terrain.surface(point).abs() < 1e-6);
        }
        let shape = &terrain.shapes()[0];
        assert!(
            shape.build().count_vertices() > 17 * 17,
            "the skirts add vertices"
        );
        for normal in normals(shape) {
            assert!((Vec3::from(normal) - Vec3::Y).length() < 1e-5);
        }
    }

    #[test]
    fn a_chunk_spans_its_square_of_the_grid_and_its_skirts_hang_below_the_ground() {
        let mut terrain = flat();
        terrain.apply(&mound(Vec2::new(0.8, 0.8), 0.4));
        let side = SIDE as f32 * PITCH;
        let bounds = terrain.shapes()[0].bounds();
        assert!((bounds.size().x - side).abs() < 1e-5 && (bounds.size().z - side).abs() < 1e-5);
        assert!((bounds.max.y - 0.4).abs() < 1e-5, "{bounds:?}");
        assert!(
            bounds.min.y < -0.1,
            "the skirt hangs below the flat edge: {bounds:?}"
        );
        let flat_bounds = terrain.shapes()[5].bounds();
        assert!(flat_bounds.max.y.abs() < 1e-6, "{flat_bounds:?}");
        for chunk in 0..6 {
            let at = terrain.chunk_center(chunk);
            let (column, row) = ((chunk % 3) as f32, (chunk / 3) as f32);
            assert!((at.x - (column + 0.5) * side).abs() < 1e-5, "{chunk}: {at}");
            assert!((at.z - (row + 0.5) * side).abs() < 1e-5, "{chunk}: {at}");
            assert!(at.y.abs() < 1e-6);
        }
    }

    #[test]
    fn every_triangle_faces_up() {
        let mut terrain = flat();
        terrain.apply(&mound(Vec2::new(0.8, 0.8), 0.4));
        let shape = &terrain.shapes()[0];
        let points = positions(shape);
        let mesh = shape.build();
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("indices expected");
        };
        for triangle in indices[..16 * 16 * 6].chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(points[triangle[i] as usize]));
            assert!((b - a).cross(c - a).y > 0.0);
        }
    }

    #[test]
    fn a_brush_raises_the_ground_most_in_the_middle() {
        let mut terrain = flat();
        let center = Vec2::new(0.8, 0.8);
        terrain.apply(&mound(center, 0.5));
        assert!((terrain.surface(center) - 0.5).abs() < 1e-5);
        let ring = |distance: f32| terrain.surface(center + Vec2::X * distance);
        assert!(ring(0.1) < ring(0.0) && ring(0.3) < ring(0.1) && ring(0.45) < ring(0.3));
        assert!(ring(0.6).abs() < 1e-6, "nothing beyond the radius");
        assert!(terrain.surface(Vec2::new(4.0, 4.0)).abs() < 1e-6);
    }

    #[test]
    fn a_hard_edged_brush_changes_everything_inside_equally() {
        let mut terrain = flat();
        let center = Vec2::new(0.8, 0.8);
        terrain.apply(&Brush {
            falloff: 0.0,
            ..mound(center, 0.3)
        });
        assert!((terrain.surface(center + Vec2::X * 0.3) - 0.3).abs() < 1e-5);
        assert!(terrain.surface(center + Vec2::X * 0.6).abs() < 1e-6);
    }

    #[test]
    fn a_negative_amount_lowers_and_heights_stay_in_range() {
        let mut terrain = flat();
        let center = Vec2::new(0.8, 0.8);
        terrain.apply(&mound(center, -0.4));
        assert!((terrain.surface(center) + 0.4).abs() < 1e-5);
        for _ in 0..20 {
            terrain.apply(&mound(center, 1.0));
        }
        assert!((terrain.surface(center) - HIGHEST).abs() < 1e-5);
        for _ in 0..20 {
            terrain.apply(&mound(center, -1.0));
        }
        assert!((terrain.surface(center) - LOWEST).abs() < 1e-5);
    }

    #[test]
    fn the_surface_is_interpolated_between_vertices() {
        let mut terrain = flat();
        terrain.apply(&Brush {
            radius: 0.05,
            falloff: 0.0,
            ..mound(Vec2::new(0.8, 0.8), 1.0)
        });
        assert!((terrain.surface(Vec2::new(0.8, 0.8)) - 1.0).abs() < 1e-5);
        assert!((terrain.surface(Vec2::new(0.85, 0.8)) - 0.5).abs() < 1e-5);
        assert!((terrain.surface(Vec2::new(0.9, 0.8))).abs() < 1e-5);
    }

    #[test]
    fn a_brush_copies_only_the_chunks_it_reaches() {
        let before = flat();
        let mut after = before.clone();
        let rebuilt = after.apply(&mound(Vec2::new(0.8, 0.8), 0.5));
        assert_eq!(
            rebuilt,
            vec![0],
            "a brush inside one chunk rebuilds one shape"
        );
        assert_eq!(kept(&before, &after), 5);
        assert_ne!(before.shapes()[0], after.shapes()[0]);
        for chunk in 1..6 {
            assert_eq!(before.shapes()[chunk], after.shapes()[chunk]);
        }
    }

    #[test]
    fn a_brush_on_a_corner_reaches_the_four_chunks_around_it() {
        let before = flat();
        let mut after = before.clone();
        let rebuilt = after.apply(&mound(Vec2::new(1.6, 1.6), 0.5));
        assert_eq!(rebuilt, vec![0, 1, 3, 4]);
        assert_eq!(kept(&before, &after), 2);
    }

    #[test]
    fn a_brush_that_changes_nothing_copies_and_rebuilds_nothing() {
        let before = flat();
        let mut after = before.clone();
        assert_eq!(
            after.apply(&mound(Vec2::new(0.8, 0.8), 0.0)),
            Vec::<usize>::new()
        );
        assert_eq!(
            after.apply(&mound(Vec2::new(90.0, 90.0), 1.0)),
            Vec::<usize>::new()
        );
        assert_eq!(kept(&before, &after), 6);
    }

    #[test]
    fn equal_ground_has_equal_shapes_and_different_ground_does_not() {
        let (mut a, mut b) = (flat(), flat());
        a.apply(&mound(Vec2::new(0.8, 0.8), 0.5));
        b.apply(&mound(Vec2::new(0.8, 0.8), 0.5));
        assert_eq!(a.shapes()[0], b.shapes()[0]);
        b.apply(&mound(Vec2::new(0.8, 0.8), 0.1));
        assert_ne!(a.shapes()[0], b.shapes()[0]);
        assert_eq!(
            flat().shapes()[0],
            flat().shapes()[1],
            "every chunk is centred on the origin, so flat chunks are the same shape"
        );
    }

    #[test]
    fn the_smallest_change_of_a_height_gives_the_chunk_a_new_key() {
        let mut terrain = flat();
        let mut previous = terrain.shapes()[0].clone();
        for _ in 0..4 {
            terrain.apply(&Brush {
                radius: 0.05,
                falloff: 0.0,
                ..mound(Vec2::new(0.8, 0.8), 1e-3)
            });
            assert_ne!(terrain.shapes()[0], previous);
            previous = terrain.shapes()[0].clone();
        }
    }

    #[test]
    fn neighbouring_chunks_meet_along_their_shared_edge() {
        let mut terrain = flat();
        terrain.apply(&mound(Vec2::new(1.6, 0.8), 0.6));
        let (left, right) = (
            positions(&terrain.shapes()[0]),
            positions(&terrain.shapes()[1]),
        );
        let (from, to) = (terrain.chunk_center(0), terrain.chunk_center(1));
        for row in 0..17 {
            let a = Vec3::from(left[row * 17 + 16]) + from;
            let b = Vec3::from(right[row * 17]) + to;
            assert!(a.distance(b) < 1e-5, "{row}: {a:?} {b:?}");
        }
        let (a, b) = (normals(&terrain.shapes()[0]), normals(&terrain.shapes()[1]));
        let crest = (Vec3::from(a[8 * 17 + 16]), Vec3::from(b[8 * 17]));
        assert!(
            crest.0.y > 0.8 && (crest.0.y - crest.1.y).abs() < 1e-5,
            "each chunk shades its edge from its own heights: {crest:?}"
        );
        assert!(
            (crest.0.x + crest.1.x).abs() < 1e-5,
            "on a crest the two edge normals lean away from each other: {crest:?}"
        );
        assert!(
            Vec3::from(a[8 * 17 + 13]).x < -0.1 && Vec3::from(b[8 * 17 + 3]).x > 0.1,
            "the flanks lean away from the crest on either side of the shared edge"
        );
    }

    #[test]
    fn a_mound_tilts_the_normals_away_from_its_top() {
        let mut terrain = flat();
        terrain.apply(&Brush {
            radius: 0.8,
            ..mound(Vec2::new(0.8, 0.8), 0.6)
        });
        let normals = normals(&terrain.shapes()[0]);
        let normal = |x: usize, z: usize| Vec3::from(normals[z * 17 + x]);
        assert!(normal(8, 8).y > 0.99, "the top is level");
        assert!(normal(12, 8).x > 0.1, "the east flank faces east");
        assert!(normal(4, 8).x < -0.1, "the west flank faces west");
        assert!(normal(8, 12).z > 0.1 && normal(8, 4).z < -0.1);
    }

    #[test]
    fn heights_beyond_the_grid_continue_at_the_height_of_the_edge() {
        let mut terrain = flat();
        terrain.apply(&Brush {
            radius: 0.3,
            ..mound(Vec2::ZERO, 0.4)
        });
        assert!((terrain.surface(Vec2::new(-3.0, -3.0)) - 0.4).abs() < 1e-5);
        assert!(terrain.surface(Vec2::new(-3.0, 3.0)).abs() < 1e-5);
    }

    #[test]
    fn the_ground_under_a_set_of_points_is_the_highest_of_them() {
        let mut terrain = flat();
        terrain.apply(&mound(Vec2::new(0.8, 0.8), 0.5));
        let points = [
            Vec2::new(2.0, 2.0),
            Vec2::new(0.8, 0.8),
            Vec2::new(3.0, 3.0),
        ];
        assert!((terrain.highest_under(&points) - 0.5).abs() < 1e-5);
        assert!(terrain.highest_under(&[]).abs() < 1e-6);
        terrain.apply(&mound(Vec2::new(0.8, 0.8), -2.0));
        assert!((terrain.highest_under(&[Vec2::new(0.8, 0.8)]) - LOWEST).abs() < 1e-5);
        assert!(terrain.highest_under(&points).abs() < 1e-6);
    }

    #[test]
    fn a_ray_hits_the_ground_where_it_is() {
        let mut terrain = flat();
        let down = |x: f32, z: f32| Ray3d::new(Vec3::new(x, 10.0, z), Dir3::NEG_Y);
        let hit = terrain.hit(down(1.0, 1.0)).unwrap();
        assert!(hit.distance(Vec3::new(1.0, 0.0, 1.0)) < 1e-3, "{hit}");
        terrain.apply(&mound(Vec2::new(0.8, 0.8), 0.5));
        let hit = terrain.hit(down(0.8, 0.8)).unwrap();
        assert!((hit.y - 0.5).abs() < 1e-3, "{hit}");
        let slanted = Ray3d::new(
            Vec3::new(4.0, 6.0, 4.0),
            Dir3::new(Vec3::new(-1.0, -1.0, -1.0)).unwrap(),
        );
        let hit = terrain.hit(slanted).unwrap();
        assert!(hit.y.abs() < 0.6, "{hit}");
        assert!(
            terrain
                .hit(Ray3d::new(Vec3::new(0.0, 5.0, 0.0), Dir3::Y))
                .is_none()
        );
        assert!(
            terrain
                .hit(Ray3d::new(Vec3::new(0.0, 5.0, 0.0), Dir3::X))
                .is_none()
        );
    }

    fn castle() -> Castle {
        Castle::empty()
    }

    #[test]
    fn the_terrain_is_shown_as_one_pressable_gradient_node_per_chunk() {
        let view = crate::view(&castle());
        let chunks: Vec<_> = view
            .nodes()
            .iter()
            .filter(|node| node.id.starts_with("terrain/"))
            .collect();
        assert_eq!(chunks.len(), 42);
        for node in chunks {
            assert!(
                node.drag.is_some() && node.drag_space.is_some(),
                "{}",
                node.id
            );
            assert!(matches!(node.geometry, Geometry::Grad(_)));
        }
    }

    #[test]
    fn every_chunk_node_stands_on_its_chunk_and_is_colored_by_height_bands() {
        let castle = castle();
        let view = crate::view(&castle);
        for index in 0..42 {
            let node = view.get(&format!("terrain/{index}")).unwrap();
            assert_eq!(
                node.transform.translation,
                castle.terrain.chunk_center(index)
            );
            assert_eq!(node.transform.rotation, Quat::IDENTITY);
            assert_eq!(node.transform.scale, Vec3::ONE);
            let style = node.style.as_ref().unwrap();
            assert_eq!(style.color_mode, ColorMode::HeightBands);
            let bands = style.height_bands.unwrap();
            assert_eq!(bands.count, 4);
            assert!((bands.heights[0] - LOWEST).abs() < 1e-6);
            assert!(bands.heights.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn the_colors_follow_the_height() {
        let bands = ground_style().height_bands.unwrap();
        let color = |height: f32| {
            let band = bands.heights[..bands.count]
                .iter()
                .rposition(|start| *start <= height)
                .unwrap_or(0);
            bands.colors[band]
        };
        let (hollow, plain, hill, peak) = (color(-0.8), color(0.0), color(1.8), color(2.5));
        assert!(plain.green > plain.red, "grass is green");
        assert!(hollow.red > hollow.blue, "the hollows are earth");
        assert!(hill.red > plain.red, "rock is browner than grass");
        assert!(peak.red > 0.7, "the heights are pale");
    }

    #[test]
    fn a_wall_stands_on_the_ground_under_it() {
        let wall = crate::wall::Wall::straight((0.0, 0.0), (2.0, 0.0));
        let flat = castle().with_wall(wall.clone());
        let hill = flat.sculpted(&Brush {
            center: Vec2::new(1.0, 0.0),
            radius: 0.6,
            amount: 0.5,
            falloff: 1.0,
        });
        assert!((hill.terrain.surface(Vec2::new(1.0, 0.0)) - 0.5).abs() < 0.02);
        let root = wall.root(&hill.terrain);
        assert!((root.translation.y - 0.5).abs() < 0.02, "{root:?}");
        assert!(wall.root(&flat.terrain).translation.y.abs() < 1e-6);
        let on_hill = wall.shape(&hill.terrain).bounds();
        let on_flat = wall.shape(&flat.terrain).bounds();
        let hill_low = on_hill.center().y - on_hill.half_extents().y;
        let flat_low = on_flat.center().y - on_flat.half_extents().y;
        assert!(
            hill_low < flat_low - 0.4,
            "the ends are below the raised middle: {on_hill:?} against {on_flat:?}"
        );
        let view = crate::view(&hill);
        let world = view.world_transform(&crate::piece_id(0)).unwrap();
        assert!((world.translation.y - 0.5).abs() < 0.02);
    }

    #[test]
    fn pieces_rest_on_raised_ground_and_follow_it_when_it_changes() {
        let mut flat = castle();
        flat.add(crate::PieceKind::Tower, 1.0, 1.0, 0.0);
        let mound = |amount| Brush {
            center: Vec2::new(1.0, 1.0),
            radius: 0.8,
            amount,
            falloff: 1.0,
        };
        let raised = flat.sculpted(&mound(0.5));
        let height = |castle: &Castle| castle.piece(0).unwrap().transform.translation.y;
        assert!(height(&flat).abs() < 1e-6);
        assert!((height(&raised) - 0.5).abs() < 0.02, "{}", height(&raised));
        let lowered = raised.sculpted(&mound(-0.5));
        assert!(height(&lowered).abs() < 0.02, "{}", height(&lowered));
        let mut stacked = flat.clone();
        stacked.add(crate::PieceKind::Banner, 1.0, 1.0, 0.0);
        let on_top = stacked.sculpted(&mound(0.5));
        let (tower, banner) = (
            on_top.piece(0).unwrap().transform.translation.y,
            on_top.piece(1).unwrap().transform.translation.y,
        );
        assert!(
            (banner - (tower + crate::PieceKind::Tower.top())).abs() < 0.02,
            "{banner} on {tower}"
        );
    }

    /// A pointer whose ray goes from `eye` through `through`.
    fn looking(eye: Vec3, through: Vec3) -> Pointer {
        Pointer {
            ray: Ray3d::new(eye, Dir3::new(through - eye).unwrap()),
            point: through,
        }
    }

    #[test]
    fn dragging_up_from_the_ground_raises_it_and_dragging_down_lowers_it() {
        let mut castle = castle();
        castle.tool = crate::menu::Tool::Sculpt;
        let eye = Vec3::new(0.8, 6.0, 6.0);
        let spot = Vec3::new(0.8, 0.0, 0.8);
        let sculpt_to = |height: f32| -> Frame<Castle> {
            let context = bevy_composable_dragging::DragContext::new(
                Arc::new(crate::view),
                "terrain/25".into(),
                castle.clone(),
            )
            .with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
            let mut behavior = ground_spec(&castle).compile(&context);
            behavior(looking(eye, spot));
            behavior(looking(eye, spot + Vec3::Y * height))
        };
        let raised = sculpt_to(0.5);
        let at = |castle: &Castle| castle.terrain.surface(Vec2::new(0.8, 0.8));
        assert!(
            (0.45..=0.5 + 1e-3).contains(&at(&raised.drop_state)),
            "{}",
            at(&raised.drop_state)
        );
        let lowered = sculpt_to(-0.3);
        assert!(
            at(&lowered.drop_state) < -0.25,
            "{}",
            at(&lowered.drop_state)
        );
        let untouched = sculpt_to(0.0);
        assert!(at(&untouched.drop_state).abs() < 1e-6);
        let shown = |frame: &Frame<Castle>| match &frame.view.get("terrain/25").unwrap().geometry {
            Geometry::Grad(shape) => shape.clone(),
            other => panic!("a shape expected, found {other:?}"),
        };
        assert_ne!(
            shown(&raised),
            shown(&untouched),
            "the preview shows the new ground"
        );
        let chunk = |castle: &Castle| castle.terrain.shapes()[25].clone();
        assert_eq!(shown(&raised), chunk(&raised.drop_state));
        assert_ne!(chunk(&raised.drop_state), chunk(&castle));
    }
}
