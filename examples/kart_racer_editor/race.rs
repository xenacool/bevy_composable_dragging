//! The race: checkpoints, laps and coins.
//!
//! Ported from the `bevy_gradmesh` kart racer example. The checkpoints are gates that span the
//! road, and the kart passes one by driving through it, wherever on the road that is. The first
//! gate is the start line: crossing it starts the clock, and crossing it again after every other
//! gate completes a lap. The logic is pure, so it can be tested and reused without the ECS.

use bevy::prelude::*;

use crate::ground::Ground;
use crate::kart::{KART_WIDTH, KartState, abyss};
use crate::track::BakedTrack;

/// How many laps make a race.
pub const LAPS: u32 = 3;
/// How close the kart has to come to a coin to collect it, along the way it drove.
pub const COIN_RADIUS: f32 = 2.0;
/// How long, in seconds, a kart that drove past a checkpoint without going through it can still
/// go back for it before the countdown begins.
pub const MISSED_GRACE: f32 = 5.0;
/// How long, in seconds, the countdown runs; a kart that has not gone through the checkpoint
/// by then is put back before it.
pub const MISSED_COUNTDOWN: f32 = 5.0;

/// A coin hovering over the road.
#[derive(Component)]
pub struct Coin {
    /// The index of the coin along the road.
    pub id: u32,
    /// Whether the kart has collected it.
    pub collected: bool,
    /// Where and how it rests; bobbing and spinning are applied relative to this.
    pub rest: Transform,
}

/// A gate across the road that the kart has to drive through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Checkpoint {
    /// The middle of the gate, on the centerline of the road.
    pub position: Vec3,
    /// How far along the road the gate is, in metres from the start line.
    pub distance: f32,
    /// The direction of travel through the gate.
    pub forward: Vec3,
    /// How far from the middle the gate reaches: the road to its edge, and the kart beyond it.
    pub reach: f32,
}

impl Checkpoint {
    /// A gate at `position` for a road `width` wide that is driven along `forward`.
    pub fn across(position: Vec3, forward: Vec3, width: f32) -> Self {
        Self {
            position,
            distance: 0.0,
            forward: forward.normalize_or(Vec3::Z),
            reach: width / 2.0 + KART_WIDTH,
        }
    }

    /// This gate at `distance` metres along the road.
    #[must_use]
    pub fn at_distance(self, distance: f32) -> Self {
        Self { distance, ..self }
    }

    /// Whether a kart that drove from `before` to `after` went through the gate the right way.
    pub fn is_crossed(&self, before: Vec3, after: Vec3) -> bool {
        let behind = (before - self.position).dot(self.forward);
        let ahead = (after - self.position).dot(self.forward);
        if behind > 0.0 || ahead <= 0.0 {
            return false;
        }
        let at = before.lerp(after, behind / (behind - ahead));
        at.distance(self.position) <= self.reach
    }
}

/// The checkpoints of the track, in the order they have to be passed; the first is the start
/// line.
#[derive(Resource, Default)]
pub struct Checkpoints(pub Vec<Checkpoint>);

/// What the kart just passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passage {
    /// The checkpoint with this index, which is the start line when it is the first.
    Checkpoint(usize),
    /// The start line again after every other checkpoint, which completes a lap.
    Lap,
}

/// The progress of the race: the next checkpoint, the laps done and the clock.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Race {
    /// How many checkpoints of the lap have been passed, the start line included. When all of
    /// them have been, only the start line is left to cross.
    pub next: usize,
    /// How many laps are complete.
    pub laps: u32,
    /// Whether the clock is running, which it does from the start line on.
    pub started: bool,
    /// The time at which the clock started.
    pub start_time: f32,
    /// The time since the clock started.
    pub elapsed: f32,
    /// Where the kart was when the race last looked, which the way it drove is measured from.
    pub last: Option<Vec3>,
    /// When the kart went past the next checkpoint without going through it, while it has not
    /// gone back.
    pub missed_since: Option<f32>,
    /// The seconds left of the countdown to being put back before the checkpoint that was
    /// missed, while it runs.
    pub countdown: Option<f32>,
}

impl Race {
    /// Whether all [`LAPS`] are complete.
    pub fn finished(&self) -> bool {
        self.laps >= LAPS
    }

    /// The index of the checkpoint the kart passed last, which is the start line after a lap, or
    /// `None` before the kart has gone through any.
    pub fn last_passed(&self) -> Option<usize> {
        self.next.checked_sub(1)
    }

    /// Watches for a missed checkpoint, at the time `now`. `past` is how far along the road,
    /// in metres, the kart is beyond the next checkpoint, negative while it is still before it,
    /// and `None` when that is not known.
    ///
    /// Once the race has begun, a kart that is beyond the next checkpoint has missed it. After
    /// [`MISSED_GRACE`] seconds of that a countdown of [`MISSED_COUNTDOWN`] seconds starts, shown
    /// by `countdown`, and the kart going through the checkpoint ends it. Returns `true`, once,
    /// when the countdown runs out: the kart is to be put back before the checkpoint.
    pub fn watch(&mut self, past: Option<f32>, now: f32) -> bool {
        self.countdown = None;
        let missed = self.started && !self.finished() && past.is_some_and(|past| past > 0.0);
        if !missed {
            self.missed_since = None;
            return false;
        }
        let since = *self.missed_since.get_or_insert(now);
        let remaining = MISSED_GRACE + MISSED_COUNTDOWN - (now - since);
        if remaining <= 0.0 {
            self.missed_since = None;
            return true;
        }
        if remaining <= MISSED_COUNTDOWN {
            self.countdown = Some(remaining);
        }
        false
    }

    /// Moves the race on for a kart that is now at `kart`, at the time `now`.
    ///
    /// Only the next checkpoint can be passed, so the checkpoints have to be driven through in
    /// order and the right way; the start line starts the clock and, once every other
    /// checkpoint has been passed, completes the lap.
    pub fn advance(&mut self, kart: Vec3, checkpoints: &[Checkpoint], now: f32) -> Option<Passage> {
        let before = self.last.replace(kart)?;
        if checkpoints.is_empty() || self.finished() {
            return None;
        }
        if self.started {
            self.elapsed = now - self.start_time;
        }
        let index = self.next % checkpoints.len();
        if !checkpoints[index].is_crossed(before, kart) {
            return None;
        }
        if !self.started {
            self.started = true;
            self.start_time = now;
        }
        self.elapsed = now - self.start_time;
        if self.next == checkpoints.len() {
            self.laps += 1;
            self.next = 1;
            Some(Passage::Lap)
        } else {
            self.next = index + 1;
            Some(Passage::Checkpoint(index))
        }
    }
}

/// Whether a kart that drove from `before` to `after` came close enough to a coin at `coin` to
/// collect it.
pub fn coin_within_reach(before: Vec3, after: Vec3, coin: Vec3) -> bool {
    let way = after - before;
    let length_squared = way.length_squared();
    let along = if length_squared > 0.0 {
        ((coin - before).dot(way) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (before + way * along).distance_squared(coin) < COIN_RADIUS * COIN_RADIUS
}

/// Passes the checkpoints the kart drives through.
pub fn check_checkpoints(
    time: Res<Time>,
    state: Res<KartState>,
    checkpoints: Res<Checkpoints>,
    mut race: ResMut<Race>,
) {
    race.advance(state.position, &checkpoints.0, time.elapsed_secs());
}

/// How far along the road the kart is beyond the checkpoint the race wants next, for a road
/// `length` metres long, or `None` when the kart is on the far side of the loop, where it is
/// before the checkpoint rather than past it.
pub fn past_next_checkpoint(
    race: &Race,
    checkpoints: &[Checkpoint],
    kart: f32,
    length: f32,
) -> Option<f32> {
    let gate = checkpoints.get(race.next % checkpoints.len().max(1))?;
    let beyond = (kart - gate.distance).rem_euclid(length);
    (beyond < length / 2.0).then_some(beyond)
}

/// Puts a kart whose countdown has run out back before the checkpoint it missed, standing.
pub fn put_back_before_missed(
    time: Res<Time>,
    track: Res<BakedTrack>,
    checkpoints: Res<Checkpoints>,
    mut race: ResMut<Race>,
    mut state: ResMut<KartState>,
) {
    let past = past_next_checkpoint(&race, &checkpoints.0, state.distance, track.length());
    if !race.watch(past, time.elapsed_secs()) {
        return;
    }
    let Some(gate) = checkpoints.0.get(race.next % checkpoints.0.len()) else {
        return;
    };
    *state = KartState {
        coins: state.coins,
        ..KartState::at_distance(&track, gate.distance - KART_WIDTH)
    };
    race.last = None;
}

/// The kart put back at the checkpoint it passed last, or at the start line when it has not
/// passed one, standing and keeping its coins.
pub fn after_a_fall(
    kart: &KartState,
    race: &Race,
    checkpoints: &[Checkpoint],
    track: &BakedTrack,
) -> KartState {
    let distance = race
        .last_passed()
        .and_then(|index| checkpoints.get(index))
        .map_or(0.0, |gate| gate.distance);
    KartState {
        coins: kart.coins,
        ..KartState::at_distance(track, distance)
    }
}

/// Puts a kart that fell below the abyss back at the last checkpoint it passed.
pub fn put_back_after_fall(
    track: Res<BakedTrack>,
    ground: Res<Ground>,
    checkpoints: Res<Checkpoints>,
    mut race: ResMut<Race>,
    mut state: ResMut<KartState>,
) {
    if state.position.y >= abyss(&ground, &track) {
        return;
    }
    *state = after_a_fall(&state, &race, &checkpoints.0, &track);
    race.last = None;
}

/// Collects the coins the kart drove through since the race last looked, which raises its top
/// speed. It runs before the checkpoints, which move the point the way is measured from.
pub fn collect_coins(
    mut commands: Commands,
    race: Res<Race>,
    mut state: ResMut<KartState>,
    mut coins: Query<(Entity, &mut Coin, &Transform)>,
) {
    let before = race.last.unwrap_or(state.position);
    let mut collected = 0;
    for (entity, mut coin, transform) in &mut coins {
        if !coin.collected && coin_within_reach(before, state.position, transform.translation) {
            coin.collected = true;
            commands.entity(entity).insert(Visibility::Hidden);
            collected += 1;
        }
    }
    state.coins += collected;
}

/// Lets the coins that are still there bob and spin.
pub fn animate_coins(time: Res<Time>, mut coins: Query<(&Coin, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (coin, mut transform) in &mut coins {
        if coin.collected {
            continue;
        }
        let id = coin.id as f32;
        transform.rotation = coin.rest.rotation * Quat::from_rotation_y(t * 2.0 + id);
        transform.translation =
            coin.rest.translation + Vec3::Y * ((t * 1.5 + id * 0.5).sin() * 0.15);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackDesign;

    const WIDTH: f32 = 16.0;

    /// Gates a hundred metres apart along `X`, the first at the origin.
    fn gates() -> Vec<Checkpoint> {
        (0..4)
            .map(|i| Checkpoint::across(Vec3::X * 100.0 * i as f32, Vec3::X, WIDTH))
            .collect()
    }

    /// Drives a race through `points` one after the other, a second apart, and returns what
    /// the kart passed.
    fn drive(race: &mut Race, gates: &[Checkpoint], points: &[Vec3]) -> Vec<Passage> {
        points
            .iter()
            .enumerate()
            .filter_map(|(second, point)| race.advance(*point, gates, second as f32))
            .collect()
    }

    /// The points of one pass through gate `index`, from just before it to just after it, at
    /// `side` metres from the middle of the road.
    fn through(index: usize, side: f32) -> [Vec3; 2] {
        let at = Vec3::X * 100.0 * index as f32 + Vec3::Z * side;
        [at - Vec3::X * 2.0, at + Vec3::X * 2.0]
    }

    #[test]
    fn a_kart_that_fell_goes_back_to_the_last_checkpoint_it_passed_standing() {
        let track = TrackDesign::upstream().bake();
        let gates: Vec<Checkpoint> = (0..4)
            .map(|index| {
                Checkpoint::across(Vec3::ZERO, Vec3::Z, WIDTH)
                    .at_distance(track.length() * index as f32 / 4.0)
            })
            .collect();
        let fallen = KartState {
            position: Vec3::new(0.0, -500.0, 0.0),
            speed: 40.0,
            climb: -90.0,
            coins: 4,
            ..KartState::default()
        };
        for (next, expected) in [
            (0, 0.0),
            (1, 0.0),
            (3, gates[2].distance),
            (4, gates[3].distance),
        ] {
            let race = Race {
                next,
                ..Race::default()
            };
            let back = after_a_fall(&fallen, &race, &gates, &track);
            assert!((back.distance - expected).abs() < 1e-3, "{next}");
            assert_eq!(back.speed, 0.0);
            assert_eq!(back.climb, 0.0);
            assert_eq!(back.coins, 4);
            let frame = track.sample_at_distance(expected);
            assert!(back.position.distance(frame.position) < 1e-3, "{next}");
        }
    }

    #[test]
    fn the_last_checkpoint_passed_follows_the_race() {
        let mut race = Race::default();
        assert_eq!(race.last_passed(), None);
        let gates = gates();
        let points: Vec<Vec3> = (0..3).flat_map(|index| through(index, 0.0)).collect();
        drive(&mut race, &gates, &points);
        assert_eq!(race.last_passed(), Some(2));
    }

    #[test]
    fn a_gate_is_passed_anywhere_across_the_road_and_only_the_right_way() {
        let gate = gates()[1];
        for side in [-WIDTH / 2.0, -5.0, 0.0, 5.0, WIDTH / 2.0] {
            let [before, after] = through(1, side);
            assert!(gate.is_crossed(before, after), "{side}");
            assert!(!gate.is_crossed(after, before), "backwards at {side}");
        }
        let [before, after] = through(1, WIDTH / 2.0 + KART_WIDTH + 1.0);
        assert!(!gate.is_crossed(before, after), "beside the road");
        let [before, after] = through(1, 0.0);
        assert!(!gate.is_crossed(before, before + (after - before) * 0.4));
    }

    #[test]
    fn a_fast_kart_cannot_skip_a_gate_between_two_steps() {
        let gate = gates()[0];
        assert!(gate.is_crossed(Vec3::new(-60.0, 0.0, 3.0), Vec3::new(60.0, 0.0, 3.0)));
    }

    #[test]
    fn the_start_line_starts_the_clock_and_the_other_gates_follow_in_order() {
        let gates = gates();
        let mut race = Race::default();
        let [before, after] = through(0, 0.0);
        assert_eq!(race.advance(before, &gates, 1.0), None, "first look");
        assert!(!race.started);
        assert_eq!(
            race.advance(after, &gates, 2.0),
            Some(Passage::Checkpoint(0))
        );
        assert!(race.started);
        assert_eq!(race.start_time, 2.0);
        assert_eq!(race.next, 1);

        race.last = None;
        let skipped = through(3, 0.0);
        assert_eq!(drive(&mut race, &gates, &skipped), vec![], "out of order");
        race.last = None;
        let [before, after] = through(1, 2.0);
        assert_eq!(race.advance(before, &gates, 5.0), None);
        assert_eq!(
            race.advance(after, &gates, 6.0),
            Some(Passage::Checkpoint(1))
        );
        assert_eq!(race.elapsed, 4.0);
    }

    #[test]
    fn a_lap_completes_when_the_start_line_is_crossed_again() {
        let gates = gates();
        let mut race = Race::default();
        let mut passed = Vec::new();
        for index in [0, 1, 2, 3] {
            passed.extend(drive(&mut race, &gates, &through(index, 1.0)));
        }
        assert_eq!(
            passed,
            vec![
                Passage::Checkpoint(0),
                Passage::Checkpoint(1),
                Passage::Checkpoint(2),
                Passage::Checkpoint(3)
            ]
        );
        assert_eq!(race.laps, 0, "the last gate is not the end of the lap");
        assert_eq!(
            drive(&mut race, &gates, &through(0, -1.0)),
            vec![Passage::Lap]
        );
        assert_eq!(race.laps, 1);
        assert_eq!(race.next, 1);
    }

    #[test]
    fn a_lap_needs_every_gate() {
        let gates = gates();
        let mut race = Race::default();
        drive(&mut race, &gates, &through(0, 0.0));
        drive(&mut race, &gates, &through(2, 0.0));
        drive(&mut race, &gates, &through(0, 0.0));
        assert_eq!(race.laps, 0);
    }

    #[test]
    fn the_race_ends_after_all_the_laps_and_the_clock_stops() {
        let gates = gates();
        let mut race = Race::default();
        let mut time = 0.0;
        while !race.finished() {
            for index in [0, 1, 2, 3] {
                for point in through(index, 0.0) {
                    race.advance(point, &gates, time);
                    time += 1.0;
                }
            }
            assert!(time < 1000.0, "the laps never end");
        }
        assert_eq!(race.laps, LAPS);
        let elapsed = race.elapsed;
        for point in through(0, 0.0) {
            assert_eq!(race.advance(point, &gates, time + 50.0), None);
        }
        assert_eq!(race.elapsed, elapsed);
        assert_eq!(race.laps, LAPS);
    }

    #[test]
    fn a_race_without_checkpoints_never_advances() {
        let mut race = Race::default();
        assert_eq!(race.advance(Vec3::ZERO, &[], 1.0), None);
        assert_eq!(race.advance(Vec3::X, &[], 2.0), None);
        assert_eq!((race.laps, race.next, race.started), (0, 0, false));
    }

    #[test]
    fn coins_are_collected_along_the_way_the_kart_drove() {
        let coin = Vec3::new(10.0, 1.0, 10.0);
        let near = |side: f32| Vec3::new(10.0 + side, 1.0, 10.0);
        assert!(coin_within_reach(near(-0.2), near(0.2), coin));
        assert!(coin_within_reach(near(0.0), near(0.0), coin));
        assert!(coin_within_reach(
            Vec3::new(0.0, 1.0, 10.0),
            Vec3::new(30.0, 1.0, 10.0),
            coin
        ));
        let beside = Vec3::Z * (COIN_RADIUS + 0.1);
        assert!(!coin_within_reach(
            Vec3::new(0.0, 1.0, 10.0) + beside,
            Vec3::new(30.0, 1.0, 10.0) + beside,
            coin
        ));
        assert!(!coin_within_reach(
            near(0.0) + Vec3::Y * 4.0,
            near(1.0) + Vec3::Y * 4.0,
            coin
        ));
        assert!(!coin_within_reach(
            Vec3::new(0.0, 1.0, 10.0),
            Vec3::new(5.0, 1.0, 10.0),
            coin
        ));
    }

    #[test]
    fn a_kart_beyond_the_next_checkpoint_has_five_seconds_and_then_five_more_counted_down() {
        let mut race = Race {
            started: true,
            next: 1,
            ..Race::default()
        };
        assert!(
            !race.watch(Some(-3.0), 0.0),
            "before the checkpoint is fine"
        );
        assert_eq!(race.countdown, None);

        assert!(!race.watch(Some(2.0), 10.0));
        assert!(!race.watch(Some(80.0), 14.9));
        assert_eq!(race.countdown, None, "the grace is not over");
        for (now, left) in [(15.0, 5.0), (17.5, 2.5), (19.9, 0.1)] {
            assert!(!race.watch(Some(300.0), now));
            let shown = race.countdown.expect("the countdown runs");
            assert!((shown - left).abs() < 1e-3, "{now}: {shown}");
        }
        assert!(race.watch(Some(400.0), 20.0), "it has run out");
        assert_eq!(race.countdown, None);
        assert!(!race.watch(Some(400.0), 20.01), "it is asked for once");
        assert_eq!(race.countdown, None, "a new grace begins");
    }

    #[test]
    fn going_through_the_checkpoint_or_coming_back_ends_the_wait() {
        let mut race = Race {
            started: true,
            next: 1,
            ..Race::default()
        };
        race.watch(Some(5.0), 0.0);
        race.watch(Some(5.0), 6.0);
        assert!(race.countdown.is_some());
        race.watch(Some(-10.0), 7.0);
        assert_eq!((race.countdown, race.missed_since), (None, None));
        race.watch(Some(5.0), 8.0);
        assert_eq!(race.countdown, None, "the grace starts over");
        race.watch(None, 9.0);
        assert_eq!(
            race.missed_since, None,
            "nothing is known, nothing is missed"
        );
    }

    #[test]
    fn nothing_is_missed_before_the_race_begins_or_after_it_ends() {
        let mut waiting = Race::default();
        for now in [0.0, 6.0, 20.0, 40.0] {
            assert!(!waiting.watch(Some(50.0), now));
            assert_eq!(waiting.countdown, None);
        }
        let mut done = Race {
            started: true,
            laps: LAPS,
            ..Race::default()
        };
        for now in [0.0, 6.0, 20.0, 40.0] {
            assert!(!done.watch(Some(50.0), now));
            assert_eq!(done.countdown, None);
        }
    }

    #[test]
    fn how_far_past_the_next_checkpoint_is_measured_along_the_loop() {
        let gates: Vec<Checkpoint> = gates()
            .into_iter()
            .enumerate()
            .map(|(index, gate)| gate.at_distance(100.0 * index as f32))
            .collect();
        let length = 400.0;
        let mut race = Race {
            next: 1,
            ..Race::default()
        };
        let past = |race: &Race, kart: f32| past_next_checkpoint(race, &gates, kart, length);
        assert_eq!(past(&race, 130.0), Some(30.0));
        assert_eq!(past(&race, 100.0), Some(0.0));
        assert_eq!(past(&race, 60.0), None, "before it");
        assert_eq!(
            past(&race, 350.0),
            None,
            "the far side of the loop is before it"
        );
        race.next = gates.len();
        assert_eq!(
            past(&race, 5.0),
            Some(5.0),
            "the start line is the one left"
        );
        assert_eq!(past(&race, 390.0), None);
        assert_eq!(past_next_checkpoint(&race, &[], 5.0, length), None);
    }
}
