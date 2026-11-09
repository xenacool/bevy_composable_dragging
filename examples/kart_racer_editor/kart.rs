//! The kart: its state, the control it is given and the systems that drive it.
//!
//! Ported from the `bevy_gradmesh` kart racer example, with avian carrying the kart: it is a
//! dynamic body that rests on the road and the ground through the colliders of their meshes, so
//! it follows their height and leans with the banking of the road without any rule of ours. All
//! that is ours is the driver: [`drive`] sets the speed and the turning from the keys, and
//! [`KartState`] mirrors the body for the race, the camera and the HUD. The kart is off the road
//! when it is further from the centerline than half the local road width, and where there is
//! neither road nor ground, over a hole, past the edge of the map or off a bridge, it falls.

use avian3d::prelude::{
    CoefficientCombine, Collider, Friction, LinearVelocity, LockedAxes, Restitution, RigidBody,
    SleepingDisabled, SpatialQuery, SpatialQueryFilter, SweptCcd,
};
use bevy::prelude::*;

use crate::ground::Ground;
use crate::race::Race;
use crate::track::{BakedTrack, MAX_GRADE};

/// The width of the kart including its wheels.
pub const KART_WIDTH: f32 = 1.4;
/// The fastest the kart drives forward without any coins.
pub const MAX_SPEED: f32 = 80.0;
/// How much faster, as a share of [`MAX_SPEED`], each collected coin lets the kart go.
pub const COIN_SPEED_GAIN: f32 = 0.02;
/// How many coins count toward the top speed; the ones after them are only collected.
pub const SPEED_COINS: u32 = 5;
/// The fastest the kart drives backward.
pub const MAX_REVERSE_SPEED: f32 = 24.0;
/// The longest time step the driver simulates at once.
pub const MAX_STEP: f32 = 0.033;
/// How far along the road, either way, the kart looks for the part of the road it is on.
const SEARCH_WINDOW: f32 = 15.0;
/// How far beyond the road edge the kart can stray before it starts to slow down.
const OFF_ROAD_MARGIN: f32 = 10.0;
/// How much further the kart then has to stray to be slowed down at full strength.
const OFF_ROAD_RAMP: f32 = 8.0;
/// The frame rate the per-frame constants of the driver are tuned for.
const REFERENCE_RATE: f32 = 60.0;
/// How much faster, each second, a kart that nothing holds up falls: well above the pull of
/// the earth, so a kart that drives off an edge drops like a game's.
pub const GRAVITY: f32 = 30.0;
/// The radius of the ball that stands for the kart in the physics.
const BALL: f32 = 0.5;
/// How far the ball is lifted so the kart does not start inside the road it rests on.
const BALL_LIFT: f32 = 0.02;
/// How far under the kart the surface it leans with is looked for, beyond its own radius.
const SURFACE_REACH: f32 = 1.5;
/// How quickly, each second, the kart takes the lean of the surface under it.
const LEAN_RATE: f32 = 12.0;

/// Marks the kart model; the transform of the entity follows the physics body.
#[derive(Component)]
pub struct Kart;

/// Marks the camera that follows the kart.
#[derive(Component)]
pub struct KartCamera;

/// The physics of the kart: a ball that rolls over whatever it is over without turning, as the
/// driver gives the kart its heading, its grip and its speed. Staying upright is the point: the
/// lean of the kart is that of the surface under it, see [`read_kart`].
pub fn body() -> impl Bundle {
    (
        RigidBody::Dynamic,
        Collider::compound(vec![(
            Vec3::Y * (BALL + BALL_LIFT),
            Quat::IDENTITY,
            Collider::sphere(BALL),
        )]),
        LockedAxes::ROTATION_LOCKED,
        Friction::new(0.0)
            .with_static_coefficient(1.0)
            .with_combine_rule(CoefficientCombine::Min),
        Restitution::ZERO.with_combine_rule(CoefficientCombine::Min),
        SweptCcd::default(),
        SleepingDisabled,
    )
}

/// Where the kart is and how it moves, as the body of the kart has it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct KartState {
    /// The kart on the road surface.
    pub position: Vec3,
    /// The direction the kart faces in plan, as an angle about `Y` from `+Z`.
    pub heading: f32,
    /// The speed along the way the kart faces; negative when reversing.
    pub speed: f32,
    /// How many coins have been collected; the first [`SPEED_COINS`] raise the top speed.
    pub coins: u32,
    /// The arc-length distance along the track of the road under the kart.
    pub distance: f32,
    /// How fast the kart rises; negative while it falls.
    pub climb: f32,
    /// The way the kart faces up, which is the normal of the surface it rests on.
    pub up: Vec3,
    /// How far the kart is to the right of the road centerline; negative on the left.
    pub lateral: f32,
    /// Half the width of the road under the kart.
    pub half_width: f32,
}

impl Default for KartState {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            heading: 0.0,
            speed: 0.0,
            coins: 0,
            distance: 0.0,
            climb: 0.0,
            up: Vec3::Y,
            lateral: 0.0,
            half_width: 8.0,
        }
    }
}

impl KartState {
    /// A standing kart on the road centerline at `distance` along the track, facing along it.
    pub fn at_distance(track: &BakedTrack, distance: f32) -> Self {
        let frame = track.sample_at_distance(distance);
        Self {
            position: frame.position,
            heading: frame.tangent.x.atan2(frame.tangent.z),
            distance: distance.rem_euclid(track.length()),
            up: frame.up,
            half_width: frame.width / 2.0,
            ..Self::default()
        }
    }

    /// A standing kart at the start line, as at the start of a race.
    pub fn at_start(track: &BakedTrack) -> Self {
        Self::at_distance(track, 0.0)
    }

    /// The kart back at the start line and standing, keeping the coins it has collected.
    #[must_use]
    pub fn respawn(self, track: &BakedTrack) -> Self {
        Self {
            coins: self.coins,
            ..Self::at_start(track)
        }
    }

    /// The horizontal direction the kart faces.
    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.heading.sin(), 0.0, self.heading.cos())
    }

    /// The orientation of the kart: turned to its heading and leaning with the road.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_arc(Vec3::Y, self.up) * Quat::from_rotation_y(self.heading)
    }

    /// Whether the kart has left the road surface.
    pub fn off_road(&self) -> bool {
        self.lateral.abs() > self.half_width
    }

    /// Whether nothing holds the kart up and it is falling: it drops faster than any road the kart
    /// can drive down would take it, plus the bumps of the way.
    pub fn falling(&self) -> bool {
        self.climb < -(self.speed.abs() * MAX_GRADE + KART_WIDTH)
    }
}

/// A kart to put down, standing, wherever its state says: the way to move the kart, as the body
/// is what holds its place.
#[derive(Message, Clone, Copy, Debug)]
pub struct PutKart(pub KartState);

/// The height below which a kart has fallen out of the world: the abyss of the ground, or a
/// relief of the ground below the lowest point of the road when that is lower.
pub fn abyss(ground: &Ground, track: &BakedTrack) -> f32 {
    let lowest_road = track
        .frames()
        .iter()
        .map(|frame| frame.position.y)
        .fold(f32::MAX, f32::min);
    ground.abyss().min(lowest_road - ground.relief())
}

/// The fastest a kart that has collected `coins` coins drives forward: each of the first
/// [`SPEED_COINS`] adds [`COIN_SPEED_GAIN`] of [`MAX_SPEED`].
pub fn top_speed(coins: u32) -> f32 {
    MAX_SPEED * (1.0 + COIN_SPEED_GAIN * coins.min(SPEED_COINS) as f32)
}

/// What the driver asks of the kart.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KartInput {
    /// `1.0` accelerates fully, `-0.6` brakes and reverses.
    pub throttle: f32,
    /// `1.0` steers left, `-1.0` steers right.
    pub steer: f32,
}

impl KartInput {
    /// Maps the pressed keys: W or Up accelerates, S or Down brakes, A or Left steers left and
    /// D or Right steers right. Braking beats accelerating and steering right beats left.
    pub fn from_keys(keys: &ButtonInput<KeyCode>) -> Self {
        let throttle = if keys.any_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
            -0.6
        } else if keys.any_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
            1.0
        } else {
            0.0
        };
        let steer = if keys.any_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) {
            -1.0
        } else if keys.any_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) {
            1.0
        } else {
            0.0
        };
        Self { throttle, steer }
    }
}

/// How the kart moves: the velocity of its body and the way it faces in plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// The linear velocity.
    pub velocity: Vec3,
    /// The direction the kart faces in plan, as an angle about `Y` from `+Z`.
    pub heading: f32,
}

/// How far the kart has strayed beyond the road and its margin, from `0.0` on or near the road
/// to `1.0` far from it, which is how strongly the land slows it down.
pub fn strayed(state: &KartState) -> f32 {
    let reach = state.half_width + OFF_ROAD_MARGIN;
    ((state.lateral.abs() - reach).max(0.0) / OFF_ROAD_RAMP).min(1.0)
}

/// What the driver makes of the `motion` of a kart that rests on a surface leaning as `up` does,
/// over `dt` seconds.
///
/// The kart turns, less the faster it goes, accelerates along the surface the way it faces, as
/// much as its `coins` allow, and loses all the speed it has sideways to its tyres. Beyond the
/// road edge and a margin, `strayed` slows it down. Gravity and the road are not here: the physics
/// gives them, and `up` is what the physics found under the kart.
pub fn drive(
    motion: Motion,
    up: Vec3,
    input: KartInput,
    coins: u32,
    strayed: f32,
    dt: f32,
) -> Motion {
    let frames = dt * REFERENCE_RATE;
    let along_before = motion.velocity.dot(level_forward(motion.heading, up));
    let speed_factor = (along_before.abs() / 20.0).min(1.0);
    let heading = motion.heading + input.steer * 2.5 * dt * (0.3 + 0.7 * (1.0 - speed_factor));

    let forward = level_forward(heading, up);
    let side = up.cross(forward);
    let along = motion.velocity.dot(forward);
    let mut speed = along;
    if input.throttle > 0.0 {
        speed += input.throttle * 50.0 * dt;
    } else if input.throttle < 0.0 {
        speed += input.throttle * 60.0 * dt;
    }
    let drag: f32 = if input.throttle > 0.0 { 0.998 } else { 0.99 };
    speed = (speed * drag.powf(frames)).clamp(-MAX_REVERSE_SPEED, top_speed(coins));
    speed *= (1.0 - strayed * 0.05).powf(frames);

    let slide = motion.velocity.dot(side);
    Motion {
        velocity: motion.velocity + forward * (speed - along) - side * slide,
        heading,
    }
}

/// The direction of `heading` along a surface that leans as `up` does.
fn level_forward(heading: f32, up: Vec3) -> Vec3 {
    let flat = Vec3::new(heading.sin(), 0.0, heading.cos());
    (flat - up * flat.dot(up)).normalize_or(flat)
}

/// What a driver who follows the centerline asks of the kart: it aims at the road a little way
/// ahead and keeps to a moderate speed. For the tests that drive whole laps.
#[cfg(test)]
pub fn autopilot(state: &KartState, track: &BakedTrack) -> KartInput {
    use std::f32::consts::{PI, TAU};

    let ahead = track.sample_at_distance(state.distance + 12.0);
    let to = ahead.position - state.position;
    let turn = (to.x.atan2(to.z) - state.heading + PI).rem_euclid(TAU) - PI;
    KartInput {
        throttle: if state.speed < 30.0 { 1.0 } else { 0.0 },
        steer: if turn > 0.03 {
            1.0
        } else if turn < -0.03 {
            -1.0
        } else {
            0.0
        },
    }
}

/// Presses the keys that [`autopilot`] asks for, for the tests that run the real systems.
#[cfg(test)]
pub fn autopilot_keys(
    state: Res<KartState>,
    track: Res<BakedTrack>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    let input = autopilot(&state, &track);
    for (key, held) in [
        (KeyCode::KeyW, input.throttle > 0.0),
        (KeyCode::KeyA, input.steer > 0.0),
        (KeyCode::KeyD, input.steer < 0.0),
    ] {
        if held {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
}

/// The road at the place a kart is over, which may be a bridge above it or the road beside it.
struct RoadUnder {
    /// How far along the track that place is.
    distance: f32,
    /// How far the place is to the right of the centerline.
    lateral: f32,
    /// Half the width of the road there.
    half_width: f32,
}

/// Finds the road under `position`, near `around` metres along the track, on the surface plane
/// of the road there.
fn road_under(position: Vec3, around: f32, track: &BakedTrack) -> RoadUnder {
    let near = track.nearest_distance_near(position, around, SEARCH_WINDOW);
    let coarse = track.sample_at_distance(near);
    let along = (position - coarse.position).dot(coarse.tangent);
    let distance = (near + along).rem_euclid(track.length());
    let frame = track.sample_at_distance(distance);

    let offset = Vec3::new(
        position.x - frame.position.x,
        0.0,
        position.z - frame.position.z,
    );
    let rise = -(frame.up.x * offset.x + frame.up.z * offset.z) / frame.up.y.max(0.2);
    RoadUnder {
        distance,
        lateral: (offset + Vec3::Y * rise).dot(frame.right),
        half_width: frame.width / 2.0,
    }
}

/// The normal of the surface under the kart, found by looking down onto the colliders of the
/// road and the ground, or `None` when there is none within reach: the kart is in the air.
fn surface_under(spatial: &SpatialQuery, kart: Entity, position: Vec3, up: Vec3) -> Option<Vec3> {
    let hit = spatial.cast_ray(
        position + up * BALL,
        Dir3::new(-up).unwrap_or(Dir3::NEG_Y),
        BALL + SURFACE_REACH,
        true,
        &SpatialQueryFilter::from_excluded_entities([kart]),
    )?;
    Some(if hit.normal.y < 0.0 {
        -hit.normal
    } else {
        hit.normal
    })
}

/// Lets [`KartState`] say where the body of the kart is and how it moves, leans the kart with the
/// surface under it, and turns its model to the way it faces.
pub fn read_kart(
    time: Res<Time>,
    track: Res<BakedTrack>,
    spatial: SpatialQuery,
    kart: Single<(Entity, &mut Transform, &LinearVelocity), With<Kart>>,
    mut state: ResMut<KartState>,
) {
    let (entity, mut transform, velocity) = kart.into_inner();
    let position = transform.translation;
    let lean = 1.0 - (-LEAN_RATE * time.delta_secs()).exp();
    let target = surface_under(&spatial, entity, position, state.up).unwrap_or(Vec3::Y);
    let up = state.up.slerp(target, lean).normalize_or(Vec3::Y);
    let road = road_under(position, state.distance, &track);
    *state = KartState {
        position,
        speed: velocity.dot(level_forward(state.heading, up)),
        distance: road.distance,
        climb: velocity.y,
        up,
        lateral: road.lateral,
        half_width: road.half_width,
        ..*state
    };
    transform.rotation = state.rotation();
}

/// Reads the keyboard, restarts the race on `R` and drives the body of the kart.
pub fn drive_kart(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    track: Res<BakedTrack>,
    mut state: ResMut<KartState>,
    mut race: ResMut<Race>,
    mut puts: MessageWriter<PutKart>,
    mut velocity: Single<&mut LinearVelocity, With<Kart>>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        puts.write(PutKart(state.respawn(&track)));
        *race = Race::default();
        return;
    }
    let moved = drive(
        Motion {
            velocity: velocity.0,
            heading: state.heading,
        },
        state.up,
        KartInput::from_keys(&keys),
        state.coins,
        strayed(&state),
        time.delta_secs().min(MAX_STEP),
    );
    velocity.0 = moved.velocity;
    state.heading = moved.heading;
}

/// Puts the kart where the last [`PutKart`] says, standing still. The way the race measured the
/// kart driving starts from there, so the jump does not count as driving through a gate.
pub fn place_kart(
    mut puts: MessageReader<PutKart>,
    mut race: ResMut<Race>,
    mut state: ResMut<KartState>,
    mut kart: Query<(&mut Transform, &mut LinearVelocity), With<Kart>>,
) {
    let Some(PutKart(put)) = puts.read().last().copied() else {
        return;
    };
    *state = put;
    race.last = None;
    for (mut transform, mut velocity) in &mut kart {
        transform.translation = put.position;
        transform.rotation = put.rotation();
        velocity.0 = Vec3::ZERO;
    }
}

/// Puts the kart where `state` says on the next frame, as the game does when it puts it back.
#[cfg(test)]
pub fn put(app: &mut App, state: KartState) {
    app.world_mut().write_message(PutKart(state));
}

/// Where the camera sits relative to a kart that faces `heading`.
pub fn camera_offset(heading: f32) -> Vec3 {
    Vec3::new(heading.sin() * -20.0, 12.0, heading.cos() * -20.0)
}

/// Lets the camera trail behind the kart and look ahead of it, smoothly.
pub fn follow_camera(
    state: Res<KartState>,
    time: Res<Time>,
    mut camera: Query<&mut Transform, With<KartCamera>>,
    mut looking_at: Local<Option<Vec3>>,
) {
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let desired = state.position + camera_offset(state.heading);
    let follow = (1.0 - 2.0_f32.powf(-4.0 * dt)).min(1.0);
    transform.translation = transform.translation.lerp(desired, follow);

    let target = state.position + Vec3::Y * 0.5 + state.forward() * 16.0;
    let look = (1.0 - 2.0_f32.powf(-6.0 * dt)).min(1.0);
    let smoothed = looking_at.unwrap_or(target).lerp(target, look);
    *looking_at = Some(smoothed);
    transform.look_at(smoothed, Vec3::Y);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use avian3d::prelude::{Gravity, PhysicsPlugins};
    use bevy::asset::AssetPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy::transform::TransformPlugin;

    use super::*;
    use crate::track::{ControlPoint, TrackDesign};

    const DT: f32 = 1.0 / 64.0;
    const GAS: KartInput = KartInput {
        throttle: 1.0,
        steer: 0.0,
    };
    const COAST: KartInput = KartInput {
        throttle: 0.0,
        steer: 0.0,
    };

    /// A kart that faces along `+Z`, level, at `speed`.
    fn at(speed: f32) -> Motion {
        Motion {
            velocity: Vec3::Z * speed,
            heading: 0.0,
        }
    }

    fn speed(motion: Motion) -> f32 {
        motion.velocity.dot(Vec3::Z)
    }

    fn run(mut motion: Motion, input: KartInput, coins: u32, strayed: f32, steps: usize) -> Motion {
        for _ in 0..steps {
            motion = drive(motion, Vec3::Y, input, coins, strayed, DT);
        }
        motion
    }

    fn input(pressed: &[KeyCode]) -> KartInput {
        let mut keys = ButtonInput::<KeyCode>::default();
        for key in pressed {
            keys.press(*key);
        }
        KartInput::from_keys(&keys)
    }

    #[test]
    fn keys_map_to_input() {
        assert_eq!(input(&[]), COAST);
        assert_eq!(input(&[KeyCode::KeyW]), GAS);
        assert_eq!(input(&[KeyCode::ArrowUp]), GAS);
        assert_eq!(input(&[KeyCode::KeyW, KeyCode::KeyS]).throttle, -0.6);
        assert_eq!(input(&[KeyCode::ArrowDown]).throttle, -0.6);
        assert_eq!(input(&[KeyCode::KeyA]).steer, 1.0);
        assert_eq!(input(&[KeyCode::ArrowLeft]).steer, 1.0);
        assert_eq!(input(&[KeyCode::KeyD]).steer, -1.0);
        assert_eq!(input(&[KeyCode::KeyA, KeyCode::ArrowRight]).steer, -1.0);
    }

    #[test]
    fn the_kart_starts_on_the_road_facing_along_it() {
        let track = TrackDesign::upstream().bake();
        let state = KartState::at_start(&track);
        let frame = track.sample(0.0);
        assert_eq!(state.position, frame.position);
        assert!(state.forward().dot(frame.tangent.with_y(0.0).normalize()) > 0.999);
        assert_eq!(state.speed, 0.0);
        assert!(!state.off_road());
    }

    #[test]
    fn accelerating_raises_the_speed_up_to_the_maximum() {
        let mut motion = at(0.0);
        let mut previous = 0.0;
        for _ in 0..120 {
            motion = run(motion, GAS, 0, 0.0, 1);
            assert!(speed(motion) >= previous);
            assert!(speed(motion) <= MAX_SPEED + 1e-3);
            previous = speed(motion);
        }
        assert!(
            (speed(motion) - MAX_SPEED).abs() < 1e-2,
            "{}",
            speed(motion)
        );
    }

    #[test]
    fn braking_reduces_the_speed_down_to_reverse() {
        let brake = KartInput {
            throttle: -0.6,
            steer: 0.0,
        };
        let mut motion = at(40.0);
        for _ in 0..30 {
            let before = speed(motion);
            motion = run(motion, brake, 0, 0.0, 1);
            assert!(speed(motion) < before);
        }
        let motion = run(motion, brake, 0, 0.0, 300);
        assert!((speed(motion) - -MAX_REVERSE_SPEED).abs() < 1.0);
    }

    #[test]
    fn each_of_the_first_five_coins_raises_the_top_speed_a_little() {
        let mut previous = top_speed(0);
        assert_eq!(previous, MAX_SPEED);
        for coins in 1..=SPEED_COINS {
            let faster = top_speed(coins);
            assert!(faster > previous);
            assert!((faster - previous - MAX_SPEED * COIN_SPEED_GAIN).abs() < 1e-4);
            previous = faster;
        }
        assert!(previous < MAX_SPEED * 1.15, "the gain is small: {previous}");
        assert_eq!(
            top_speed(SPEED_COINS + 20),
            previous,
            "the sixth adds nothing"
        );
    }

    #[test]
    fn the_kart_reaches_the_top_speed_its_coins_allow_and_no_more() {
        for coins in [0, 3, 5, 9] {
            let motion = run(at(0.0), GAS, coins, 0.0, 120);
            assert!(speed(motion) > 0.0);
            assert!(speed(motion) <= top_speed(coins) + 1e-3);
            let over = run(at(top_speed(coins) * 2.0), GAS, coins, 0.0, 1);
            assert!((speed(over) - top_speed(coins)).abs() < 1e-3, "{coins}");
        }
    }

    #[test]
    fn coasting_slows_the_kart_down() {
        let coasted = run(at(40.0), COAST, 0, 0.0, 1);
        assert!(speed(coasted) < 40.0 && speed(coasted) > 39.0);
    }

    #[test]
    fn the_tyres_take_the_speed_a_kart_has_sideways() {
        let sliding = Motion {
            velocity: Vec3::new(10.0, 0.0, 20.0),
            heading: 0.0,
        };
        let held = run(sliding, COAST, 0, 0.0, 1);
        assert!(held.velocity.x.abs() < 1e-4, "{}", held.velocity.x);
        assert!(speed(held) > 19.0, "it keeps going ahead: {}", speed(held));
    }

    #[test]
    fn steering_turns_the_kart_less_at_speed_and_leaves_the_fall_alone() {
        let left = KartInput {
            throttle: 0.0,
            steer: 1.0,
        };
        let right = KartInput {
            throttle: 0.0,
            steer: -1.0,
        };
        let turned = |motion, input| drive(motion, Vec3::Y, input, 0, 0.0, DT).heading;
        assert!((turned(at(0.0), left) - 2.5 * DT).abs() < 1e-5);
        assert!((turned(at(0.0), right) + 2.5 * DT).abs() < 1e-5);
        let fast = turned(at(40.0), left);
        assert!(fast > 0.0 && fast < turned(at(0.0), left));

        let falling = Motion {
            velocity: Vec3::new(0.0, -12.0, 0.0),
            heading: 0.0,
        };
        assert_eq!(drive(falling, Vec3::Y, left, 0, 0.0, DT).velocity.y, -12.0);
    }

    #[test]
    fn a_kart_on_a_slope_drives_along_the_slope() {
        let up = Vec3::new(0.0, 1.0, -0.3).normalize();
        let along = Motion {
            velocity: level_forward(0.0, up) * 10.0,
            heading: 0.0,
        };
        let moved = drive(along, up, GAS, 0, 0.0, DT);
        assert!(
            moved.velocity.dot(up).abs() < 1e-4,
            "along the surface, not into it"
        );
        assert!(moved.velocity.y > 0.0, "it climbs");
    }

    #[test]
    fn straying_far_from_the_road_slows_the_kart() {
        let track = TrackDesign::upstream().bake();
        let on_road = KartState::at_start(&track);
        let frame = track.sample_at_distance(on_road.distance);
        let straying = KartState {
            lateral: frame.width / 2.0 + OFF_ROAD_MARGIN + OFF_ROAD_RAMP * 2.0,
            ..on_road
        };
        assert_eq!(strayed(&on_road), 0.0);
        assert_eq!(strayed(&straying), 1.0);
        assert!(!on_road.off_road() && straying.off_road());

        let kept = run(at(40.0), COAST, 0, strayed(&on_road), 1);
        let slowed = run(at(40.0), COAST, 0, strayed(&straying), 1);
        assert!(speed(slowed) < speed(kept));
        let kept = run(at(40.0), GAS, 0, strayed(&on_road), 20);
        let slowed = run(at(40.0), GAS, 0, strayed(&straying), 20);
        assert!(speed(slowed) < speed(kept));
    }

    /// An app with avian and the real driving systems, a road of `design` as a collider and a kart
    /// on it `lateral` metres from the centerline at `distance` metres along it, at `speed`.
    fn app_on(design: &TrackDesign, distance: f32, lateral: f32, speed: f32) -> App {
        let track = design.bake();
        let frame = track.sample_at_distance(distance);
        let state = KartState {
            position: frame.surface_point(lateral),
            up: frame.up,
            ..KartState::at_distance(&track, distance)
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .init_asset::<Mesh>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Race>()
        .add_message::<PutKart>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            DT,
        )))
        .insert_resource(Gravity(Vec3::NEG_Y * GRAVITY))
        .insert_resource(track)
        .insert_resource(state)
        .add_systems(Update, (read_kart, drive_kart, place_kart).chain());
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(design.road_shape().build());
        let collider = {
            let meshes = app.world().resource::<Assets<Mesh>>();
            Collider::trimesh_from_mesh(meshes.get(&mesh).unwrap()).expect("a road has triangles")
        };
        app.world_mut().spawn((RigidBody::Static, collider));
        app.world_mut().spawn((
            Kart,
            Transform::from_translation(state.position).with_rotation(state.rotation()),
            LinearVelocity(state.rotation() * Vec3::Z * speed),
            body(),
        ));
        app.finish();
        app.cleanup();
        app
    }

    #[test]
    fn the_kart_sits_on_a_banked_road_and_leans_with_it() {
        let mut design = TrackDesign::upstream();
        for ControlPoint { position, roll, .. } in &mut design.control_points {
            position.y = 10.0;
            *roll = 0.3;
        }
        let track = design.bake();
        let distance = track.length() * 0.3;
        let frame = track.sample_at_distance(distance);
        let mut app = app_on(&design, distance, 4.0, 0.0);
        for _ in 0..128 {
            app.update();
        }
        let state = *app.world().resource::<KartState>();
        assert!(
            (state.lateral - 4.0).abs() < 0.5,
            "it stays where it was put: {}",
            state.lateral
        );
        assert!(
            state.up.distance(frame.up) < 0.1,
            "it leans with the road: {} against {}",
            state.up,
            frame.up
        );
        assert!(
            state.position.y < frame.position.y,
            "it is on the low side of the bank"
        );
        assert!(!state.off_road());
        assert!(!state.falling());
    }

    #[test]
    fn the_kart_follows_the_height_of_the_road() {
        let design = TrackDesign::upstream().with_point_raised(3, 4.0);
        let track = design.bake();
        let start = track.length() * 0.45;
        let mut app = app_on(&design, start, 0.0, 20.0);
        for step in 0..128 {
            app.update();
            let state = *app.world().resource::<KartState>();
            let frame = track.sample_at_distance(state.distance);
            assert!(
                (state.position.y - frame.position.y).abs() < 0.6,
                "{step}: {} against {}",
                state.position.y,
                frame.position.y
            );
            assert!(!state.falling(), "{step}");
        }
        let state = *app.world().resource::<KartState>();
        assert!(state.speed > 1.0, "it kept rolling: {}", state.speed);
    }

    #[test]
    fn the_abyss_is_below_the_ground_and_below_a_road_that_dips_under_it() {
        use crate::ground::{BASE, CHUNKS};

        let ground = Ground::new(Vec2::ZERO, 1200.0, CHUNKS, BASE);
        let track = TrackDesign::upstream().bake();
        assert_eq!(abyss(&ground, &track), ground.abyss());
        let dip = -(ground.relief() * 4.0);
        let deep = TrackDesign::upstream().with_point_raised(3, dip).bake();
        assert!(abyss(&ground, &deep) < ground.abyss());
        let lowest = deep
            .frames()
            .iter()
            .map(|frame| frame.position.y)
            .fold(f32::MAX, f32::min);
        assert!(abyss(&ground, &deep) < lowest);
    }

    #[test]
    fn respawning_keeps_the_coins_and_stops_the_kart() {
        let track = TrackDesign::upstream().bake();
        let state = KartState {
            speed: 50.0,
            coins: 5,
            position: Vec3::new(1.0, 2.0, 3.0),
            ..KartState::default()
        };
        let respawned = state.respawn(&track);
        assert_eq!(respawned.coins, 5);
        assert_eq!(respawned.speed, 0.0);
        assert_eq!(respawned.position, KartState::at_start(&track).position);
    }

    #[test]
    fn the_camera_trails_behind_the_kart() {
        let offset = camera_offset(0.0);
        assert_eq!(offset, Vec3::new(0.0, 12.0, -20.0));
        let turned = camera_offset(std::f32::consts::FRAC_PI_2);
        assert!(turned.distance(Vec3::new(-20.0, 12.0, 0.0)) < 1e-4);
    }
}
