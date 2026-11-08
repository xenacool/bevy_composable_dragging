//! The camera of the editor: a map. It looks down at the ground at 45 degrees through an
//! orthographic projection, so the track keeps its proportions wherever it is on the screen and
//! a handle is as big on the screen on every part of it.
//!
//! The right mouse button turns the map and tilts it, from a plan looking straight down to a
//! side view that shows the heights of the road, the middle button slides it over the ground,
//! and the wheel or the up and down keys zoom. Q and E turn the map to the next corner, which
//! is how a map is drawn at 45 degrees, and Home goes back to the start. The left button is for
//! the handles of the track.
//!
//! The view starts framed on the whole track, and the wheel zooms out beyond it, all the way to
//! the whole space of the map. Zooming in
//! comes in on the control point that is selected: the view glides from the middle of the track
//! to the point in step with the zoom, so the point is on the screen at every zoom and in the
//! middle of it at the closest one. With nothing selected, the terrain is what is selected, and
//! the view zooms all the way out again by itself. Every change eases in, so the view never
//! jumps.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_composable_dragging::DragModel;

use crate::state::{Editor, space_of};

/// How far the camera looks down when the map starts, in radians above the ground: a map seen
/// at 45 degrees, from the corner.
pub const MAP_PITCH: f32 = FRAC_PI_4;
/// The lowest the camera looks across the map, which shows the heights of the road from the side
/// and keeps the ground a surface rather than a line.
const MIN_PITCH: f32 = PI / 18.0;
/// The highest the camera looks down: straight down, as a plan.
const MAX_PITCH: f32 = FRAC_PI_2;
/// How far the map turns for a pixel of mouse movement, in radians.
const TURN_PER_PIXEL: f32 = 0.005;
/// How much closer one notch of the wheel brings the view, as a fraction of its height.
const ZOOM_PER_NOTCH: f32 = 0.12;
/// How many notches a pixel of scroll counts for, on a device that scrolls in pixels.
const NOTCHES_PER_PIXEL: f32 = 0.05;
/// How many notches a second a held zoom key counts for.
const NOTCHES_PER_SECOND: f32 = 6.0;
/// How many times closer than the framing of the whole track the view can zoom.
const ZOOM_RANGE: f32 = 8.0;
/// How fast the view eases to where it is going: the share of the way left falls by this factor
/// of itself every second, as `e` to the power of it.
const EASE_RATE: f32 = 9.0;
/// How close to its goal, as a fraction of the framing, the view is taken to be there.
const SETTLED: f32 = 1e-3;
/// How much room there is around the track in its framing, as a factor of its size.
const FRAMING_MARGIN: f32 = 1.2;

/// Marks the camera of the editor.
#[derive(Component)]
pub struct EditorCamera;

/// Where the camera is: it looks at a point of the map from the diagonal, `yaw` turned about the
/// vertical axis, and shows `view_height` of the world from the bottom of the screen to its top.
///
/// The point it looks at is worked out from the rest: the middle of the track when the view is
/// as far out as it goes, the `focus` when it is as close as it goes and in between for the
/// zooms between, plus what the map has been slid by. The view eases toward the goals it is
/// given, so it moves smoothly.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MapCamera {
    /// The middle of the track, which the view looks at when it frames the whole track.
    pub center: Vec3,
    /// The point the view comes in on, on its way to `goal_focus`.
    pub focus: Vec3,
    /// How far the map has been slid over the ground from where the zoom puts it.
    pub pan: Vec3,
    /// The turn about the vertical axis, in radians.
    pub yaw: f32,
    /// How far the camera looks down, in radians above the ground.
    pub pitch: f32,
    /// The turn the view is easing to.
    pub goal_yaw: f32,
    /// The pitch the view is easing to.
    pub goal_pitch: f32,
    /// The view height that shows the whole space: the ground the map can be slid and zoomed over,
    /// which is as far out as the view zooms.
    pub space_height: f32,
    /// How much of the world, measured across the screen, fits from the bottom of the screen to
    /// its top. The smaller it is, the closer the view.
    pub view_height: f32,
    /// The view height that frames the whole track, which is as far out as the view zooms and
    /// which the closest zoom follows from.
    pub frame_height: f32,
    /// The view height the view is zooming to.
    pub goal_height: f32,
    /// The point the view is coming in on: the selected control point, or the middle of the track.
    pub goal_focus: Vec3,
    /// The control point `goal_focus` belongs to.
    pub selected: Option<usize>,
}

impl MapCamera {
    /// The camera over `points`, which it frames on a screen `aspect` times as wide as it is
    /// high, seen from the diagonal.
    pub fn framing(points: &[Vec3], aspect: f32) -> Self {
        let (low, high) = points.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(low, high), point| (low.min(*point), high.max(*point)),
        );
        let target = if points.is_empty() {
            Vec3::ZERO
        } else {
            (low + high) / 2.0
        };
        let camera = Self {
            center: target,
            focus: target,
            pan: Vec3::ZERO,
            yaw: FRAC_PI_4,
            pitch: MAP_PITCH,
            goal_yaw: FRAC_PI_4,
            goal_pitch: MAP_PITCH,
            space_height: 1.0,
            view_height: 1.0,
            frame_height: 1.0,
            goal_height: 1.0,
            goal_focus: target,
            selected: None,
        };
        let fit = |points: &[Vec3]| {
            let (right, up) = (camera.rotation() * Vec3::X, camera.rotation() * Vec3::Y);
            let reach = |axis: Vec3| {
                points
                    .iter()
                    .map(|point| (*point - target).dot(axis).abs())
                    .fold(0.0, f32::max)
            };
            (reach(up) * 2.0).max(reach(right) * 2.0 / aspect) * FRAMING_MARGIN
        };
        let height = fit(points);
        let (middle, half) = space_of(points);
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| middle + Vec3::new(x * half, 0.0, z * half));
        Self {
            view_height: height,
            frame_height: height,
            goal_height: height,
            space_height: fit(&corners).max(height),
            ..camera
        }
    }

    /// This camera framed again on `points` for a screen `aspect` times as wide as it is high,
    /// as when the track has grown or shrunk. A view that was all the way out stays all the way
    /// out; a closer one keeps its zoom, kept within the new limits. The turn and the focus stay
    /// as they are.
    #[must_use]
    pub fn reframed(self, points: &[Vec3], aspect: f32) -> Self {
        let framed = Self::framing(points, aspect);
        let out = (self.goal_height - self.frame_height).abs() <= self.frame_height * SETTLED;
        let limits = framed.frame_height / ZOOM_RANGE..=framed.space_height;
        let clamp = |height: f32| height.clamp(*limits.start(), *limits.end());
        Self {
            center: framed.center,
            frame_height: framed.frame_height,
            space_height: framed.space_height,
            pan: Vec3::ZERO,
            view_height: if out {
                framed.frame_height
            } else {
                clamp(self.view_height)
            },
            goal_height: if out {
                framed.frame_height
            } else {
                clamp(self.goal_height)
            },
            ..self
        }
    }

    /// How far in the view is, from `0.0` framing the whole track to `1.0` as close as it goes.
    /// It is the share of the way from the framing to the closest zoom, which is how much of the
    /// way the view has glided to its focus.
    pub fn closeness(&self) -> f32 {
        if self.frame_height <= 0.0 {
            return 0.0;
        }
        ((1.0 - self.view_height / self.frame_height) / (1.0 - 1.0 / ZOOM_RANGE)).clamp(0.0, 1.0)
    }

    /// The point the camera looks at. Everything that frames the track is seen from its middle,
    /// and the closer the view, the nearer to the focus it looks, so that the focus is always on
    /// the screen: its distance from the middle of the view is the framing's own share, which
    /// the framing's margin keeps inside it.
    pub fn target(&self) -> Vec3 {
        self.center.lerp(self.focus, self.closeness()) + self.pan
    }

    /// The way the camera faces.
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0)
    }

    /// How far the camera stands from its point: everything that can be seen of the map, which
    /// reaches as far as the view is high in front of the point and behind it, is in front of it.
    pub fn reach(&self) -> f32 {
        self.view_height
    }

    /// The transform that puts the camera there.
    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.target() + self.rotation() * Vec3::Z * self.reach())
            .with_rotation(self.rotation())
    }

    /// The projection of the camera: orthographic, with the view height it shows.
    pub fn projection(&self) -> Projection {
        Projection::Orthographic(OrthographicProjection {
            near: -self.reach() * 2.0,
            far: self.reach() * 3.0,
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: self.view_height,
            },
            ..OrthographicProjection::default_3d()
        })
    }

    /// The camera turned by a mouse movement of `delta` pixels: sideways turns it about the
    /// map, and down looks more from above, up looks more across. The pitch stays between
    /// looking across the map and looking straight down.
    #[must_use]
    pub fn turned(self, delta: Vec2) -> Self {
        let yaw = self.yaw - delta.x * TURN_PER_PIXEL;
        let pitch = (self.pitch + delta.y * TURN_PER_PIXEL).clamp(MIN_PITCH, MAX_PITCH);
        Self {
            yaw,
            pitch,
            goal_yaw: yaw,
            goal_pitch: pitch,
            ..self
        }
    }

    /// The camera easing to the next corner of the map in `direction`, positive turning to the
    /// right: one of the four diagonals a map is drawn from, the nearest one to the way the
    /// camera is turned when it is not on one.
    #[must_use]
    pub fn cornered(self, direction: f32) -> Self {
        let corners = ((self.goal_yaw - FRAC_PI_4) / FRAC_PI_2).round();
        let step = if direction >= 0.0 { -1.0 } else { 1.0 };
        let on_corner = (self.goal_yaw - FRAC_PI_4 - corners * FRAC_PI_2).abs() < 1e-3;
        let goal = if on_corner {
            corners + step
        } else if step < 0.0 {
            (self.goal_yaw - FRAC_PI_4).div_euclid(FRAC_PI_2)
        } else {
            (self.goal_yaw - FRAC_PI_4).div_euclid(FRAC_PI_2) + 1.0
        };
        Self {
            goal_yaw: goal * FRAC_PI_2 + FRAC_PI_4,
            ..self
        }
    }

    /// The camera easing back to the view the map starts in: from a corner, at 45 degrees.
    #[must_use]
    pub fn homed(self) -> Self {
        let corners = ((self.goal_yaw - FRAC_PI_4) / FRAC_PI_2).round();
        Self {
            goal_yaw: corners * FRAC_PI_2 + FRAC_PI_4,
            goal_pitch: MAP_PITCH,
            ..self
        }
    }

    /// The camera with its point slid over the ground by a mouse movement of `delta` pixels on a
    /// window `window_height` pixels high, so that the ground follows the mouse.
    #[must_use]
    pub fn slid(self, delta: Vec2, window_height: f32) -> Self {
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let per_pixel = self.view_height / window_height;
        Self {
            pan: self.pan - right * delta.x * per_pixel
                + forward * delta.y * per_pixel / self.pitch.sin(),
            ..self
        }
    }

    /// The camera zooming by `notches` of the wheel, positive coming closer. It zooms toward
    /// the focus and stays between the closest zoom and the view of the whole space, which is
    /// further out than the framing of the whole track.
    #[must_use]
    pub fn zoomed(self, notches: f32) -> Self {
        Self {
            goal_height: (self.goal_height * (1.0 - ZOOM_PER_NOTCH).powf(notches))
                .clamp(self.frame_height / ZOOM_RANGE, self.space_height),
            ..self
        }
    }

    /// The camera with control point `selected`, at `at`, as what it comes in on, or the
    /// terrain, the whole track, when nothing is selected. Choosing the terrain sends the view
    /// all the way out, and the camera does not follow a point that is being moved: it only
    /// changes what it comes in on when another point is selected.
    #[must_use]
    pub fn focused_on(self, selected: Option<(usize, Vec3)>) -> Self {
        if self.selected == selected.map(|(index, _)| index) {
            return self;
        }
        Self {
            selected: selected.map(|(index, _)| index),
            goal_focus: selected.map_or(self.center, |(_, at)| at),
            goal_height: if selected.is_some() {
                self.goal_height
            } else {
                self.frame_height
            },
            pan: Vec3::ZERO,
            ..self
        }
    }

    /// The camera `seconds` further on its way to its goals. The share of the way left falls
    /// by the same factor in the same time however many steps the time is cut into, so the
    /// view eases the same at any frame rate.
    #[must_use]
    pub fn eased(self, seconds: f32) -> Self {
        let share = 1.0 - (-seconds * EASE_RATE).exp();
        let near = self.frame_height * SETTLED;
        let mut height = self.view_height + (self.goal_height - self.view_height) * share;
        if (self.goal_height - height).abs() <= near {
            height = self.goal_height;
        }
        let mut focus = self.focus.lerp(self.goal_focus, share);
        if focus.distance(self.goal_focus) <= near {
            focus = self.goal_focus;
        }
        let ease = |value: f32, goal: f32| {
            let next = value + (goal - value) * share;
            if (goal - next).abs() <= SETTLED {
                goal
            } else {
                next
            }
        };
        Self {
            view_height: height,
            focus,
            yaw: ease(self.yaw, self.goal_yaw),
            pitch: ease(self.pitch, self.goal_pitch),
            ..self
        }
    }
}

/// Moves the camera with the mouse and the keys, brings it on the control point that is
/// selected, and puts its transform and projection where the camera says.
#[allow(
    clippy::too_many_arguments,
    reason = "a system takes each thing it reads as a parameter"
)]
pub fn map_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    model: Res<DragModel<Editor>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&mut MapCamera, &mut Transform, &mut Projection), With<EditorCamera>>,
    mut points: Local<Option<usize>>,
) {
    let (mut map, mut transform, mut projection) = camera.into_inner();
    let editor = model.state();
    let mut next = *map;
    let count = editor.design.control_points.len();
    if points.replace(count).is_some_and(|before| before != count) && window.height() > 0.0 {
        next = next.reframed(&editor.framing_points(), window.width() / window.height());
    }
    next = next.focused_on(editor.point().map(|(index, point)| (index, point.position)));
    if buttons.pressed(MouseButton::Right) {
        next = next.turned(motion.delta);
    }
    if buttons.pressed(MouseButton::Middle) {
        next = next.slid(motion.delta, window.height());
    }
    let mut notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y * NOTCHES_PER_PIXEL,
    };
    let held =
        f32::from(keys.pressed(KeyCode::ArrowUp)) - f32::from(keys.pressed(KeyCode::ArrowDown));
    notches += held * NOTCHES_PER_SECOND * time.delta_secs();
    if keys.just_pressed(KeyCode::KeyE) {
        next = next.cornered(1.0);
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        next = next.cornered(-1.0);
    }
    if keys.just_pressed(KeyCode::Home) {
        next = next.homed();
    }
    next = next.zoomed(notches).eased(time.delta_secs());
    if next != *map {
        *map = next;
    }
    *transform = map.transform();
    *projection = map.projection();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Vec<Vec3> {
        vec![
            Vec3::new(-300.0, 0.0, -100.0),
            Vec3::new(200.0, 8.0, -150.0),
            Vec3::new(250.0, 3.0, 120.0),
            Vec3::new(-250.0, 0.0, 160.0),
        ]
    }

    fn camera() -> MapCamera {
        MapCamera::framing(&track(), 16.0 / 9.0)
    }

    #[test]
    fn the_map_is_seen_at_forty_five_degrees_from_the_diagonal() {
        let transform = camera().transform();
        let toward = (camera().target() - transform.translation).normalize();
        let level = Vec3::new(toward.x, 0.0, toward.z).normalize();
        assert!(
            (toward.dot(level).acos() - FRAC_PI_4).abs() < 1e-4,
            "looks down at 45 degrees"
        );
        assert!(
            (level.x.abs() - level.z.abs()).abs() < 1e-4,
            "from a diagonal: {level}"
        );
        assert!(matches!(camera().projection(), Projection::Orthographic(_)));
    }

    #[test]
    fn the_camera_looks_at_its_point_from_its_reach() {
        let map = camera();
        let transform = map.transform();
        assert!((transform.translation.distance(map.target()) - map.reach()).abs() < 1e-2);
        let ahead = transform.forward() * map.reach();
        assert!((transform.translation + ahead).distance(map.target()) < 1e-2);
        assert!(transform.translation.y > map.target().y, "it looks down");
    }

    #[test]
    fn the_framing_holds_every_point_of_the_track_on_the_screen() {
        for aspect in [16.0 / 9.0, 1.0, 9.0 / 16.0] {
            let map = MapCamera::framing(&track(), aspect);
            let (right, up) = (map.rotation() * Vec3::X, map.rotation() * Vec3::Y);
            for point in track() {
                let offset = point - map.target();
                assert!(offset.dot(up).abs() <= map.view_height / 2.0, "{aspect}");
                assert!(
                    offset.dot(right).abs() <= map.view_height * aspect / 2.0,
                    "{aspect}"
                );
            }
            assert_eq!(map.view_height, map.frame_height);
        }
    }

    #[test]
    fn the_framing_of_a_bigger_track_shows_more_of_the_world_and_of_nothing_is_a_point() {
        let doubled: Vec<Vec3> = track().iter().map(|point| *point * 2.0).collect();
        let big = MapCamera::framing(&doubled, 16.0 / 9.0);
        assert!((big.view_height / camera().view_height - 2.0).abs() < 1e-3);
        let nothing = MapCamera::framing(&[], 16.0 / 9.0);
        assert_eq!(nothing.target(), Vec3::ZERO);
        assert_eq!(nothing.view_height, 0.0);
    }

    #[test]
    fn everything_the_map_shows_is_in_front_of_the_camera_and_inside_its_depth() {
        let map = camera();
        let transform = map.transform();
        let Projection::Orthographic(projection) = map.projection() else {
            panic!("an orthographic projection");
        };
        let toward = *transform.forward();
        for point in track() {
            let depth = (point - transform.translation).dot(toward);
            assert!(depth > projection.near && depth < projection.far, "{depth}");
        }
    }

    #[test]
    fn turning_turns_the_map_about_the_vertical_axis_and_nothing_else() {
        let map = camera();
        let aside = map.turned(Vec2::new(100.0, 0.0));
        assert!(aside.yaw < map.yaw);
        assert_eq!(aside.target(), map.target());
        assert_eq!(aside.view_height, map.view_height);
        let down = map.turned(Vec2::new(0.0, 20.0));
        assert_eq!(down.yaw, map.yaw, "up and down keep the turn");
        assert!(down.pitch > map.pitch, "down looks more from above");
        assert_eq!(down.goal_pitch, down.pitch, "a drag is not eased");
        assert_eq!(down.target(), map.target());
        let up = map.turned(Vec2::new(0.0, -20.0));
        assert!(up.pitch < map.pitch, "up looks more across");
    }

    #[test]
    fn the_map_tilts_from_a_side_view_to_a_plan_and_no_further() {
        let map = camera();
        let plan = map.turned(Vec2::new(0.0, 10_000.0));
        assert_eq!(plan.pitch, MAX_PITCH, "straight down is a plan");
        let side = map.turned(Vec2::new(0.0, -10_000.0));
        assert_eq!(side.pitch, MIN_PITCH, "across the map still has a ground");
        for pitch in [MIN_PITCH, MAP_PITCH, MAX_PITCH] {
            let tilted = MapCamera { pitch, ..map };
            let transform = tilted.transform();
            assert!(transform.translation.is_finite(), "{pitch}");
            assert!(transform.rotation.is_normalized(), "{pitch}");
            assert!(
                (*transform.forward()).dot(Vec3::NEG_Y) > 0.0,
                "{pitch}: the camera looks down at the map"
            );
            let slid = tilted.slid(Vec2::new(10.0, 10.0), 720.0);
            assert!(slid.pan.is_finite(), "{pitch}");
        }
    }

    #[test]
    fn the_corner_keys_turn_the_map_by_quarter_turns_between_the_diagonals() {
        let map = camera();
        assert!(
            (map.yaw - FRAC_PI_4).abs() < 1e-6,
            "the map starts on a diagonal"
        );
        let mut turned = map;
        for step in 1..=8 {
            turned = turned.cornered(1.0);
            let expected = FRAC_PI_4 - step as f32 * FRAC_PI_2;
            assert!((turned.goal_yaw - expected).abs() < 1e-4, "{step}");
            assert_eq!(turned.yaw, map.yaw, "the turn eases rather than jumps");
        }
        let settled = map.cornered(-1.0).eased(f32::INFINITY);
        assert!((settled.yaw - (FRAC_PI_4 + FRAC_PI_2)).abs() < 1e-4);
        let off = map.turned(Vec2::new(-50.0, 0.0));
        assert!(off.yaw > map.yaw && off.yaw < map.yaw + FRAC_PI_2);
        for direction in [-1.0, 1.0] {
            let corner = off.cornered(direction).goal_yaw;
            let quarters = (corner - FRAC_PI_4) / FRAC_PI_2;
            assert!((quarters - quarters.round()).abs() < 1e-4, "{direction}");
            assert!(
                (corner - off.yaw).abs() < FRAC_PI_2 + 1e-4,
                "{direction}: the next corner is near"
            );
        }
    }

    #[test]
    fn home_goes_back_to_the_start_tilt_at_the_nearest_corner() {
        let away = camera()
            .turned(Vec2::new(-20.0, 90.0))
            .homed()
            .eased(f32::INFINITY);
        assert!((away.pitch - MAP_PITCH).abs() < 1e-4);
        let quarters = (away.yaw - FRAC_PI_4) / FRAC_PI_2;
        assert!((quarters - quarters.round()).abs() < 1e-4);
    }

    /// The camera with control point `index` selected and zoomed in by `notches`, settled.
    fn zoomed_on(index: usize, notches: f32) -> MapCamera {
        camera()
            .focused_on(Some((index, track()[index])))
            .zoomed(notches)
            .eased(f32::INFINITY)
    }

    #[test]
    fn zooming_stays_between_the_closest_zoom_and_the_framing_and_is_symmetric() {
        let map = camera();
        assert!(map.zoomed(1.0).goal_height < map.goal_height);
        assert!(
            map.zoomed(-1.0).goal_height > map.frame_height,
            "the wheel zooms out beyond the framing of the track"
        );
        assert_eq!(
            map.zoomed(-1000.0).goal_height,
            map.space_height,
            "the whole space is as far out as it goes"
        );
        assert!(map.space_height > map.frame_height * 2.0);
        let in_between = map.zoomed(5.0);
        assert!(
            (in_between.zoomed(3.0).zoomed(-3.0).goal_height / in_between.goal_height - 1.0).abs()
                < 1e-4
        );
        assert!(
            (map.zoomed(100.0).goal_height - map.frame_height / ZOOM_RANGE).abs() < 1e-3,
            "as close as the limit"
        );
    }

    #[test]
    fn the_view_eases_to_its_zoom_instead_of_jumping() {
        let map = camera().zoomed(5.0);
        assert_eq!(map.view_height, map.frame_height, "nothing moves by itself");
        let part = map.eased(0.05);
        assert!(part.view_height < map.view_height && part.view_height > map.goal_height);
        let settled = map.eased(f32::INFINITY);
        assert_eq!(settled.view_height, settled.goal_height);
        assert_eq!(settled.eased(1.0), settled, "a settled view stays put");
    }

    #[test]
    fn easing_takes_the_same_way_in_any_number_of_steps() {
        let map = camera().focused_on(Some((1, track()[1]))).zoomed(6.0);
        let once = map.eased(0.1);
        let twice = map.eased(0.05).eased(0.05);
        assert!((once.view_height / twice.view_height - 1.0).abs() < 1e-4);
        assert!(once.focus.distance(twice.focus) < 1e-2);
    }

    #[test]
    fn zooming_in_comes_in_on_the_selected_point_and_keeps_it_on_the_screen() {
        for index in 0..track().len() {
            let point = track()[index];
            for notches in 0..=24 {
                let map = zoomed_on(index, notches as f32);
                let offset = point - map.target();
                let (right, up) = (map.rotation() * Vec3::X, map.rotation() * Vec3::Y);
                assert!(
                    offset.dot(up).abs() <= map.view_height / 2.0,
                    "{index} {notches}"
                );
                assert!(
                    offset.dot(right).abs() <= map.view_height * 16.0 / 9.0 / 2.0,
                    "{index} {notches}"
                );
            }
            let closest = zoomed_on(index, 100.0);
            assert!(closest.target().distance(point) < 1e-3, "{index}");
        }
        assert_eq!(zoomed_on(2, 0.0).target(), camera().center);
    }

    #[test]
    fn zooming_out_goes_back_to_the_middle_of_the_track() {
        let map = zoomed_on(3, 100.0);
        let out = map.focused_on(None).eased(f32::INFINITY);
        assert_eq!(out.view_height, out.frame_height);
        assert!(out.target().distance(out.center) < 1e-3);
        let space = map.zoomed(-100.0).eased(f32::INFINITY);
        assert_eq!(space.view_height, space.space_height);
        assert!(space.target().distance(space.center) < 1e-3);
    }

    #[test]
    fn nothing_selected_zooms_on_the_middle_of_the_track() {
        let map = camera().zoomed(100.0).eased(f32::INFINITY);
        assert!(map.view_height < map.frame_height);
        assert!(map.target().distance(map.center) < 1e-3);
    }

    #[test]
    fn choosing_the_terrain_zooms_all_the_way_out_by_itself() {
        let close = zoomed_on(3, 100.0);
        let terrain = close.focused_on(None);
        assert_eq!(terrain.goal_height, close.frame_height);
        assert_eq!(terrain.goal_focus, close.center);
        assert_eq!(terrain.selected, None);
        let partway = terrain.eased(0.1);
        assert!(
            partway.view_height > close.view_height && partway.view_height < close.frame_height
        );
        let out = terrain.eased(f32::INFINITY);
        assert_eq!(out.view_height, out.frame_height);
        assert!(out.target().distance(out.center) < 1e-3);
    }

    #[test]
    fn selecting_another_point_keeps_the_zoom_and_slides_back_to_the_focus() {
        let close = zoomed_on(3, 6.0).slid(Vec2::new(40.0, 10.0), 720.0);
        assert!(close.pan != Vec3::ZERO);
        let next = close.focused_on(Some((1, track()[1])));
        assert_eq!(next.goal_height, close.goal_height);
        assert_eq!(next.goal_focus, track()[1]);
        assert_eq!(next.pan, Vec3::ZERO);
    }

    #[test]
    fn the_camera_does_not_chase_a_point_that_is_being_moved() {
        let map = camera().focused_on(Some((1, track()[1])));
        let moved = map.focused_on(Some((1, track()[1] + Vec3::new(50.0, 0.0, 20.0))));
        assert_eq!(moved, map);
    }

    #[test]
    fn sliding_moves_the_point_over_the_ground_with_the_mouse() {
        let map = MapCamera {
            yaw: 0.0,
            ..camera()
        };
        let slid = map.slid(Vec2::new(50.0, 0.0), 720.0);
        assert!(
            slid.target().x < map.target().x,
            "dragging right pulls the ground right"
        );
        assert_eq!(slid.target().y, map.target().y);
        assert_eq!(slid.target().z, map.target().z);
        let forward = map.slid(Vec2::new(0.0, 50.0), 720.0);
        assert!(
            forward.target().z < map.target().z,
            "dragging down pulls the ground towards you, which moves the point away"
        );
    }

    #[test]
    fn the_ground_under_the_pointer_follows_it_exactly_on_the_screen() {
        let map = camera();
        let window = Vec2::new(1280.0, 720.0);
        let delta = Vec2::new(60.0, -35.0);
        let slid = map.slid(delta, window.y);
        let ground = Vec3::new(40.0, 0.0, -20.0);
        let on_screen = |camera: &MapCamera, point: Vec3| {
            let local = camera
                .transform()
                .compute_affine()
                .inverse()
                .transform_point3(point);
            Vec2::new(local.x, -local.y) * window.y / camera.view_height
        };
        let moved = on_screen(&slid, ground) - on_screen(&map, ground);
        assert!((moved - delta).length() < 1e-2, "{moved} against {delta}");
    }

    #[test]
    fn reframing_follows_the_track_and_keeps_the_view_within_the_new_limits() {
        let aspect = 16.0 / 9.0;
        let small = camera();
        let bigger: Vec<Vec3> = track().iter().map(|point| *point * 3.0).collect();
        let out = small.reframed(&bigger, aspect);
        assert!(out.frame_height > small.frame_height * 2.0);
        assert_eq!(
            out.view_height, out.frame_height,
            "all the way out stays out"
        );
        assert_eq!(out.goal_height, out.frame_height);
        assert_eq!(out.center, MapCamera::framing(&bigger, aspect).center);
        assert_eq!((out.yaw, out.selected), (small.yaw, small.selected));

        let close = MapCamera {
            view_height: small.frame_height / 2.0,
            goal_height: small.frame_height / 2.0,
            pan: Vec3::X,
            ..small
        };
        let kept = close.reframed(&bigger, aspect);
        assert_eq!(
            kept.view_height, close.view_height,
            "a closer view keeps its zoom"
        );
        assert_eq!(kept.pan, Vec3::ZERO);

        let tiny: Vec<Vec3> = track().iter().map(|point| *point * 0.01).collect();
        let shrunk = close.reframed(&tiny, aspect);
        assert!(shrunk.view_height <= shrunk.space_height);
        assert!(shrunk.goal_height >= shrunk.frame_height / ZOOM_RANGE);
    }
}
