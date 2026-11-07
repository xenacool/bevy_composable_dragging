//! A scripted pointer that drags a node at the speed of a finger.
//!
//! When `DSD_SCREENSHOT` is set, the plugin presses the pointer at the start of a path in world
//! coordinates, drags it along the path with smooth easing and subtle finger-like wobble, and
//! stays pressed at the end so the animation shows the drag in progress. It sends real pointer
//! input, so picking, drag events and the engine behave exactly as they do for a user.
//!
//! The drag is timed in wall-clock seconds (`Time<Real>`), not by frame count. Its speed is
//! `FINGER_SPEED` screen heights per second, which is about 100 mm/s on a 150 mm tall screen
//! and inside the 80 to 120 mm/s range reported for finger sliding on touch surfaces. The
//! movement uses an ease-in-out curve plus a small sinusoidal wobble so it looks like a human
//! finger rather than a robotic line.
//!
//! Without `DSD_SCREENSHOT` it does nothing.

use bevy::camera::NormalizedRenderTarget;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowRef};

/// Frames to wait before the script starts, so shaders compile and picking sees the scene.
const WARM_UP_FRAMES: u32 = 5;

/// Seconds the pointer rests at the start position before pressing.
const START_DELAY: f32 = 0.2;

/// Drag speed in screen heights per second (about 100 mm/s on a 150 mm tall screen).
const FINGER_SPEED: f32 = 0.67;

/// Number of samples used to measure the on-screen length of the path.
const PATH_SAMPLES: u32 = 64;

/// Amplitude of the finger wobble in world units (very subtle).
const WOBBLE_AMPLITUDE: f32 = 0.005;

/// Frequency of the finger wobble (cycles per drag).
const WOBBLE_CYCLES: f32 = 3.0;

/// Drags the pointer along a path of world positions.
pub struct ScriptedDragPlugin {
    /// Start position in world space.
    from: Vec2,
    /// End position in world space.
    to: Vec2,
    /// For arc drags: center, radius, start angle, end angle.
    arc: Option<(Vec2, f32, f32, f32)>,
}

impl ScriptedDragPlugin {
    /// Presses at `from` and drags in a straight line to `to` with natural easing.
    pub fn straight(from: Vec2, to: Vec2) -> Self {
        Self {
            from,
            to,
            arc: None,
        }
    }

    /// Presses on the circle around `center` at angle `from` and drags along it to angle `to`
    /// (radians, counterclockwise from the positive X axis) with natural easing.
    pub fn arc(center: Vec2, radius: f32, from: f32, to: f32) -> Self {
        Self {
            from: center + Vec2::from_angle(from) * radius,
            to: center + Vec2::from_angle(to) * radius,
            arc: Some((center, radius, from, to)),
        }
    }
}

/// Real-time window of the scripted drag, inserted when the drag starts.
#[derive(Resource)]
pub struct DragTimeline {
    /// `Time<Real>` elapsed seconds at which the pointer is released.
    pub end: f32,
}

#[derive(Resource)]
struct Script {
    from: Vec2,
    to: Vec2,
    arc: Option<(Vec2, f32, f32, f32)>,
}

#[derive(Clone, Copy)]
struct Timing {
    start: f32,
    duration: f32,
}

impl Plugin for ScriptedDragPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("DSD_SCREENSHOT").is_err() {
            return;
        }
        app.insert_resource(Script {
            from: self.from,
            to: self.to,
            arc: self.arc,
        })
        .add_systems(Update, run_script);
    }
}

/// Smooth ease-in-out curve: starts slow, accelerates, then decelerates.
fn ease_in_out(t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    3.0 * t2 - 2.0 * t3
}

/// Subtle finger wobble: a sum of two sine waves at different frequencies.
fn finger_wobble(t: f32) -> Vec2 {
    let phase = t * WOBBLE_CYCLES * std::f32::consts::TAU;
    let phase2 = phase * 1.7 + 0.5;
    let envelope = (std::f32::consts::PI * t).sin();
    Vec2::new(
        WOBBLE_AMPLITUDE * phase.sin() * envelope,
        WOBBLE_AMPLITUDE * phase2.cos() * envelope,
    )
}

/// Compute the position along the drag path at progress `t` (0..1).
fn path_position(script: &Script, t: f32) -> Vec2 {
    let t = t.clamp(0.0, 1.0);
    let eased = ease_in_out(t);

    let base = match script.arc {
        Some((center, radius, from_angle, to_angle)) => {
            let angle = from_angle + (to_angle - from_angle) * eased;
            center + Vec2::from_angle(angle) * radius
        }
        None => script.from.lerp(script.to, eased),
    };

    base + finger_wobble(t)
}

/// On-screen length in pixels of the whole drag path.
fn screen_path_length(script: &Script, screen: impl Fn(Vec2) -> Option<Vec2>) -> Option<f32> {
    let mut length = 0.0;
    let mut previous = None;
    for i in 0..=PATH_SAMPLES {
        let position = screen(path_position(script, i as f32 / PATH_SAMPLES as f32))?;
        if let Some(last) = previous {
            length += position.distance(last);
        }
        previous = Some(position);
    }
    Some(length)
}

fn run_script(
    script: Res<Script>,
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut warm_up: Local<u32>,
    mut timing: Local<Option<Timing>>,
    mut pressed: Local<bool>,
    mut previous: Local<Option<Vec2>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<super::SceneCamera>>,
    mut inputs: MessageWriter<PointerInput>,
) {
    if *warm_up < WARM_UP_FRAMES {
        *warm_up += 1;
        return;
    }
    let (Ok((window_entity, window)), Ok((camera, camera_transform))) =
        (windows.single(), cameras.single())
    else {
        return;
    };
    let Some(target) = WindowRef::Primary.normalize(Some(window_entity)) else {
        return;
    };
    let target = NormalizedRenderTarget::Window(target);
    let screen = |world: Vec2| {
        camera
            .world_to_viewport(camera_transform, world.extend(0.0))
            .ok()
    };

    let now = time.elapsed_secs();
    if timing.is_none() {
        let Some(length) = screen_path_length(&script, screen) else {
            return;
        };
        let duration = (length / (FINGER_SPEED * window.height())).max(f32::EPSILON);
        *timing = Some(Timing {
            start: now,
            duration,
        });
        commands.insert_resource(DragTimeline {
            end: now + START_DELAY + duration,
        });
    }
    let Some(Timing { start, duration }) = *timing else {
        return;
    };

    let elapsed = now - start;
    let t = (elapsed - START_DELAY) / duration;
    let Some(position) = screen(path_position(&script, t)) else {
        return;
    };

    let mut send = |action: PointerAction| {
        inputs.write(PointerInput::new(
            PointerId::Mouse,
            Location {
                target: target.clone(),
                position,
            },
            action,
        ));
    };

    if !*pressed {
        if elapsed >= START_DELAY {
            send(PointerAction::Press(PointerButton::Primary));
            *pressed = true;
        } else {
            send(PointerAction::Move { delta: Vec2::ZERO });
        }
    } else if t < 1.0 {
        let delta = previous.map_or(Vec2::ZERO, |last| position - last);
        send(PointerAction::Move { delta });
    }
    *previous = Some(position);
}
