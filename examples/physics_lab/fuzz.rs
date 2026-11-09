//! Property-based fuzz testing for the physics lab.
//!
//! A seeded run that checks properties over many scenes and logs every failure so it replays
//! from one index.  The PRNG is `SplitMix64` so every `u64` index appears exactly once, and the
//! run accepts `--offset N` to visit `N, N+1, ...`.
//!
//! Run with:
//! ```text
//! cargo test --example physics_lab --features shapes-3d fuzz::run_fuzz -- --nocapture
//! ```

use std::fs;
use std::path::Path;

use avian3d::prelude::RigidBody;
use bevy::prelude::*;
use bevy_gradmesh::{Shape, ShapeDetail};

use crate::harness::{Body, Lab, Reading, Rig};
use crate::scenario::{ColliderKind, DETAILS, Primitive};

// ---------------------------------------------------------------------------
// PRNG – SplitMix64
// ---------------------------------------------------------------------------

/// Advance the state with the golden-ratio increment and a bijective mixer.
fn split_mix64(state: u64) -> u64 {
    let z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = (z ^ (z.wrapping_shr(30))).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z.wrapping_shr(27))).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z.wrapping_shr(31))
}

/// Draw a value in `[lo, hi)` from a stream value.
fn draw_range(stream: u64, lo: f32, hi: f32) -> f32 {
    let frac = (stream.wrapping_shr(11)) as f32 / (u64::MAX >> 11) as f32;
    lo + frac * (hi - lo)
}

/// Pick an index from a slice, returning `None` when the slice is empty.
fn draw_index<T>(stream: u64, slice: &[T]) -> Option<usize> {
    if slice.is_empty() {
        return None;
    }
    let length = u64::try_from(slice.len()).ok()?;
    usize::try_from(stream % length).ok()
}

/// How many steps of `timestep` seconds cover `duration` seconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive, small, rounded-up count of steps"
)]
fn steps_for(duration: f32, timestep: f32) -> usize {
    (duration / timestep).ceil() as usize
}

// ---------------------------------------------------------------------------
// Scene parameters derived from a single index
// ---------------------------------------------------------------------------

/// The parameters that define one fuzz scene.
#[derive(Clone, Debug)]
struct SceneParams {
    /// Fixed-step length in seconds.
    timestep: f32,
    /// Solver substeps per step.
    substeps: u32,
    /// Mesh detail level.
    detail: ShapeDetail,
    /// Collider kind.
    collider: ColliderKind,
    /// Piece scale.
    scale: f32,
    /// Mass ratio between bodies (when two bodies).
    mass_ratio: f32,
    /// Restitution.
    restitution: f32,
    /// Friction.
    friction: f32,
    /// Shape of the primary body.
    shape: Primitive,
}

impl SceneParams {
    /// Derive all parameters from a single index using low-discrepancy sampling.
    fn from_index(index: u64) -> Self {
        // Generate a sequence of stream values from the index.
        let mut streams: Vec<u64> = Vec::new();
        let mut s = index;
        for _ in 0..16 {
            s = split_mix64(s);
            streams.push(s);
        }

        // Timestep: pick from common rates.
        let timesteps = [1.0 / 120.0, 1.0 / 60.0, 1.0 / 30.0];
        let timestep = timesteps[draw_index(streams[0], &timesteps).unwrap_or(1)];

        // Substeps.
        let substeps_options = [1u32, 2, 4, 6, 12];
        let substeps = substeps_options[draw_index(streams[1], &substeps_options).unwrap_or(2)];

        // Detail.
        let detail = DETAILS[draw_index(streams[2], &DETAILS).unwrap_or(2)];

        // Collider kind – skip trimesh for dynamic bodies (avian limitation).
        let collider_options = [ColliderKind::ConvexHull, ColliderKind::Decomposition];
        let collider = collider_options[draw_index(streams[3], &collider_options).unwrap_or(0)];

        // Scale: very small to very large.
        let scale = draw_range(streams[4], 0.05, 5.0);

        // Mass ratio: 0.1 to 10.0.
        let mass_ratio = draw_range(streams[5], 0.1, 10.0);

        // Restitution: 0.0 to 1.0.
        let restitution = draw_range(streams[6], 0.0, 1.0);

        // Friction: 0.0 to 1.0.
        let friction = draw_range(streams[7], 0.0, 1.0);

        // Shape.
        let shape = Primitive::ALL[draw_index(streams[9], &Primitive::ALL).unwrap_or(0)];

        Self {
            timestep,
            substeps,
            detail,
            collider,
            scale,
            mass_ratio,
            restitution,
            friction,
            shape,
        }
    }
}

// ---------------------------------------------------------------------------
// Property checks
// ---------------------------------------------------------------------------

/// A single property violation logged for a failing index.
#[derive(Clone, Debug)]
struct Failure {
    index: u64,
    property: &'static str,
    message: String,
}

impl Failure {
    fn log_line(&self) -> String {
        format!(
            "index={} property={} {}",
            self.index, self.property, self.message
        )
    }
}

/// Check property 1: finite poses – no NaN or infinity in any position, rotation or velocity.
fn check_finite_poses(readings: &[Reading], index: u64) -> Vec<Failure> {
    let mut failures = Vec::new();
    for (i, r) in readings.iter().enumerate() {
        if !r.position.is_finite() {
            failures.push(Failure {
                index,
                property: "finite_poses",
                message: format!("body {i} position not finite: {:?}", r.position),
            });
        }
        if !r.velocity.is_finite() {
            failures.push(Failure {
                index,
                property: "finite_poses",
                message: format!("body {i} velocity not finite: {:?}", r.velocity),
            });
        }
    }
    failures
}

/// Check property 6: per-step displacement bound.
/// `abs(step) <= (abs(v) + g * dt) * dt + slop` for every body and step.
fn check_per_step_displacement(
    before: &[Reading],
    after: &[Reading],
    dt: f32,
    gravity: f32,
    index: u64,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let slop = 0.01; // small tolerance for solver discretization
    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        let displacement = (a.position - b.position).length();
        let vel_bound = b.velocity.length();
        let max_displacement = (vel_bound + gravity * dt) * dt + slop;
        if displacement > max_displacement {
            failures.push(Failure {
                index,
                property: "per_step_displacement",
                message: format!(
                    "body {i} displacement {displacement} > bound {max_displacement} (vel {vel_bound} dt {dt})"
                ),
            });
        }
    }
    failures
}

/// Check property 2: horizontal displacement bound for free flight.
/// `abs(dx) <= abs(vx0) * t + eps`
fn check_horizontal_displacement(
    initial: &Reading,
    final_r: &Reading,
    t: f32,
    eps: f32,
    index: u64,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let dx = (final_r.position.x - initial.position.x).abs();
    let bound = initial.velocity.x.abs() * t + eps;
    if dx > bound {
        failures.push(Failure {
            index,
            property: "horizontal_displacement",
            message: format!(
                "dx {} > bound {} (vx0 {} t {} eps {})",
                dx, bound, initial.velocity.x, t, eps
            ),
        });
    }
    failures
}

/// Check property 3: vertical rise bound for free flight.
/// `dy_up <= vy0^2 / (2g) + eps`
fn check_vertical_rise(
    initial: &Reading,
    final_r: &Reading,
    gravity: f32,
    eps: f32,
    index: u64,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let dy_up = final_r.position.y - initial.position.y;
    if dy_up > 0.0 && gravity > 0.0 {
        let bound = (initial.velocity.y.powi(2)) / (2.0 * gravity) + eps;
        if dy_up > bound {
            failures.push(Failure {
                index,
                property: "vertical_rise",
                message: format!(
                    "dy_up {} > bound {} (vy0 {} g {} eps {})",
                    dy_up, bound, initial.velocity.y, gravity, eps
                ),
            });
        }
    }
    failures
}

/// Check property 4: vertical fall bound.
/// `dy_down <= abs(vy0) * t + g * t * (t + dt) / 2 + eps`. The solver adds the velocity of a step
/// before it moves, so a body falls `g * dt * t / 2` further than the continuous `g * t^2 / 2`.
fn check_vertical_fall(
    initial: &Reading,
    final_r: &Reading,
    t: f32,
    dt: f32,
    gravity: f32,
    eps: f32,
    index: u64,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let dy_down = initial.position.y - final_r.position.y;
    if dy_down > 0.0 {
        let bound = initial.velocity.y.abs() * t + gravity * t * (t + dt) / 2.0 + eps;
        if dy_down > bound {
            failures.push(Failure {
                index,
                property: "vertical_fall",
                message: format!(
                    "dy_down {} > bound {} (vy0 {} t {} g {} eps {})",
                    dy_down, bound, initial.velocity.y, t, gravity, eps
                ),
            });
        }
    }
    failures
}

/// Check property 5: depth lock – the centre of mass, which is what a locked axis holds, stays
/// within eps of its initial z. The origin of a body whose mass is off-centre moves in z when
/// the body turns about an axis in the plane, so it is not measured.
fn check_depth_lock(initial: &Reading, final_r: &Reading, eps: f32, index: u64) -> Vec<Failure> {
    let mut failures = Vec::new();
    let dz = (final_r.center.z - initial.center.z).abs();
    if dz > eps {
        failures.push(Failure {
            index,
            property: "depth_lock",
            message: format!(
                "z drift {} > eps {} (z0 {} z1 {})",
                dz, eps, initial.center.z, final_r.center.z
            ),
        });
    }
    failures
}

// ---------------------------------------------------------------------------
// Scene builders
// ---------------------------------------------------------------------------

/// Build a single body from params, positioned for free-flight testing.
fn build_free_flight_body(params: &SceneParams, position: Vec3, velocity: Vec3) -> Body {
    let shape: Shape = params.shape.shape(1.0 * params.scale, params.detail);
    let mesh = shape.build();
    let collider = params
        .collider
        .build(&mesh)
        .expect("collider should build from shape");

    Body {
        collider,
        position,
        rotation: Quat::IDENTITY,
        velocity,
        restitution: params.restitution,
        friction: params.friction,
        density: 1.0,
        kind: RigidBody::Dynamic,
        swept: false,
        points: Vec::new(),
    }
}

/// Run a free-flight scene: one body dropped with an initial velocity, no contacts.
fn run_free_flight(params: &SceneParams, index: u64) -> Vec<Failure> {
    let mut failures = Vec::new();
    let gravity = 9.81;
    let rig = Rig {
        step: params.timestep,
        substeps: params.substeps,
        gravity,
    };

    let mut lab = Lab::new(rig);

    // Position the body high enough to avoid the ground for the test duration.
    let start_pos = Vec3::new(0.0, 20.0, 0.0);
    // Give it some horizontal and vertical velocity.
    let stream = split_mix64(index.wrapping_add(100));
    let vx = draw_range(stream, -5.0, 5.0);
    let vy = draw_range(stream.wrapping_add(1), 0.0, 10.0);
    let velocity = Vec3::new(vx, vy, 0.0);

    let body = build_free_flight_body(params, start_pos, velocity);
    let entity = lab.spawn(body);
    lab.lock_depth(entity);

    // Record initial state.
    let initial = lab.read(entity).expect("body should exist after spawn");

    // Step for a short duration.
    let duration = 1.0; // 1 second
    let steps = steps_for(duration, params.timestep);
    lab.step(steps);
    let t = lab.seconds();

    let final_r = lab.read(entity).expect("body should still exist");

    let eps = params.scale * 0.001;

    // Property 1: finite poses
    failures.extend(check_finite_poses(&[initial, final_r], index));

    // Property 2: horizontal displacement
    failures.extend(check_horizontal_displacement(
        &initial, &final_r, t, eps, index,
    ));

    // Property 3: vertical rise
    failures.extend(check_vertical_rise(&initial, &final_r, gravity, eps, index));

    // Property 4: vertical fall
    failures.extend(check_vertical_fall(
        &initial,
        &final_r,
        t,
        params.timestep,
        gravity,
        eps,
        index,
    ));

    // Property 5: depth lock
    failures.extend(check_depth_lock(&initial, &final_r, eps, index));

    failures
}

/// Run a per-step displacement check: step one at a time and check each step.
fn run_per_step_check(params: &SceneParams, index: u64) -> Vec<Failure> {
    let mut failures = Vec::new();
    let gravity = 9.81;
    let rig = Rig {
        step: params.timestep,
        substeps: params.substeps,
        gravity,
    };

    let mut lab = Lab::new(rig);

    let start_pos = Vec3::new(0.0, 10.0, 0.0);
    let stream = split_mix64(index.wrapping_add(200));
    let vx = draw_range(stream, -3.0, 3.0);
    let vy = draw_range(stream.wrapping_add(1), -5.0, 5.0);
    let velocity = Vec3::new(vx, vy, 0.0);

    let body = build_free_flight_body(params, start_pos, velocity);
    let entity = lab.spawn(body);
    lab.lock_depth(entity);

    let step_count = 10;
    for _ in 0..step_count {
        let before = lab.read(entity).expect("body should exist");
        lab.step(1);
        let after = lab.read(entity).expect("body should exist");

        // Property 1: finite poses
        failures.extend(check_finite_poses(&[before, after], index));

        // Property 6: per-step displacement
        failures.extend(check_per_step_displacement(
            &[before],
            &[after],
            params.timestep,
            gravity,
            index,
        ));
    }

    failures
}

/// Run a two-body scene for mass ratio testing.
fn run_two_body_scene(params: &SceneParams, index: u64) -> Vec<Failure> {
    let mut failures = Vec::new();
    let gravity = 9.81;
    let rig = Rig {
        step: params.timestep,
        substeps: params.substeps,
        gravity,
    };

    let mut lab = Lab::new(rig);

    // Body 1
    let pos1 = Vec3::new(-2.0, 10.0, 0.0);
    let vel1 = Vec3::new(1.0, 0.0, 0.0);
    let body1 = build_free_flight_body(params, pos1, vel1);
    let e1 = lab.spawn(body1);
    lab.lock_depth(e1);

    // Body 2 with different mass
    let pos2 = Vec3::new(2.0, 10.0, 0.0);
    let vel2 = Vec3::new(-1.0, 0.0, 0.0);
    let mut body2 = build_free_flight_body(params, pos2, vel2);
    body2.density = params.mass_ratio;
    let e2 = lab.spawn(body2);
    lab.lock_depth(e2);
    let starts = [e1, e2].map(|entity| lab.read(entity).expect("body should exist after spawn"));

    let duration = 0.5;
    let steps = steps_for(duration, params.timestep);
    lab.step(steps);

    let eps = params.scale * 0.001;

    // Check both bodies
    for ((label, entity), start) in [("body1", e1), ("body2", e2)].into_iter().zip(starts) {
        if let Some(r) = lab.read(entity) {
            if !r.position.is_finite() || !r.velocity.is_finite() {
                failures.push(Failure {
                    index,
                    property: "finite_poses",
                    message: format!("{label} not finite after {steps} steps"),
                });
            }
            // Depth lock
            let dz = (r.center.z - start.center.z).abs();
            if dz > eps {
                failures.push(Failure {
                    index,
                    property: "depth_lock",
                    message: format!("{label} z drift {dz} > eps {eps}"),
                });
            }
        }
    }

    failures
}

// ---------------------------------------------------------------------------
// Fuzz runner
// ---------------------------------------------------------------------------

/// Run the fuzz suite over `count` indices starting at `offset`.
/// Returns the list of failures.
fn run_fuzz(offset: u64, count: usize) -> Vec<Failure> {
    let mut all_failures = Vec::new();

    for i in 0..count {
        let idx = offset + i as u64;
        let params = SceneParams::from_index(idx);

        // Skip if collider can't be built for this shape.
        let shape: Shape = params.shape.shape(1.0 * params.scale, params.detail);
        let mesh = shape.build();
        if params.collider.build(&mesh).is_none() {
            continue;
        }

        // Run the test scenes.
        all_failures.extend(run_free_flight(&params, idx));
        all_failures.extend(run_per_step_check(&params, idx));
        all_failures.extend(run_two_body_scene(&params, idx));
    }

    all_failures
}

/// Write failures to the fuzz-logs directory.
fn write_failures(failures: &[Failure]) {
    let log_dir = Path::new("fuzz-logs");
    fs::create_dir_all(log_dir).expect("fuzz-logs directory can be created");

    if failures.is_empty() {
        return;
    }

    let mut lines = String::new();
    for f in failures {
        lines.push_str(&f.log_line());
        lines.push('\n');
    }

    fs::write(log_dir.join("failures.log"), &lines).expect("failures log can be written");
}

// ---------------------------------------------------------------------------
// Public test entry points
// ---------------------------------------------------------------------------

/// Run 10,000 indices and log any failures.
#[test]
fn run_fuzz_suite() {
    let failures = run_fuzz(0, 10_000);
    write_failures(&failures);

    if failures.is_empty() {
        eprintln!("Fuzz suite: 10,000 indices passed, no failures");
    } else {
        eprintln!(
            "Fuzz suite: {} failures logged to fuzz-logs/failures.log",
            failures.len()
        );
        for f in &failures {
            eprintln!("  {}", f.log_line());
        }
    }

    assert!(
        failures.is_empty(),
        "{} property violations found; see fuzz-logs/failures.log",
        failures.len()
    );
}

/// Verify that replaying a specific index reproduces the same result.
#[test]
fn replay_determinism() {
    let index = 42;
    let run1 = run_fuzz(index, 1);
    let run2 = run_fuzz(index, 1);

    assert_eq!(
        run1.len(),
        run2.len(),
        "replay count differs for index {index}"
    );
    for (a, b) in run1.iter().zip(run2.iter()) {
        assert_eq!(a.property, b.property, "property differs for index {index}");
        assert_eq!(a.message, b.message, "message differs for index {index}");
    }
}

/// Quick smoke test: run a small number of indices to catch obvious failures.
#[test]
fn fuzz_smoke() {
    let failures = run_fuzz(0, 100);
    if !failures.is_empty() {
        eprintln!("Smoke test: {} failures", failures.len());
        for f in &failures {
            eprintln!("  {}", f.log_line());
        }
    }
    assert!(
        failures.is_empty(),
        "{} property violations in smoke test",
        failures.len()
    );
}
