//! Checks of a mesh as a collider sees it: closed, wound consistently, outward and free of
//! degenerate triangles.

use std::collections::HashMap;

use bevy::prelude::*;

/// What is wrong, or right, with the triangles of a mesh.
///
/// Vertices that sit at the same place are welded first, because a render mesh repeats a vertex
/// for each normal and each UV seam while a collider only cares about where the corners are.
#[derive(Clone, Debug, Default)]
pub struct MeshReport {
    /// The number of triangles.
    pub triangles: usize,
    /// Triangles with an area of next to nothing, or with two corners at the same place.
    pub degenerate: usize,
    /// Corners with a NaN or infinite coordinate.
    pub non_finite: usize,
    /// Edges that are not shared by exactly two triangles.
    pub open_edges: usize,
    /// Edges that two triangles walk in the same direction, which means their winding disagrees.
    pub inconsistent_edges: usize,
    /// The volume the triangles enclose, negative when they face inward.
    pub volume: f32,
    /// The volume with every triangle turned to face the way the normals of its corners point,
    /// so that a wrong winding does not change it.
    pub oriented_volume: f32,
    /// The size of the box around the mesh.
    pub size: Vec3,
    /// The fraction of triangles whose winding faces the way the vertex normals of their corners
    /// point, or 1 when the mesh has no normals.
    pub facing_normals: f32,
}

/// The fraction of the diagonal under which two corners count as the same place.
const WELD: f32 = 1e-5;
/// The fraction of the squared diagonal under which a triangle counts as degenerate.
const DEGENERATE_AREA: f32 = 1e-9;

/// The corners of `mesh`, with repeats, in the order the mesh stores them.
pub fn vertices(mesh: &Mesh) -> Vec<Vec3> {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|values| values.as_float3())
        .map(|values| values.iter().map(|p| Vec3::from(*p)).collect())
        .unwrap_or_default()
}

impl MeshReport {
    /// Reads `mesh`, which must be a triangle list.
    pub fn of(mesh: &Mesh) -> Self {
        let positions = vertices(mesh);
        let indices: Vec<usize> = match mesh.indices() {
            Some(indices) => indices.iter().collect(),
            None => (0..positions.len()).collect(),
        };
        let non_finite = positions.iter().filter(|p| !p.is_finite()).count();
        let finite: Vec<Vec3> = positions
            .iter()
            .copied()
            .filter(|p| p.is_finite())
            .collect();
        let (low, high) = finite.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(low, high), p| (low.min(*p), high.max(*p)),
        );
        let size = if finite.is_empty() {
            Vec3::ZERO
        } else {
            high - low
        };
        let diagonal = size.length().max(f32::MIN_POSITIVE);

        let mut welded: HashMap<[u32; 3], usize> = HashMap::new();
        let ids: Vec<usize> = positions
            .iter()
            .map(|p| {
                // Adding zero turns a negative zero into a positive one, so both weld.
                let cell = (*p / (diagonal * WELD)).round() + Vec3::ZERO;
                let next = welded.len();
                *welded
                    .entry(cell.to_array().map(f32::to_bits))
                    .or_insert(next)
            })
            .collect();

        let normals: Vec<Vec3> = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|values| values.as_float3())
            .map(|values| values.iter().map(|n| Vec3::from(*n)).collect())
            .unwrap_or_default();
        let mut facing = 0_usize;
        let mut report = Self {
            triangles: indices.len() / 3,
            non_finite,
            size,
            facing_normals: 1.0,
            ..Self::default()
        };
        let mut directed: HashMap<(usize, usize), usize> = HashMap::new();
        for triangle in indices.chunks_exact(3) {
            let corners = [triangle[0], triangle[1], triangle[2]];
            let [a, b, c] = corners.map(|i| positions.get(i).copied().unwrap_or(Vec3::NAN));
            if !(a.is_finite() && b.is_finite() && c.is_finite()) {
                continue;
            }
            report.volume += a.dot(b.cross(c)) / 6.0;
            let area = (b - a).cross(c - a).length() / 2.0;
            let mut turn = 1.0;
            if !normals.is_empty() {
                let normal: Vec3 = corners
                    .iter()
                    .map(|&i| normals.get(i).copied().unwrap_or(Vec3::ZERO))
                    .sum();
                if (b - a).cross(c - a).dot(normal) > 0.0 {
                    facing += 1;
                } else {
                    turn = -1.0;
                }
            }
            report.oriented_volume += turn * a.dot(b.cross(c)) / 6.0;
            let [ia, ib, ic] = corners.map(|i| ids[i]);
            if area <= DEGENERATE_AREA * diagonal * diagonal || ia == ib || ib == ic || ia == ic {
                report.degenerate += 1;
                continue;
            }
            for (from, to) in [(ia, ib), (ib, ic), (ic, ia)] {
                *directed.entry((from, to)).or_default() += 1;
            }
        }
        if !normals.is_empty() && report.triangles > 0 {
            report.facing_normals = facing as f32 / report.triangles as f32;
        }
        for (&(from, to), &count) in &directed {
            if count > 1 {
                report.inconsistent_edges += 1;
            }
            if from < to {
                let back = directed.get(&(to, from)).copied().unwrap_or(0);
                if count + back != 2 {
                    report.open_edges += 1;
                }
            } else if !directed.contains_key(&(to, from)) {
                report.open_edges += 1;
            }
        }
        report
    }

    /// Whether every edge is shared by exactly two consistently wound triangles.
    pub fn watertight(&self) -> bool {
        self.open_edges == 0 && self.inconsistent_edges == 0 && self.triangles > 0
    }

    /// Whether the triangles face outward.
    pub fn outward(&self) -> bool {
        self.volume > 0.0
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    use super::*;

    fn tetrahedron(flip: bool) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
        );
        let mut indices = vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        if flip {
            indices.reverse();
        }
        mesh.insert_indices(Indices::U32(indices));
        mesh
    }

    #[test]
    fn a_closed_outward_mesh_is_watertight_with_positive_volume() {
        let report = MeshReport::of(&tetrahedron(false));
        assert!(report.watertight(), "{report:?}");
        assert!(report.outward(), "{report:?}");
        assert!((report.volume - 1.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn a_reversed_mesh_is_still_closed_but_faces_inward() {
        let report = MeshReport::of(&tetrahedron(true));
        assert!(report.watertight(), "{report:?}");
        assert!(!report.outward(), "{report:?}");
    }

    #[test]
    fn a_missing_face_leaves_open_edges() {
        let mut mesh = tetrahedron(false);
        mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 1, 3, 0, 3, 2]));
        let report = MeshReport::of(&mesh);
        assert_eq!(report.open_edges, 3, "{report:?}");
        assert!(!report.watertight());
    }

    #[test]
    fn a_triangle_with_no_area_is_degenerate() {
        let mut mesh = tetrahedron(false);
        mesh.insert_indices(Indices::U32(vec![
            0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3, 0, 0, 1,
        ]));
        assert_eq!(MeshReport::of(&mesh).degenerate, 1);
    }
}
