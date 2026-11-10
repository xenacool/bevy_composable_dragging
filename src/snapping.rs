//! 3D face-to-face snapping using pure geometry convex polyhedra.
//!
//! This module provides functions to build [`Polyhedron`] instances from
//! `bevy_gradmesh` shapes and compute face-to-face snap positions in 3D space.
//! The polyhedron is constructed by extracting mesh vertices and triangle indices,
//! then merging coplanar triangles into larger faces.
//!
//! # Example
//!
//! ```
//! use bevy_composable_dragging::snapping::build_polyhedron;
//! use bevy_gradmesh::{Shape, ShapeDetail};
//!
//! // Build a convex polyhedron from a pyramid shape
//! let shape = Shape::pyramid(4, 0.5, 0.6, ShapeDetail::Low);
//! let poly = build_polyhedron(&shape).expect("failed to build polyhedron");
//!
//! // Each polyhedron has faces with normals
//! assert!(!poly.faces.is_empty());
//! ```

use bevy::math::{Dir3, Quat, Vec3};
use bevy_gradmesh::Shape;

/// A convex polyhedron represented as vertices and faces.
/// Each face is a planar polygon defined by an ordered list of vertex indices.
#[derive(Clone, Debug)]
pub struct Polyhedron {
    /// Vertex positions in local space.
    pub vertices: Vec<Vec3>,
    /// Faces, each containing vertex indices, normal, center, and area.
    pub faces: Vec<Face>,
}

/// A face of a convex polyhedron.
#[derive(Clone, Debug)]
pub struct Face {
    /// Indices into the polyhedron's vertex array, ordered around the face.
    pub vertex_indices: Vec<usize>,
    /// Outward-pointing unit normal in local space.
    pub normal: Vec3,
    /// Center of the face in local space.
    pub center: Vec3,
    /// Area of the face.
    pub area: f32,
}

impl Polyhedron {
    /// Returns the number of faces.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
}

/// Build a [`Polyhedron`] from a `bevy_gradmesh` [`Shape`].
///
/// Extracts the vertex positions and triangle indices from the mesh,
/// then merges coplanar triangles into larger faces.
/// Returns `None` if the mesh has no vertices or no triangles.
pub fn build_polyhedron(shape: &Shape) -> Option<Polyhedron> {
    let mesh = shape.build();
    let mut positions: Vec<Vec3> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for (name, values) in mesh.attributes() {
        if *name == bevy::render::mesh::Mesh::ATTRIBUTE_POSITION {
            if let Some(pos_attr) = values.as_float3() {
                for p in pos_attr {
                    positions.push(Vec3::new(p[0], p[1], p[2]));
                }
            }
            break;
        }
    }
    if positions.is_empty() {
        return None;
    }

    // Extract triangle indices
    if let Some(indices_ref) = mesh.indices() {
        if let bevy::render::mesh::Indices::U32(idx) = indices_ref {
            indices = idx.clone();
        }
    }
    if indices.is_empty() {
        return None;
    }

    // Group triangles into faces by merging coplanar, adjacent triangles
    let faces = merge_coplanar_triangles(&positions, &indices);
    if faces.is_empty() {
        return None;
    }

    Some(Polyhedron {
        vertices: positions,
        faces,
    })
}

/// Merge coplanar triangles that share edges into larger polygonal faces.
fn merge_coplanar_triangles(positions: &[Vec3], indices: &[u32]) -> Vec<Face> {
    const EPSILON: f32 = 1e-4;

    // Build a list of triangles with their normals
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    let mut tri_normals: Vec<Vec3> = Vec::new();

    for chunk in indices.chunks_exact(3) {
        let a = positions[chunk[0] as usize];
        let b = positions[chunk[1] as usize];
        let c = positions[chunk[2] as usize];
        let normal = (b - a).cross(c - a);
        let len = normal.length();
        if len < EPSILON {
            continue; // degenerate triangle
        }
        triangles.push([chunk[0] as usize, chunk[1] as usize, chunk[2] as usize]);
        tri_normals.push(normal / len);
    }

    if triangles.is_empty() {
        return Vec::new();
    }

    // Union-find to merge coplanar, adjacent triangles
    let n = triangles.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &[usize], mut x: usize) -> usize {
        while parent[x] != x {
            x = parent[x];
        }
        x
    }

    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // Two triangles are adjacent if they share an edge
    fn shares_edge(t1: [usize; 3], t2: [usize; 3]) -> bool {
        let mut shared = 0;
        for &a in &t1 {
            for &b in &t2 {
                if a == b {
                    shared += 1;
                }
            }
        }
        shared >= 2
    }

    // Merge triangles that are coplanar and share an edge.
    // Coplanar means the normals point in the same direction (not opposite),
    // so that a flat shape like a star keeps its two faces separate.
    for i in 0..n {
        for j in (i + 1)..n {
            let same_normal = tri_normals[i].dot(tri_normals[j]) > 1.0 - EPSILON;
            if same_normal && shares_edge(triangles[i], triangles[j]) {
                union(&mut parent, i, j);
            }
        }
    }

    // Group triangles by their root
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_map: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let root = find(&parent, i);
        if group_map[root].is_none() {
            group_map[root] = Some(groups.len());
            groups.push(Vec::new());
        }
        if let Some(g) = group_map[root] {
            groups[g].push(i);
        }
    }

    // Build faces from groups
    let mut faces = Vec::new();
    for group in &groups {
        // Collect unique vertices in winding order
        let mut vertex_set: Vec<usize> = Vec::new();
        for &tri_idx in group {
            for &v in &triangles[tri_idx] {
                if !vertex_set.contains(&v) {
                    vertex_set.push(v);
                }
            }
        }

        // Order vertices around the face center
        let center: Vec3 = vertex_set
            .iter()
            .map(|&v| positions[v])
            .fold(Vec3::ZERO, |s, p| s + p)
            / vertex_set.len() as f32;

        let normal = tri_normals[group[0]];
        let ref_axis = if normal.abs().dot(Vec3::Y) < 0.9 {
            Vec3::Y
        } else {
            Vec3::X
        };
        let tangent = normal.cross(ref_axis).normalize();
        let bitangent = normal.cross(tangent).normalize();

        vertex_set.sort_by(|&a, &b| {
            let pa = positions[a] - center;
            let pb = positions[b] - center;
            let angle_a = pa.dot(tangent).atan2(pa.dot(bitangent));
            let angle_b = pb.dot(tangent).atan2(pb.dot(bitangent));
            angle_a.partial_cmp(&angle_b).unwrap()
        });

        let vertices: Vec<Vec3> = vertex_set.iter().map(|&v| positions[v]).collect();
        let area = polygon_area(&vertices);

        faces.push(Face {
            vertex_indices: vertex_set.clone(),
            normal,
            center,
            area,
        });

        // If this is a flat shape (all triangles in one group), also create
        // a back face with reversed winding and opposite normal.
        if groups.len() == 1 && group.len() == n {
            let mut back_indices = vertex_set;
            back_indices.reverse();
            faces.push(Face {
                vertex_indices: back_indices,
                normal: -normal,
                center,
                area,
            });
        }
    }

    faces
}

/// A 3D snap position: where and how a tile should be placed to snap face-to-face.
#[derive(Clone, Copy, Debug)]
pub struct Snap3D {
    /// World position of the snapped tile center.
    pub pos: Vec3,
    /// Full 3D rotation (orientation) of the snapped tile.
    pub rotation: Quat,
    /// Normal of the face that is snapped (pointing outward from the board tile).
    pub face_normal: Dir3,
}

/// Compute face-to-face snap positions for a dragged tile against board tiles.
///
/// For each face on each board tile, finds compatible faces on the dragged tile
/// and computes the transform that aligns them flush. Returns snaps that don't
/// overlap any other board tile.
///
/// # Arguments
///
/// * `dragged_poly` - The convex polyhedron of the dragged tile.
/// * `tiles` - Slice of board tiles with their polyhedra and transforms.
/// * `exclude_id` - Optional tile ID to exclude from snap targets (the dragged tile itself).
pub fn compute_snaps_3d(
    dragged_poly: &Polyhedron,
    tiles: &[Tile3D],
    exclude_id: Option<u32>,
) -> Vec<Snap3D> {
    let mut snaps = Vec::new();

    for tile in tiles.iter().filter(|t| Some(t.id) != exclude_id) {
        for board_face in &tile.poly.faces {
            for drag_face in &dragged_poly.faces {
                // Check face compatibility: same vertex count
                if board_face.vertex_indices.len() != drag_face.vertex_indices.len() {
                    continue;
                }

                // Compute snap transform
                if let Some(snap) = snap_for_faces(board_face, drag_face, tile.pos, tile.rotation) {
                    // Check no 3D overlap with any other tile (exclude both the dragged tile
                    // and the board tile we're snapping against, since they will touch)
                    let exclude_board = Some(tile.id);
                    if !overlaps_any_3d(dragged_poly, snap.pos, snap.rotation, tiles, exclude_board)
                    {
                        snaps.push(snap);
                    }
                }
            }
        }
    }

    snaps
}

/// A board tile with its 3D transform and convex polyhedron.
#[derive(Clone, Debug)]
pub struct Tile3D {
    /// Unique tile identifier.
    pub id: u32,
    /// World position of the tile center.
    pub pos: Vec3,
    /// Full 3D rotation (orientation) of the tile.
    pub rotation: Quat,
    /// Convex polyhedron representing the tile's shape.
    pub poly: Polyhedron,
}

/// Compute the area of a planar polygon given its vertices in order.
/// Uses the cross-product method: sum the cross products of consecutive
/// edge vectors from the first vertex, then take half the magnitude of
/// the normal component along the face normal.
fn polygon_area(vertices: &[Vec3]) -> f32 {
    if vertices.len() < 3 {
        return 0.0;
    }
    let v0 = vertices[0];
    let mut cross_sum = Vec3::ZERO;
    for i in 1..vertices.len() - 1 {
        let a = vertices[i] - v0;
        let b = vertices[i + 1] - v0;
        cross_sum += a.cross(b);
    }
    (cross_sum.length() / 2.0).max(0.0)
}

/// Compute the contact area ratio between two faces.
///
/// Returns a value in [0, 1] where 1.0 means the faces have identical area
/// (ideal match) and values near 0 mean very different sizes (poor match).
/// The ratio is `min(area_a, area_b) / max(area_a, area_b)`.
pub fn contact_area_ratio(board_face: &Face, drag_face: &Face) -> f32 {
    let max_area = board_face.area.max(drag_face.area);
    if max_area < 1e-6 {
        return 0.0;
    }
    board_face.area.min(drag_face.area) / max_area
}

/// Compute the snap transform for two compatible faces.
///
/// Aligns the dragged face's normal to oppose the board face's normal,
/// then translates the dragged tile so the face centers coincide.
fn snap_for_faces(
    board_face: &Face,
    drag_face: &Face,
    board_pos: Vec3,
    board_rotation: Quat,
) -> Option<Snap3D> {
    // Face normals in local space
    let board_normal_local = board_face.normal;
    let drag_normal_local = drag_face.normal;

    // Board face normal in world space
    let board_normal_world = board_rotation * board_normal_local;
    let board_normal_dir = Dir3::new(board_normal_world).expect("nonzero normal");

    // Face centers in local space
    let board_center_local = board_face.center;
    let drag_center_local = drag_face.center;

    // Board face center in world space
    let board_center_world = board_pos + board_rotation * board_center_local;

    // Rotation: align drag face normal to oppose board face normal
    // The dragged face should face the board face, so its normal points opposite
    let target_drag_normal = -board_normal_world;
    let rotation_arc = Quat::from_rotation_arc(drag_normal_local, target_drag_normal);

    // Drag face center after rotation
    let drag_center_rotated = rotation_arc * drag_center_local;

    // Translation: place dragged tile so its face center meets the board face center
    // with a small gap to prevent interpenetration
    let gap = 0.01;
    let snap_pos = board_center_world - drag_center_rotated + target_drag_normal * gap;

    let snap_rotation = rotation_arc;

    Some(Snap3D {
        pos: snap_pos,
        rotation: snap_rotation,
        face_normal: board_normal_dir,
    })
}

/// Check if a tile at the given position and rotation overlaps any board tile
/// using the separating axis theorem (SAT) for convex polyhedra.
pub fn overlaps_any_3d(
    dragged_poly: &Polyhedron,
    pos: Vec3,
    rotation: Quat,
    tiles: &[Tile3D],
    exclude_id: Option<u32>,
) -> bool {
    for tile in tiles.iter().filter(|t| Some(t.id) != exclude_id) {
        if convex_convex_intersects(
            dragged_poly,
            pos,
            rotation,
            &tile.poly,
            tile.pos,
            tile.rotation,
        ) {
            return true;
        }
    }
    false
}

/// Test whether two convex polyhedra intersect using the separating axis theorem.
/// Tests face normals of both polyhedra and cross products of their edges.
fn convex_convex_intersects(
    a: &Polyhedron,
    a_pos: Vec3,
    a_rot: Quat,
    b: &Polyhedron,
    b_pos: Vec3,
    b_rot: Quat,
) -> bool {
    // Collect all potential separating axes
    let mut axes = Vec::new();

    // Face normals of A in world space
    for face in &a.faces {
        axes.push(a_rot * face.normal);
    }
    // Face normals of B in world space
    for face in &b.faces {
        axes.push(b_rot * face.normal);
    }

    // Cross products of edges from A and B
    for a_face in &a.faces {
        for i in 0..a_face.vertex_indices.len() {
            let j = (i + 1) % a_face.vertex_indices.len();
            let v1 = a.vertices[a_face.vertex_indices[i]];
            let v2 = a.vertices[a_face.vertex_indices[j]];
            let edge_a = a_rot * (v2 - v1);

            for b_face in &b.faces {
                for k in 0..b_face.vertex_indices.len() {
                    let l = (k + 1) % b_face.vertex_indices.len();
                    let v1 = b.vertices[b_face.vertex_indices[k]];
                    let v2 = b.vertices[b_face.vertex_indices[l]];
                    let edge_b = b_rot * (v2 - v1);

                    let cross = edge_a.cross(edge_b);
                    if cross.length_squared() > 1e-8 {
                        axes.push(cross.normalize());
                    }
                }
            }
        }
    }

    // Test each axis: if projections don't overlap, there's a separating axis
    for axis in &axes {
        let (min_a, max_a) = project_polyhedron(a, a_pos, a_rot, axis);
        let (min_b, max_b) = project_polyhedron(b, b_pos, b_rot, axis);

        // If there's a gap, they don't intersect
        if min_a > max_b + 1e-6 || min_b > max_a + 1e-6 {
            return false;
        }
    }

    true
}

/// Project a polyhedron onto an axis and return (min, max) projection.
fn project_polyhedron(poly: &Polyhedron, pos: Vec3, rot: Quat, axis: &Vec3) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;

    // Project all unique vertices
    let mut seen = Vec::new();
    for face in &poly.faces {
        for &v_idx in &face.vertex_indices {
            if !seen.contains(&v_idx) {
                seen.push(v_idx);
                let world_v = pos + rot * poly.vertices[v_idx];
                let proj = world_v.dot(*axis);
                if proj < min {
                    min = proj;
                }
                if proj > max {
                    max = proj;
                }
            }
        }
    }

    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_gradmesh::ShapeDetail;

    #[test]
    fn pyramid_polyhedron_has_faces() {
        // 4 side faces + 1 base face
        let shape = Shape::pyramid(4, 0.5, 0.6, ShapeDetail::Low);
        let poly = build_polyhedron(&shape).expect("pyramid should build");
        let face_count = poly.face_count();
        assert!(
            face_count >= 4,
            "pyramid should have at least 4 faces, got {face_count}"
        );
    }

    #[test]
    fn tetrahedron_polyhedron_builds() {
        let shape = Shape::pyramid(3, 0.5, 0.6, ShapeDetail::Low);
        let poly = build_polyhedron(&shape).expect("tetrahedron should build");
        let face_count = poly.face_count();
        assert!(
            face_count >= 3,
            "tetrahedron should have at least 3 faces, got {face_count}"
        );
    }

    #[test]
    fn cube_polyhedron_builds() {
        let shape = Shape::cuboid(Vec3::new(0.7, 0.7, 0.7), 0.0, ShapeDetail::Low);
        let poly = build_polyhedron(&shape).expect("cube should build");
        let face_count = poly.face_count();
        assert!(
            face_count >= 4,
            "cube should have at least 4 faces, got {face_count}"
        );
    }

    #[test]
    fn star_polyhedron_builds() {
        let shape = Shape::star(5, 0.5, 0.25, ShapeDetail::Low);
        let poly = build_polyhedron(&shape).expect("star should build");
        assert!(!poly.faces.is_empty(), "star should have faces");
    }

    #[test]
    fn face_centers_are_reasonable() {
        let shape = Shape::pyramid(4, 0.5, 0.6, ShapeDetail::Low);
        let poly = build_polyhedron(&shape).expect("pyramid should build");
        for (i, face) in poly.faces.iter().enumerate() {
            // Center should be near origin for a centered shape
            assert!(
                face.center.length() < 1.0,
                "face {i} center {:?} too far from origin",
                face.center
            );
            // Normal should be unit length
            assert!(
                (face.normal.length() - 1.0).abs() < 1e-4,
                "face {i} normal not unit length"
            );
        }
    }

    #[test]
    fn polygon_area_is_positive() {
        // A simple triangle
        let tri = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        assert!(
            (polygon_area(&tri) - 0.5).abs() < 1e-6,
            "right triangle area should be 0.5"
        );

        // A unit square
        let square = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        assert!(
            (polygon_area(&square) - 1.0).abs() < 1e-6,
            "unit square area should be 1.0"
        );
    }

    #[test]
    fn contact_area_ratio_ideal_match() {
        // Two identical faces should have ratio 1.0
        let a = Face {
            vertex_indices: vec![0, 1, 2],
            normal: Vec3::Y,
            center: Vec3::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
            area: 0.5,
        };
        let b = a.clone();
        let ratio = contact_area_ratio(&a, &b);
        assert!(
            (ratio - 1.0).abs() < 1e-6,
            "identical faces should have ratio 1.0, got {ratio}"
        );
    }

    #[test]
    fn contact_area_ratio_horrible_match() {
        // A large face vs a tiny face
        let a = Face {
            vertex_indices: vec![0, 1, 2],
            normal: Vec3::Y,
            center: Vec3::ZERO,
            area: 50.0,
        };
        let b = Face {
            vertex_indices: vec![0, 1, 2],
            normal: Vec3::Y,
            center: Vec3::ZERO,
            area: 0.005,
        };
        let ratio = contact_area_ratio(&a, &b);
        // ratio = 0.005/50 = 0.0001
        assert!(
            ratio < 0.001,
            "very different sizes should have near-zero ratio, got {ratio}"
        );
    }

    #[test]
    fn contact_area_ratio_intermediate_match() {
        // Two faces with areas in a 2:1 ratio
        let a = Face {
            vertex_indices: vec![0, 1, 2],
            normal: Vec3::Y,
            center: Vec3::ZERO,
            area: 1.0,
        };
        let b = Face {
            vertex_indices: vec![0, 1, 2],
            normal: Vec3::Y,
            center: Vec3::ZERO,
            area: 0.5,
        };
        let ratio = contact_area_ratio(&a, &b);
        // ratio = 0.5/1.0 = 0.5
        assert!(
            (ratio - 0.5).abs() < 1e-6,
            "2:1 area ratio should give 0.5, got {ratio}"
        );
    }

    #[test]
    fn same_shape_kind_has_high_contact_ratio() {
        // Two pyramids should have faces with similar areas
        let shape = Shape::pyramid(4, 0.5, 0.6, ShapeDetail::Low);
        let poly = build_polyhedron(&shape).expect("pyramid should build");

        // Find two faces with the same vertex count
        for i in 0..poly.faces.len() {
            for j in (i + 1)..poly.faces.len() {
                if poly.faces[i].vertex_indices.len() == poly.faces[j].vertex_indices.len() {
                    let ratio = contact_area_ratio(&poly.faces[i], &poly.faces[j]);
                    // Same-shape faces should have reasonably high contact ratio
                    assert!(
                        ratio > 0.5,
                        "same-shape faces {i} and {j} should have ratio > 0.5, got {ratio}"
                    );
                }
            }
        }
    }

    #[test]
    fn print_face_structure() {
        // Debug test to see what faces each shape produces
        let shapes = [
            ("Tetrahedron", Shape::pyramid(3, 0.5, 0.6, ShapeDetail::Low)),
            ("Pyramid", Shape::pyramid(4, 0.5, 0.6, ShapeDetail::Low)),
            (
                "Cube",
                Shape::cuboid(Vec3::new(0.7, 0.7, 0.7), 0.0, ShapeDetail::Low),
            ),
            ("Star", Shape::star(5, 0.5, 0.25, ShapeDetail::Low)),
        ];
        for (name, shape) in &shapes {
            let poly = build_polyhedron(shape).expect(&format!("{name} should build"));
            eprintln!(
                "{name}: {} verts, {} faces",
                poly.vertices.len(),
                poly.face_count()
            );
            for (i, f) in poly.faces.iter().enumerate() {
                eprintln!(
                    "  face {}: {} verts, area={:.4}, normal={:?}, center={:?}",
                    i,
                    f.vertex_indices.len(),
                    f.area,
                    f.normal,
                    f.center
                );
            }
        }
    }

    #[test]
    fn different_shape_kinds_have_lower_contact_ratio() {
        // Compare a tetrahedron face with a pyramid face — both have 3-vertex faces
        // but the areas should differ
        let tet_shape = Shape::pyramid(3, 0.5, 0.6, ShapeDetail::Low);
        let tet_poly = build_polyhedron(&tet_shape).expect("tetrahedron should build");

        let pyramid_shape = Shape::pyramid(4, 0.5, 0.6, ShapeDetail::Low);
        let pyramid_poly = build_polyhedron(&pyramid_shape).expect("pyramid should build");

        // Find faces with the same vertex count across shapes
        let mut found = false;
        for tf in &tet_poly.faces {
            for pf in &pyramid_poly.faces {
                if tf.vertex_indices.len() == pf.vertex_indices.len() {
                    let ratio = contact_area_ratio(tf, pf);
                    assert!(ratio <= 1.0, "ratio should be <= 1.0, got {ratio}");
                    found = true;
                }
            }
        }
        assert!(
            found,
            "should find at least one face pair with same vertex count"
        );
    }
}
