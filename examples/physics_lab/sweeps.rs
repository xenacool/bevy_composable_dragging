//! Table-driven sweeps over shape, detail, collider kind and rig. Each one writes a CSV of where
//! things hold and where they break to `target/physics_lab`, and asserts only what has to hold.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_composable_dragging::Geometry;
use bevy_gradmesh::ShapeDetail;

use crate::harness::{Body, Lab, Rig, Table};
use crate::mesh_check::MeshReport;
use crate::scenario::{ColliderKind, DETAILS, Primitive, detail_label};

/// The height a body is dropped from, above its lowest point.
const DROP: f32 = 0.5;
/// The speed under which a body counts as at rest.
const REST_SPEED: f32 = 0.05;

/// How many fixed steps of `step` seconds cover `seconds`, rounded up.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn steps_in(seconds: f32, step: f32) -> usize {
    (seconds / step).ceil().max(0.0) as usize
}

fn yes(value: bool) -> String {
    if value { "yes" } else { "no" }.into()
}

/// The volume of the convex hull of `mesh`, or zero when there is none.
fn hull_volume(mesh: &Mesh) -> f32 {
    Collider::convex_hull_from_mesh(mesh).map_or(0.0, |hull| hull.mass_properties(1.0).mass)
}

/// Which shapes have a mesh that avian can use, and how clean it is.
#[test]
fn every_shape_at_every_detail_goes_through_collider_construction() {
    let mut table = Table::new(
        "mesh_validity",
        &[
            "shape",
            "detail",
            "collider",
            "built",
            "triangles",
            "degenerate",
            "open_edges",
            "inconsistent_edges",
            "outward",
            "facing_normals",
            "convexity",
            "bounds_error",
        ],
    );
    let mut labelled = Vec::new();
    for primitive in Primitive::ALL {
        for detail in DETAILS {
            let shape = primitive.shape(1.0, detail);
            let mesh = shape.build();
            let report = MeshReport::of(&mesh);
            assert_eq!(report.non_finite, 0, "{primitive:?} {detail:?} has a NaN");
            // Either the winding or the normals are consistent on a shape that is closed, and
            // whichever is gives the volume.
            let hull = hull_volume(&mesh);
            let convexity = if hull > 0.0 {
                report.volume.abs().max(report.oriented_volume.abs()) / hull
            } else {
                0.0
            };
            if detail == ShapeDetail::Medium {
                labelled.push((primitive, convexity));
            }
            let bounds = shape.bounds();
            for kind in ColliderKind::ALL {
                // Decomposition is slow, and finer detail only adds triangles to split.
                let fine = detail.segments() > ShapeDetail::Low.segments();
                let built = if kind == ColliderKind::Decomposition && fine {
                    None
                } else {
                    Some(kind.build(&mesh))
                };
                let bounds_error =
                    built
                        .as_ref()
                        .and_then(Option::as_ref)
                        .map_or(f32::NAN, |collider| {
                            let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY, 0.0);
                            (aabb.min - bounds.min)
                                .abs()
                                .max_element()
                                .max((aabb.max - bounds.max).abs().max_element())
                        });
                table.row(vec![
                    primitive.label().into(),
                    detail_label(detail).into(),
                    kind.label().into(),
                    built
                        .as_ref()
                        .map_or("skipped".into(), |c| yes(c.is_some())),
                    report.triangles.to_string(),
                    report.degenerate.to_string(),
                    report.open_edges.to_string(),
                    report.inconsistent_edges.to_string(),
                    yes(report.outward()),
                    format!("{:.2}", report.facing_normals),
                    format!("{convexity:.3}"),
                    format!("{bounds_error:.4}"),
                ]);
            }
        }
    }
    table.write();
    // Flat shapes enclose nothing, so only solids say whether they are their own hull.
    for (primitive, convexity) in labelled.into_iter().filter(|(primitive, _)| {
        !matches!(
            primitive,
            Primitive::Disc | Primitive::Ring | Primitive::Star
        )
    }) {
        assert_eq!(
            primitive.convex(),
            convexity > 0.99,
            "{primitive:?} is labelled convex = {} but fills {convexity} of its hull",
            primitive.convex()
        );
    }
    for row in table.rows() {
        assert!(
            row[2] != "trimesh" || row[3] != "no",
            "{} at {} has no trimesh collider",
            row[0],
            row[1]
        );
    }
}

/// How a shape comes to rest on the ground.
struct Rest {
    settled_at: f32,
    residual: f32,
    asleep: bool,
    smallest: f32,
    impact_depth: f32,
    resting_depth: f32,
    travelled: f32,
    energy_gain: f32,
}

/// Drops `primitive` flat onto the ground and steps it for `seconds`.
fn rest_on_plane(
    primitive: Primitive,
    detail: ShapeDetail,
    kind: ColliderKind,
    rig: Rig,
    seconds: f32,
) -> Option<Rest> {
    let shape = primitive.shape(1.0, detail);
    let mesh = shape.build();
    let lowest = shape.bounds().min.y;
    let start = Vec3::new(0.0, DROP - lowest, 0.0);
    let mut lab = Lab::new(rig);
    let body = lab.spawn(Body::from_mesh(&mesh, kind, start)?);
    let first = lab.read(body)?;
    let initial = first.energy(rig.gravity);
    let steps = steps_in(seconds, rig.step);
    let mut rest = Rest {
        settled_at: 0.0,
        residual: 0.0,
        asleep: false,
        smallest: MeshReport::of(&mesh).size.min_element(),
        impact_depth: 0.0,
        resting_depth: 0.0,
        travelled: 0.0,
        energy_gain: 0.0,
    };
    for _ in 0..steps {
        lab.step(1);
        let now = lab.read(body)?;
        if now.velocity.length() > REST_SPEED {
            rest.settled_at = lab.seconds();
        }
        rest.impact_depth = rest.impact_depth.max(-now.lowest);
        rest.energy_gain = rest
            .energy_gain
            .max((now.energy(rig.gravity) - initial) / initial.abs().max(f32::EPSILON));
        if !now.position.is_finite() {
            rest.impact_depth = f32::NAN;
            break;
        }
    }
    let end = lab.read(body)?;
    rest.residual = end.velocity.length();
    rest.asleep = end.asleep;
    rest.resting_depth = (-end.lowest).max(0.0);
    rest.travelled = (end.position - start).xz().length();
    Some(rest)
}

#[test]
fn solids_dropped_on_a_plane_settle_and_sleep() {
    let mut table = Table::new(
        "rest_on_plane",
        &[
            "shape",
            "detail",
            "collider",
            "settled_at_s",
            "residual_speed",
            "asleep",
            "impact_depth",
            "resting_depth",
            "rolled",
            "energy_gain",
        ],
    );
    let solids = [
        Primitive::Sphere,
        Primitive::Cuboid,
        Primitive::Cylinder,
        Primitive::Cone,
        Primitive::Capsule,
        Primitive::Torus,
        Primitive::Pyramid,
        Primitive::Compound,
    ];
    let mut sunk = Vec::new();
    for primitive in solids {
        for detail in [ShapeDetail::Low, ShapeDetail::Medium, ShapeDetail::High] {
            for kind in [ColliderKind::Trimesh, ColliderKind::ConvexHull] {
                let Some(rest) = rest_on_plane(primitive, detail, kind, Rig::default(), 5.0) else {
                    continue;
                };
                assert!(
                    rest.impact_depth.is_finite(),
                    "{primitive:?} {detail:?} {kind:?} left the world"
                );
                // The resting depth has to stay under 5% of the smallest dimension.
                if rest.resting_depth > 0.05 * rest.smallest {
                    sunk.push(format!("{primitive:?} {detail:?} {kind:?}"));
                }
                table.row(vec![
                    primitive.label().into(),
                    detail_label(detail).into(),
                    kind.label().into(),
                    format!("{:.2}", rest.settled_at),
                    format!("{:.4}", rest.residual),
                    yes(rest.asleep),
                    format!("{:.4}", rest.impact_depth),
                    format!("{:.4}", rest.resting_depth),
                    format!("{:.2}", rest.travelled),
                    format!("{:.3}", rest.energy_gain),
                ]);
            }
        }
    }
    table.write();
    assert!(
        sunk.is_empty(),
        "resting more than 5% into the ground: {sunk:?}"
    );
}

/// A repeatable stream of numbers between -1 and 1, so scattered tests fail the same way twice.
fn scatter(seed: &mut u64) -> f32 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((*seed >> 33) as f32 / (1_u64 << 31) as f32) * 2.0 - 1.0
}

/// How a stack of unit cuboids stands.
struct Stack {
    holds: bool,
    top_error: f32,
    drift: f32,
    asleep: bool,
}

/// Stacks `count` unit cuboids a hair apart and slightly off centre, and steps them for six
/// seconds.
fn stack_of(count: usize, kind: ColliderKind, rig: Rig, mut seed: u64) -> Stack {
    let mesh = Primitive::Cuboid.shape(1.0, ShapeDetail::Low).build();
    let mut lab = Lab::new(rig);
    let mut bodies = Vec::new();
    for i in 0..count {
        let offset = Vec3::new(scatter(&mut seed), 0.0, scatter(&mut seed)) * 0.02;
        let at = Vec3::new(0.0, 0.5 + i as f32 * 1.002, 0.0) + offset;
        let body = Body::from_mesh(&mesh, kind, at).expect("a cuboid has a collider");
        bodies.push(lab.spawn(body));
    }
    lab.step(steps_in(6.0, rig.step));
    let top = lab.read(bodies[count - 1]).expect("the top cuboid exists");
    let top_error = (top.position.y - (count as f32 - 0.5)).abs();
    let drift = top.position.xz().length();
    let asleep = bodies
        .iter()
        .all(|body| lab.read(*body).is_some_and(|r| r.asleep));
    Stack {
        holds: top_error < 0.1 && drift < 0.5 && top.position.is_finite(),
        top_error,
        drift,
        asleep,
    }
}

#[test]
fn stacks_of_cuboids_hold_up_to_a_height_that_is_recorded() {
    let mut table = Table::new(
        "stacks",
        &["collider", "count", "holds", "top_error", "drift", "asleep"],
    );
    for kind in [ColliderKind::Trimesh, ColliderKind::ConvexHull] {
        for count in [2, 5, 10, 20] {
            let stack = stack_of(count, kind, Rig::default(), 7);
            table.row(vec![
                kind.label().into(),
                count.to_string(),
                yes(stack.holds),
                format!("{:.3}", stack.top_error),
                format!("{:.3}", stack.drift),
                yes(stack.asleep),
            ]);
            if count <= 5 {
                assert!(stack.holds, "{count} cuboids with {kind:?} collapse");
            }
        }
    }
    table.write();
}

/// What became of a sphere fired at a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shot {
    /// It stayed on its side of the wall.
    Held,
    /// It went through and carried on in a straight line.
    Passed,
    /// It touched the wall and was thrown off the line it was fired along.
    Flung,
}

impl Shot {
    fn label(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Passed => "passed",
            Self::Flung => "flung",
        }
    }
}

/// Fires a sphere at a static wall and tells what became of it.
fn fire(rig: Rig, kind: ColliderKind, speed: f32, thickness: f32, swept: bool) -> Shot {
    let rig = Rig {
        gravity: 0.0,
        ..rig
    };
    let mesh = Primitive::Sphere.shape(0.5, ShapeDetail::Medium).build();
    let mut lab = Lab::new(rig);
    let wall = 5.0;
    lab.wall(Vec3::new(thickness, 4.0, 4.0), Vec3::new(wall, 1.0, 0.0));
    let mut body = Body::from_mesh(&mesh, kind, Vec3::new(0.0, 1.0, 0.0))
        .expect("a sphere has a collider of every kind");
    body.velocity = Vec3::X * speed;
    body.swept = swept;
    let ball = lab.spawn(body);
    let flight = wall / speed + 0.2;
    lab.step(steps_in(flight, rig.step));
    let Some(end) = lab.read(ball).filter(|r| r.position.is_finite()) else {
        return Shot::Flung;
    };
    let off_line = (end.position - Vec3::new(end.position.x, 1.0, 0.0)).length();
    if end.position.x <= wall + thickness / 2.0 {
        Shot::Held
    } else if off_line < 0.5 {
        Shot::Passed
    } else {
        Shot::Flung
    }
}

#[test]
fn thin_walls_are_broken_below_a_ratio_of_thickness_to_travel_per_step() {
    let mut table = Table::new(
        "tunneling",
        &[
            "collider",
            "step_s",
            "substeps",
            "speed",
            "thickness",
            "swept",
            "ratio",
            "outcome",
        ],
    );
    let swept = false;
    let mut shots = Vec::new();
    for kind in [ColliderKind::Trimesh, ColliderKind::ConvexHull] {
        for step in [1.0 / 30.0, 1.0 / 60.0, 1.0 / 120.0, 1.0 / 240.0] {
            for substeps in [1, 6, 12] {
                for speed in [20.0, 50.0, 200.0] {
                    for thickness in [0.01, 0.1, 0.5] {
                        let rig = Rig {
                            step,
                            substeps,
                            ..Rig::default()
                        };
                        let ratio = thickness / (speed * step);
                        let shot = fire(rig, kind, speed, thickness, swept);
                        shots.push((kind, step, substeps, speed, thickness, swept, shot));
                        table.row(vec![
                            kind.label().into(),
                            format!("{step:.5}"),
                            substeps.to_string(),
                            speed.to_string(),
                            thickness.to_string(),
                            yes(swept),
                            format!("{ratio:.3}"),
                            shot.label().into(),
                        ]);
                    }
                }
            }
        }
    }
    table.write();
    for kind in [ColliderKind::Trimesh, ColliderKind::ConvexHull] {
        let of = |wanted: Shot| {
            shots
                .iter()
                .filter(|s| s.0 == kind && s.6 == wanted)
                .count()
        };
        assert!(of(Shot::Held) > 0, "{kind:?} never held a wall");
        assert!(
            of(Shot::Passed) + of(Shot::Flung) > 0,
            "{kind:?} never failed"
        );
    }
    let slow = shots
        .iter()
        .filter(|s| (s.1 - 1.0 / 240.0).abs() < 1e-6 && s.3 <= 50.0 && s.4 >= 0.1);
    for (kind, _, substeps, speed, thickness, _, shot) in slow {
        assert_eq!(
            *shot,
            Shot::Held,
            "{kind:?} at {speed} m/s, {substeps} substeps, {thickness} m"
        );
    }
}

/// Avian's swept collision is documented to stop fast bodies from passing through thin ones.
/// This records what it does to the shots that fail without it.
#[test]
fn swept_collision_is_compared_on_the_shots_that_fail_without_it() {
    let mut table = Table::new(
        "swept_collision",
        &["collider", "substeps", "thickness", "plain", "swept"],
    );
    for kind in [ColliderKind::Trimesh, ColliderKind::ConvexHull] {
        for substeps in [1, 12] {
            for thickness in [0.01, 0.1] {
                let rig = Rig {
                    step: 1.0 / 30.0,
                    substeps,
                    ..Rig::default()
                };
                table.row(vec![
                    kind.label().into(),
                    substeps.to_string(),
                    thickness.to_string(),
                    fire(rig, kind, 200.0, thickness, false).label().into(),
                    fire(rig, kind, 200.0, thickness, true).label().into(),
                ]);
            }
        }
    }
    table.write();
}

/// How the collider of a body is swapped for the next shape of a morph.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Swap {
    /// The new collider is inserted over the old one.
    InPlace,
    /// The body is inserted again with the new collider.
    Reinserted,
}

/// What a morph does to a body resting on the ground.
struct Morph {
    swaps: usize,
    deepest: f32,
    resting_depth: f32,
    pop: f32,
    asleep: bool,
    rebuild_ms: f32,
}

/// Grows a cuboid resting on the ground into a larger, rounder one over `duration` seconds,
/// through the quantised blend of `Geometry`, replacing the collider each time it changes.
fn morph_on_plane(body: RigidBody, duration: f32, detail: ShapeDetail, swap: Swap) -> Morph {
    let rig = Rig::default();
    let from = Geometry::Grad(bevy_gradmesh::Shape::cuboid(Vec3::splat(1.0), 0.0, detail));
    let to = Geometry::Grad(bevy_gradmesh::Shape::cuboid(Vec3::splat(1.4), 0.5, detail));
    let Geometry::Grad(first) = &from else {
        unreachable!("the start is a gradmesh shape")
    };
    let mut lab = Lab::new(rig);
    let mut start = Body::from_mesh(
        &first.build(),
        ColliderKind::ConvexHull,
        Vec3::new(0.0, 0.5, 0.0),
    )
    .expect("a cuboid has a hull");
    start.kind = body;
    let entity = lab.spawn(start);
    lab.step(120);
    let mut current = from.clone();
    let mut morph = Morph {
        swaps: 0,
        deepest: 0.0,
        resting_depth: 0.0,
        pop: 0.0,
        asleep: false,
        rebuild_ms: 0.0,
    };
    let steps = steps_in(duration, rig.step);
    let mut spent = std::time::Duration::ZERO;
    for step in 1..=steps {
        let blend = from.lerp(&to, step as f32 / steps as f32);
        if blend != current {
            let Geometry::Grad(shape) = &blend else {
                unreachable!("a blend of gradmesh shapes is a gradmesh shape")
            };
            let begun = std::time::Instant::now();
            let mesh = shape.build();
            spent += begun.elapsed();
            match swap {
                Swap::InPlace => assert!(lab.replace_mesh(entity, &mesh, ColliderKind::ConvexHull)),
                Swap::Reinserted => assert!(lab.swap_mesh(entity, &mesh, ColliderKind::ConvexHull)),
            }
            morph.swaps += 1;
            current = blend;
        }
        lab.step(1);
        let now = lab.read(entity).expect("the body exists");
        morph.deepest = morph.deepest.max(-now.lowest);
        morph.pop = morph.pop.max(now.velocity.y);
    }
    lab.step(120);
    let end = lab.read(entity).expect("the body exists");
    morph.resting_depth = (-end.lowest).max(0.0);
    morph.asleep = end.asleep;
    morph.rebuild_ms = spent.as_secs_f32() * 1000.0 / morph.swaps.max(1) as f32;
    morph
}

#[test]
fn a_morph_swaps_colliders_under_a_resting_body_when_the_body_is_inserted_again() {
    let mut table = Table::new(
        "morph",
        &[
            "body",
            "swap",
            "detail",
            "duration_s",
            "swaps",
            "swaps_per_s",
            "deepest",
            "resting_depth",
            "pop_speed",
            "asleep",
            "rebuild_ms",
        ],
    );
    let cases = [
        ("dynamic", RigidBody::Dynamic, Swap::InPlace),
        ("dynamic", RigidBody::Dynamic, Swap::Reinserted),
        ("kinematic", RigidBody::Kinematic, Swap::Reinserted),
    ];
    for (label, body, swap) in cases {
        for detail in [ShapeDetail::Low, ShapeDetail::Medium] {
            for duration in [0.05, 0.2, 0.5, 2.0] {
                let morph = morph_on_plane(body, duration, detail, swap);
                assert!(morph.deepest.is_finite() && morph.pop.is_finite());
                assert!(morph.swaps <= 33, "{} swaps for one morph", morph.swaps);
                if body == RigidBody::Dynamic && swap == Swap::Reinserted {
                    assert!(
                        morph.resting_depth < 0.05,
                        "{detail:?} {duration}s ends {} deep",
                        morph.resting_depth
                    );
                }
                table.row(vec![
                    label.into(),
                    if swap == Swap::Reinserted {
                        "reinserted"
                    } else {
                        "in_place"
                    }
                    .into(),
                    detail_label(detail).into(),
                    duration.to_string(),
                    morph.swaps.to_string(),
                    format!("{:.1}", morph.swaps as f32 / duration),
                    format!("{:.3}", morph.deepest),
                    format!("{:.3}", morph.resting_depth),
                    format!("{:.2}", morph.pop),
                    yes(morph.asleep),
                    format!("{:.3}", morph.rebuild_ms),
                ]);
            }
        }
    }
    table.write();
}

/// A collider inserted over the old one leaves a resting body inside whatever the new one
/// overlaps, even when the body is woken, while inserting the body again lets avian push it out.
#[test]
fn a_collider_replaced_in_place_leaves_a_body_inside_what_it_overlaps() {
    let small = bevy_gradmesh::Shape::cuboid(Vec3::splat(1.0), 0.0, ShapeDetail::Low).build();
    let large = bevy_gradmesh::Shape::cuboid(Vec3::splat(1.4), 0.0, ShapeDetail::Low).build();
    let settle = |swap: Swap| {
        let mut lab = Lab::new(Rig::default());
        let body = Body::from_mesh(&small, ColliderKind::ConvexHull, Vec3::new(0.0, 0.5, 0.0))
            .expect("a cuboid has a hull");
        let entity = lab.spawn(body);
        lab.step(120);
        match swap {
            Swap::InPlace => {
                assert!(lab.replace_mesh(entity, &large, ColliderKind::ConvexHull));
                lab.wake(entity);
            }
            Swap::Reinserted => assert!(lab.swap_mesh(entity, &large, ColliderKind::ConvexHull)),
        }
        lab.step(120);
        (-lab.read(entity).expect("the body exists").lowest).max(0.0)
    };
    assert!(
        settle(Swap::InPlace) > 0.15,
        "in place now resolves the overlap, so a gap is not needed"
    );
    assert!(settle(Swap::Reinserted) < 0.01);
}

// The bounds a shape caches are not part of its equality or its hash.
#[allow(clippy::mutable_key_type)]
#[test]
fn thousands_of_morph_frames_use_only_the_quantised_shapes() {
    use std::collections::HashSet;
    let from = Geometry::Grad(bevy_gradmesh::Shape::cuboid(
        Vec3::splat(1.0),
        0.0,
        ShapeDetail::Medium,
    ));
    let to = Geometry::Grad(bevy_gradmesh::Shape::cuboid(
        Vec3::splat(1.4),
        0.5,
        ShapeDetail::Medium,
    ));
    let mut seen = HashSet::new();
    for frame in 0..=10_000 {
        seen.insert(from.lerp(&to, frame as f32 / 10_000.0));
    }
    assert!(
        seen.len() <= 33,
        "{} distinct shapes, so a cache keyed by shape would keep growing",
        seen.len()
    );
}
