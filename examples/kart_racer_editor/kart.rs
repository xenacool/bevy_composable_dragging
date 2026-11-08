//! The kart: its state, the pure physics step, the input mapping and the systems that drive it.
//!
//! Ported from the `bevy_gradmesh` kart racer example. The kart follows the height and the
//! banking of the road while it is on it: it sits on the road surface under it, its up vector
//! leans with the road, and it is off the road when it is further from the centerline than half
//! the local road width. Off the road it drives on the [`Ground`], and where there is neither, over
//! a hole, past the edge of the map or off a bridge, it falls under gravity.

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
/// The longest time step the systems simulate at once.
pub const MAX_STEP: f32 = 0.033;
/// How far along the road, either way, the kart looks for the part of the road it is on.
const SEARCH_WINDOW: f32 = 15.0;
/// How far beyond the road edge the kart can stray before it starts to slow down.
const OFF_ROAD_MARGIN: f32 = 10.0;
/// How much further the kart then has to stray to be slowed down at full strength.
const OFF_ROAD_RAMP: f32 = 8.0;
/// The frame rate the per-frame constants of the physics are tuned for.
const REFERENCE_RATE: f32 = 60.0;
/// How much faster, each second, a kart that nothing holds up falls: well above the pull of
/// the earth, so a kart that drives off an edge drops like a game's.
pub const GRAVITY: f32 = 30.0;

/// Marks the kart model; the transform of the entity follows [`KartState`].
#[derive(Component)]
pub struct Kart;

/// Marks the camera that follows the kart.
#[derive(Component)]
pub struct KartCamera;

/// Where the kart is and how it moves.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct KartState {
    /// The kart on the road surface.
    pub position: Vec3,
    /// The direction the kart faces in plan, as an angle about `Y` from `+Z`.
    pub heading: f32,
    /// The speed along the heading; negative when reversing.
    pub speed: f32,
    /// How many coins have been collected; the first [`SPEED_COINS`] raise the top speed.
    pub coins: u32,
    /// The arc-length distance along the track of the road under the kart.
    pub distance: f32,
    /// How fast the kart rises; negative while it falls.
    pub climb: f32,
    /// The normal of the surface the kart is on.
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

    /// Whether nothing holds the kart up and it is falling.
    pub fn falling(&self) -> bool {
        self.climb < 0.0
    }
}

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

/// Advances the kart by `dt` seconds on `track` and `ground`.
///
/// The kart accelerates, steers less the faster it goes and drives along its heading. It is then
/// held by the highest thing under it that it is over: the road, while it is between the road
/// edges at about the height of the road, and the ground, unless there is a hole or the map ends
/// there. When nothing holds it, it falls, and it lands on whatever it falls onto. Beyond the
/// road edge and a margin it slows down.
pub fn step(
    state: KartState,
    input: KartInput,
    dt: f32,
    track: &BakedTrack,
    ground: &Ground,
) -> KartState {
    let frames = dt * REFERENCE_RATE;
    let mut next = state;

    let speed_factor = (state.speed.abs() / 20.0).min(1.0);
    next.heading += input.steer * 2.5 * dt * (0.3 + 0.7 * (1.0 - speed_factor));

    if input.throttle > 0.0 {
        next.speed += input.throttle * 50.0 * dt;
    } else if input.throttle < 0.0 {
        next.speed += input.throttle * 60.0 * dt;
    }
    let drag: f32 = if input.throttle > 0.0 { 0.998 } else { 0.99 };
    next.speed = (next.speed * drag.powf(frames)).clamp(-MAX_REVERSE_SPEED, top_speed(state.coins));

    next.position += next.forward() * next.speed * dt;
    let road = road_under(next.position, state.distance, track);
    next.distance = road.distance;
    next.lateral = road.lateral;
    next.half_width = road.half_width;
    next.carry(&road, ground, dt);

    let reach = next.half_width + OFF_ROAD_MARGIN;
    let strayed = ((next.lateral.abs() - reach).max(0.0) / OFF_ROAD_RAMP).min(1.0);
    next.speed *= (1.0 - strayed * 0.05).powf(frames);
    next
}

/// The road at the place a kart is over, which may be a bridge above it or the road beside it.
struct RoadUnder {
    /// How far along the track that place is.
    distance: f32,
    /// The way the road faces there.
    up: Vec3,
    /// How far the place is to the right of the centerline.
    lateral: f32,
    /// Half the width of the road there.
    half_width: f32,
    /// The height of the surface plane of the road at the place.
    height: f32,
}

/// A surface that can hold the kart: its height and the way it faces.
struct Support {
    height: f32,
    up: Vec3,
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
        up: frame.up,
        lateral: (offset + Vec3::Y * rise).dot(frame.right),
        half_width: frame.width / 2.0,
        height: frame.position.y + rise,
    }
}

impl KartState {
    /// Holds the kart up on the road or the ground under it, or lets it fall.
    ///
    /// A surface holds the kart when the kart is not higher above it than its own height plus how
    /// far the surface can fall away in a step, so it follows slopes down instead of leaving the
    /// ground. The road also needs the kart to be between its edges and not far under it, so the
    /// ground under a bridge is not the bridge.
    fn carry(&mut self, road: &RoadUnder, ground: &Ground, dt: f32) {
        let height = self.position.y;
        let slack = KART_WIDTH + self.speed.abs() * dt * MAX_GRADE;
        let xz = self.position.xz();
        let supports: Vec<Support> = [
            (road.lateral.abs() <= road.half_width && height >= road.height - slack).then_some(
                Support {
                    height: road.height,
                    up: road.up,
                },
            ),
            ground.height_at(xz).map(|height| Support {
                height,
                up: ground.normal_at(xz),
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        let highest = |held: &dyn Fn(&Support) -> bool| {
            supports
                .iter()
                .filter(|support| held(support))
                .max_by(|a, b| a.height.total_cmp(&b.height))
        };
        if let Some(support) = highest(&|support| height <= support.height + slack) {
            self.position.y = support.height;
            self.climb = 0.0;
            self.up = support.up;
            return;
        }
        self.climb -= GRAVITY * dt;
        self.position.y += self.climb * dt;
        let below = self.position.y;
        if let Some(support) = highest(&|support| below <= support.height) {
            self.position.y = support.height;
            self.climb = 0.0;
            self.up = support.up;
        }
    }
}

/// Where the camera sits relative to a kart that faces `heading`.
pub fn camera_offset(heading: f32) -> Vec3 {
    Vec3::new(heading.sin() * -20.0, 12.0, heading.cos() * -20.0)
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

/// Reads the keyboard, restarts the race on `R` and advances the kart.
pub fn drive_kart(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    track: Res<BakedTrack>,
    ground: Res<Ground>,
    mut state: ResMut<KartState>,
    mut race: ResMut<Race>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        *state = state.respawn(&track);
        *race = Race::default();
    }
    let input = KartInput::from_keys(&keys);
    *state = step(
        *state,
        input,
        time.delta_secs().min(MAX_STEP),
        &track,
        &ground,
    );
}

/// Moves the kart model to the kart state.
pub fn update_kart_transform(state: Res<KartState>, mut kart: Query<&mut Transform, With<Kart>>) {
    if let Ok(mut transform) = kart.single_mut() {
        transform.translation = state.position;
        transform.rotation = state.rotation();
    }
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
    use std::sync::LazyLock;

    use super::*;
    use crate::ground::{BASE, Brush, CHUNKS};
    use crate::track::{ControlPoint, TrackDesign};

    const DT: f32 = 1.0 / 60.0;

    /// Flat ground under the upstream track.
    static GROUND: LazyLock<Ground> =
        LazyLock::new(|| Ground::new(Vec2::ZERO, 1200.0, CHUNKS, BASE));

    /// A step on the flat ground under the upstream track.
    fn step(state: KartState, input: KartInput, dt: f32, track: &BakedTrack) -> KartState {
        super::step(state, input, dt, track, &GROUND)
    }
    const GAS: KartInput = KartInput {
        throttle: 1.0,
        steer: 0.0,
    };
    const COAST: KartInput = KartInput {
        throttle: 0.0,
        steer: 0.0,
    };

    /// The distance along the default track of the straight that starts at its second point.
    fn straight_start(track: &BakedTrack) -> f32 {
        track.nearest_t(Vec3::new(80.0, 0.8, 100.0)) * track.length()
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
        let track = TrackDesign::upstream().bake();
        let mut state = KartState::at_distance(&track, straight_start(&track));
        let mut previous = state.speed;
        for _ in 0..110 {
            state = step(state, GAS, DT, &track);
            assert!(state.speed >= previous);
            assert!(state.speed <= MAX_SPEED);
            previous = state.speed;
        }
        assert_eq!(state.speed, MAX_SPEED);
        assert!(!state.off_road());
        assert!(
            state
                .position
                .distance(KartState::at_start(&track).position)
                > 50.0
        );
    }

    #[test]
    fn braking_reduces_the_speed_down_to_reverse() {
        let track = TrackDesign::upstream().bake();
        let mut state = KartState::at_distance(&track, straight_start(&track));
        state.speed = 40.0;
        let brake = KartInput {
            throttle: -0.6,
            steer: 0.0,
        };
        for _ in 0..30 {
            let before = state.speed;
            state = step(state, brake, DT, &track);
            assert!(state.speed < before);
        }
        for _ in 0..300 {
            state = step(state, brake, DT, &track);
        }
        assert!((state.speed - -MAX_REVERSE_SPEED).abs() < 1.0);
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
        let track = TrackDesign::upstream().bake();
        for coins in [0, 3, 5, 9] {
            let mut state = KartState {
                coins,
                ..KartState::at_distance(&track, straight_start(&track))
            };
            for _ in 0..100 {
                state = step(state, GAS, DT, &track);
            }
            assert!(state.speed > 0.0);
            assert!(state.speed <= top_speed(coins) + 1e-3);
            state.speed = top_speed(coins) * 2.0;
            state = step(state, GAS, DT, &track);
            assert!((state.speed - top_speed(coins)).abs() < 1e-3, "{coins}");
        }
    }

    #[test]
    fn coasting_slows_the_kart_down() {
        let track = TrackDesign::upstream().bake();
        let mut state = KartState::at_distance(&track, straight_start(&track));
        state.speed = 40.0;
        let coasted = step(state, COAST, DT, &track);
        assert!(coasted.speed < 40.0 && coasted.speed > 39.0);
    }

    #[test]
    fn steering_changes_the_heading_less_at_speed() {
        let track = TrackDesign::upstream().bake();
        let state = KartState::at_distance(&track, straight_start(&track));
        let left = KartInput {
            throttle: 0.0,
            steer: 1.0,
        };
        let right = KartInput {
            throttle: 0.0,
            steer: -1.0,
        };
        let turned_left = step(state, left, DT, &track);
        let turned_right = step(state, right, DT, &track);
        assert!((turned_left.heading - state.heading - 2.5 * DT).abs() < 1e-5);
        assert!((turned_right.heading - state.heading + 2.5 * DT).abs() < 1e-5);

        let fast = KartState {
            speed: 40.0,
            ..state
        };
        let fast_turn = step(fast, left, DT, &track).heading - state.heading;
        assert!(fast_turn > 0.0 && fast_turn < turned_left.heading - state.heading);
    }

    #[test]
    fn straying_far_from_the_road_slows_the_kart() {
        let track = TrackDesign::upstream().bake();
        let on_road = KartState {
            speed: 40.0,
            ..KartState::at_distance(&track, straight_start(&track))
        };
        let frame = track.sample_at_distance(on_road.distance);
        let straying = KartState {
            position: frame.surface_point(frame.width / 2.0 + 14.0),
            ..on_road
        };

        let kept = step(on_road, COAST, DT, &track);
        let slowed = step(straying, COAST, DT, &track);
        assert!(!kept.off_road());
        assert!(slowed.off_road());
        assert!(slowed.speed < kept.speed);

        let (mut on_road, mut straying) = (on_road, straying);
        for _ in 0..20 {
            on_road = step(on_road, GAS, DT, &track);
            straying = step(straying, GAS, DT, &track);
        }
        assert!(straying.speed < on_road.speed);
    }

    #[test]
    fn the_kart_sits_on_a_banked_road_and_leans_with_it() {
        let mut design = TrackDesign::upstream();
        for ControlPoint { position, roll, .. } in &mut design.control_points {
            position.y = 10.0;
            *roll = 0.3;
        }
        let track = design.bake();
        let frame = track.sample(0.3);
        let state = KartState {
            position: frame.surface_point(4.0),
            distance: track.length() * 0.3,
            ..KartState::default()
        };

        let placed = step(state, COAST, 0.0, &track);
        assert!((placed.lateral - 4.0).abs() < 0.1);
        assert!(placed.position.distance(frame.surface_point(4.0)) < 0.1);
        assert!(placed.position.y < frame.position.y);
        assert!(placed.up.distance(frame.up) < 0.02);
        assert!((placed.rotation() * Vec3::Y).distance(frame.up) < 0.02);
        assert!(!placed.off_road());
    }

    #[test]
    fn the_kart_follows_the_height_of_the_road() {
        let design = TrackDesign::upstream().with_point_raised(3, 4.0);
        let track = design.bake();
        for index in 0..40 {
            let distance = track.length() * index as f32 / 40.0;
            let state = KartState {
                speed: 20.0,
                ..KartState::at_distance(&track, distance)
            };
            let moved = step(state, COAST, DT, &track);
            let frame = track.sample_at_distance(moved.distance);
            assert!(
                (moved.position.y - frame.position.y).abs() < 0.3,
                "{index}: {} against {}",
                moved.position.y,
                frame.position.y
            );
            assert!(!moved.falling(), "{index}");
        }
    }

    /// A kart on the road at the start of the straight, driving across it at `speed`.
    fn crossing(track: &BakedTrack, speed: f32) -> KartState {
        let state = KartState::at_distance(track, straight_start(track));
        KartState {
            speed,
            heading: state.heading - std::f32::consts::FRAC_PI_2,
            ..state
        }
    }

    #[test]
    fn a_kart_that_leaves_the_road_drives_on_the_ground_beside_it() {
        let track = TrackDesign::upstream().bake();
        let mut state = crossing(&track, 30.0);
        for _ in 0..120 {
            state = step(state, COAST, DT, &track);
        }
        assert!(state.off_road());
        assert!(!state.falling());
        let ground = GROUND.height_at(state.position.xz()).unwrap();
        assert!(
            (state.position.y - ground).abs() < 1e-4,
            "{}",
            state.position.y
        );
        assert_eq!(state.climb, 0.0);
    }

    #[test]
    fn a_kart_that_drives_off_a_raised_road_falls_and_lands_on_the_ground() {
        let design = TrackDesign::upstream().with_point_raised(1, 12.0);
        let track = design.bake();
        let start = track.nearest_t(design.control_points[1].position) * track.length();
        let mut state = crossing(&track, 30.0);
        state = KartState {
            speed: 30.0,
            ..KartState {
                heading: state.heading,
                ..KartState::at_distance(&track, start)
            }
        };
        let mut fell = false;
        let mut lowest_climb = 0.0_f32;
        for _ in 0..240 {
            state = step(state, COAST, DT, &track);
            fell |= state.falling();
            lowest_climb = lowest_climb.min(state.climb);
        }
        assert!(
            fell,
            "the kart should drop off the edge of a road 12 metres up"
        );
        assert!(
            lowest_climb < -5.0,
            "it gathers speed as it falls: {lowest_climb}"
        );
        assert!(state.position.y < 0.0, "{}", state.position.y);
        assert!(!state.falling(), "it has landed");
    }

    #[test]
    fn a_kart_follows_a_hill_beside_the_road() {
        let track = TrackDesign::upstream().bake();
        let state = crossing(&track, 30.0);
        let aside =
            state.position + Vec3::new(state.heading.sin(), 0.0, state.heading.cos()) * 40.0;
        let hill = GROUND.raised(&Brush {
            center: aside.xz(),
            radius: 30.0,
            amount: 3.0,
            falloff: 1.0,
        });
        let mut state = state;
        let mut top = f32::MIN;
        for _ in 0..150 {
            state = super::step(state, COAST, DT, &track, &hill);
            top = top.max(state.position.y);
            if let Some(height) = hill.height_at(state.position.xz()) {
                if state.off_road() && !state.falling() {
                    assert!((state.position.y - height).abs() < 0.5);
                }
            }
        }
        assert!(top > BASE + 1.0, "the kart climbed the hill: {top}");
    }

    #[test]
    fn a_kart_over_a_hole_falls_through_the_ground() {
        let track = TrackDesign::upstream().bake();
        let state = crossing(&track, 0.0);
        let ground = GROUND.with_holes(state.position.xz(), 12.0, true);
        let mut state = KartState {
            position: state.position.with_y(BASE),
            ..state
        };
        let mut previous = state.position.y;
        for _ in 0..30 {
            state = super::step(state, COAST, DT, &track, &ground);
            assert!(state.position.y < previous, "{}", state.position.y);
            previous = state.position.y;
        }
        assert!(state.falling());
        assert!(state.position.y < BASE - 3.0, "{}", state.position.y);
        assert!(state.climb < -10.0, "{}", state.climb);
        let abyss = abyss(&ground, &track);
        for _ in 0..600 {
            state = super::step(state, COAST, DT, &track, &ground);
        }
        assert!(state.position.y < abyss, "nothing catches it");
    }

    #[test]
    fn a_kart_beyond_the_edge_of_the_map_falls() {
        let track = TrackDesign::upstream().bake();
        let edge = GROUND.middle() + Vec2::new(GROUND.half() + 1.0, 0.0);
        let mut state = KartState {
            position: Vec3::new(edge.x, BASE, edge.y),
            ..KartState::default()
        };
        for _ in 0..20 {
            state = step(state, COAST, DT, &track);
        }
        assert!(state.falling());
        assert!(state.position.y < BASE);
    }

    #[test]
    fn the_ground_under_a_bridge_is_not_mistaken_for_the_road_above_it() {
        let design = TrackDesign::upstream().with_point_raised(3, 8.0);
        let track = design.bake();
        let top = track
            .frames()
            .iter()
            .max_by(|a, b| a.position.y.total_cmp(&b.position.y))
            .copied()
            .unwrap();
        let state = KartState {
            position: top.position.with_y(BASE),
            distance: track.nearest_t(top.position) * track.length(),
            ..KartState::default()
        };
        let moved = step(state, COAST, DT, &track);
        assert!(
            (moved.position.y - BASE).abs() < 1e-3,
            "{}",
            moved.position.y
        );
        assert!(!moved.falling());
    }

    #[test]
    fn the_abyss_is_below_the_ground_and_below_a_road_that_dips_under_it() {
        let track = TrackDesign::upstream().bake();
        assert_eq!(abyss(&GROUND, &track), GROUND.abyss());
        let dip = -(GROUND.relief() * 4.0);
        let deep = TrackDesign::upstream().with_point_raised(3, dip).bake();
        assert!(abyss(&GROUND, &deep) < GROUND.abyss());
        let lowest = deep
            .frames()
            .iter()
            .map(|frame| frame.position.y)
            .fold(f32::MAX, f32::min);
        assert!(abyss(&GROUND, &deep) < lowest);
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
